//! Sublime-style editor preferences at run time: the font size keys (Cmd/Ctrl + `=`, `-`, `0`),
//! the View toggles, and the on-save whitespace tidying.

use eframe::egui;

use crate::settings::{EDITOR_FONT_SIZE_DEFAULT, EDITOR_FONT_SIZE_MAX, EDITOR_FONT_SIZE_MIN};

use super::super::OxiApp;

const FONT_SIZE_STEP: f32 = 1.0;

impl OxiApp {
    /// Publish the editor preferences the layout code reads globally. Cheap when nothing
    /// changed, so it runs every frame and also covers Cancel restoring the old settings.
    pub(crate) fn sync_editor_prefs(&self, ctx: &egui::Context) {
        let prefs = &self.conv.settings.editor;
        crate::theme::set_editor_font_size(
            prefs
                .font_size
                .clamp(EDITOR_FONT_SIZE_MIN, EDITOR_FONT_SIZE_MAX),
        );
        crate::theme::set_editor_wrap_indent(prefs.wrap_indent);
        // Cmd+0 must restore the chosen text size, not egui's 100 %; the keys are handled in
        // `handle_font_size_keys` instead.
        if ctx.options(|options| options.zoom_with_keyboard) {
            ctx.options_mut(|options| options.zoom_with_keyboard = false);
        }
    }

    /// Cmd/Ctrl + `=` / `-` / `0`. Over an open file they change the editor's font size, as
    /// in Sublime; anywhere else they zoom the whole window.
    pub(crate) fn handle_font_size_keys(&mut self, ctx: &egui::Context) {
        if super::chord_pending(ctx) {
            return;
        }
        use egui::gui_zoom::kb_shortcuts::{ZOOM_IN, ZOOM_IN_SECONDARY, ZOOM_OUT, ZOOM_RESET};
        let (bigger, smaller, reset) = ctx.input_mut(|input| {
            (
                input.consume_shortcut(&ZOOM_IN) || input.consume_shortcut(&ZOOM_IN_SECONDARY),
                input.consume_shortcut(&ZOOM_OUT),
                input.consume_shortcut(&ZOOM_RESET),
            )
        });
        if !(bigger || smaller || reset) {
            return;
        }
        let code_shown = self
            .conv
            .editor
            .active_document()
            .is_some_and(|document| document.media.is_none())
            || (self.conv.diff_view.open && self.conv.git.diff.is_some());
        let editor_shown = !self.conv.settings_page.open && code_shown;
        if !editor_shown {
            if reset {
                ctx.set_zoom_factor(self.conv.settings.ui_density.zoom_factor());
            } else if bigger {
                egui::gui_zoom::zoom_in(ctx);
            } else {
                egui::gui_zoom::zoom_out(ctx);
            }
            return;
        }
        let size = self.conv.settings.editor.font_size;
        let next = if reset {
            EDITOR_FONT_SIZE_DEFAULT
        } else if bigger {
            size + FONT_SIZE_STEP
        } else {
            size - FONT_SIZE_STEP
        };
        self.set_editor_font_size(next);
    }

    fn set_editor_font_size(&mut self, size: f32) {
        // Half-point steps, like the Settings slider, so the 12.5 default stays reachable.
        let size = ((size * 2.0).round() / 2.0).clamp(EDITOR_FONT_SIZE_MIN, EDITOR_FONT_SIZE_MAX);
        if self.conv.settings.editor.font_size == size {
            return;
        }
        self.conv.settings.editor.font_size = size;
        self.persist_editor_prefs();
    }

    pub(crate) fn toggle_word_wrap(&mut self) {
        self.conv.settings.editor.word_wrap = !self.conv.settings.editor.word_wrap;
        self.persist_editor_prefs();
    }

    pub(crate) fn toggle_minimap(&mut self) {
        self.conv.settings.editor.minimap = !self.conv.settings.editor.minimap;
        self.persist_editor_prefs();
    }

    /// Like Sublime writing `Preferences.sublime-settings`: a View toggle sticks at once. With
    /// the Settings page open it is just one more unsaved change there.
    fn persist_editor_prefs(&mut self) {
        if self.conv.settings_page.open {
            return;
        }
        self.save_settings_quietly();
    }

    /// Apply the on-save whitespace preferences to document `index`, as an undoable edit.
    pub(super) fn tidy_before_save(&mut self, ctx: Option<&egui::Context>, index: usize) {
        let prefs = &self.conv.settings.editor;
        let document = &self.conv.editor.documents[index];
        if document.is_scratchpad || document.media.is_some() {
            return;
        }
        let Some(tidied) = tidy_for_save(
            &document.content,
            prefs.trim_trailing_white_space_on_save,
            prefs.ensure_newline_at_eof_on_save,
        ) else {
            return;
        };
        let old = std::mem::replace(&mut self.conv.editor.documents[index].content, tidied);
        let new = self.conv.editor.documents[index].content.clone();
        super::editor_body::mark_document_edited(&mut self.conv.editor.documents[index]);
        if let Some(ctx) = ctx {
            self.record_replacement_undo(ctx, index, old, new);
        }
    }
}

/// The text with trailing spaces/tabs removed from every line and/or a final newline added,
/// or `None` when that changes nothing. Line endings (`\n` or `\r\n`) are kept.
pub(super) fn tidy_for_save(
    text: &str,
    trim_trailing: bool,
    final_newline: bool,
) -> Option<String> {
    let mut out = String::with_capacity(text.len() + 1);
    if trim_trailing {
        let mut rest = text;
        while let Some(newline) = rest.find('\n') {
            let line = &rest[..newline];
            let (body, cr) = match line.strip_suffix('\r') {
                Some(body) => (body, "\r"),
                None => (line, ""),
            };
            out.push_str(body.trim_end_matches([' ', '\t']));
            out.push_str(cr);
            out.push('\n');
            rest = &rest[newline + 1..];
        }
        out.push_str(rest.trim_end_matches([' ', '\t']));
    } else {
        out.push_str(text);
    }
    if final_newline && !out.is_empty() && !out.ends_with('\n') {
        out.push_str(if text.contains("\r\n") { "\r\n" } else { "\n" });
    }
    (out != text).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::tidy_for_save;

    #[test]
    fn trims_trailing_whitespace_and_keeps_line_endings() {
        assert_eq!(
            tidy_for_save("a  \nb\t\r\n  c ", true, false).as_deref(),
            Some("a\nb\r\n  c")
        );
        assert_eq!(tidy_for_save("a\nb", true, false), None);
    }

    #[test]
    fn adds_one_final_newline() {
        assert_eq!(tidy_for_save("a", false, true).as_deref(), Some("a\n"));
        assert_eq!(
            tidy_for_save("a\r\nb", false, true).as_deref(),
            Some("a\r\nb\r\n")
        );
        assert_eq!(tidy_for_save("a\n", false, true), None);
        assert_eq!(tidy_for_save("", false, true), None);
        assert_eq!(tidy_for_save("a  ", true, true).as_deref(), Some("a\n"));
    }
}
