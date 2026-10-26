//! Text editor preferences, named after their Sublime Text counterparts, with a live preview.

use eframe::egui::{self, RichText, Ui};

use crate::settings::{DrawWhiteSpace, EDITOR_FONT_SIZE_MAX, EDITOR_FONT_SIZE_MIN, EditorSettings};
use crate::theme::*;
use crate::ui::chrome::{
    card_frame, field_hint, hairline, settings_card_header, settings_section_title,
};

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
            "Editor",
            Some("How files look and behave in the editor. Changes show as you make them."),
        );
        let mod_key = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Ctrl+"
        };
        let prefs = &mut self.conv.settings.editor;

        card_frame().show(ui, |ui| {
            settings_card_header(
                ui,
                "Font size",
                Some("The code font itself is chosen under Appearance."),
            );
            ui.spacing_mut().slider_width = 260.0;
            ui.add(
                egui::Slider::new(
                    &mut prefs.font_size,
                    EDITOR_FONT_SIZE_MIN..=EDITOR_FONT_SIZE_MAX,
                )
                .step_by(0.5)
                .suffix(" pt"),
            );
            field_hint(
                ui,
                &format!(
                    "{mod_key}= and {mod_key}- change it while a file is open; {mod_key}0 resets."
                ),
            );
            ui.add_space(10.0);
            preview(ui, prefs);
        });

        ui.add_space(10.0);
        card_frame().show(ui, |ui| {
            settings_card_header(ui, "Layout", None);
            check(
                ui,
                &mut prefs.word_wrap,
                "Word wrap",
                "Long lines wrap at the window edge.",
            );
            ui.add_enabled_ui(prefs.word_wrap, |ui| {
                check(
                    ui,
                    &mut prefs.wrap_indent,
                    "Indent wrapped lines",
                    "Wrapped rows continue at the line's indentation.",
                );
            });
            check(
                ui,
                &mut prefs.minimap,
                "Minimap",
                "A zoomed-out map of the file beside the text.",
            );
            check(
                ui,
                &mut prefs.scroll_past_end,
                "Scroll past end",
                "The last line can be scrolled up to the top of the view.",
            );

            ui.add_space(8.0);
            hairline(ui);
            ui.add_space(10.0);
            settings_card_header(ui, "Highlights", None);
            check(
                ui,
                &mut prefs.auto_complete,
                "Auto complete",
                "Offer words from the file while you type; Enter or Tab inserts one.",
            );
            check(
                ui,
                &mut prefs.highlight_line,
                "Highlight current line",
                "Tint the line the caret is on.",
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Draw white space")
                        .size(FS_SMALL)
                        .color(c_text()),
                );
                ui.add_space(8.0);
                egui::ComboBox::from_id_salt("editor_draw_white_space")
                    .icon(crate::ui::chrome::combo_chevron_icon)
                    .selected_text(prefs.draw_white_space.label())
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for mode in DrawWhiteSpace::ALL {
                            ui.selectable_value(&mut prefs.draw_white_space, mode, mode.label());
                        }
                    });
            });
        });

        ui.add_space(10.0);
        card_frame().show(ui, |ui| {
            settings_card_header(ui, "On save", None);
            check(
                ui,
                &mut prefs.trim_trailing_white_space_on_save,
                "Trim trailing white space",
                "Spaces and tabs at the ends of lines are removed.",
            );
            check(
                ui,
                &mut prefs.ensure_newline_at_eof_on_save,
                "Ensure newline at end of file",
                "A file that doesn't end with a newline gets one.",
            );
            field_hint(ui, "Both are undoable with the save's own undo step.");
        });

        ui.add_space(10.0);
        if ui
            .add_enabled(
                *prefs != EditorSettings::default(),
                egui::Button::new(RichText::new("Restore defaults").size(FS_SMALL)),
            )
            .clicked()
        {
            *prefs = EditorSettings::default();
        }
    }
}

/// A toggle with its explanation underneath, aligned with the label.
fn check(ui: &mut Ui, value: &mut bool, label: &str, help: &str) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 1.0;
        crate::ui::chrome::toggle(
            ui,
            value,
            RichText::new(label).size(FS_SMALL).color(c_text()),
        );
        let indent = crate::ui::chrome::TOGGLE_W + crate::ui::chrome::TOGGLE_GAP;
        ui.horizontal(|ui| {
            ui.add_space(indent);
            ui.label(RichText::new(help).size(FS_TINY).color(c_text_faint()));
        });
    });
    ui.add_space(8.0);
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
