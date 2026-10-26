//! "N files changed" card under an agent turn that changed the workspace: the files with their
//! line counts, a diff per file (against the snapshot taken before the turn, with per-block
//! revert), and reverting one file or the whole turn.

use std::collections::{HashMap, HashSet};

use eframe::egui::{self, FontId, RichText, Sense, Ui};

use super::OxiApp;
use crate::git::{CompareFile, GitOp};
use crate::model::TurnChanges;
use crate::theme::*;

/// Per-turn view state, keyed by the turn's `before..after` trees.
#[derive(Default)]
pub(crate) struct TurnChangesCache {
    entries: HashMap<String, TurnEntry>,
}

struct TurnEntry {
    files: Result<Vec<CompareFile>, String>,
    expanded: bool,
    /// Paths put back from this card ("*" for the whole turn).
    reverted: HashSet<String>,
}

/// Inset of the Revert button from the card's top, right and bottom edges.
const CARD_INSET: f32 = 6.0;
/// The header row's height: the Revert button's.
const HEADER_H: f32 = 26.0;
const FILE_ROW_H: f32 = 22.0;
/// Chevron width plus its gap, so file rows start under "N files changed".
const FILE_ROW_INDENT: f32 = 14.0;

fn cache_key(changes: &TurnChanges) -> String {
    format!("{}..{}", changes.before, changes.after)
}

enum CardAction {
    Toggle,
    OpenDiff(String),
    RevertFile(String),
    RevertAll,
    Review(crate::settings::LlmProviderKind),
}

impl OxiApp {
    /// `author`: the agent whose response made the changes, when known.
    pub(crate) fn render_turn_changes(
        &mut self,
        ui: &mut Ui,
        changes: &TurnChanges,
        author: Option<crate::settings::LlmProviderKind>,
    ) {
        let key = cache_key(changes);
        let repo = self.turn_repo(&changes.repo);
        let entry = self
            .conv
            .transcript
            .turn_changes
            .entries
            .entry(key.clone())
            .or_insert_with(|| TurnEntry {
                files: crate::git::checkpoint::changed_files(
                    &repo,
                    &changes.before,
                    &changes.after,
                ),
                expanded: false,
                reverted: HashSet::new(),
            });
        let files = match &entry.files {
            Ok(files) if !files.is_empty() => files.clone(),
            // Snapshot gone (gc'd) or nothing changed: no card.
            _ => return,
        };
        let expanded = entry.expanded;
        let all_reverted = entry.reverted.contains("*");
        let reverted = entry.reverted.clone();
        let reviewers = if all_reverted {
            Vec::new()
        } else {
            let session = self.active_session_key();
            self.review_candidates(session, author)
        };
        let busy = self.active_waiting_response();
        let (added, deleted) = files
            .iter()
            .fold((0, 0), |(a, d), f| (a + f.added, d + f.deleted));

        let mut action = None;
        egui::Frame::new()
            .fill(c_bg_elevated())
            .stroke(egui::Stroke::new(1.0, c_border()))
            .corner_radius(crate::theme::RADIUS_PANEL)
            // The Revert button sits CARD_INSET from the top, right and bottom edges, so its
            // corners run concentric with the card's.
            .inner_margin(egui::Margin {
                left: 12,
                right: CARD_INSET as i8,
                top: CARD_INSET as i8,
                bottom: CARD_INSET as i8,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    // Same height with or without the button ("Reverted" is a plain label).
                    ui.set_min_height(HEADER_H);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let chevron = if expanded {
                        ICON_ANGLE_DOWN
                    } else {
                        ICON_CHEVRON_RIGHT
                    };
                    let icon = ui
                        .add(
                            egui::Label::new(
                                RichText::new(chevron)
                                    .font(FontId::new(FS_TINY, icon_font()))
                                    .color(c_text_muted()),
                            )
                            .sense(Sense::click())
                            .selectable(false),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    let header = ui
                        .add(
                            egui::Label::new(
                                RichText::new(format!(
                                    "{} file{} changed",
                                    files.len(),
                                    if files.len() == 1 { "" } else { "s" }
                                ))
                                .size(FS_SMALL)
                                .color(c_text()),
                            )
                            .sense(Sense::click())
                            .selectable(false),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if header.clicked() || icon.clicked() {
                        action = Some(CardAction::Toggle);
                    }
                    ui.label(
                        RichText::new(format!("+{added}"))
                            .font(FontId::monospace(FS_TINY))
                            .color(c_success()),
                    );
                    ui.label(
                        RichText::new(format!("−{deleted}"))
                            .font(FontId::monospace(FS_TINY))
                            .color(c_danger()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if all_reverted {
                            ui.label(
                                RichText::new("Reverted")
                                    .size(FS_TINY)
                                    .color(c_text_muted()),
                            );
                        } else if crate::ui::chrome::ghost_button_icon(
                            ui, ICON_UNDO, "Revert", true,
                        )
                        .on_hover_text("Put back every file this response changed")
                        .clicked()
                        {
                            action = Some(CardAction::RevertAll);
                        }
                        if let Some(&reviewer) = reviewers.first() {
                            let label = crate::app::composer::composer_provider_label;
                            let button = crate::ui::chrome::ghost_button_icon_enabled(
                                ui,
                                ICON_SEARCH,
                                &format!("Review with {}", label(reviewer)),
                                false,
                                !busy,
                            )
                            .on_hover_text(
                                "Ask another agent to review these changes in this chat \
                                 (right-click to pick the agent)",
                            );
                            if button.clicked() {
                                action = Some(CardAction::Review(reviewer));
                            }
                            if !busy {
                                button.context_menu(|ui| {
                                    for &other in &reviewers {
                                        if ui
                                            .button(format!("Review with {}", label(other)))
                                            .clicked()
                                        {
                                            action = Some(CardAction::Review(other));
                                            ui.close();
                                        }
                                    }
                                });
                            }
                        }
                    });
                });
                if expanded {
                    ui.add_space(4.0);
                    for file in &files {
                        let file_reverted = all_reverted || reverted.contains(&file.path);
                        if let Some(a) = render_file_row(ui, file, file_reverted) {
                            action = Some(a);
                        }
                    }
                }
            });
        // Breathing room before the next turn, like a user bubble leaves after itself.
        ui.add_space(12.0);

        match action {
            Some(CardAction::Toggle) => {
                if let Some(entry) = self.conv.transcript.turn_changes.entries.get_mut(&key) {
                    entry.expanded = !entry.expanded;
                }
            }
            Some(CardAction::OpenDiff(path)) => {
                let base = format!(
                    "{}{}",
                    crate::git::checkpoint::TURN_BASE_PREFIX,
                    changes.before
                );
                let source = crate::app::file_explorer::DiffSource::Compare {
                    base: base.clone(),
                    old_path: None,
                };
                if !self.open_diff_editor(&path, source, None) {
                    self.request(GitOp::ShowCompareDiff {
                        base,
                        path,
                        old_path: None,
                    });
                    self.conv.diff_view.open = true;
                    self.conv.editor.diff_tab_active = true;
                }
            }
            Some(CardAction::RevertFile(path)) => {
                self.revert_turn_changes(changes, &key, Some(path));
            }
            Some(CardAction::RevertAll) => self.revert_turn_changes(changes, &key, None),
            Some(CardAction::Review(reviewer)) => {
                self.review_turn_changes(changes, author, reviewer)
            }
            None => {}
        }
    }

