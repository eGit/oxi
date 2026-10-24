//! Sublime Text editing commands for the single-selection editor: toggle comment, duplicate,
//! select/delete/swap/join lines, indent, insert line, select word, matching bracket and
//! expand selection. Pure functions over the text and a sorted byte selection, so they are
//! testable without egui; [`handle_editor_commands`] wires them to the keyboard.

use std::ops::Range;

use eframe::egui::{self, Event, Key, Modifiers};

use super::editor_text::{byte_index, char_index};

/// What a command did.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum CommandResult {
    /// The text changed; the new selection is in the new text.
    Edit {
        text: String,
        selection: Range<usize>,
    },
    /// Only the selection changed.
    Select(Range<usize>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EditorCommand {
    ToggleComment,
    Duplicate,
    SelectLine,
    SelectWord,
    DeleteLine,
    SwapUp,
    SwapDown,
    InsertLineAfter,
    InsertLineBefore,
    Indent,
    Unindent,
    JoinLines,
    JumpToBracket,
    ExpandSelection,
    UpperCase,
    LowerCase,
    DeleteToLineEnd,
    DeleteToLineStart,
    SortLines,
    Transpose,
    ExpandToIndentation,
    ExpandToBrackets,
}

/// Remove and return the editor command bound to a key event this frame, if any. Shifted
/// shortcuts are checked as distinct bindings (egui's `consume_key` would let Cmd+Shift+D
/// trigger Cmd+D).
fn take_command(input: &mut egui::InputState, chord_k: bool) -> Option<EditorCommand> {
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
        command_for(*key, *modifiers, mac, chord_k).is_some()
    })?;
    let Event::Key { key, modifiers, .. } = input.events.remove(position) else {
        return None;
    };
    command_for(key, modifiers, mac, chord_k)
}

fn command_for(key: Key, modifiers: Modifiers, mac: bool, chord_k: bool) -> Option<EditorCommand> {
    use EditorCommand::*;
    let plain_cmd =
        modifiers.command && !modifiers.alt && !modifiers.shift && (!mac || !modifiers.ctrl);
    // The second key of a Cmd+K chord.
    if chord_k && plain_cmd {
        return match key {
            Key::U => Some(UpperCase),
            Key::L => Some(LowerCase),
            Key::K => Some(DeleteToLineEnd),
            Key::Backspace => Some(DeleteToLineStart),
            _ => None,
        };
    }
    // Sublime: F5 sorts lines on macOS, F9 elsewhere.
    if modifiers.is_none() && key == if mac { Key::F5 } else { Key::F9 } {
        return Some(SortLines);
    }
    if mac
        && modifiers.ctrl
        && !modifiers.mac_cmd
        && !modifiers.alt
        && !modifiers.shift
        && key == Key::T
    {
        return Some(Transpose);
    }
    if modifiers.ctrl
        && modifiers.shift
        && !modifiers.alt
        && (!mac || !modifiers.mac_cmd)
        && key == Key::M
    {
        return Some(ExpandToBrackets);
    }
    if modifiers.command
        && modifiers.shift
        && !modifiers.alt
        && (!mac || !modifiers.ctrl)
        && key == Key::J
    {
        return Some(ExpandToIndentation);
    }
    // On macOS `command` is Cmd and Ctrl is separate; elsewhere `command` is Ctrl itself.
    let ctrl_only = modifiers.ctrl && !modifiers.alt && !modifiers.shift && !modifiers.mac_cmd;
    let cmd = modifiers.command && !modifiers.alt && (!mac || !modifiers.ctrl);
    let swap = if mac {
        modifiers.mac_cmd && modifiers.ctrl && !modifiers.shift && !modifiers.alt
    } else {
        modifiers.ctrl && modifiers.shift && !modifiers.alt
    };
    if swap {
        return match key {
            Key::ArrowUp => Some(SwapUp),
            Key::ArrowDown => Some(SwapDown),
            _ => None,
        };
    }
    if mac && ctrl_only && key == Key::M {
        return Some(JumpToBracket);
    }
    if !cmd {
        return None;
    }
    Some(match (key, modifiers.shift) {
        (Key::Slash, _) => ToggleComment,
        (Key::D, true) => Duplicate,
        (Key::D, false) => SelectWord,
        (Key::L, false) => SelectLine,
        (Key::K, true) => DeleteLine,
        (Key::Enter, false) => InsertLineAfter,
        (Key::Enter, true) => InsertLineBefore,
        (Key::CloseBracket, false) => Indent,
        (Key::OpenBracket, false) => Unindent,
        (Key::J, false) => JoinLines,
        (Key::M, false) if !mac => JumpToBracket,
        (Key::Space, true) => ExpandSelection,
        _ => return None,
    })
}

