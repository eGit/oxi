//! Code folding by indentation, as Sublime does for any syntax: a line folds the lines after it
//! that are indented deeper (blank lines inside included, trailing ones not). The closing line
//! (`}` at the opening line's depth) stays visible.
//!
//! A fold hides whole logical lines; the editor lays them out as zero-height rows (see
//! [`super::line_layout::collapse`]), so text, caret mapping and undo stay untouched.

use std::ops::Range;

use eframe::egui;

/// Fold regions of one text revision: `ends[line]` is the last line a fold at `line` would hide.
#[derive(Default)]
pub(crate) struct FoldMap {
    pub revision: Option<u64>,
    pub ends: Vec<Option<usize>>,
    /// Indentation depth of each line in columns (`None` for blank lines).
    pub depth: Vec<Option<usize>>,
}

const TAB_WIDTH: usize = 4;

pub(crate) fn line_depths(text: &str) -> Vec<Option<usize>> {
    text.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            let rest = line.trim_start_matches([' ', '\t']);
            if rest.trim().is_empty() {
                return None;
            }
            let mut depth = 0;
            for byte in line[..line.len() - rest.len()].bytes() {
                depth += if byte == b'\t' {
                    TAB_WIDTH - depth % TAB_WIDTH
                } else {
                    1
                };
            }
            Some(depth)
        })
        .collect()
}

/// For each line, the last line of the deeper-indented block right after it, if any.
pub(crate) fn fold_ends(depths: &[Option<usize>]) -> Vec<Option<usize>> {
    let mut ends = vec![None; depths.len()];
    // Open candidates: (line, depth), depths strictly increasing.
    let mut open: Vec<(usize, usize)> = Vec::new();
    let mut last_content = None;
    for (line, depth) in depths.iter().enumerate() {
        let Some(depth) = *depth else {
            continue;
        };
        while let Some(&(start, start_depth)) = open.last() {
            if start_depth < depth {
                break;
            }
            open.pop();
            if let Some(last) = last_content
                && last > start
            {
                ends[start] = Some(last);
            }
        }
        open.push((line, depth));
        last_content = Some(line);
    }
    for (start, _) in open {
        if let Some(last) = last_content
            && last > start
        {
            ends[start] = Some(last);
        }
    }
    ends
}

impl FoldMap {
    pub(crate) fn refresh(&mut self, text: &str, revision: u64) {
        if self.revision == Some(revision) {
            return;
        }
        self.depth = line_depths(text);
        self.ends = fold_ends(&self.depth);
        self.revision = Some(revision);
    }

    /// The innermost foldable block holding `line` (a line that folds itself counts).
    pub(crate) fn region_at(&self, line: usize) -> Option<Range<usize>> {
        if let Some(Some(end)) = self.ends.get(line) {
            return Some(line..*end + 1);
        }
        (0..line.min(self.ends.len())).rev().find_map(|start| {
            self.ends[start]
                .filter(|end| *end >= line)
                .map(|end| start..end + 1)
        })
    }

    /// Every block opened at indentation level `level` (1 = top level), Cmd+K Cmd+<level>.
    pub(crate) fn regions_at_level(&self, level: usize, unit: usize) -> Vec<Range<usize>> {
        let want = (level.saturating_sub(1)) * unit.max(1);
        self.ends
            .iter()
            .enumerate()
            .filter_map(|(start, end)| {
                let end = (*end)?;
                (self.depth[start] == Some(want)).then_some(start..end + 1)
            })
            .collect()
    }
}

/// Folded blocks of a document: line ranges `start..end`, where `start` stays visible and
/// `start + 1..end` is hidden. Sorted, never nested (folding an outer block absorbs inner ones).
#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Folds {
    pub ranges: Vec<Range<usize>>,
    /// Bumped on every change, for layout caches.
    pub generation: u64,
}

impl Folds {
    pub(crate) fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub(crate) fn fold(&mut self, region: Range<usize>) {
        if region.len() < 2 {
            return;
        }
        self.ranges
            .retain(|other| !(other.start >= region.start && other.end <= region.end));
        if self
            .ranges
            .iter()
            .any(|other| other.start <= region.start && other.end >= region.end)
        {
            return;
        }
        let at = self
            .ranges
            .partition_point(|other| other.start < region.start);
        self.ranges.insert(at, region);
        self.generation += 1;
    }

