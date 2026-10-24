//! Sublime's status bar items for the open file: caret position (or what is selected), the
//! indentation (Spaces: 4 / Tab Size: 4, with Sublime's indentation menu) and the syntax, which
//! can be changed for the tab.

use eframe::egui::{self, RichText};

use crate::theme::*;

use super::super::OxiApp;

/// Syntaxes offered in the status bar menu: (label, language id used by the highlighter).
const SYNTAXES: &[(&str, &str)] = &[
    ("Plain Text", "txt"),
    ("Bash", "sh"),
    ("C", "c"),
    ("C++", "cpp"),
    ("CSS", "css"),
    ("Go", "go"),
    ("HTML", "html"),
    ("Java", "java"),
    ("JavaScript", "js"),
    ("JavaScript (JSX)", "jsx"),
    ("JSON", "json"),
    ("Markdown", "md"),
    ("Python", "py"),
    ("Rust", "rs"),
    ("TOML", "toml"),
    ("TypeScript", "ts"),
    ("TypeScriptReact", "tsx"),
    ("YAML", "yaml"),
];

pub(crate) fn syntax_name(language: &str) -> &'static str {
    SYNTAXES
        .iter()
        .find(|(_, id)| *id == language)
        .map_or("Plain Text", |(name, _)| name)
}

/// The language the editor treats document `path` as: the tab's choice, else its extension.
pub(crate) fn document_language(document: &crate::app::state::EditorDocument) -> &'static str {
    document
        .view
        .syntax_override
        .unwrap_or_else(|| super::support::language_for_path(&document.path))
}

/// `"Line 3, Column 7"`, `"12 characters selected"` or `"3 selection regions"`.
pub(crate) fn caret_status(
    text: &str,
    caret_char: usize,
    selected: Option<(usize, usize)>,
    regions: usize,
) -> String {
    if regions > 1 {
        return format!("{regions} selection regions");
    }
    if let Some((start, end)) = selected.filter(|(start, end)| end > start) {
        let count = end - start;
        return format!(
            "{count} character{} selected",
            if count == 1 { "" } else { "s" }
        );
    }
    let byte = super::editor_text::byte_index(text, caret_char).min(text.len());
    let line_start = text[..byte].rfind('\n').map_or(0, |index| index + 1);
    let line = text[..line_start].bytes().filter(|b| *b == b'\n').count() + 1;
    let column = text[line_start..byte].chars().count() + 1;
    format!("Line {line}, Column {column}")
}

/// Leading white space of every line re-expressed with tabs (`to_tabs`) or spaces.
pub(crate) fn convert_indentation(text: &str, width: usize, to_tabs: bool) -> String {
    let width = width.max(1);
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split('\n').enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let rest = line.trim_start_matches([' ', '\t']);
        let lead = &line[..line.len() - rest.len()];
        let mut columns = 0;
        for byte in lead.bytes() {
            columns += if byte == b'\t' {
                width - columns % width
            } else {
                1
            };
        }
        if to_tabs {
            out.push_str(&"\t".repeat(columns / width));
            out.push_str(&" ".repeat(columns % width));
        } else {
            out.push_str(&" ".repeat(columns));
        }
        out.push_str(rest);
    }
    out
}