fn chord_flag() -> egui::Id {
    egui::Id::new("editor_chord_k")
}

/// Cmd+K was pressed in the editor and waits for its second key; app-wide shortcuts (Cmd+0,
/// Cmd+1…) stand aside so the chord gets it.
pub(crate) fn chord_pending(ctx: &egui::Context) -> bool {
    ctx.data(|data| data.get_temp::<bool>(chord_flag()))
        .unwrap_or(false)
}

/// Sublime's Cmd+K chords: true on the frame of the key that follows Cmd+K in the focused
/// editor. Cmd+K itself is taken out of the input.
pub(super) fn chord_k(ui: &egui::Ui, editor_id: egui::Id) -> bool {
    let flag = chord_flag();
    if !ui.memory(|memory| memory.has_focus(editor_id)) {
        ui.data_mut(|data| data.remove_temp::<bool>(flag));
        return false;
    }
    let mac = cfg!(target_os = "macos");
    let pressed_k = ui.input_mut(|input| {
        let position = input.events.iter().position(|event| {
            matches!(event, Event::Key { key: Key::K, pressed: true, modifiers, .. }
                if modifiers.command && !modifiers.shift && !modifiers.alt && (!mac || !modifiers.ctrl))
        });
        position.map(|position| input.events.remove(position)).is_some()
    });
    let pending = ui.data(|data| data.get_temp::<bool>(flag)).unwrap_or(false);
    if pressed_k && !pending {
        ui.data_mut(|data| data.insert_temp(flag, true));
        return false;
    }
    let next_key = pressed_k
        || ui.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, Event::Key { pressed: true, .. } | Event::Text(_)))
        });
    if pending && next_key {
        ui.data_mut(|data| data.remove_temp::<bool>(flag));
        if pressed_k {
            // Cmd+K Cmd+K: give the K back as the chord's second key.
            ui.input_mut(|input| {
                input.events.push(Event::Key {
                    key: Key::K,
                    physical_key: Some(Key::K),
                    pressed: true,
                    repeat: false,
                    modifiers: input.modifiers,
                });
            });
        }
        return true;
    }
    false
}

/// Run a bound command on the focused editor before `TextEdit` sees the key. Returns the
/// caret's new char index when the text or selection changed, so the caller can reveal it;
/// `true` in the pair means the text was edited.
pub(super) fn handle_editor_commands(
    ui: &egui::Ui,
    editor_id: egui::Id,
    content: &mut String,
    language: &str,
    chord_k: bool,
) -> Option<(usize, bool)> {
    if !ui.memory(|memory| memory.has_focus(editor_id)) {
        return None;
    }
    let command = ui.input_mut(|input| take_command(input, chord_k))?;
    let mut state = egui::text_edit::TextEditState::load(ui.ctx(), editor_id)?;
    let range = state.cursor.char_range()?;
    let sorted = range.as_sorted_char_range();
    let selection = byte_index(content, sorted.start.0)..byte_index(content, sorted.end.0);
    let result = run_command(command, content, selection, language)?;
    let (selection, edited) = match result {
        CommandResult::Edit { text, selection } => {
            // One undo step per command, independent of egui's time-based grouping.
            let mut undoer = state.undoer();
            undoer.add_undo(&(range, content.clone()));
            *content = text;
            let start = char_index(content, selection.start);
            let end = char_index(content, selection.end);
            let new_range = char_range(start, end);
            undoer.add_undo(&(new_range, content.clone()));
            state.set_undoer(undoer);
            (selection, true)
        }
        CommandResult::Select(selection) => (selection, false),
    };
    let start = char_index(content, selection.start);
    let end = char_index(content, selection.end);
    state.cursor.set_char_range(Some(char_range(start, end)));
    state.store(ui.ctx(), editor_id);
    Some((end, edited))
}

fn char_range(start: usize, end: usize) -> egui::text::CCursorRange {
    egui::text::CCursorRange::two(
        egui::text::CCursor::new(start),
        egui::text::CCursor::new(end),
    )
}