    fn revert_turn_changes(&mut self, changes: &TurnChanges, key: &str, only: Option<String>) {
        let result = crate::git::checkpoint::restore(
            &self.turn_repo(&changes.repo),
            &changes.before,
            &changes.after,
            only.as_deref(),
        );
        match result {
            Ok(()) => {
                if let Some(entry) = self.conv.transcript.turn_changes.entries.get_mut(key) {
                    entry.reverted.insert(only.unwrap_or_else(|| "*".into()));
                }
                self.conv.explorer.cache.invalidate();
                self.request(GitOp::Refresh);
                self.notify_composer("Changes reverted.");
            }
            Err(e) => self.notify_composer(format!("Revert failed: {e}")),
        }
    }
}

fn render_file_row(ui: &mut Ui, file: &CompareFile, reverted: bool) -> Option<CardAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.set_min_height(FILE_ROW_H);
        // Line up the status letter with the header text, past the chevron.
        ui.add_space(FILE_ROW_INDENT);
        ui.spacing_mut().item_spacing.x = 6.0;
        // Right side first, so a long path truncates instead of pushing the counts and the
        // revert button out of the card.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Keep the icons off the card edge, inset like the Revert button's label.
            ui.add_space(8.0);
            if !reverted
                && ui
                    .add(
                        egui::Button::new(
                            RichText::new(ICON_UNDO)
                                .font(FontId::new(FS_TINY, icon_font()))
                                .color(c_text_muted()),
                        )
                        .frame(false),
                    )
                    .on_hover_text("Revert this file")
                    .clicked()
            {
                action = Some(CardAction::RevertFile(file.path.clone()));
            }
            if file.deleted > 0 {
                ui.label(
                    RichText::new(format!("−{}", file.deleted))
                        .font(FontId::monospace(FS_TINY))
                        .color(c_danger()),
                );
            }
            if file.added > 0 {
                ui.label(
                    RichText::new(format!("+{}", file.added))
                        .font(FontId::monospace(FS_TINY))
                        .color(c_success()),
                );
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(file.status.to_string())
                        .font(FontId::monospace(FS_TINY))
                        .color(crate::app::file_explorer::git_status_color(file.status)),
                );
                let name = RichText::new(&file.path).size(FS_SMALL);
                let name = if reverted || file.status == 'D' {
                    name.strikethrough().color(c_text_muted())
                } else {
                    name.color(c_text())
                };
                let row = ui
                    .add(
                        egui::Label::new(name)
                            .truncate()
                            .sense(Sense::click())
                            .selectable(false),
                    )
                    .on_hover_text("Show the diff against the files before this response")
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                if row.clicked() && !reverted {
                    action = Some(CardAction::OpenDiff(file.path.clone()));
                }
            });
        });
    });
    action
}
