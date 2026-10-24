//! Sublime's typing model: multiple selections, auto-paired brackets and quotes, auto-indent on
//! Enter, tab stops, whole-line copy with an empty selection.
//!
//! egui's `TextEdit` holds one selection, the primary. The others live in
//! [`DocumentView::extra`] as byte ranges. While there are extras, every key that edits or moves
//! is taken out of the input and applied here to all selections at once; with a single caret only
//! typing goes through here (for the pairing and indentation rules) and `TextEdit` keeps moving
//! the caret itself, so wrapped rows, IME and mouse selection behave as before.
//!
//! The edit functions are pure, over the text and sorted byte selections.

use std::ops::Range;

use eframe::egui::{self, Event, Key, Modifiers};

use super::editor_text::{byte_index, char_index};

/// One selection, in bytes. `head` is where the caret is; `anchor == head` for a bare caret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) struct Sel {
    pub anchor: usize,
    pub head: usize,
}

impl Sel {
    pub(crate) fn caret(at: usize) -> Self {
        Self {
            anchor: at,
            head: at,
        }
    }

    pub(crate) fn new(anchor: usize, head: usize) -> Self {
        Self { anchor, head }
    }

    pub(crate) fn range(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.anchor == self.head
    }
}

/// Per-document editor state that is not text: extra selections, folds, bookmarks.
#[derive(Default)]
pub(crate) struct DocumentView {
    /// Selections besides `TextEdit`'s own (the primary), valid for `extra_revision`.
    pub extra: Vec<Sel>,
    pub extra_revision: u64,
    /// The word Cmd+D selected from a bare caret: adding occurrences of it matches whole words.
    pub whole_word_from: Option<Range<usize>>,
    /// Whether the current run of Cmd+D presses matches whole words.
    pub whole_word: bool,
    /// The text of the last copy/cut of whole lines (empty selection), so pasting it goes in
    /// above the caret's line, like Sublime.
    pub line_clip: Option<String>,
    /// Folded blocks, and the fold regions of the current text.
    pub folds: super::folding::Folds,
    pub fold_map: super::folding::FoldMap,
    /// Text revision and line count the folds were last placed for.
    pub fold_revision: u64,
    pub fold_line_count: usize,
    /// The primary caret's logical line on the last frame.
    pub last_caret_line: usize,
    /// Indentation unit chosen in the status bar, instead of the one guessed from the text.
    pub indent_override: Option<String>,
    /// Syntax chosen in the status bar, instead of the one from the file extension.
    pub syntax_override: Option<&'static str>,
    /// Bookmarked logical lines, sorted (Sublime's Cmd+F2 / F2).
    pub bookmarks: Vec<usize>,
    /// Where an Alt+drag (column selection) started, in galley coordinates.
    pub column_drag: Option<egui::Vec2>,
    /// The open word completion list, the buffer's words, and an item clicked in the list.
    pub completion: Option<super::completion::Completion>,
    pub words: super::completion::WordIndex,
    pub completion_pick: Option<String>,
    /// Where the text was laid out on screen last frame (for tests that point at text).
    pub galley_pos: egui::Pos2,
}

/// All selections, sorted and merged, with the index of the primary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Selections {
    pub sels: Vec<Sel>,
    pub primary: usize,
}

impl Selections {
    pub(crate) fn new(primary: Sel, extra: &[Sel]) -> Self {
        let mut sels = extra.to_vec();
        sels.push(primary);
        let mut selections = Self {
            primary: sels.len() - 1,
            sels,
        };
        selections.normalize();
        selections
    }

    pub(crate) fn primary(&self) -> Sel {
        self.sels[self.primary]
    }

    pub(crate) fn extra(&self) -> Vec<Sel> {
        self.sels
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != self.primary)
            .map(|(_, sel)| *sel)
            .collect()
    }

    /// Sort by position and merge selections that overlap (or carets that coincide).
    pub(crate) fn normalize(&mut self) {
        let primary = self.sels[self.primary];
        let mut indexed: Vec<(Sel, bool)> = self
            .sels
            .iter()
            .enumerate()
            .map(|(index, sel)| (*sel, index == self.primary))
            .collect();
        indexed.sort_by_key(|(sel, _)| (sel.range().start, sel.range().end));
        let mut merged: Vec<(Sel, bool)> = Vec::with_capacity(indexed.len());
        for (sel, is_primary) in indexed {
            if let Some((last, last_primary)) = merged.last_mut() {
                let (a, b) = (last.range(), sel.range());
                // Overlapping, or two carets at the same spot. A caret touching a selection's
                // edge stays separate, as in Sublime.
                let overlap =
                    b.start < a.end || (a.is_empty() && b.is_empty() && a.start == b.start);
                if overlap {
                    let start = a.start.min(b.start);
                    let end = a.end.max(b.end);
                    let forward = last.head >= last.anchor;
                    *last = if forward {
                        Sel::new(start, end)
                    } else {
                        Sel::new(end, start)
                    };
                    *last_primary |= is_primary;
                    continue;
                }
            }
            merged.push((sel, is_primary));
        }
        self.primary = merged
            .iter()
            .position(|(_, is_primary)| *is_primary)
            .unwrap_or_else(|| {
                merged
                    .iter()
                    .position(|(sel, _)| sel.range().contains(&primary.head))
                    .unwrap_or(merged.len() - 1)
            });
        self.sels = merged.into_iter().map(|(sel, _)| sel).collect();
    }
}

/// `range` of the old text becomes `insert`; the selection afterwards is
/// `anchor`/`head` bytes from the start of the inserted text.
struct Edit {
    range: Range<usize>,
    insert: String,
    anchor: usize,
    head: usize,
}