impl OxiApp {
    /// Left side of the status bar while a file is open: where the caret is.
    pub(crate) fn render_caret_status(&mut self, ui: &mut egui::Ui) {
        let Some(document) = self.conv.editor.active_document() else {
            return;
        };
        if document.media.is_some() {
            return;
        }
        let label = caret_status(
            &document.content,
            self.conv.editor.navigation_cursor_char,
            self.conv.editor.editor_selection_chars,
            1 + document.view.extra.len(),
        );
        let dirty = if document.is_dirty() { " \u{2022}" } else { "" };
        let response = ui
            .add(
                egui::Label::new(
                    RichText::new(format!("{label}{dirty}"))
                        .size(FS_TINY)
                        .color(c_text_muted()),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text("Go to line");
        if response.clicked() {
            self.open_file_picker_with(":");
        }
    }

    /// Right side of the status bar while a file is open: indentation and syntax menus.
    pub(crate) fn render_file_status(&mut self, ui: &mut egui::Ui) {
        let Some(index) = self.conv.editor.active else {
            return;
        };
        let Some(document) = self.conv.editor.documents.get(index) else {
            return;
        };
        if document.media.is_some() {
            return;
        }
        let language = document_language(document);
        let unit = document
            .view
            .indent_override
            .clone()
            .unwrap_or_else(|| super::editor_commands::indent_unit(&document.content));
        let tabs = unit == "\t";
        let width = if tabs { 4 } else { unit.len() };

        let item = |ui: &mut egui::Ui, text: String| {
            ui.add(
                egui::Label::new(RichText::new(text).size(FS_TINY).color(c_text_muted()))
                    .sense(egui::Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
        };
        // Right-to-left: the syntax is the rightmost item, as in Sublime.
        let syntax = item(ui, syntax_name(language).to_owned());
        ui.add_space(10.0);
        let indentation = item(
            ui,
            if tabs {
                format!("Tab Size: {width}")
            } else {
                format!("Spaces: {width}")
            },
        );

        let mut pick_syntax = None;
        egui::Popup::menu(&syntax).show(|ui| {
            ui.set_min_width(170.0);
            egui::ScrollArea::vertical()
                .max_height(360.0)
                .show(ui, |ui| {
                    for (name, id) in SYNTAXES {
                        if ui.selectable_label(*id == language, *name).clicked() {
                            pick_syntax = Some(*id);
                        }
                    }
                });
        });

        enum Indent {
            UseSpaces(bool),
            Width(usize),
            Convert(bool),
            Guess,
        }
        let mut pick_indent = None;
        egui::Popup::menu(&indentation).show(|ui| {
            ui.set_min_width(220.0);
            if ui.selectable_label(!tabs, "Indent Using Spaces").clicked() {
                pick_indent = Some(Indent::UseSpaces(tabs));
            }
            ui.separator();
            for candidate in 1..=8 {
                if ui
                    .selectable_label(candidate == width, format!("Tab Width: {candidate}"))
                    .clicked()
                {
                    pick_indent = Some(Indent::Width(candidate));
                }
            }
            ui.separator();
            if ui.button("Guess Settings From Buffer").clicked() {
                pick_indent = Some(Indent::Guess);
            }
            ui.separator();
            if ui.button("Convert Indentation to Spaces").clicked() {
                pick_indent = Some(Indent::Convert(false));
            }
            if ui.button("Convert Indentation to Tabs").clicked() {
                pick_indent = Some(Indent::Convert(true));
            }
        });

        if let Some(id) = pick_syntax {
            let document = &mut self.conv.editor.documents[index];
            let natural = super::support::language_for_path(&document.path);
            document.view.syntax_override = (id != natural).then_some(id);
            document.syntax_state = None;
            document.layout_cache = Default::default();
            document.minimap_cache = None;
        }
        let Some(choice) = pick_indent else {
            return;
        };
        let document = &mut self.conv.editor.documents[index];
        match choice {
            Indent::UseSpaces(spaces) => {
                document.view.indent_override = Some(if spaces {
                    " ".repeat(width)
                } else {
                    "\t".to_owned()
                });
            }
            Indent::Width(width) => {
                document.view.indent_override = Some(if tabs {
                    "\t".to_owned()
                } else {
                    " ".repeat(width)
                });
            }
            Indent::Guess => document.view.indent_override = None,
            Indent::Convert(to_tabs) => {
                let converted = convert_indentation(&document.content, width, to_tabs);
                document.view.indent_override = Some(if to_tabs {
                    "\t".to_owned()
                } else {
                    " ".repeat(width)
                });
                if converted != document.content {
                    let old = std::mem::replace(&mut document.content, converted);
                    let new = document.content.clone();
                    super::editor_body::mark_document_edited(document);
                    self.record_replacement_undo(ui.ctx(), index, old, new);
                }
            }
        }
        self.conv.editor.focus_editor_next_frame = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_and_selection_status() {
        let text = "ab\ncdé\n";
        assert_eq!(caret_status(text, 0, None, 1), "Line 1, Column 1");
        assert_eq!(caret_status(text, 6, None, 1), "Line 2, Column 4");
        assert_eq!(
            caret_status(text, 6, Some((1, 2)), 1),
            "1 character selected"
        );
        assert_eq!(
            caret_status(text, 6, Some((0, 5)), 1),
            "5 characters selected"
        );
        assert_eq!(caret_status(text, 6, None, 3), "3 selection regions");
    }

    #[test]
    fn converts_indentation_both_ways() {
        assert_eq!(
            convert_indentation("\tx\n\t\ty\n  z", 4, false),
            "    x\n        y\n  z"
        );
        assert_eq!(convert_indentation("    x\n      y", 4, true), "\tx\n\t  y");
    }
}