pub(super) fn run_command(
    command: EditorCommand,
    text: &str,
    selection: Range<usize>,
    language: &str,
) -> Option<CommandResult> {
    use EditorCommand::*;
    match command {
        ToggleComment => {
            toggle_comment(text, selection, crate::code_nav::comment_tokens(language)?)
        }
        Duplicate => Some(duplicate(text, selection)),
        SelectLine => Some(CommandResult::Select(select_line(text, selection))),
        SelectWord => select_word(text, selection).map(CommandResult::Select),
        DeleteLine => Some(delete_line(text, selection)),
        SwapUp => swap_lines(text, selection, true),
        SwapDown => swap_lines(text, selection, false),
        InsertLineAfter => Some(insert_line(text, selection, false)),
        InsertLineBefore => Some(insert_line(text, selection, true)),
        Indent => Some(indent(text, selection, false)),
        Unindent => indent_or_none(text, selection),
        JoinLines => join_lines(text, selection),
        JumpToBracket => {
            // Land just before the partner bracket, so a second press jumps back.
            let (_, other) = bracket_pair_at(text, selection.end)?;
            Some(CommandResult::Select(other..other))
        }
        ExpandSelection => {
            let expanded = crate::code_nav::expand_selection(language, text, selection.clone())
                .or_else(|| {
                    // No grammar: word, then line.
                    select_word(text, selection.clone())
                        .filter(|word| *word != selection)
                        .or_else(|| Some(select_line(text, selection.clone())))
                })?;
            Some(CommandResult::Select(expanded))
        }
        UpperCase | LowerCase => {
            let range = if selection.is_empty() {
                crate::code_nav::identifier_at(text, selection.start)?.1
            } else {
                selection
            };
            let original = &text[range.clone()];
            let changed = if command == UpperCase {
                original.to_uppercase()
            } else {
                original.to_lowercase()
            };
            let end = range.start + changed.len();
            Some(CommandResult::Edit {
                text: apply(text, &[(range.clone(), changed)]),
                selection: range.start..end,
            })
        }
        DeleteToLineEnd => {
            let end = line_end(text, selection.end);
            let end = if end == selection.end {
                (end + 1).min(text.len())
            } else {
                end
            };
            (end > selection.start).then(|| CommandResult::Edit {
                text: apply(text, &[(selection.start..end, String::new())]),
                selection: selection.start..selection.start,
            })
        }
        DeleteToLineStart => {
            let start = line_start(text, selection.start);
            let start = if start == selection.start {
                start.saturating_sub(1)
            } else {
                start
            };
            (start < selection.start).then(|| CommandResult::Edit {
                text: apply(text, &[(start..selection.start, String::new())]),
                selection: start..start,
            })
        }
        SortLines => sort_lines(text, selection),
        Transpose => transpose(text, selection),
        ExpandToIndentation => expand_to_indentation(text, selection).map(CommandResult::Select),
        ExpandToBrackets => expand_to_brackets(text, selection).map(CommandResult::Select),
    }
}

/// Sublime's Sort Lines (case-insensitive): the selected lines, or the whole file.
fn sort_lines(text: &str, selection: Range<usize>) -> Option<CommandResult> {
    let block = if selection.is_empty() {
        0..text.trim_end_matches('\n').len()
    } else {
        line_block(text, &selection)
    };
    let mut lines: Vec<&str> = text[block.clone()].split('\n').collect();
    lines.sort_by_key(|line| line.to_lowercase());
    let sorted = lines.join("\n");
    (sorted != text[block.clone()]).then(|| CommandResult::Edit {
        selection: block.start..block.start + sorted.len(),
        text: apply(text, &[(block, sorted)]),
    })
}

/// Sublime's Ctrl+T: swap the characters around the caret (at a line end, the two before it).
fn transpose(text: &str, selection: Range<usize>) -> Option<CommandResult> {
    if !selection.is_empty() {
        return None;
    }
    let mut at = selection.start;
    if at == line_end(text, at) && at > line_start(text, at) {
        at = text.floor_char_boundary(at - 1);
    }
    let before = text[..at].chars().next_back().filter(|c| *c != '\n')?;
    let after = text[at..].chars().next().filter(|c| *c != '\n')?;
    let start = at - before.len_utf8();
    let end = at + after.len_utf8();
    Some(CommandResult::Edit {
        text: apply(text, &[(start..end, format!("{after}{before}"))]),
        selection: end..end,
    })
}

/// Sublime's Cmd+Shift+J: the lines around the selection indented at least as deep as it.
fn expand_to_indentation(text: &str, selection: Range<usize>) -> Option<Range<usize>> {
    let indent = |start: usize| -> Option<usize> {
        let line = &text[start..line_end(text, start)];
        (!line.trim().is_empty()).then(|| line.len() - line.trim_start().len())
    };
    let first = line_start(text, selection.start);
    let level = indent(first)?;
    let mut start = first;
    while start > 0 {
        let previous = line_start(text, start - 1);
        match indent(previous) {
            Some(depth) if depth < level => break,
            _ => start = previous,
        }
    }
    let mut end = line_end(text, selection.end.max(first));
    while end < text.len() {
        let next = end + 1;
        match indent(next) {
            Some(depth) if depth < level => break,
            _ => end = line_end(text, next),
        }
    }
    // Leading and trailing blank lines are not part of the block.
    let block = &text[start..end];
    let lead = block.len() - block.trim_start_matches(['\n', ' ', '\t']).len();
    let lead = block[..lead].rfind('\n').map_or(0, |index| index + 1);
    let trail = block.trim_end().len();
    let range = start + lead..start + trail;
    (range != selection).then_some(range)
}