impl Edit {
    fn replace(range: Range<usize>, insert: String) -> Self {
        let caret = insert.len();
        Self {
            range,
            insert,
            anchor: caret,
            head: caret,
        }
    }
}

/// Apply one optional edit per selection (in selection order) and place the selections.
fn apply_edits(
    text: &str,
    selections: &Selections,
    edits: Vec<Option<Edit>>,
) -> (String, Selections) {
    let mut out = String::with_capacity(text.len() + 16);
    let mut cursor = 0;
    let mut sels = Vec::with_capacity(selections.sels.len());
    for (sel, edit) in selections.sels.iter().zip(edits) {
        match edit {
            Some(edit) => {
                let start = edit.range.start.max(cursor).min(text.len());
                out.push_str(&text[cursor..start]);
                let new_start = out.len();
                out.push_str(&edit.insert);
                cursor = edit.range.end.max(start).min(text.len());
                sels.push(Sel::new(new_start + edit.anchor, new_start + edit.head));
            }
            None => {
                let shift = |position: usize| {
                    (out.len() as isize + position as isize - cursor as isize).max(0) as usize
                };
                sels.push(Sel::new(
                    shift(sel.anchor.max(cursor)),
                    shift(sel.head.max(cursor)),
                ));
            }
        }
    }
    out.push_str(&text[cursor..]);
    let mut selections = Selections {
        sels,
        primary: selections.primary,
    };
    selections.normalize();
    (out, selections)
}

fn prev_char(text: &str, at: usize) -> Option<char> {
    text[..at].chars().next_back()
}

fn next_char(text: &str, at: usize) -> Option<char> {
    text[at..].chars().next()
}

