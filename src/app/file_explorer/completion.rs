//! Sublime's `auto_complete` from the buffer: while a word is typed, the words of the file
//! that start with it (then those containing it) are offered under the caret; Up/Down choose,
//! Enter or Tab insert, Escape or any non-word key closes the list.

use eframe::egui::{self, Event, Key};

use crate::theme::*;

/// An open completion list.
#[derive(Clone, Debug, Default)]
pub(crate) struct Completion {
    /// Byte where the word being typed starts.
    pub start: usize,
    pub prefix: String,
    pub items: Vec<String>,
    pub selected: usize,
}

/// The words of a text with their counts and lowercase forms. Gathering them walks the whole
/// file, so it happens when a new word starts being typed, and at most every
/// [`REFRESH_EVERY`]: a word typed a moment ago may be missing until then.
#[derive(Default)]
pub(crate) struct WordIndex {
    revision: Option<u64>,
    built_at: Option<std::time::Instant>,
    /// Where the word being typed started when the index was last consulted.
    session: Option<usize>,
    words: Vec<(String, u32)>,
    lower: Vec<String>,
}

const REFRESH_EVERY: std::time::Duration = std::time::Duration::from_secs(2);

const MAX_ITEMS: usize = 8;
const MAX_WORD: usize = 64;

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl WordIndex {
    fn refresh(&mut self, text: &str, revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
        let mut counts: std::collections::HashMap<&str, u32> = std::collections::HashMap::new();
        for word in text.split(|c: char| !is_word(c)) {
            if word.len() >= 3
                && word.len() <= MAX_WORD
                && !word.starts_with(|c: char| c.is_ascii_digit())
            {
                *counts.entry(word).or_default() += 1;
            }
        }
        self.words = counts
            .into_iter()
            .map(|(word, count)| (word.to_owned(), count))
            .collect();
        self.lower = self
            .words
            .iter()
            .map(|(word, _)| word.to_lowercase())
            .collect();
        self.revision = Some(revision);
        self.built_at = Some(std::time::Instant::now());
    }

    /// Refresh for a word starting at `start`, unless this session already did, or the index
    /// is recent enough.
    fn refresh_for(&mut self, text: &str, revision: u64, start: usize) {
        if self.session == Some(start) && self.revision.is_some() {
            return;
        }
        self.session = Some(start);
        let fresh = self.built_at.is_some_and(|at| at.elapsed() < REFRESH_EVERY);
        if !fresh {
            self.refresh(text, revision);
        }
    }
}

/// Candidates for `prefix`: words starting with it (case-insensitively), most frequent first,
/// then words merely containing it. The word being typed itself (seen once) is left out.
pub(crate) fn candidates(index: &WordIndex, prefix: &str) -> Vec<String> {
    let needle = prefix.to_lowercase();
    let mut starts: Vec<&(String, u32)> = Vec::new();
    let mut contains: Vec<&(String, u32)> = Vec::new();
    for (entry, lower) in index.words.iter().zip(&index.lower) {
        let (word, count) = entry;
        if word == prefix && *count <= 1
            || word.len() <= prefix.len() && word.eq_ignore_ascii_case(prefix)
        {
            continue;
        }
        if lower.starts_with(&needle) {
            starts.push(entry);
        } else if needle.len() >= 2 && lower.contains(&needle) {
            contains.push(entry);
        }
    }
    let order = |a: &&(String, u32), b: &&(String, u32)| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0));
    starts.sort_by(order);
    contains.sort_by(order);
    starts
        .into_iter()
        .chain(contains)
        .take(MAX_ITEMS)
        .map(|(word, _)| word.clone())
        .collect()
}

/// The word being typed at byte `caret`: where it starts and its text.
pub(crate) fn word_before(text: &str, caret: usize) -> Option<(usize, &str)> {
    let caret = caret.min(text.len());
    let start = text[..caret]
        .char_indices()
        .rev()
        .take_while(|(_, c)| is_word(*c))
        .last()
        .map(|(index, _)| index)?;
    // A word that continues after the caret is being edited, not typed.
    if text[caret..].chars().next().is_some_and(is_word) {
        return None;
    }
    let word = &text[start..caret];
    (!word.starts_with(|c: char| c.is_ascii_digit())).then_some((start, word))
}

/// What the open list did with this frame's keys.
pub(crate) enum ListKey {
    /// Insert this word in place of the typed prefix.
    Accept(String),
    Moved,
}

/// Take Up/Down/Enter/Tab out of the input while the list is open.
pub(crate) fn take_list_keys(
    input: &mut egui::InputState,
    completion: &mut Completion,
) -> Option<ListKey> {
    let mut result = None;
    let count = completion.items.len();
    input.events.retain(|event| {
        let Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            return true;
        };
        if !modifiers.is_none()
            || result
                .as_ref()
                .is_some_and(|r| matches!(r, ListKey::Accept(_)))
        {
            return true;
        }
        match key {
            Key::ArrowDown => {
                completion.selected = (completion.selected + 1) % count.max(1);
                result = Some(ListKey::Moved);
                false
            }
            Key::ArrowUp => {
                completion.selected = (completion.selected + count.max(1) - 1) % count.max(1);
                result = Some(ListKey::Moved);
                false
            }
            Key::Enter | Key::Tab => {
                if let Some(item) = completion.items.get(completion.selected) {
                    result = Some(ListKey::Accept(item.clone()));
                }
                false
            }
            _ => true,
        }
    });
    result
}

