//! Settings building blocks: titled sections holding label-left / control-right rows, so every
//! settings page is laid out the same way and uses the controls the chat already uses (the
//! composer's [`SelectMenu`](crate::ui::select_menu::SelectMenu), the same switch, the same
//! text fields).

use eframe::egui::{self, Align, Layout, RichText, Sense, Stroke, Ui, UiBuilder};

use super::{card_frame, settings_card_header, settings_reveal_flash, toggle};
use crate::theme::*;
use crate::ui::select_menu::{SelectMenu, SelectOption};

/// Vertical padding above and below each row's content.
const ROW_PAD: f32 = 9.0;
/// Gap between a row's text and its control.
const ROW_GAP: f32 = 20.0;
/// Space between two sections on a page.
pub const SECTION_GAP: f32 = 30.0;
/// Width of select and short text controls on the right of a row.
pub const CONTROL_W: f32 = 220.0;

/// A titled group of rows: one card, the title and an optional one-line description at its
/// top, rows separated by hairlines.
pub fn settings_section<R>(
    ui: &mut Ui,
    title: &str,
    help: Option<&str>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    card_frame()
        .show(ui, |ui| {
            settings_card_header(ui, title, help);
            add_contents(ui)
        })
        .inner
}

/// Hairline between rows; nothing above the first thing in a section.
fn row_separator(ui: &mut Ui) {
    if ui.min_rect().height() > 0.5 {
        let y = ui.cursor().top();
        ui.painter().hline(
            ui.max_rect().x_range(),
            y,
            Stroke::new(1.0, c_border_subtle()),
        );
    }
}

/// The label + description column of a row, in `rect`.
fn row_text(ui: &mut Ui, rect: egui::Rect, label: &str, help: Option<&str>, id: egui::Id) -> egui::Rect {
    let mut text_ui = ui.new_child(
        UiBuilder::new()
            .id_salt(id.with("text"))
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
    );
    text_ui.spacing_mut().item_spacing.y = 2.0;
    text_ui.add(
        egui::Label::new(RichText::new(label).size(FS_SMALL).color(c_text()))
            .wrap()
            .selectable(false),
    );
    if let Some(help) = help.filter(|help| !help.is_empty()) {
        text_ui.add(
            egui::Label::new(RichText::new(help).size(FS_TINY).color(c_text_muted()))
                .wrap()
                .selectable(false),
        );
    }
    text_ui.min_rect()
}

/// One setting: label and optional description on the left, `control` on the right (laid out
/// right to left, so add the rightmost widget first).
pub fn setting_row<R>(
    ui: &mut Ui,
    label: &str,
    help: Option<&str>,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    row_separator(ui);
    let id = ui.id().with(("setting_row", label));
    let flash = ui.painter().add(egui::Shape::Noop);
    let full = ui.available_rect_before_wrap();
    let top = ui.cursor().top();
    let content_top = top + ROW_PAD;
    let mut control_ui = ui.new_child(
        UiBuilder::new()
            .id_salt(id.with("control"))
            .max_rect(egui::Rect::from_min_max(
                egui::pos2(full.left(), content_top),
                egui::pos2(full.right(), content_top + 4000.0),
            ))
            .layout(Layout::right_to_left(Align::Min)),
    );
    let inner = control(&mut control_ui);
    let control_rect = control_ui.min_rect();
    let text_right = if control_rect.width() > 0.5 {
        control_rect.left() - ROW_GAP
    } else {
        full.right()
    };
    // Center the label's first line on a typical control (switch, select, field ~24px).
    let label_nudge = 4.0;
    let text_rect = row_text(
        ui,
        egui::Rect::from_min_max(
            egui::pos2(full.left(), content_top + label_nudge),
            egui::pos2(text_right.max(full.left() + 60.0), content_top + 4000.0),
        ),
        label,
        help,
        id,
    );
    let bottom = control_rect.bottom().max(text_rect.bottom()) + ROW_PAD;
    let row = egui::Rect::from_min_max(egui::pos2(full.left(), top), egui::pos2(full.right(), bottom));
    settings_reveal_flash(ui, label, text_rect, flash);
    ui.advance_cursor_after_rect(row);
    inner
}

/// A setting whose control needs the full width (URLs, tokens, prompts, lists): label and
/// description on top, the control underneath.
pub fn setting_block<R>(
    ui: &mut Ui,
    label: &str,
    help: Option<&str>,
    control: impl FnOnce(&mut Ui) -> R,
) -> R {
    row_separator(ui);
    ui.add_space(ROW_PAD);
    let flash = ui.painter().add(egui::Shape::Noop);
    let full = ui.available_rect_before_wrap();
    let id = ui.id().with(("setting_block", label));
    let text_rect = row_text(
        ui,
        egui::Rect::from_min_max(full.min, egui::pos2(full.right(), full.top() + 4000.0)),
        label,
        help,
        id,
    );
    settings_reveal_flash(ui, label, text_rect, flash);
    ui.advance_cursor_after_rect(text_rect);
    ui.add_space(6.0);
    let inner = control(ui);
    ui.add_space(ROW_PAD);
    inner
}