/// Sublime's Ctrl+Shift+M: the text inside the brackets around the selection; again, with them.
fn expand_to_brackets(text: &str, selection: Range<usize>) -> Option<Range<usize>> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut at = selection.start;
    let limit = selection.start.saturating_sub(BRACKET_SCAN_LIMIT);
    while at > limit {
        at -= 1;
        match bytes[at] {
            b')' | b']' | b'}' => depth += 1,
            b'(' | b'[' | b'{' if depth > 0 => depth -= 1,
            b'(' | b'[' | b'{' => {
                let (open, close) = bracket_pair_at(text, at)?;
                if close < selection.end {
                    continue;
                }
                let inside = open + 1..close;
                return Some(if inside == selection {
                    open..close + 1
                } else {
                    inside
                });
            }
            _ => {}
        }
    }
    None
}

fn line_start(text: &str, byte: usize) -> usize {
    text[..byte].rfind('\n').map_or(0, |index| index + 1)
}

fn line_end(text: &str, byte: usize) -> usize {
    text[byte..]
        .find('\n')
        .map_or(text.len(), |index| byte + index)
}

/// Whole lines touched by the selection, without the final newline. A selection ending at the
/// very start of a line (after Cmd+L) does not include that line, like Sublime.
fn line_block(text: &str, selection: &Range<usize>) -> Range<usize> {
    let start = line_start(text, selection.start);
    let mut last = selection.end;
    if selection.end > selection.start && text[..selection.end].ends_with('\n') {
        last -= 1;
    }
    start..line_end(text, last.max(start))
}

fn column(text: &str, byte: usize) -> usize {
    byte - line_start(text, byte)
}

/// Byte offset at `column` on the line starting at `start`, clamped to the line.
fn at_column(text: &str, start: usize, column: usize) -> usize {
    let end = line_end(text, start);
    text.floor_char_boundary((start + column).min(end))
}

/// Apply sorted, non-overlapping replacements.
fn apply(text: &str, edits: &[(Range<usize>, String)]) -> String {
    let mut output = String::with_capacity(text.len() + 64);
    let mut cursor = 0;
    for (range, insert) in edits {
        output.push_str(&text[cursor..range.start]);
        output.push_str(insert);
        cursor = range.end;
    }
    output.push_str(&text[cursor..]);
    output
}

/// Where `position` ends up after `edits`. `sticky` keeps a position that sits exactly at an
/// insertion point before the inserted text (selection starts at a line start).
fn map_position(position: usize, edits: &[(Range<usize>, String)], sticky: bool) -> usize {
    let mut delta = 0isize;
    for (range, insert) in edits {
        if position < range.start || (sticky && position == range.start) {
            break;
        }
        if position < range.end {
            let new_start = range.start.saturating_add_signed(delta);
            return new_start + (position - range.start).min(insert.len());
        }
        delta += insert.len() as isize - range.len() as isize;
    }
    position.saturating_add_signed(delta)
}

fn map_selection(selection: &Range<usize>, edits: &[(Range<usize>, String)]) -> Range<usize> {
    if selection.is_empty() {
        let caret = map_position(selection.start, edits, false);
        caret..caret
    } else {
        map_position(selection.start, edits, true)..map_position(selection.end, edits, false)
    }
}

fn toggle_comment(
    text: &str,
    selection: Range<usize>,
    (prefix, suffix): (&str, &str),
) -> Option<CommandResult> {
    let block = line_block(text, &selection);
    let mut lines = Vec::new();
    let mut start = block.start;
    loop {
        let end = line_end(text, start);
        lines.push(start..end);
        if end >= block.end {
            break;
        }
        start = end + 1;
    }
    let content_lines: Vec<_> = lines
        .iter()
        .filter(|line| !text[(*line).clone()].trim().is_empty())
        .collect();
    if content_lines.is_empty() {
        return None;
    }
    let indent_of =
        |line: &Range<usize>| text[line.clone()].len() - text[line.clone()].trim_start().len();
    let commented = content_lines.iter().all(|line| {
        let trimmed = text[(*line).clone()].trim();
        trimmed.starts_with(prefix) && trimmed.ends_with(suffix)
    });
    let mut edits = Vec::new();
    if commented {
        for line in content_lines {
            let marker = line.start + indent_of(line);
            let after = marker + prefix.len();
            let remove_end = if text[after..line.end].starts_with(' ') {
                after + 1
            } else {
                after
            };
            edits.push((marker..remove_end, String::new()));
            if !suffix.is_empty() {
                let trimmed_end = line.start + text[line.clone()].trim_end().len();
                let mut suffix_start = trimmed_end - suffix.len();
                if suffix_start > remove_end && text[..suffix_start].ends_with(' ') {
                    suffix_start -= 1;
                }
                edits.push((suffix_start.max(remove_end)..trimmed_end, String::new()));
            }
        }
    } else {
        // Comment markers line up at the shallowest indentation, like Sublime.
        let column = content_lines.iter().map(|line| indent_of(line)).min()?;
        for line in content_lines {
            let at = line.start + column;
            edits.push((at..at, format!("{prefix} ")));
            if !suffix.is_empty() {
                let end = line.start + text[line.clone()].trim_end().len();
                edits.push((end..end, format!(" {suffix}")));
            }
        }
    }
    Some(CommandResult::Edit {
        text: apply(text, &edits),
        selection: map_selection(&selection, &edits),
    })
}