/// Refresh the list after this frame's typing: open or narrow it when a word is being typed at
/// a single caret, close it otherwise.
pub(crate) fn update(
    slot: &mut Option<Completion>,
    index: &mut WordIndex,
    text: &str,
    revision: u64,
    caret: Option<usize>,
    typed: bool,
) {
    let Some(caret) = caret else {
        *slot = None;
        return;
    };
    let Some((start, word)) = word_before(text, caret) else {
        *slot = None;
        return;
    };
    if !typed {
        // The caret moved without typing: keep the list only while it still sits on the word.
        if slot
            .as_ref()
            .is_some_and(|open| open.start != start || open.prefix != word)
        {
            *slot = None;
        }
        return;
    }
    if word.chars().count() < 2 {
        *slot = None;
        return;
    }
    index.refresh_for(text, revision, start);
    let items = candidates(index, word);
    if items.is_empty() {
        *slot = None;
        return;
    }
    let selected = slot
        .as_ref()
        .and_then(|open| open.items.get(open.selected))
        .and_then(|current| items.iter().position(|item| item == current))
        .unwrap_or(0);
    *slot = Some(Completion {
        start,
        prefix: word.to_owned(),
        items,
        selected,
    });
}

/// Paint the list under `caret` (screen rect). Returns the clicked item.
pub(crate) fn show(
    ctx: &egui::Context,
    id: egui::Id,
    caret: egui::Rect,
    completion: &mut Completion,
) -> Option<String> {
    let mut picked = None;
    let font = crate::theme::editor_font();
    egui::Area::new(id)
        .order(egui::Order::Foreground)
        .fixed_pos(caret.left_bottom() + egui::vec2(-4.0, 2.0))
        .interactable(true)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(c_bg_elevated())
                .stroke(egui::Stroke::new(1.0, c_border()))
                .corner_radius(RADIUS_CHIP)
                .inner_margin(egui::Margin::same(3))
                .shadow(egui::Shadow {
                    offset: [0, 4],
                    blur: 12,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(60),
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let width = completion
                        .items
                        .iter()
                        .map(|item| {
                            ui.fonts_mut(|fonts| {
                                fonts
                                    .layout_no_wrap(item.clone(), font.clone(), c_text())
                                    .rect
                                    .width()
                            })
                        })
                        .fold(120.0f32, f32::max)
                        + 24.0;
                    let row_h = crate::theme::editor_line_height() + 4.0;
                    for (index, item) in completion.items.iter().enumerate() {
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(width, row_h), egui::Sense::click());
                        let selected = index == completion.selected;
                        if selected || response.hovered() {
                            ui.painter().rect_filled(
                                rect,
                                egui::CornerRadius::same(3),
                                if selected {
                                    c_row_active()
                                } else {
                                    c_row_hover()
                                },
                            );
                        }
                        // The typed part in the accent color, like Sublime's match highlight.
                        let mut job = egui::text::LayoutJob::default();
                        let matched = item
                            .to_lowercase()
                            .find(&completion.prefix.to_lowercase())
                            .map(|at| at..at + completion.prefix.len())
                            .filter(|range| {
                                item.is_char_boundary(range.start)
                                    && item.is_char_boundary(range.end.min(item.len()))
                            });
                        let mut cursor = 0;
                        if let Some(range) = matched {
                            let end = range.end.min(item.len());
                            job.append(
                                &item[..range.start],
                                0.0,
                                egui::TextFormat::simple(font.clone(), c_text()),
                            );
                            job.append(
                                &item[range.start..end],
                                0.0,
                                egui::TextFormat::simple(font.clone(), c_accent()),
                            );
                            cursor = end;
                        }
                        job.append(
                            &item[cursor..],
                            0.0,
                            egui::TextFormat::simple(font.clone(), c_text()),
                        );
                        let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
                        ui.painter().galley(
                            egui::pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0),
                            galley,
                            c_text(),
                        );
                        if response.clicked() {
                            picked = Some(item.clone());
                        }
                    }
                });
        });
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offers_buffer_words_by_prefix_then_substring() {
        let mut index = WordIndex::default();
        let text = "ordered = sorted(values)\nreturn ordered[mid] + order_total + reordered + or";
        index.refresh(text, 1);
        let found = candidates(&index, "ord");
        assert_eq!(found[0], "ordered");
        assert!(found.contains(&"order_total".to_owned()));
        assert_eq!(found.last().unwrap(), "reordered");
        assert_eq!(word_before(text, text.len()), Some((text.len() - 2, "or")));
        assert_eq!(word_before("abc def", 2), None);
    }

    #[test]
    fn list_opens_while_typing_and_closes_on_other_input() {
        let mut slot = None;
        let mut index = WordIndex::default();
        let text = "values valid va";
        update(&mut slot, &mut index, text, 1, Some(text.len()), true);
        let open = slot.clone().unwrap();
        assert_eq!(open.items, vec!["valid", "values"]);
        update(&mut slot, &mut index, "values valid va ", 2, Some(16), true);
        assert!(slot.is_none());
    }
}