    /// Unfold the block starting on `line`, or the one hiding it.
    pub(crate) fn unfold_at(&mut self, line: usize) -> bool {
        let before = self.ranges.len();
        self.ranges
            .retain(|range| !(range.start == line || (range.start < line && line < range.end)));
        let changed = self.ranges.len() != before;
        if changed {
            self.generation += 1;
        }
        changed
    }

    pub(crate) fn clear(&mut self) {
        if !self.ranges.is_empty() {
            self.ranges.clear();
            self.generation += 1;
        }
    }

    pub(crate) fn starting_at(&self, line: usize) -> Option<&Range<usize>> {
        self.ranges.iter().find(|range| range.start == line)
    }

    /// The fold hiding `line`, if any.
    pub(crate) fn hiding(&self, line: usize) -> Option<&Range<usize>> {
        self.ranges
            .iter()
            .find(|range| range.start < line && line < range.end)
    }

    /// Hidden line ranges, for the layout.
    pub(crate) fn hidden(&self) -> Vec<Range<usize>> {
        self.ranges
            .iter()
            .map(|range| range.start + 1..range.end)
            .collect()
    }

    /// Follow an edit at `line` that changed the line count by `delta`: later folds move, a
    /// fold the edit happened inside opens, and folds past the end are dropped.
    pub(crate) fn shift(&mut self, line: usize, delta: isize, line_count: usize) {
        let before = self.ranges.clone();
        self.ranges.retain(|range| {
            !((range.start < line && line < range.end) || (range.start == line && delta != 0))
        });
        for range in &mut self.ranges {
            if range.start > line {
                let start = range.start as isize + delta;
                let end = range.end as isize + delta;
                *range = start.max(0) as usize..end.max(0) as usize;
            }
        }
        self.ranges
            .retain(|range| range.end <= line_count && range.len() >= 2);
        if self.ranges != before {
            self.generation += 1;
        }
    }
}

/// Fold keys: Cmd+Alt+[ / ] (Ctrl+Shift+[ / ] elsewhere), and after Cmd+K: 0 or J unfolds
/// all, 1–9 folds every block at that level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FoldCommand {
    Fold,
    Unfold,
    UnfoldAll,
    Level(usize),
}

pub(crate) fn take_fold_command(
    input: &mut egui::InputState,
    chord_k: bool,
) -> Option<FoldCommand> {
    use egui::{Event, Key};
    let mac = cfg!(target_os = "macos");
    let command =
        |key: Key, physical: Option<Key>, modifiers: egui::Modifiers| -> Option<FoldCommand> {
            let physical = physical.unwrap_or(key);
            let bracket = if mac {
                modifiers.mac_cmd && modifiers.alt && !modifiers.shift && !modifiers.ctrl
            } else {
                modifiers.ctrl && modifiers.shift && !modifiers.alt
            };
            if bracket {
                return match physical {
                    Key::OpenBracket => Some(FoldCommand::Fold),
                    Key::CloseBracket => Some(FoldCommand::Unfold),
                    _ => None,
                };
            }
            let cmd = modifiers.command && !modifiers.alt && !modifiers.shift;
            if chord_k && cmd {
                return match physical {
                    Key::Num0 | Key::J => Some(FoldCommand::UnfoldAll),
                    Key::Num1 => Some(FoldCommand::Level(1)),
                    Key::Num2 => Some(FoldCommand::Level(2)),
                    Key::Num3 => Some(FoldCommand::Level(3)),
                    Key::Num4 => Some(FoldCommand::Level(4)),
                    Key::Num5 => Some(FoldCommand::Level(5)),
                    Key::Num6 => Some(FoldCommand::Level(6)),
                    Key::Num7 => Some(FoldCommand::Level(7)),
                    Key::Num8 => Some(FoldCommand::Level(8)),
                    Key::Num9 => Some(FoldCommand::Level(9)),
                    _ => None,
                };
            }
            None
        };
    let position = input.events.iter().position(|event| {
        matches!(event, Event::Key { key, physical_key, pressed: true, modifiers, .. }
            if command(*key, *physical_key, *modifiers).is_some())
    })?;
    let Event::Key {
        key,
        physical_key,
        modifiers,
        ..
    } = input.events.remove(position)
    else {
        return None;
    };
    command(key, physical_key, modifiers)
}