fn line_start(text: &str, at: usize) -> usize {
    text[..at].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(text: &str, at: usize) -> usize {
    text[at..].find('\n').map_or(text.len(), |index| at + index)
}

fn leading_whitespace(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn closer(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' | '\'' | '`' => Some(open),
        _ => None,
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where a typed pair may open: before nothing, white space or a closing character.
fn pair_may_open(next: Option<char>) -> bool {
    next.is_none_or(|c| c.is_whitespace() || matches!(c, ')' | ']' | '}' | ',' | ';' | ':'))
}

/// Type `typed` into every selection.
pub(crate) fn insert(
    text: &str,
    selections: &Selections,
    typed: &str,
    unit: &str,
    language: &str,
) -> (String, Selections) {
    // Rust's `'` mostly opens a lifetime or a char literal typed in one go; pairing it gets in
    // the way.
    let pair_single_quote = language != "rs";
    let single = {
        let mut chars = typed.chars();
        chars.next().filter(|_| chars.next().is_none())
    };
    let edits = selections
        .sels
        .iter()
        .map(|sel| {
            let range = sel.range();
            let Some(c) = single else {
                return Some(Edit::replace(range, typed.to_owned()));
            };
            let prev = prev_char(text, range.start);
            let next = next_char(text, range.end);
            let is_quote = matches!(c, '"' | '\'' | '`');
            // Typing the closing character that is already there steps over it.
            if range.is_empty() && next == Some(c) && (matches!(c, ')' | ']' | '}') || is_quote) {
                return Some(Edit {
                    range: range.start..range.start,
                    insert: String::new(),
                    anchor: c.len_utf8(),
                    head: c.len_utf8(),
                });
            }
            if let Some(close) = closer(c).filter(|_| c != '\'' || pair_single_quote) {
                if !range.is_empty() {
                    // Wrap the selection, keeping it selected inside.
                    let inner = &text[range.clone()];
                    let open_len = c.len_utf8();
                    let forward = sel.head >= sel.anchor;
                    let (anchor, head) = if forward {
                        (open_len, open_len + inner.len())
                    } else {
                        (open_len + inner.len(), open_len)
                    };
                    return Some(Edit {
                        range,
                        insert: format!("{c}{inner}{close}"),
                        anchor,
                        head,
                    });
                }
                let quote_ok = !is_quote || prev.is_none_or(|p| !is_word(p) && p != c && p != '\\');
                if range.is_empty() && pair_may_open(next) && quote_ok {
                    return Some(Edit {
                        range,
                        insert: format!("{c}{close}"),
                        anchor: c.len_utf8(),
                        head: c.len_utf8(),
                    });
                }
            }
            // A closing bracket typed on a blank line moves back one indentation level.
            if matches!(c, ')' | ']' | '}') && range.is_empty() {
                let start = line_start(text, range.start);
                let before = &text[start..range.start];
                if !before.is_empty() && before.trim().is_empty() {
                    let dedented = before
                        .strip_suffix(unit)
                        .or_else(|| before.strip_suffix('\t'))
                        .unwrap_or(before);
                    return Some(Edit::replace(start..range.start, format!("{dedented}{c}")));
                }
            }
            Some(Edit::replace(range, typed.to_owned()))
        })
        .collect();
    apply_edits(text, selections, edits)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum By {
    Char,
    Word,
    Line,
}

fn word_left(text: &str, at: usize) -> usize {
    let mut chars = text[..at].char_indices().rev().peekable();
    let mut position = at;
    while let Some(&(index, c)) = chars.peek() {
        if c.is_whitespace() && c != '\n' {
            position = index;
            chars.next();
        } else {
            break;
        }
    }
    let Some(&(_, first)) = chars.peek() else {
        return position;
    };
    if first == '\n' {
        return if position == at {
            position - 1
        } else {
            position
        };
    }
    let word = is_word(first);
    while let Some(&(index, c)) = chars.peek() {
        if c != '\n' && !c.is_whitespace() && is_word(c) == word {
            position = index;
            chars.next();
        } else {
            break;
        }
    }
    position
}

fn word_right(text: &str, at: usize) -> usize {
    let mut position = at;
    let mut chars = text[at..].char_indices().peekable();
    while let Some(&(offset, c)) = chars.peek() {
        if c.is_whitespace() && c != '\n' {
            position = at + offset + c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    let Some(&(_, first)) = chars.peek() else {
        return position;
    };
    if first == '\n' {
        return if position == at {
            position + 1
        } else {
            position
        };
    }
    let word = is_word(first);
    while let Some(&(offset, c)) = chars.peek() {
        if c != '\n' && !c.is_whitespace() && is_word(c) == word {
            position = at + offset + c.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    position
}

/// Backspace (`forward == false`) or Delete in every selection.
pub(crate) fn delete(
    text: &str,
    selections: &Selections,
    forward: bool,
    by: By,
    unit: &str,
) -> (String, Selections) {
    let edits = selections
        .sels
        .iter()
        .map(|sel| {
            let range = sel.range();
            if !range.is_empty() {
                return Some(Edit::replace(range, String::new()));
            }
            let at = range.start;
            let span = if forward {
                if at >= text.len() {
                    return None;
                }
                let end = match by {
                    By::Char => at + next_char(text, at).map_or(0, char::len_utf8),
                    By::Word => word_right(text, at),
                    By::Line => {
                        let end = line_end(text, at);
                        if end == at { at + 1 } else { end }
                    }
                };
                at..end.min(text.len())
            } else {
                if at == 0 {
                    return None;
                }
                let start = match by {
                    By::Char => {
                        let prev = prev_char(text, at).map_or(0, char::len_utf8);
                        let start = at - prev;
                        // Between an auto-paired couple: remove both.
                        if let (Some(open), Some(next)) = (prev_char(text, at), next_char(text, at))
                            && closer(open) == Some(next)
                        {
                            return Some(Edit::replace(start..at + next.len_utf8(), String::new()));
                        }
                        // In leading spaces, back to the previous tab stop.
                        let line = line_start(text, at);
                        let before = &text[line..at];
                        if unit != "\t" && !before.is_empty() && before.bytes().all(|b| b == b' ') {
                            let width = unit.len().max(1);
                            let remove = match before.len() % width {
                                0 => width,
                                rest => rest,
                            };
                            at - remove
                        } else {
                            start
                        }
                    }
                    By::Word => word_left(text, at),
                    By::Line => {
                        let start = line_start(text, at);
                        if start == at { at - 1 } else { start }
                    }
                };
                start..at
            };
            Some(Edit::replace(span, String::new()))
        })
        .collect();
    apply_edits(text, selections, edits)
}

/// Enter: a new line indented like the current one, one level deeper after an opening bracket
/// (or a `:` in Python), with the closing bracket pushed to its own line when the caret sits
/// between a pair.
pub(crate) fn newline(
    text: &str,
    selections: &Selections,
    unit: &str,
    language: &str,
) -> (String, Selections) {
    let edits = selections
        .sels
        .iter()
        .map(|sel| {
            let range = sel.range();
            let start = line_start(text, range.start);
            let line_before = &text[start..range.start];
            let indent = leading_whitespace(line_before).to_owned();
            let trimmed = line_before.trim_end();
            let opens = trimmed
                .chars()
                .next_back()
                .filter(|c| matches!(c, '{' | '(' | '[') || (*c == ':' && language == "py"));
            let next = next_char(text, range.end);
            if let Some(open) = opens
                && next.is_some_and(|next| closer(open) == Some(next))
            {
                let inner = format!("\n{indent}{unit}");
                let caret = inner.len();
                return Some(Edit {
                    range,
                    insert: format!("{inner}\n{indent}"),
                    anchor: caret,
                    head: caret,
                });
            }
            let deeper = if opens.is_some() { unit } else { "" };
            // Leaving a line that holds only indentation takes the indentation along.
            let rest_of_line = &text[range.end..line_end(text, range.end)];
            if !line_before.is_empty()
                && line_before.trim().is_empty()
                && rest_of_line.trim().is_empty()
            {
                return Some(Edit::replace(start..range.end, format!("\n{indent}")));
            }
            // Text after the caret starts at the new indentation, without its old spaces.
            let skip = rest_of_line.len() - rest_of_line.trim_start_matches([' ', '\t']).len();
            Some(Edit::replace(
                range.start..range.end + skip,
                format!("\n{indent}{deeper}"),
            ))
        })
        .collect();
    apply_edits(text, selections, edits)
}

/// Tab / Shift+Tab: indent or outdent the lines of every selection that spans lines (or all of
/// them with Shift); otherwise fill to the next tab stop at each caret.
pub(crate) fn tab(
    text: &str,
    selections: &Selections,
    unit: &str,
    outdent: bool,
) -> (String, Selections) {
    let multiline = selections
        .sels
        .iter()
        .any(|sel| text[sel.range()].contains('\n'));
    if !outdent && !multiline {
        let edits = selections
            .sels
            .iter()
            .map(|sel| {
                let range = sel.range();
                let insert = if unit == "\t" {
                    "\t".to_owned()
                } else {
                    let column = text[line_start(text, range.start)..range.start]
                        .chars()
                        .count();
                    let width = unit.len().max(1);
                    " ".repeat(width - column % width)
                };
                Some(Edit::replace(range, insert))
            })
            .collect();
        return apply_edits(text, selections, edits);
    }
    // Every line touched by a selection, once.
    let mut lines: Vec<usize> = Vec::new();
    for sel in &selections.sels {
        let range = sel.range();
        let mut last = range.end;
        if range.end > range.start && text[..range.end].ends_with('\n') {
            last -= 1;
        }
        let mut start = line_start(text, range.start);
        loop {
            if lines.last() != Some(&start) {
                lines.push(start);
            }
            let end = line_end(text, start);
            if end >= last.max(start) || end >= text.len() {
                break;
            }
            start = end + 1;
        }
    }
    lines.sort_unstable();
    lines.dedup();
    let mut changes: Vec<(Range<usize>, String)> = Vec::new();
    for start in lines {
        let line = &text[start..line_end(text, start)];
        if outdent {
            let remove = if line.starts_with('\t') {
                1
            } else {
                (line.len() - line.trim_start_matches(' ').len()).min(unit.len().max(1))
            };
            if remove > 0 {
                changes.push((start..start + remove, String::new()));
            }
        } else if !line.trim().is_empty() {
            changes.push((start..start, unit.to_owned()));
        }
    }
    let map = |position: usize, sticky: bool| {
        let mut delta = 0isize;
        for (range, insert) in &changes {
            if position < range.start || (sticky && position == range.start && insert.is_empty()) {
                break;
            }
            if position < range.end {
                return (range.start as isize + delta) as usize;
            }
            delta += insert.len() as isize - range.len() as isize;
        }
        (position as isize + delta) as usize
    };
    let mut out = String::with_capacity(text.len() + changes.len() * unit.len());
    let mut cursor = 0;
    for (range, insert) in &changes {
        out.push_str(&text[cursor..range.start]);
        out.push_str(insert);
        cursor = range.end;
    }
    out.push_str(&text[cursor..]);
    let mut moved = selections.clone();
    for sel in &mut moved.sels {
        *sel = Sel::new(map(sel.anchor, false), map(sel.head, false));
    }
    moved.normalize();
    (out, moved)
}

/// Move (or with `extend`, stretch) every selection's caret sideways.
pub(crate) fn move_horizontal(
    text: &str,
    selections: &Selections,
    forward: bool,
    by: By,
    extend: bool,
) -> Selections {
    let mut moved = selections.clone();
    for sel in &mut moved.sels {
        let range = sel.range();
        if !extend && !range.is_empty() && by == By::Char {
            *sel = Sel::caret(if forward { range.end } else { range.start });
            continue;
        }
        let at = sel.head;
        let head = match (by, forward) {
            (By::Char, true) => at + next_char(text, at).map_or(0, char::len_utf8),
            (By::Char, false) => at - prev_char(text, at).map_or(0, char::len_utf8),
            (By::Word, true) => word_right(text, at),
            (By::Word, false) => word_left(text, at),
            (By::Line, true) => line_end(text, at),
            (By::Line, false) => {
                // Sublime's Home: first non-blank character, then column 0.
                let start = line_start(text, at);
                let first = start + leading_whitespace(&text[start..line_end(text, start)]).len();
                if at == first { start } else { first }
            }
        };
        *sel = Sel::new(if extend { sel.anchor } else { head }, head);
    }
    moved.normalize();
    moved
}

/// Move every caret one line up or down, keeping its column.
pub(crate) fn move_vertical(
    text: &str,
    selections: &Selections,
    down: bool,
    extend: bool,
) -> Selections {
    let mut moved = selections.clone();
    for sel in &mut moved.sels {
        let head = vertical(text, sel.head, down).unwrap_or(if down { text.len() } else { 0 });
        *sel = Sel::new(if extend { sel.anchor } else { head }, head);
    }
    moved.normalize();
    moved
}

/// The same column on the line above/below `at`, if there is such a line.
fn vertical(text: &str, at: usize, down: bool) -> Option<usize> {
    let start = line_start(text, at);
    let column = text[start..at].chars().count();
    let target = if down {
        let end = line_end(text, at);
        (end < text.len()).then_some(end + 1)?
    } else {
        (start > 0).then(|| line_start(text, start - 1))?
    };
    let line = &text[target..line_end(text, target)];
    Some(
        target
            + line
                .char_indices()
                .nth(column)
                .map_or(line.len(), |(index, _)| index),
    )
}

/// Sublime's Ctrl+Shift+Up/Down: a new caret on the line above the topmost (or below the
/// bottommost) selection, at its column.
pub(crate) fn add_caret_vertical(text: &str, selections: &Selections, down: bool) -> Selections {
    let edge = if down {
        *selections.sels.last().expect("at least one selection")
    } else {
        selections.sels[0]
    };
    let mut grown = selections.clone();
    if let Some(at) = vertical(text, edge.head, down) {
        grown.sels.push(Sel::caret(at));
        grown.primary = grown.sels.len() - 1;
        grown.normalize();
    }
    grown
}

fn occurrences<'a>(
    text: &'a str,
    needle: &'a str,
    whole_word: bool,
) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(needle)
        .map(|(index, _)| index)
        .filter(move |&index| {
            !whole_word
                || (prev_char(text, index).is_none_or(|c| !is_word(c))
                    && next_char(text, index + needle.len()).is_none_or(|c| !is_word(c)))
        })
}

/// Sublime's Cmd+D on a selection: also select the next occurrence of the primary's text,
/// wrapping around the file and skipping occurrences already selected.
pub(crate) fn add_next_occurrence(
    text: &str,
    selections: &Selections,
    whole_word: bool,
) -> Option<Selections> {
    let primary = selections.primary();
    let needle = &text[primary.range()];
    if needle.is_empty() {
        return None;
    }
    let taken = |start: usize| selections.sels.iter().any(|sel| sel.range().start == start);
    let after = primary.range().end;
    let next = occurrences(text, needle, whole_word)
        .filter(|&index| index >= after && !taken(index))
        .chain(
            occurrences(text, needle, whole_word).filter(|&index| index < after && !taken(index)),
        )
        .next()?;
    let mut grown = selections.clone();
    grown.sels.push(Sel::new(next, next + needle.len()));
    grown.primary = grown.sels.len() - 1;
    grown.normalize();
    Some(grown)
}

/// Cmd+K Cmd+D: drop the primary selection and select the next occurrence instead.
pub(crate) fn skip_to_next_occurrence(
    text: &str,
    selections: &Selections,
    whole_word: bool,
) -> Option<Selections> {
    let grown = add_next_occurrence(text, selections, whole_word)?;
    let skipped = selections.primary();
    let mut sels: Vec<Sel> = Vec::new();
    let mut primary = 0;
    for (index, sel) in grown.sels.iter().enumerate() {
        if *sel == skipped {
            continue;
        }
        if index == grown.primary {
            primary = sels.len();
        }
        sels.push(*sel);
    }
    Some(Selections { sels, primary })
}

/// Sublime's Ctrl+Cmd+G / Alt+F3: select every occurrence of the primary's text (or of the word
/// at the caret).
pub(crate) fn select_all_occurrences(text: &str, selections: &Selections) -> Option<Selections> {
    let primary = selections.primary();
    let (range, whole_word) = if primary.is_empty() {
        (crate::code_nav::identifier_at(text, primary.head)?.1, true)
    } else {
        (primary.range(), false)
    };
    let needle = &text[range.clone()];
    let sels: Vec<Sel> = occurrences(text, needle, whole_word)
        .map(|index| Sel::new(index, index + needle.len()))
        .collect();
    let primary = sels
        .iter()
        .position(|sel| sel.range() == range)
        .unwrap_or(0);
    let mut all = Selections { sels, primary };
    all.normalize();
    Some(all)
}

/// Sublime's Cmd+Shift+L: one selection per line of every multi-line selection.
pub(crate) fn split_into_lines(text: &str, selections: &Selections) -> Selections {
    let mut sels = Vec::new();
    for sel in &selections.sels {
        let range = sel.range();
        if !text[range.clone()].contains('\n') {
            sels.push(*sel);
            continue;
        }
        let mut start = range.start;
        loop {
            let end = line_end(text, start).min(range.end);
            if start < end || end == range.end {
                sels.push(Sel::new(start, end));
            }
            if end >= range.end {
                break;
            }
            start = end + 1;
        }
    }
    let primary = sels.len().saturating_sub(1);
    let mut split = Selections { sels, primary };
    split.normalize();
    split
}

/// What Cmd+C copies: the selections joined by newlines, or the caret lines when nothing is
/// selected (`true` in the pair).
pub(crate) fn copy_text(text: &str, selections: &Selections) -> (String, bool) {
    if selections.sels.iter().all(Sel::is_empty) {
        let mut lines = String::new();
        let mut last = None;
        for sel in &selections.sels {
            let start = line_start(text, sel.head);
            if last == Some(start) {
                continue;
            }
            last = Some(start);
            let end = line_end(text, start);
            lines.push_str(&text[start..end]);
            lines.push('\n');
        }
        return (lines, true);
    }
    let parts: Vec<&str> = selections
        .sels
        .iter()
        .map(|sel| &text[sel.range()])
        .collect();
    (parts.join("\n"), false)
}

/// Cmd+X: cut the selections, or the whole caret lines when nothing is selected.
pub(crate) fn cut(text: &str, selections: &Selections) -> (String, Selections, String, bool) {
    let (copied, lines) = copy_text(text, selections);
    let edits = selections
        .sels
        .iter()
        .map(|sel| {
            if lines {
                let start = line_start(text, sel.head);
                let end = (line_end(text, start) + 1).min(text.len());
                Some(Edit::replace(start..end, String::new()))
            } else {
                Some(Edit::replace(sel.range(), String::new()))
            }
        })
        .collect();
    let (new_text, sels) = apply_edits(text, selections, edits);
    (new_text, sels, copied, lines)
}

/// Paste: one clipboard line per selection when the counts match (Sublime), a whole-line clip
/// above each caret's line, otherwise the clipboard at every selection.
pub(crate) fn paste(
    text: &str,
    selections: &Selections,
    clip: &str,
    whole_lines: bool,
) -> (String, Selections) {
    let pieces: Vec<&str> = clip
        .strip_suffix('\n')
        .unwrap_or(clip)
        .split('\n')
        .collect();
    let distribute = selections.sels.len() > 1 && pieces.len() == selections.sels.len();
    let edits = selections
        .sels
        .iter()
        .enumerate()
        .map(|(index, sel)| {
            if whole_lines && sel.is_empty() && !distribute {
                let start = line_start(text, sel.head);
                let offset = sel.head - start;
                return Some(Edit {
                    range: start..start,
                    insert: clip.to_owned(),
                    anchor: clip.len() + offset,
                    head: clip.len() + offset,
                });
            }
            let insert = if distribute { pieces[index] } else { clip };
            Some(Edit::replace(sel.range(), insert.to_owned()))
        })
        .collect();
    apply_edits(text, selections, edits)
}

/// The screen point of `line`/`column` (chars) in `document` as laid out last frame.
#[cfg(test)]
pub(crate) fn text_point(
    document: &crate::app::state::EditorDocument,
    line: usize,
    column: usize,
) -> Option<egui::Pos2> {
    let galley = document.layout_cache.geometry.as_ref()?;
    let start = super::folding::line_start_byte(&document.content, line);
    let byte = document.content[start..]
        .char_indices()
        .nth(column)
        .map_or(document.content.len(), |(offset, _)| start + offset);
    let rect = galley.pos_from_cursor(egui::text::CCursor::new(char_index(
        &document.content,
        byte,
    )));
    Some(rect.center() + document.view.galley_pos.to_vec2())
}

// --- egui wiring -------------------------------------------------------------------------

/// What [`handle_typing`] did this frame.
pub(crate) struct Typed {
    /// The primary caret, as a char index, to reveal.
    pub caret: usize,
    pub edited: bool,
}

/// Sublime's selection keys, checked before `TextEdit` and the other editor commands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectionCommand {
    SelectWord,
    AddNext,
    SkipNext,
    SelectAll,
    SplitLines,
    CaretAbove,
    CaretBelow,
}

/// Take one bound selection command out of this frame's input.
fn take_selection_command(
    input: &mut egui::InputState,
    has_selection: bool,
    chord_k: bool,
) -> Option<SelectionCommand> {
    let mac = cfg!(target_os = "macos");
    let position = input.events.iter().position(|event| {
        let Event::Key {
            key,
            pressed: true,
            modifiers,
            ..
        } = event
        else {
            return false;
        };
        selection_command(*key, *modifiers, mac, has_selection, chord_k).is_some()
    })?;
    let Event::Key { key, modifiers, .. } = input.events.remove(position) else {
        return None;
    };
    selection_command(key, modifiers, mac, has_selection, chord_k)
}

fn selection_command(
    key: Key,
    modifiers: Modifiers,
    mac: bool,
    has_selection: bool,
    chord_k: bool,
) -> Option<SelectionCommand> {
    use SelectionCommand::*;
    let cmd = modifiers.command && !modifiers.alt && (!mac || !modifiers.ctrl);
    if cmd && !modifiers.shift && key == Key::D {
        return Some(if !has_selection {
            SelectWord
        } else if chord_k {
            SkipNext
        } else {
            AddNext
        });
    }
    if cmd && modifiers.shift && key == Key::L {
        return Some(SplitLines);
    }
    if mac {
        if modifiers.mac_cmd
            && modifiers.ctrl
            && !modifiers.shift
            && !modifiers.alt
            && key == Key::G
        {
            return Some(SelectAll);
        }
        if modifiers.ctrl && modifiers.shift && !modifiers.mac_cmd && !modifiers.alt {
            return match key {
                Key::ArrowUp => Some(CaretAbove),
                Key::ArrowDown => Some(CaretBelow),
                _ => None,
            };
        }
    } else {
        if modifiers.alt && !modifiers.ctrl && !modifiers.shift && key == Key::F3 {
            return Some(SelectAll);
        }
        if modifiers.ctrl && modifiers.alt && !modifiers.shift {
            return match key {
                Key::ArrowUp => Some(CaretAbove),
                Key::ArrowDown => Some(CaretBelow),
                _ => None,
            };
        }
    }
    None
}

/// Run the typing model on the focused editor before `TextEdit` sees this frame's input.
/// `chord_k` says Cmd+K was the previous key (for Cmd+K Cmd+D).
pub(crate) fn handle_typing(
    ui: &egui::Ui,
    editor_id: egui::Id,
    content: &mut String,
    view: &mut DocumentView,
    revision: u64,
    language: &str,
    chord_k: bool,
) -> Option<Typed> {
    if view.extra_revision != revision {
        view.extra.clear();
    }
    if !ui.memory(|memory| memory.has_focus(editor_id)) {
        return None;
    }
    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), editor_id)?;
    let range = state.cursor.char_range()?;
    let to_byte = |content: &str, index: usize| byte_index(content, index);
    let primary = Sel::new(
        to_byte(content, range.secondary.index.0),
        to_byte(content, range.primary.index.0),
    );
    let mut selections = Selections::new(primary, &view.extra);
    // IME composition belongs to TextEdit.
    if ui.input(|input| {
        input
            .events
            .iter()
            .any(|event| matches!(event, Event::Ime(_)))
    }) {
        return None;
    }
    let unit = super::editor_commands::indent_unit(content);
    let mac = cfg!(target_os = "macos");
    let multi = selections.sels.len() > 1;
    let mut edited = false;
    let mut moved = false;
    let mut clipboard: Option<String> = None;

    let has_selection = !selections.primary().is_empty();
    if let Some(command) =
        ui.input_mut(|input| take_selection_command(input, has_selection, chord_k))
    {
        if matches!(
            command,
            SelectionCommand::AddNext | SelectionCommand::SkipNext
        ) && !multi
        {
            view.whole_word = view.whole_word_from == Some(selections.primary().range());
        }
        let next = match command {
            SelectionCommand::SelectWord => {
                let primary = selections.primary();
                crate::code_nav::identifier_at(content, primary.head).map(|(_, word)| {
                    view.whole_word_from = Some(word.clone());
                    Selections {
                        sels: vec![Sel::new(word.start, word.end)],
                        primary: 0,
                    }
                })
            }
            SelectionCommand::AddNext => add_next_occurrence(content, &selections, view.whole_word),
            SelectionCommand::SkipNext => {
                skip_to_next_occurrence(content, &selections, view.whole_word)
            }
            SelectionCommand::SelectAll => select_all_occurrences(content, &selections),
            SelectionCommand::SplitLines => Some(split_into_lines(content, &selections)),
            SelectionCommand::CaretAbove => Some(add_caret_vertical(content, &selections, false)),
            SelectionCommand::CaretBelow => Some(add_caret_vertical(content, &selections, true)),
        };
        if let Some(next) = next {
            selections = next;
            moved = true;
        }
    }

    // Undo and redo restore TextEdit's single selection.
    if ui.input(|input| {
        input.events.iter().any(|event| {
            matches!(event, Event::Key { key: Key::Z | Key::Y, pressed: true, modifiers, .. } if modifiers.command)
        })
    }) {
        view.extra.clear();
        return None;
    }

    let multi = selections.sels.len() > 1;
    let events = ui.input(|input| input.events.clone());
    let mut handled = vec![false; events.len()];
    for (index, event) in events.iter().enumerate() {
        match event {
            Event::Text(typed) if !typed.is_empty() && typed != "\n" && typed != "\r" => {
                let (text, sels) = insert(content, &selections, typed, &unit, language);
                *content = text;
                selections = sels;
                edited = true;
                handled[index] = true;
            }
            Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } => {
                let word = if mac { modifiers.alt } else { modifiers.ctrl };
                let line = mac && modifiers.mac_cmd;
                let plain = !modifiers.command && !modifiers.ctrl && !modifiers.alt;
                let by = if line {
                    By::Line
                } else if word {
                    By::Word
                } else {
                    By::Char
                };
                match key {
                    Key::Enter if plain && !modifiers.shift => {
                        let (text, sels) = newline(content, &selections, &unit, language);
                        *content = text;
                        selections = sels;
                        edited = true;
                        handled[index] = true;
                    }
                    Key::Tab if plain => {
                        let (text, sels) = tab(content, &selections, &unit, modifiers.shift);
                        *content = text;
                        selections = sels;
                        edited = true;
                        handled[index] = true;
                    }
                    Key::Backspace | Key::Delete if !modifiers.shift => {
                        // A single caret leaves word/line deletion to TextEdit.
                        if !multi && by != By::Char {
                            continue;
                        }
                        let (text, sels) =
                            delete(content, &selections, *key == Key::Delete, by, &unit);
                        if text != *content || sels != selections {
                            *content = text;
                            selections = sels;
                            edited = true;
                        }
                        handled[index] = true;
                    }
                    Key::ArrowLeft | Key::ArrowRight if multi => {
                        selections = move_horizontal(
                            content,
                            &selections,
                            *key == Key::ArrowRight,
                            by,
                            modifiers.shift,
                        );
                        moved = true;
                        handled[index] = true;
                    }
                    Key::Home | Key::End if multi => {
                        selections = move_horizontal(
                            content,
                            &selections,
                            *key == Key::End,
                            By::Line,
                            modifiers.shift,
                        );
                        moved = true;
                        handled[index] = true;
                    }
                    Key::ArrowUp | Key::ArrowDown
                        if multi && !modifiers.command && !modifiers.alt && !modifiers.ctrl =>
                    {
                        selections = move_vertical(
                            content,
                            &selections,
                            *key == Key::ArrowDown,
                            modifiers.shift,
                        );
                        moved = true;
                        handled[index] = true;
                    }
                    _ => {}
                }
            }
            Event::Copy if multi || selections.primary().is_empty() => {
                let (text, lines) = copy_text(content, &selections);
                view.line_clip = lines.then(|| text.clone());
                clipboard = Some(text);
                handled[index] = true;
            }
            Event::Cut if multi || selections.primary().is_empty() => {
                let (text, sels, copied, lines) = cut(content, &selections);
                view.line_clip = lines.then(|| copied.clone());
                *content = text;
                selections = sels;
                clipboard = Some(copied);
                edited = true;
                handled[index] = true;
            }
            Event::Paste(clip) => {
                let whole_lines = view.line_clip.as_deref() == Some(clip.as_str());
                if multi || (whole_lines && selections.primary().is_empty()) {
                    let (text, sels) = paste(content, &selections, clip, whole_lines);
                    *content = text;
                    selections = sels;
                    edited = true;
                    handled[index] = true;
                }
            }
            _ => {}
        }
    }
    if !(edited || moved || handled.iter().any(|h| *h)) {
        return None;
    }
    ui.input_mut(|input| {
        let mut index = 0;
        input.events.retain(|_| {
            let keep = !handled.get(index).copied().unwrap_or(false);
            index += 1;
            keep
        });
    });
    if let Some(text) = clipboard {
        ui.ctx().copy_text(text);
    }
    let primary = selections.primary();
    let anchor = char_index(content, primary.anchor);
    let head = char_index(content, primary.head);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(anchor),
            egui::text::CCursor::new(head),
        )));
    state.store(ui.ctx(), editor_id);
    view.extra = selections.extra();
    Some(Typed {
        caret: head,
        edited,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sels(list: &[(usize, usize)]) -> Selections {
        Selections {
            sels: list.iter().map(|&(a, h)| Sel::new(a, h)).collect(),
            primary: list.len() - 1,
        }
    }

    fn carets(selections: &Selections) -> Vec<(usize, usize)> {
        selections.sels.iter().map(|s| (s.anchor, s.head)).collect()
    }

    #[test]
    fn types_at_every_caret() {
        let (text, after) = insert("ab\ncd", &sels(&[(1, 1), (4, 4)]), "x", "    ", "txt");
        assert_eq!(text, "axb\ncxd");
        assert_eq!(carets(&after), vec![(2, 2), (6, 6)]);
    }

    #[test]
    fn pairs_brackets_and_quotes_and_steps_over_them() {
        let (text, after) = insert("f", &sels(&[(1, 1)]), "(", "    ", "txt");
        assert_eq!((text.as_str(), carets(&after)), ("f()", vec![(2, 2)]));
        let (text, after) = insert(&text, &after, ")", "    ", "txt");
        assert_eq!((text.as_str(), carets(&after)), ("f()", vec![(3, 3)]));
        // No pair before a word, no quote pair after a letter (don't).
        assert_eq!(insert("x", &sels(&[(0, 0)]), "(", "    ", "txt").0, "(x");
        assert_eq!(
            insert("don", &sels(&[(3, 3)]), "'", "    ", "txt").0,
            "don'"
        );
        assert_eq!(
            insert("a = ", &sels(&[(4, 4)]), "\"", "    ", "txt").0,
            "a = \"\""
        );
        assert_eq!(insert("&", &sels(&[(1, 1)]), "'", "    ", "rs").0, "&'");
        // Wrapping keeps the selection inside.
        let (text, after) = insert("say hi", &sels(&[(4, 6)]), "\"", "    ", "txt");
        assert_eq!(
            (text.as_str(), carets(&after)),
            ("say \"hi\"", vec![(5, 7)])
        );
    }

    #[test]
    fn backspace_removes_an_empty_pair_and_indent_stops() {
        assert_eq!(
            delete("f()", &sels(&[(2, 2)]), false, By::Char, "    ").0,
            "f"
        );
        let (text, after) = delete("      x", &sels(&[(6, 6)]), false, By::Char, "    ");
        assert_eq!((text.as_str(), carets(&after)), ("    x", vec![(4, 4)]));
        assert_eq!(
            delete("ab cd", &sels(&[(5, 5)]), false, By::Word, "    ").0,
            "ab "
        );
    }

    #[test]
    fn enter_indents_and_splits_pairs() {
        let (text, after) = newline("    if x {}", &sels(&[(10, 10)]), "    ", "rs");
        assert_eq!(text, "    if x {\n        \n    }");
        assert_eq!(carets(&after), vec![(19, 19)]);
        let (text, _) = newline("    a", &sels(&[(5, 5)]), "    ", "rs");
        assert_eq!(text, "    a\n    ");
        let (text, _) = newline("def f():", &sels(&[(8, 8)]), "    ", "py");
        assert_eq!(text, "def f():\n    ");
        // An indentation-only line gives its indentation to the next one.
        let (text, _) = newline("    ", &sels(&[(4, 4)]), "    ", "rs");
        assert_eq!(text, "\n    ");
    }

    #[test]
    fn closing_bracket_dedents_a_blank_line() {
        let (text, _) = insert("{\n        ", &sels(&[(10, 10)]), "}", "    ", "txt");
        assert_eq!(text, "{\n    }");
    }

    #[test]
    fn tab_fills_to_the_tab_stop_or_indents_lines() {
        assert_eq!(tab("ab", &sels(&[(2, 2)]), "    ", false).0, "ab  ");
        let (text, after) = tab("a\nb", &sels(&[(0, 3)]), "    ", false);
        assert_eq!(text, "    a\n    b");
        assert_eq!(carets(&after), vec![(4, 11)]);
        assert_eq!(
            tab("    a\n  b", &sels(&[(0, 0), (8, 8)]), "    ", true).0,
            "a\nb"
        );
    }

    #[test]
    fn next_occurrence_wraps_and_respects_whole_words() {
        let text = "foo food foo";
        let grown = add_next_occurrence(text, &sels(&[(0, 3)]), true).unwrap();
        assert_eq!(carets(&grown), vec![(0, 3), (9, 12)]);
        let grown = add_next_occurrence(text, &sels(&[(0, 3)]), false).unwrap();
        assert_eq!(carets(&grown), vec![(0, 3), (4, 7)]);
        let wrapped = add_next_occurrence(text, &sels(&[(9, 12)]), true).unwrap();
        assert_eq!(carets(&wrapped), vec![(0, 3), (9, 12)]);
        assert_eq!(wrapped.primary, 0);
        let all = select_all_occurrences(text, &sels(&[(1, 1)])).unwrap();
        assert_eq!(carets(&all), vec![(0, 3), (9, 12)]);
        let skipped = skip_to_next_occurrence(text, &sels(&[(0, 3)]), true).unwrap();
        assert_eq!(carets(&skipped), vec![(9, 12)]);
    }

    #[test]
    fn split_lines_and_vertical_carets() {
        let text = "ab\ncd\nef";
        assert_eq!(
            carets(&split_into_lines(text, &sels(&[(1, 7)]))),
            vec![(1, 2), (3, 5), (6, 7)]
        );
        let added = add_caret_vertical(text, &sels(&[(1, 1)]), true);
        assert_eq!(carets(&added), vec![(1, 1), (4, 4)]);
        let moved = move_vertical(text, &added, true, false);
        assert_eq!(carets(&moved), vec![(4, 4), (7, 7)]);
        let home = move_horizontal("  ab", &sels(&[(4, 4)]), false, By::Line, false);
        assert_eq!(carets(&home), vec![(2, 2)]);
    }

    #[test]
    fn copy_cut_paste_lines_and_per_caret() {
        let text = "one\ntwo\n";
        let (copied, lines) = copy_text(text, &sels(&[(1, 1)]));
        assert_eq!((copied.as_str(), lines), ("one\n", true));
        let (rest, _, cut_text, _) = cut(text, &sels(&[(5, 5)]));
        assert_eq!((rest.as_str(), cut_text.as_str()), ("one\n", "two\n"));
        // A whole-line clip goes in above the caret's line.
        let (pasted, after) = paste("a\nb", &sels(&[(3, 3)]), "x\n", true);
        assert_eq!((pasted.as_str(), carets(&after)), ("a\nx\nb", vec![(5, 5)]));
        // One line per caret when the counts match.
        let (pasted, _) = paste("a\nb", &sels(&[(1, 1), (3, 3)]), "1\n2", false);
        assert_eq!(pasted, "a1\nb2");
    }

    #[test]
    fn coinciding_carets_merge() {
        let mut merged = sels(&[(2, 2), (2, 2), (0, 3)]);
        merged.normalize();
        assert_eq!(carets(&merged), vec![(0, 3)]);
    }
}
