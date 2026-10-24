//! Incremental whole-document layout for the editor's `TextEdit`.
//!
//! `TextEdit` needs a galley for the complete text. egui's own cache lays each paragraph out once,
//! but on every edit it still hashes the whole document and copies, hashes, and looks up every
//! paragraph again: ~10 ms per keystroke at 40k lines. Here the paragraph galleys of the previous
//! text are kept, a paragraph whose text is unchanged reuses its galley, and only edited
//! paragraphs are laid out. The result is assembled with the same [`Galley::concat`] egui uses, so
//! rows, positions and cursor mapping are identical.

use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{self, Galley, text::LayoutJob};

/// The paragraph galleys of the last text laid out, and what they were laid out for.
pub(crate) struct LineLayout {
    text: String,
    paragraphs: Vec<Range<usize>>,
    galleys: Vec<Arc<Galley>>,
    /// The assembled galley for `text`. `TextEdit` lays the document out twice per frame (before
    /// and after applying input), and the first call nearly always repeats the previous text.
    merged: Arc<Galley>,
    key: LayoutKey,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct LayoutKey {
    hanging: bool,
    /// [`super::folding::Folds::generation`] the hidden lines came from.
    folds: u64,
    wrap_width_bits: u32,
    pixels_per_point_bits: u32,
    fonts_generation: u64,
}

/// Paragraphs as egui splits them: on `\n`, excluding it, except that a final `\n` stays in the
/// last paragraph (which then gets its trailing empty row).
fn paragraph_ranges(text: &str, capacity: usize) -> Vec<Range<usize>> {
    let mut paragraphs = Vec::with_capacity(capacity);
    let mut start = 0;
    for newline in memchr::memchr_iter(b'\n', text.as_bytes()) {
        paragraphs.push(start..newline);
        start = newline + 1;
    }
    if start < text.len() {
        paragraphs.push(start..text.len());
    } else if let Some(last) = paragraphs.last_mut() {
        // The text ends with `\n`: egui keeps it in the last paragraph.
        last.end += 1;
    }
    paragraphs
}

/// The job for one paragraph, as egui derives it from a single-section document job.
fn paragraph_job(job: &LayoutJob, text: &str, first: bool) -> LayoutJob {
    let section = &job.sections[0];
    LayoutJob {
        text: text.to_owned(),
        sections: vec![egui::text::LayoutSection {
            leading_space: if first { section.leading_space } else { 0.0 },
            byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(text.len()),
            format: section.format.clone(),
        }],
        wrap: job.wrap.clone(),
        first_row_min_height: if first { job.first_row_min_height } else { 0.0 },
        break_on_newline: job.break_on_newline,
        halign: job.halign,
        justify: job.justify,
        round_output_to_gui: job.round_output_to_gui,
        keep_trailing_whitespace: job.keep_trailing_whitespace,
    }
}

/// Lay out `job` (one section covering all of its text), reusing the paragraphs of the previous
/// call in `cache`. `lay_out` lays out a single paragraph job.
///
/// With `hanging`, soft-wrapped rows continue at their line's indentation (see
/// [`hanging_paragraph`]). Callers must key `cache` on it: it is not part of the cache's own key.
///
/// Lines in `hidden` (folded, see [`super::folding`]) become zero-height rows; `folds` must
/// change whenever `hidden` does.
pub(crate) fn layout(
    cache: &mut Option<LineLayout>,
    mut job: LayoutJob,
    pixels_per_point: f32,
    hanging: bool,
    (hidden, folds): (&[Range<usize>], u64),
    mut lay_out: impl FnMut(LayoutJob) -> Arc<Galley>,
) -> Arc<Galley> {
    let is_hidden = |line: usize| {
        let at = hidden.partition_point(|range| range.end <= line);
        hidden.get(at).is_some_and(|range| range.contains(&line))
    };
    // egui rounds the wrap width before laying out; match it so paragraphs hit its cache and the
    // merged galley reports the same width.
    if job.wrap.max_width.is_finite() {
        job.wrap.max_width = job.wrap.max_width.round();
    }
    if job.sections.len() != 1 || !job.break_on_newline || job.wrap.max_rows != usize::MAX {
        *cache = None;
        return if (hanging && job.wrap.max_width.is_finite()) || !hidden.is_empty() {
            layout_paragraphs(job, pixels_per_point, hanging, is_hidden, lay_out)
        } else {
            lay_out(job)
        };
    }
    let hanging = hanging && job.wrap.max_width.is_finite();
    let key = LayoutKey {
        hanging,
        folds,
        wrap_width_bits: job.wrap.max_width.to_bits(),
        pixels_per_point_bits: pixels_per_point.to_bits(),
        fonts_generation: crate::theme::fonts_generation(),
    };
    let previous = cache.take().filter(|previous| previous.key == key);
    if let Some(unchanged) = previous
        .as_ref()
        .filter(|previous| previous.text == job.text)
    {
        let merged = Arc::clone(&unchanged.merged);
        *cache = previous;
        return merged;
    }

    let paragraphs = paragraph_ranges(
        &job.text,
        previous
            .as_ref()
            .map_or(0, |previous| previous.paragraphs.len() + 1),
    );
    // Bytes shared with the previous text at the start and at the end. A paragraph whose text
    // and closing (or opening) newline lie inside them is unchanged, with no need to compare it.
    let (new_len, old_len) = (
        job.text.len(),
        previous.as_ref().map_or(0, |previous| previous.text.len()),
    );
    let (prefix, suffix) = previous.as_ref().map_or((0, 0), |previous| {
        let prefix =
            crate::text_diff::common_prefix_len(previous.text.as_bytes(), job.text.as_bytes());
        let suffix = crate::text_diff::common_suffix_len(
            previous.text.as_bytes(),
            job.text.as_bytes(),
            old_len.min(new_len) - prefix,
        );
        (prefix, suffix)
    });
    let mut galleys = Vec::with_capacity(paragraphs.len());
    for (index, range) in paragraphs.iter().enumerate() {
        if let Some(previous) = &previous {
            // Its newline is shared and is not the old text's last byte (which egui folds into
            // the last paragraph), so the old paragraph at the same index is identical.
            if range.end < prefix && range.end + 1 < old_len {
                galleys.push(Arc::clone(&previous.galleys[index]));
                continue;
            }
            // The newline before it is shared, so is everything after: same paragraph, counted
            // from the end.
            if range.start > new_len - suffix {
                let old = previous.paragraphs.len() + index - paragraphs.len();
                galleys.push(Arc::clone(&previous.galleys[old]));
                continue;
            }
        }
        let text = &job.text[range.clone()];
        let reused = previous.as_ref().and_then(|previous| {
            // Paragraphs before an edit keep their index; those after it keep their distance
            // from the end. Either way the text must match exactly.
            let same = |old: usize| {
                (previous.text.get(previous.paragraphs[old].clone()) == Some(text))
                    .then(|| Arc::clone(&previous.galleys[old]))
            };
            let from_start = (index < previous.paragraphs.len()).then_some(index);
            let from_end = (previous.paragraphs.len() + index).checked_sub(paragraphs.len());
            // The first paragraph also carries the job's leading space and first-row height.
            from_start.and_then(same).or_else(|| {
                from_end
                    .filter(|old| (*old == 0) == (index == 0))
                    .and_then(same)
            })
        });
        galleys.push(reused.unwrap_or_else(|| {
            let paragraph = paragraph_job(&job, text, index == 0);
            if hanging {
                hanging_paragraph(paragraph, &mut lay_out)
            } else {
                lay_out(paragraph)
            }
        }));
    }

    let text = job.text.clone();
    let shown: Vec<Arc<Galley>>;
    let visible: &[Arc<Galley>] = if hidden.is_empty() {
        &galleys
    } else {
        shown = galleys
            .iter()
            .enumerate()
            .map(|(line, galley)| {
                if is_hidden(line) {
                    collapse(galley)
                } else {
                    Arc::clone(galley)
                }
            })
            .collect();
        &shown
    };
    let merged = if galleys.len() == 1 && paragraphs[0] == (0..text.len()) {
        // A single paragraph: exactly what egui returns for it.
        Arc::clone(&galleys[0])
    } else if galleys.is_empty() {
        lay_out(job)
    } else {
        Arc::new(Galley::concat(Arc::new(job), visible, pixels_per_point))
    };
    *cache = Some(LineLayout {
        text,
        paragraphs,
        galleys,
        merged: Arc::clone(&merged),
        key,
    });
    merged
}

/// Lay out one paragraph (no `\n` inside, except a trailing one) so that its soft-wrapped rows
/// start at the paragraph's indentation, like Sublime's `indent_subsequent_lines`. egui has no
/// hanging indent, so the paragraph is wrapped that much narrower and its continuation rows are
/// moved right; cursor mapping is relative to each row's position, so it stays exact.
pub(crate) fn hanging_paragraph(
    job: LayoutJob,
    lay_out: &mut impl FnMut(LayoutJob) -> Arc<Galley>,
) -> Arc<Galley> {
    let width = job.wrap.max_width;
    if !width.is_finite() {
        return lay_out(job);
    }
    let indent_bytes = job
        .text
        .bytes()
        .position(|byte| byte != b' ' && byte != b'\t')
        .unwrap_or(0);
    if indent_bytes == 0 {
        return lay_out(job);
    }
    let galley = lay_out(job.clone());
    if galley.rows.len() < 2 {
        return galley;
    }
    let indent = galley.rows[0]
        .row
        .glyphs
        .get(indent_bytes)
        .map_or(0.0, |glyph| glyph.pos.x);
    // Deep indentation would squeeze the rest of the line into a sliver: wrap it flush instead.
    if indent <= 0.0 || indent > width * 0.5 {
        return galley;
    }
    let mut narrow = job;
    narrow.wrap.max_width = (width - indent).round().max(1.0);
    let mut galley = (*lay_out(narrow)).clone();
    for placed in galley.rows.iter_mut().skip(1) {
        placed.pos.x += indent;
    }
    let mut rect = egui::Rect::NOTHING;
    let mut mesh_bounds = egui::Rect::NOTHING;
    for placed in &galley.rows {
        rect |= placed.rect();
        mesh_bounds |= placed.visuals.mesh_bounds.translate(placed.pos.to_vec2());
    }
    galley.rect = rect;
    galley.mesh_bounds = mesh_bounds;
    Arc::new(galley)
}

/// A folded line: its rows keep their glyphs (so caret mapping is unchanged) but take no
/// height and draw nothing.
pub(crate) fn collapse(galley: &Galley) -> Arc<Galley> {
    let mut collapsed = galley.clone();
    for placed in &mut collapsed.rows {
        let mut row = (*placed.row).clone();
        row.size.y = 0.0;
        row.visuals.mesh = Default::default();
        row.visuals.mesh_bounds = egui::Rect::NOTHING;
        row.visuals.glyph_vertex_range = 0..0;
        placed.row = Arc::new(row);
        placed.pos.y = 0.0;
    }
    collapsed.rect = egui::Rect::from_min_size(
        egui::pos2(collapsed.rect.min.x, 0.0),
        egui::vec2(collapsed.rect.width(), 0.0),
    );
    collapsed.mesh_bounds = egui::Rect::NOTHING;
    collapsed.num_vertices = 0;
    collapsed.num_indices = 0;
    Arc::new(collapsed)
}

/// A whole job of any number of sections, laid out paragraph by paragraph (with
/// [`hanging_paragraph`] when `hanging`, collapsed when `hidden(line)`) and joined the way egui
/// joins paragraphs, so rows match the geometry [`layout`] produces for the same text.
pub(crate) fn layout_paragraphs(
    job: LayoutJob,
    pixels_per_point: f32,
    hanging: bool,
    hidden: impl Fn(usize) -> bool,
    mut lay_out: impl FnMut(LayoutJob) -> Arc<Galley>,
) -> Arc<Galley> {
    let mut job = job;
    if job.wrap.max_width.is_finite() {
        job.wrap.max_width = job.wrap.max_width.round();
    }
    let hanging = hanging && job.wrap.max_width.is_finite();
    if !job.break_on_newline || job.text.is_empty() {
        return lay_out(job);
    }
    let paragraphs = paragraph_ranges(&job.text, 0);
    if paragraphs.is_empty() {
        return lay_out(job);
    }
    let mut galleys = Vec::with_capacity(paragraphs.len());
    let mut section_index = 0;
    for (index, range) in paragraphs.iter().enumerate() {
        let mut sections = Vec::new();
        while section_index < job.sections.len()
            && job.sections[section_index].byte_range.end.0 <= range.start
        {
            section_index += 1;
        }
        for (offset, section) in job.sections[section_index..].iter().enumerate() {
            let (start, end) = (section.byte_range.start.0, section.byte_range.end.0);
            if start >= range.end {
                break;
            }
            let (start, end) = (start.max(range.start), end.min(range.end));
            if start < end {
                sections.push(egui::text::LayoutSection {
                    leading_space: if index == 0 && offset == 0 {
                        section.leading_space
                    } else {
                        0.0
                    },
                    byte_range: egui::text::ByteIndex(start - range.start)
                        ..egui::text::ByteIndex(end - range.start),
                    format: section.format.clone(),
                });
            }
        }
        if sections.is_empty() {
            // An empty line still needs a font for its row height.
            let format = job
                .sections
                .get(section_index.min(job.sections.len().saturating_sub(1)))
                .map(|section| section.format.clone())
                .unwrap_or_default();
            sections.push(egui::text::LayoutSection {
                leading_space: 0.0,
                byte_range: egui::text::ByteIndex(0)..egui::text::ByteIndex(range.len()),
                format,
            });
        }
        let paragraph = LayoutJob {
            text: job.text[range.clone()].to_owned(),
            sections,
            wrap: job.wrap.clone(),
            first_row_min_height: if index == 0 {
                job.first_row_min_height
            } else {
                0.0
            },
            break_on_newline: job.break_on_newline,
            halign: job.halign,
            justify: job.justify,
            round_output_to_gui: job.round_output_to_gui,
            keep_trailing_whitespace: job.keep_trailing_whitespace,
        };
        let galley = if hanging {
            hanging_paragraph(paragraph, &mut lay_out)
        } else {
            lay_out(paragraph)
        };
        galleys.push(if hidden(index) {
            collapse(&galley)
        } else {
            galley
        });
    }
    if galleys.len() == 1 && paragraphs[0] == (0..job.text.len()) {
        return galleys.pop().expect("one paragraph");
    }
    Arc::new(Galley::concat(Arc::new(job), &galleys, pixels_per_point))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(text: &str) -> LayoutJob {
        let mut job = LayoutJob::simple(
            text.to_owned(),
            egui::FontId::monospace(12.0),
            egui::Color32::TRANSPARENT,
            f32::INFINITY,
        );
        job.wrap.max_width = f32::INFINITY;
        job
    }

    fn rows(galley: &Galley) -> Vec<(egui::Pos2, usize, bool)> {
        galley
            .rows
            .iter()
            .map(|row| (row.pos, row.row.glyphs.len(), row.ends_with_newline))
            .collect()
    }

    /// Lay `text` out incrementally and from scratch, and compare the results.
    fn check(ctx: &egui::Context, cache: &mut Option<LineLayout>, text: &str) -> usize {
        let mut laid_out = 0;
        let mut incremental = None;
        let mut full = None;
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.fonts_mut(|fonts| {
                incremental = Some(layout(cache, job(text), 1.0, false, (&[], 0), |job| {
                    laid_out += 1;
                    fonts.layout_job(job)
                }));
                full = Some(fonts.layout_job(job(text)));
            });
        });
        let (incremental, full) = (incremental.unwrap(), full.unwrap());
        assert_eq!(incremental.job.text, text);
        assert_eq!(rows(&incremental), rows(&full), "rows differ for {text:?}");
        assert_eq!(incremental.rect, full.rect);
        assert_eq!(incremental.end().index, full.end().index);
        laid_out
    }

    #[test]
    fn random_edits_match_a_full_layout() {
        let ctx = egui::Context::default();
        let mut cache = None;
        let mut text = "a\n".to_owned();
        check(&ctx, &mut cache, &text);
        check(&ctx, &mut cache, "a\nb");
        check(&ctx, &mut cache, "a\n");
        let pieces = ["x", "\n", "↑", "\n\n", "", "fn f() {}\n"];
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed % bound.max(1) as u64) as usize
        };
        for _ in 0..300 {
            let boundaries: Vec<usize> = text
                .char_indices()
                .map(|(i, _)| i)
                .chain([text.len()])
                .collect();
            let start = boundaries[next(boundaries.len())];
            let end_choices: Vec<usize> =
                boundaries.iter().copied().filter(|b| *b >= start).collect();
            let end = end_choices[next(end_choices.len().min(4))];
            text.replace_range(start..end, pieces[next(pieces.len())]);
            check(&ctx, &mut cache, &text);
        }
    }

    #[test]
    fn splits_paragraphs_like_egui() {
        assert_eq!(paragraph_ranges("", 0), Vec::<Range<usize>>::new());
        assert_eq!(paragraph_ranges("a", 0), vec![0..1]);
        assert_eq!(paragraph_ranges("a\nb", 0), vec![0..1, 2..3]);
        assert_eq!(paragraph_ranges("a\nb\n", 0), vec![0..1, 2..4]);
        assert_eq!(paragraph_ranges("a\n\n", 0), vec![0..1, 2..3]);
        assert_eq!(paragraph_ranges("\n", 0), vec![0..1]);
    }

    #[test]
    fn edits_match_a_full_layout_and_only_relayout_changed_lines() {
        let ctx = egui::Context::default();
        let mut cache = None;
        let base: String = (0..200).map(|i| format!("line {i} ↑ text\n")).collect();
        assert_eq!(check(&ctx, &mut cache, &base), 200);

        // Typing in the middle of one line relays just that line.
        let typed = base.replacen("line 100 ", "line 100 x", 1);
        assert_eq!(check(&ctx, &mut cache, &typed), 1);
        // A new line shifts everything after it, which must still be reused.
        let split = typed.replacen("line 100 x", "line 100\n x", 1);
        assert!(check(&ctx, &mut cache, &split) <= 2);
        // Joining lines, deleting at the ends, and edge cases.
        for text in [
            base.replacen("line 50 ↑ text\nline 51", "line 50 ↑ textline 51", 1),
            base.trim_end().to_owned(),
            format!("{base}tail"),
            format!("head\n{base}"),
            "\n".to_owned(),
            "\n\n\n".to_owned(),
            "no newline".to_owned(),
            String::new(),
            base.clone(),
        ] {
            check(&ctx, &mut cache, &text);
        }
    }

    /// The caret geometry (one section per paragraph) and the colored text (many sections)
    /// must wrap and indent identically, or the caret drifts off the painted glyphs.
    #[test]
    fn hanging_indent_matches_between_geometry_and_syntax_layouts() {
        let ctx = egui::Context::default();
        let text = "fn main() {\n    let words = \"a long line that has to wrap a few times at this width\";\n\n\tif words.len() > 3 { println!(\"{words} and then some more words\"); }\n}\n";
        let width = 160.0;
        let font = egui::FontId::monospace(12.0);
        let mut geometry = None;
        let mut colored = None;
        let mut flush = None;
        let _ = ctx.run_ui(Default::default(), |ui| {
            ui.fonts_mut(|fonts| {
                let mut plain = LayoutJob::simple(
                    text.to_owned(),
                    font.clone(),
                    egui::Color32::TRANSPARENT,
                    width,
                );
                plain.wrap.max_width = width;
                geometry = Some(layout(
                    &mut None,
                    plain.clone(),
                    1.0,
                    true,
                    (&[], 0),
                    |job| fonts.layout_job(job),
                ));
                flush = Some(fonts.layout_job(plain));
                // Sections cut every 7 bytes, across newlines, like a syntax highlighter's.
                let mut job = LayoutJob::default();
                job.wrap.max_width = width;
                job.text = text.to_owned();
                let mut start = 0;
                while start < text.len() {
                    let end = (start + 7).min(text.len());
                    job.sections.push(egui::text::LayoutSection {
                        leading_space: 0.0,
                        byte_range: egui::text::ByteIndex(start)..egui::text::ByteIndex(end),
                        format: egui::TextFormat::simple(
                            font.clone(),
                            egui::Color32::from_gray(start as u8),
                        ),
                    });
                    start = end;
                }
                colored = Some(layout_paragraphs(
                    job,
                    1.0,
                    true,
                    |_| false,
                    |job| fonts.layout_job(job),
                ));
            });
        });
        let (geometry, colored, flush) = (geometry.unwrap(), colored.unwrap(), flush.unwrap());
        assert_eq!(rows(&geometry), rows(&colored));
        assert_eq!(geometry.rect, colored.rect);
        // Wrapped rows of the indented lines start right of column 0; the flush layout's don't.
        let indented = geometry
            .rows
            .windows(2)
            .filter(|pair| !pair[0].ends_with_newline && pair[1].pos.x > 0.0)
            .count();
        assert!(indented >= 2, "wrapped rows were not indented");
        assert!(flush.rows.iter().all(|row| row.pos.x == 0.0));
        // Every row stays inside the wrap width.
        assert!(
            geometry
                .rows
                .iter()
                .all(|row| row.rect().right() <= width + 0.5)
        );
    }
}