/// Bookmark keys: Cmd+F2 (Ctrl+F2) toggles, F2 / Shift+F2 go to the next / previous one,
/// Cmd+Shift+F2 clears them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BookmarkCommand {
    Toggle,
    Next,
    Previous,
    Clear,
}

pub(crate) fn take_bookmark_command(input: &mut egui::InputState) -> Option<BookmarkCommand> {
    use egui::{Event, Key};
    let command = |modifiers: egui::Modifiers| -> Option<BookmarkCommand> {
        let cmd = modifiers.command && !modifiers.alt;
        Some(match (cmd, modifiers.shift) {
            (true, false) => BookmarkCommand::Toggle,
            (true, true) => BookmarkCommand::Clear,
            (false, false) if modifiers.is_none() => BookmarkCommand::Next,
            (false, true) if !modifiers.alt && !modifiers.ctrl => BookmarkCommand::Previous,
            _ => return None,
        })
    };
    let position = input.events.iter().position(|event| {
        matches!(event, Event::Key { key: Key::F2, pressed: true, modifiers, .. } if command(*modifiers).is_some())
    })?;
    let Event::Key { modifiers, .. } = input.events.remove(position) else {
        return None;
    };
    command(modifiers)
}

/// Move bookmarks after an edit at `line` that changed the line count by `delta`.
pub(crate) fn shift_lines(lines: &mut Vec<usize>, line: usize, delta: isize, line_count: usize) {
    for bookmark in lines.iter_mut() {
        if *bookmark > line {
            *bookmark = (*bookmark as isize + delta).max(line as isize) as usize;
        }
    }
    lines.retain(|bookmark| *bookmark < line_count);
    lines.dedup();
}

/// Byte offset where logical line `line` starts (the text's end past the last line).
pub(crate) fn line_start_byte(text: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }
    memchr::memchr_iter(b'\n', text.as_bytes())
        .nth(line - 1)
        .map_or(text.len(), |newline| newline + 1)
}

/// Byte offset where logical line `line` ends (before its newline).
pub(crate) fn line_end_byte(text: &str, line: usize) -> usize {
    let start = line_start_byte(text, line);
    text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folds_by_indentation_and_keeps_the_closing_line() {
        let text = "fn a() {\n    x;\n\n    if y {\n        z;\n    }\n}\n\nfn b() {}\n";
        let depths = line_depths(text);
        let ends = fold_ends(&depths);
        assert_eq!(ends[0], Some(5));
        assert_eq!(ends[3], Some(4));
        assert_eq!(ends[1], None);
        assert_eq!(ends[6], None);
        let mut map = FoldMap::default();
        map.refresh(text, 1);
        assert_eq!(map.region_at(4), Some(3..5));
        assert_eq!(map.region_at(1), Some(0..6));
        assert_eq!(map.regions_at_level(1, 4), vec![0..6]);
        assert_eq!(map.regions_at_level(2, 4), vec![3..5]);
    }

    #[test]
    fn python_blocks_end_at_their_last_line() {
        let ends = fold_ends(&line_depths("def f():\n    a\n    b\n\nx = 1\n"));
        assert_eq!(ends[0], Some(2));
    }

    #[test]
    fn outer_folds_absorb_inner_ones_and_edits_shift_them() {
        let mut folds = Folds::default();
        folds.fold(3..5);
        folds.fold(0..6);
        assert_eq!(folds.ranges, vec![0..6]);
        folds.fold(2..4);
        assert_eq!(folds.ranges, vec![0..6]);
        assert_eq!(folds.hidden(), vec![1..6]);
        assert!(
            folds.hiding(3).is_some() && folds.hiding(0).is_none() && folds.hiding(6).is_none()
        );

        let mut folds = Folds::default();
        folds.fold(10..14);
        folds.fold(20..22);
        folds.shift(2, 3, 100);
        assert_eq!(folds.ranges, vec![13..17, 23..25]);
        folds.shift(15, 1, 100);
        assert_eq!(folds.ranges, vec![24..26]);
        assert!(folds.unfold_at(24) && folds.is_empty());
    }
}