/// A free-form item in a section (one MCP server, one model…), separated from its neighbors
/// like a row.
pub fn setting_item<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    row_separator(ui);
    ui.add_space(ROW_PAD);
    let inner = add_contents(ui);
    ui.add_space(ROW_PAD);
    inner
}

/// An on/off setting; `.changed()` on the result tells whether it flipped this frame.
pub fn setting_switch(ui: &mut Ui, on: &mut bool, label: &str, help: Option<&str>) -> egui::Response {
    setting_row(ui, label, help, |ui| toggle(ui, on, ""))
}

/// A pick-one setting using the composer's dropdown. Returns the index picked this frame.
pub fn setting_select(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    help: Option<&str>,
    current: &str,
    options: impl FnOnce() -> Vec<SelectOption>,
) -> Option<usize> {
    setting_row(ui, label, help, |ui| {
        SelectMenu::new(id_salt, current)
            .framed(CONTROL_W)
            .show(ui, options)
    })
}

/// A whole-number setting typed into a short field. Empty input means `empty`.
pub fn setting_number(
    ui: &mut Ui,
    label: &str,
    help: Option<&str>,
    value: &mut u32,
    empty: u32,
    clamp: std::ops::RangeInclusive<u32>,
) -> egui::Response {
    setting_row(ui, label, help, |ui| {
        // The draft lives in egui memory while the field is focused, so typing "12" into a
        // field showing "0" doesn't fight the parsed value on every key.
        let id = ui.id().with(("setting_number", label));
        let mut text = ui
            .data(|d| d.get_temp::<String>(id))
            .unwrap_or_else(|| value.to_string());
        let response = super::settings_text_field_width(ui, &mut text, &empty.to_string(), 72.0);
        if response.changed() {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                *value = empty;
            } else if let Ok(n) = trimmed.parse::<u32>() {
                *value = n.clamp(*clamp.start(), *clamp.end());
            }
        }
        if response.has_focus() {
            ui.data_mut(|d| d.insert_temp(id, text));
        } else {
            ui.data_mut(|d| d.remove::<String>(id));
        }
        response
    })
}

/// A folded group of rarely needed settings at the end of a section ("Advanced"). Opens by
/// itself when a Settings search jumps to one of `reveals` inside it.
pub fn settings_disclosure(
    ui: &mut Ui,
    title: &str,
    reveals: &[&str],
    add_contents: impl FnOnce(&mut Ui),
) {
    row_separator(ui);
    let id = ui.id().with(("settings_disclosure", title));
    let mut open = ui.data_mut(|d| *d.get_persisted_mut_or_default::<bool>(id));
    if let Some(pending) = super::settings_reveal_pending(ui.ctx())
        && reveals
            .iter()
            .chain(std::iter::once(&title))
            .any(|r| r.eq_ignore_ascii_case(&pending))
    {
        open = true;
    }
    ui.add_space(4.0);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), Sense::click());
    let hovered = response.hovered() || response.has_focus();
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        ui.painter().rect_filled(
            rect.expand2(egui::vec2(6.0, 0.0)),
            egui::CornerRadius::same(RADIUS_ROW),
            c_row_hover(),
        );
    }
    super::keyboard_focus_ring(ui, &response, egui::CornerRadius::same(RADIUS_ROW));
    let color = if hovered { c_text() } else { c_text_muted() };
    ui.painter().text(
        egui::pos2(rect.left() + 5.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        if open { ICON_ANGLE_DOWN } else { ICON_CHEVRON_RIGHT },
        egui::FontId::new(FS_TINY - 2.0, icon_font()),
        color,
    );
    let flash = ui.painter().add(egui::Shape::Noop);
    let galley = ui.painter().layout_no_wrap(
        title.to_owned(),
        egui::FontId::proportional(FS_SMALL),
        color,
    );
    let text_pos = egui::pos2(rect.left() + 16.0, rect.center().y - galley.size().y * 0.5);
    let text_rect = egui::Rect::from_min_size(text_pos, galley.size());
    ui.painter().galley(text_pos, galley, color);
    settings_reveal_flash(ui, title, text_rect, flash);
    if response.clicked() {
        open = !open;
    }
    ui.data_mut(|d| d.insert_persisted(id, open));
    ui.add_space(if open { 2.0 } else { 4.0 });
    if open {
        add_contents(ui);
    }
}