fn duplicate(text: &str, selection: Range<usize>) -> CommandResult {
    if !selection.is_empty() {
        let copy = &text[selection.clone()];
        let edits = [(selection.end..selection.end, copy.to_owned())];
        return CommandResult::Edit {
            text: apply(text, &edits),
            selection: selection.end..selection.end + copy.len(),
        };
    }
    let block = line_block(text, &selection);
    let lines = &text[block.clone()];
    let caret = selection.start + lines.len() + 1;
    CommandResult::Edit {
        text: apply(text, &[(block.end..block.end, format!("\n{lines}"))]),
        selection: caret..caret,
    }
}

fn select_line(text: &str, selection: Range<usize>) -> Range<usize> {
    let block = line_block(text, &selection);
    let end = (block.end + 1).min(text.len());
    if (block.start..end) == selection && end < text.len() {
        // Already whole lines: extend by one more, like pressing Cmd+L again in Sublime.
        let next_end = line_end(text, end);
        return block.start..(next_end + 1).min(text.len());
    }
    block.start..end
}

/// The word at an empty caret; with a selection, the next occurrence of it (wrapping).
fn select_word(text: &str, selection: Range<usize>) -> Option<Range<usize>> {
    if selection.is_empty() {
        return crate::code_nav::identifier_at(text, selection.start).map(|(_, range)| range);
    }
    let needle = &text[selection.clone()];
    let next = text[selection.end..]
        .find(needle)
        .map(|offset| selection.end + offset)
        .or_else(|| text.find(needle))?;
    Some(next..next + needle.len())
}

fn delete_line(text: &str, selection: Range<usize>) -> CommandResult {
    let block = line_block(text, &selection);
    let caret_column = column(text, selection.start);
    let (remove, next_line) = if block.end < text.len() {
        (block.start..block.end + 1, block.start)
    } else if block.start > 0 {
        // Last line: remove the newline before it and land on the previous line.
        let previous = line_start(text, block.start - 1);
        (block.start - 1..block.end, previous)
    } else {
        (block.clone(), 0)
    };
    let new_text = apply(text, &[(remove, String::new())]);
    let caret = at_column(&new_text, next_line, caret_column);
    CommandResult::Edit {
        text: new_text,
        selection: caret..caret,
    }
}

fn swap_lines(text: &str, selection: Range<usize>, up: bool) -> Option<CommandResult> {
    let block = line_block(text, &selection);
    let lines = &text[block.clone()];
    if up {
        if block.start == 0 {
            return None;
        }
        let previous_start = line_start(text, block.start - 1);
        let previous = &text[previous_start..block.start - 1];
        let shift = previous.len() + 1;
        Some(CommandResult::Edit {
            text: apply(
                text,
                &[(previous_start..block.end, format!("{lines}\n{previous}"))],
            ),
            selection: selection.start - shift..selection.end - shift,
        })
    } else {
        if block.end >= text.len() {
            return None;
        }
        let next_end = line_end(text, block.end + 1);
        let next = &text[block.end + 1..next_end];
        let shift = next.len() + 1;
        Some(CommandResult::Edit {
            text: apply(text, &[(block.start..next_end, format!("{next}\n{lines}"))]),
            selection: selection.start + shift..selection.end + shift,
        })
    }
}

fn leading_whitespace(text: &str, line_start: usize) -> &str {
    let line = &text[line_start..line_end(text, line_start)];
    &line[..line.len() - line.trim_start().len()]
}

