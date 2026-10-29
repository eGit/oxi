//! Text editor preferences, named after their Sublime Text counterparts, with a live preview.

use eframe::egui::{self, Ui};

use crate::settings::{DrawWhiteSpace, EDITOR_FONT_SIZE_MAX, EDITOR_FONT_SIZE_MIN, EditorSettings};
use crate::theme::*;
use crate::ui::chrome::{
    SECTION_GAP, setting_row, setting_select, setting_switch, settings_section,
    settings_section_title,
};
use crate::ui::select_menu::SelectOption;

use super::super::OxiApp;

const PREVIEW: &str = "fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mid = sorted.len() / 2;
    Some(sorted[mid])
}";
/// The preview's caret line (0-based), for `highlight_line`.
const PREVIEW_CARET_LINE: usize = 3;

impl OxiApp {
    pub(super) fn render_settings_editor_panel(&mut self, ui: &mut Ui) {
        settings_section_title(
            ui,
            "Editor & terminal",
            Some("How files look and behave in the editor. Changes show as you make them."),
        );
        let mod_key = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl+"
        };
        let prefs = &mut self.conv.settings.editor;

        settings_section(ui, "Text", None, |ui| {
            let help = format!(
                "{mod_key}= and {mod_key}- change it while a file is open; {mod_key}0 resets. \
                 The code font is under General."
            );
            setting_row(ui, "Font size", Some(&help), |ui| {
                ui.spacing_mut().slider_width = 150.0;
                ui.add(
                    egui::Slider::new(
                        &mut prefs.font_size,
                        EDITOR_FONT_SIZE_MIN..=EDITOR_FONT_SIZE_MAX,
                    )
                    .step_by(0.5)
                    .suffix(" pt"),
                )
            });
            ui.add_space(2.0);
            preview(ui, prefs);
            ui.add_space(10.0);
        });

        settings_section(ui, "Layout", None, |ui| {
            setting_switch(ui, &mut prefs.word_wrap, "Word wrap", None);
            if prefs.word_wrap {
                setting_switch(
                    ui,
                    &mut prefs.wrap_indent,
                    "Indent wrapped lines",
                    Some("Wrapped rows continue at the line's indentation."),
                );
            }
            setting_switch(ui, &mut prefs.minimap, "Minimap", None);
            setting_switch(
                ui,
                &mut prefs.scroll_past_end,
                "Scroll past end",
                Some("The last line can be scrolled up to the top of the view."),
            );
        });

        settings_section(ui, "Editing", None, |ui| {
            setting_switch(
                ui,
                &mut prefs.auto_complete,
                "Auto complete",
                Some("Offer words from the file while you type; Enter or Tab inserts one."),
            );
            setting_switch(
                ui,
                &mut prefs.highlight_line,
                "Highlight current line",
                None,
            );
            let current = prefs.draw_white_space;
            if let Some(mode) = setting_select(
                ui,
                "editor_draw_white_space",
                "Draw white space",
                None,
                current.label(),
                || {
                    DrawWhiteSpace::ALL
                        .iter()
                        .map(|mode| SelectOption::new(mode.label(), *mode == current))
                        .collect()
                },
            )
            .and_then(|i| DrawWhiteSpace::ALL.get(i))
            {
                prefs.draw_white_space = *mode;
            }
        });

        settings_section(
            ui,
            "On save",
            Some("Both are undone with the save's own undo step."),
            |ui| {
                setting_switch(
                    ui,
                    &mut prefs.trim_trailing_white_space_on_save,
                    "Trim trailing white space",
                    None,
                );
                setting_switch(
                    ui,
                    &mut prefs.ensure_newline_at_eof_on_save,
                    "Ensure newline at end of file",
                    None,
                );
            },
        );

        if *prefs != EditorSettings::default() {
            if crate::ui::chrome::ghost_button(ui, "Restore editor defaults", false).clicked() {
                *prefs = EditorSettings::default();
            }
            ui.add_space(SECTION_GAP);
        }

        self.render_settings_terminal_section(ui);
    }
}

/// A few lines of code drawn the way the editor will: font size, line numbers, the caret line
/// and white space markers follow the preferences.
fn preview(ui: &mut Ui, prefs: &EditorSettings) {
    let size = prefs
        .font_size
        .clamp(EDITOR_FONT_SIZE_MIN, EDITOR_FONT_SIZE_MAX);
    let font = egui::FontId::monospace(size);
    let job = crate::theme::highlight_code_async(PREVIEW, "rs", font.clone(), ui.ctx())
        .unwrap_or_else(|| {
            egui::text::LayoutJob::simple(PREVIEW.to_owned(), font.clone(), c_text(), f32::INFINITY)
        });
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let digit = ui.fonts_mut(|fonts| fonts.glyph_width(&font, '0'));
    let gutter = digit * 2.0 + 22.0;
    let pad = egui::vec2(10.0, 8.0);
    let height = galley.rect.height() + pad.y * 2.0;
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect(
        rect,
        RADIUS_CHIP,
        c_bg_main(),
        egui::Stroke::new(1.0, c_border_subtle()),
        egui::StrokeKind::Inside,
    );
    painter.vline(
        rect.left() + gutter,
        rect.y_range(),
        egui::Stroke::new(1.0, c_border_subtle()),
    );
    let text_origin = egui::pos2(rect.left() + gutter + pad.x, rect.top() + pad.y);
    for (line, row) in galley.rows.iter().enumerate() {
        let top = text_origin.y + row.min_y();
        let row_rect = egui::Rect::from_x_y_ranges(rect.x_range(), top..=top + row.rect().height());
        let caret_line = line == PREVIEW_CARET_LINE;
        if caret_line && prefs.highlight_line {
            painter.rect_filled(
                row_rect.shrink2(egui::vec2(1.0, 0.0)),
                0.0,
                crate::theme::blend_color(c_row_hover(), c_bg_main(), 0.6),
            );
        }
        painter.text(
            egui::pos2(rect.left() + gutter - 10.0, row_rect.center().y),
            egui::Align2::RIGHT_CENTER,
            line + 1,
            font.clone(),
            if caret_line {
                c_text_muted()
            } else {
                c_text_faint()
            },
        );
        if prefs.draw_white_space == DrawWhiteSpace::All {
            for glyph in row.glyphs.iter().take_while(|glyph| glyph.chr == ' ') {
                painter.text(
                    egui::pos2(
                        text_origin.x + row.pos.x + glyph.pos.x + glyph.advance_width * 0.5,
                        row_rect.center().y,
                    ),
                    egui::Align2::CENTER_CENTER,
                    "·",
                    egui::FontId::monospace(size * (FS_TINY / FS_SMALL)),
                    crate::theme::blend_color(c_text_faint(), c_bg_main(), 0.4),
                );
            }
        }
    }
    painter.galley(text_origin, galley, c_text());
}