fn insert_line(text: &str, selection: Range<usize>, above: bool) -> CommandResult {
    let start = line_start(
        text,
        if above {
            selection.start
        } else {
            selection.end
        },
    );
    let indent = leading_whitespace(text, start).to_owned();
    if above {
        let caret = start + indent.len();
        CommandResult::Edit {
            text: apply(text, &[(start..start, format!("{indent}\n"))]),
            selection: caret..caret,
        }
    } else {
        let end = line_end(text, start);
        let caret = end + 1 + indent.len();
        CommandResult::Edit {
            text: apply(text, &[(end..end, format!("\n{indent}"))]),
            selection: caret..caret,
        }
    }
}

thread_local! {
    /// The active tab's indentation chosen in the status bar (see [`set_indent_override`]).
    static INDENT_OVERRIDE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Set by the editor each frame from the shown tab, so commands indent the way the status bar
/// says without threading it through every one of them.
pub(super) fn set_indent_override(unit: Option<String>) {
    INDENT_OVERRIDE.with(|current| *current.borrow_mut() = unit);
}

/// The file's indentation unit: a tab, or the smallest space indent seen (2 or 4), unless the
/// status bar overrides it.
pub(super) fn indent_unit(text: &str) -> String {
    if let Some(unit) = INDENT_OVERRIDE.with(|current| current.borrow().clone()) {
        return unit;
    }
    let mut smallest = usize::MAX;
    for line in text.lines().take(2000) {
        if line.starts_with('\t') {
            return "\t".into();
        }
        let spaces = line.len() - line.trim_start_matches(' ').len();
        if spaces > 0 && spaces < line.len() {
            smallest = smallest.min(spaces);
        }
    }
    if smallest == 2 {
        "  ".into()
    } else {
        "    ".into()
    }
}

fn indent_or_none(text: &str, selection: Range<usize>) -> Option<CommandResult> {
    let result = indent(text, selection, true);
    match &result {
        CommandResult::Edit { text: new, .. } if new == text => None,
        _ => Some(result),
    }
}

fn indent(text: &str, selection: Range<usize>, outdent: bool) -> CommandResult {
    let unit = indent_unit(text);
    let block = line_block(text, &selection);
    let mut edits = Vec::new();
    let mut start = block.start;
    loop {
        let end = line_end(text, start);
        let line = &text[start..end];
        if outdent {
            let remove = if line.starts_with('\t') {
                1
            } else {
                line.len() - line.trim_start_matches(' ').len()
            }
            .min(unit.len().max(1));
            if remove > 0 {
                edits.push((start..start + remove, String::new()));
            }
        } else if !line.trim().is_empty() {
            edits.push((start..start, unit.clone()));
        }
        if end >= block.end {
            break;
        }
        start = end + 1;
    }
    CommandResult::Edit {
        text: apply(text, &edits),
        selection: map_selection(&selection, &edits),
    }
}

fn join_lines(text: &str, selection: Range<usize>) -> Option<CommandResult> {
    let block = line_block(text, &selection);
    // A caret or single-line selection joins the following line, like Sublime's Cmd+J.
    let last = if text[block.clone()].contains('\n') {
        block.end
    } else {
        if block.end >= text.len() {
            return None;
        }
        line_end(text, block.end + 1)
    };
    let mut edits = Vec::new();
    let mut search = block.start;
    while let Some(offset) = text[search..last].find('\n') {
        let newline = search + offset;
        let gap_start = newline - (text[..newline].len() - text[..newline].trim_end().len());
        let rest = &text[newline + 1..];
        let gap_end = newline + 1 + (rest.len() - rest.trim_start_matches([' ', '\t']).len());
        let next_empty = gap_end >= text.len() || text[gap_end..].starts_with('\n');
        let joiner = if gap_start == line_start(text, newline) || next_empty {
            ""
        } else {
            " "
        };
        edits.push((gap_start..gap_end, joiner.to_owned()));
        search = gap_end;
        if search >= last {
            break;
        }
    }
    if edits.is_empty() {
        return None;
    }
    let new_text = apply(text, &edits);
    let selection = if selection.is_empty() {
        let (range, joiner) = &edits[0];
        let caret = range.start + joiner.len();
        caret..caret
    } else {
        map_selection(&selection, &edits)
    };
    Some(CommandResult::Edit {
        text: new_text,
        selection,
    })
}

/// Bracket pairs searched at most this far from the caret, keeping the per-caret-move cost flat
/// in huge files.
const BRACKET_SCAN_LIMIT: usize = 256 * 1024;

/// The bracket touching `caret` (after it, else before it) and its partner, as byte offsets.
pub(super) fn bracket_pair_at(text: &str, caret: usize) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let candidates = [Some(caret), caret.checked_sub(1)];
    for at in candidates.into_iter().flatten() {
        let Some(&byte) = bytes.get(at) else {
            continue;
        };
        let (open, close, forward) = match byte {
            b'(' => (b'(', b')', true),
            b'[' => (b'[', b']', true),
            b'{' => (b'{', b'}', true),
            b')' => (b'(', b')', false),
            b']' => (b'[', b']', false),
            b'}' => (b'{', b'}', false),
            _ => continue,
        };
        let mut depth = 0usize;
        if forward {
            let limit = (at + BRACKET_SCAN_LIMIT).min(bytes.len());
            for (index, &current) in bytes[at..limit].iter().enumerate() {
                if current == open {
                    depth += 1;
                } else if current == close {
                    depth -= 1;
                    if depth == 0 {
                        return Some((at, at + index));
                    }
                }
            }
        } else {
            let limit = at.saturating_sub(BRACKET_SCAN_LIMIT);
            for index in (limit..=at).rev() {
                let current = bytes[index];
                if current == close {
                    depth += 1;
                } else if current == open {
                    depth -= 1;
                    if depth == 0 {
                        return Some((at, index));
                    }
                }
            }
        }
        return None;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(result: Option<CommandResult>) -> (String, Range<usize>) {
        match result.expect("command applies") {
            CommandResult::Edit { text, selection } => (text, selection),
            CommandResult::Select(_) => panic!("expected an edit"),
        }
    }

    fn run(command: EditorCommand, text: &str, selection: Range<usize>) -> (String, Range<usize>) {
        edit(run_command(command, text, selection, "rs"))
    }

    #[test]
    fn toggle_comment_round_trips_at_shared_indent() {
        let text = "fn a() {\n    let x = 1;\n\n        y();\n}\n";
        let start = text.find("    let").unwrap();
        let end = text.find("}\n").unwrap();
        let (commented, selection) = run(EditorCommand::ToggleComment, text, start..end);
        assert_eq!(
            commented,
            "fn a() {\n    // let x = 1;\n\n    //     y();\n}\n"
        );
        assert_eq!(selection.start, start);
        let (back, _) = run(EditorCommand::ToggleComment, &commented, selection);
        assert_eq!(back, text);
    }

    #[test]
    fn toggle_comment_keeps_caret_on_its_character() {
        let text = "let x = 1;";
        let (commented, selection) = run(EditorCommand::ToggleComment, text, 4..4);
        assert_eq!(commented, "// let x = 1;");
        assert_eq!(&commented[selection.start..selection.start + 1], "x");
        let (css, _) = edit(run_command(
            EditorCommand::ToggleComment,
            "a { color: red; }",
            0..0,
            "css",
        ));
        assert_eq!(css, "/* a { color: red; } */");
        let (back, _) = edit(run_command(EditorCommand::ToggleComment, &css, 0..0, "css"));
        assert_eq!(back, "a { color: red; }");
    }

    #[test]
    fn duplicate_line_and_selection() {
        let (text, selection) = run(EditorCommand::Duplicate, "one\ntwo", 1..1);
        assert_eq!(text, "one\none\ntwo");
        assert_eq!(selection, 5..5);
        let (text, selection) = run(EditorCommand::Duplicate, "ab", 0..1);
        assert_eq!(text, "aab");
        assert_eq!(selection, 1..2);
    }

    #[test]
    fn select_line_extends_on_repeat() {
        let text = "one\ntwo\nthree";
        let Some(CommandResult::Select(first)) =
            run_command(EditorCommand::SelectLine, text, 1..1, "rs")
        else {
            panic!()
        };
        assert_eq!(first, 0..4);
        let Some(CommandResult::Select(second)) =
            run_command(EditorCommand::SelectLine, text, first, "rs")
        else {
            panic!()
        };
        assert_eq!(second, 0..8);
    }

    #[test]
    fn delete_and_swap_lines() {
        let (text, selection) = run(EditorCommand::DeleteLine, "one\ntwo\nthree", 5..5);
        assert_eq!(text, "one\nthree");
        assert_eq!(selection, 5..5);
        let (text, _) = run(EditorCommand::DeleteLine, "one\ntwo", 5..5);
        assert_eq!(text, "one");
        let (text, selection) = run(EditorCommand::SwapUp, "one\ntwo\nthree", 5..5);
        assert_eq!(text, "two\none\nthree");
        assert_eq!(selection, 1..1);
        let (text, selection) = run(EditorCommand::SwapDown, "one\ntwo\nthree", 1..1);
        assert_eq!(text, "two\none\nthree");
        assert_eq!(selection, 5..5);
        assert!(run_command(EditorCommand::SwapDown, "one\ntwo", 5..5, "rs").is_none());
    }

    #[test]
    fn insert_line_keeps_indentation() {
        let (text, selection) = run(EditorCommand::InsertLineAfter, "    a();\nb", 6..6);
        assert_eq!(text, "    a();\n    \nb");
        assert_eq!(selection, 13..13);
        let (text, selection) = run(EditorCommand::InsertLineBefore, "  a", 3..3);
        assert_eq!(text, "  \n  a");
        assert_eq!(selection, 2..2);
    }

    #[test]
    fn indent_and_unindent_lines() {
        let text = "fn a() {\n    b();\n}";
        let (indented, selection) = run(EditorCommand::Indent, text, 0..text.len());
        assert_eq!(indented, "    fn a() {\n        b();\n    }");
        let (back, _) = run(EditorCommand::Unindent, &indented, selection);
        assert_eq!(back, text);
        assert!(run_command(EditorCommand::Unindent, "a", 0..0, "rs").is_none());
    }

    #[test]
    fn join_lines_collapses_whitespace() {
        let (text, selection) = run(EditorCommand::JoinLines, "let a =\n    1;", 2..2);
        assert_eq!(text, "let a = 1;");
        assert_eq!(selection, 8..8);
    }

    #[test]
    fn select_word_then_next_occurrence() {
        let text = "foo bar foo";
        let Some(CommandResult::Select(word)) =
            run_command(EditorCommand::SelectWord, text, 1..1, "rs")
        else {
            panic!()
        };
        assert_eq!(word, 0..3);
        let Some(CommandResult::Select(next)) =
            run_command(EditorCommand::SelectWord, text, word, "rs")
        else {
            panic!()
        };
        assert_eq!(next, 8..11);
    }

    #[test]
    fn brackets_match_both_ways() {
        let text = "f(a, (b), c)";
        assert_eq!(bracket_pair_at(text, 1), Some((1, 11)));
        assert_eq!(bracket_pair_at(text, 12), Some((11, 1)));
        assert_eq!(bracket_pair_at(text, 0), None);
        let Some(CommandResult::Select(jump)) =
            run_command(EditorCommand::JumpToBracket, text, 1..1, "rs")
        else {
            panic!()
        };
        assert_eq!(jump, 11..11);
    }

    #[test]
    fn shortcuts_distinguish_shift() {
        let cmd = Modifiers::COMMAND;
        assert_eq!(
            command_for(Key::D, cmd, true, false),
            Some(EditorCommand::SelectWord)
        );
        assert_eq!(
            command_for(Key::D, cmd.plus(Modifiers::SHIFT), true, false),
            Some(EditorCommand::Duplicate)
        );
        let swap = Modifiers {
            ctrl: true,
            mac_cmd: true,
            command: true,
            ..Default::default()
        };
        assert_eq!(
            command_for(Key::ArrowUp, swap, true, false),
            Some(EditorCommand::SwapUp)
        );
        assert_eq!(command_for(Key::ArrowUp, cmd, true, false), None);
    }

    #[test]
    fn sublime_text_commands() {
        let edit = |command, text: &str, selection: Range<usize>| match run_command(
            command, text, selection, "rs",
        ) {
            Some(CommandResult::Edit { text, selection }) => (text, selection),
            other => panic!("{other:?}"),
        };
        let select = |command, text: &str, selection: Range<usize>| match run_command(
            command, text, selection, "rs",
        ) {
            Some(CommandResult::Select(range)) => range,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            edit(EditorCommand::UpperCase, "let foo = 1", 5..5),
            ("let FOO = 1".into(), 4..7)
        );
        assert_eq!(edit(EditorCommand::LowerCase, "A B", 0..3).0, "a b");
        assert_eq!(
            edit(EditorCommand::DeleteToLineEnd, "abc\nd", 1..1).0,
            "a\nd"
        );
        assert_eq!(
            edit(EditorCommand::DeleteToLineEnd, "abc\nd", 3..3).0,
            "abcd"
        );
        assert_eq!(edit(EditorCommand::DeleteToLineStart, "abc", 2..2).0, "c");
        assert_eq!(
            edit(EditorCommand::SortLines, "b\nC\na\n", 0..0).0,
            "a\nb\nC\n"
        );
        assert_eq!(edit(EditorCommand::Transpose, "ab", 1..1).0, "ba");
        assert_eq!(edit(EditorCommand::Transpose, "ab", 2..2).0, "ba");
        let text = "fn a() {\n    x;\n    y;\n}";
        assert_eq!(
            select(EditorCommand::ExpandToIndentation, text, 14..14),
            9..22
        );
        assert_eq!(
            select(EditorCommand::ExpandToBrackets, "f(a, b)", 3..3),
            2..6
        );
        assert_eq!(
            select(EditorCommand::ExpandToBrackets, "f(a, b)", 2..6),
            1..7
        );
    }
}
