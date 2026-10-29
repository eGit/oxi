//! Shared confirmation flow for destructive actions: one pending [`ConfirmAction`]
//! app-wide, rendered as a centered modal over everything (see `chrome::confirm_modal`).

use eframe::egui;

use crate::ui::chrome::{ModalOutcome, confirm_modal};

use super::OxiApp;
use super::state::ConfirmAction;

impl OxiApp {
    /// Queue a destructive action for confirmation. The modal renders on top of the
    /// whole window at the end of the frame.
    pub(crate) fn request_confirm(&mut self, mut action: ConfirmAction) {
        // Removing the work tree too is the default when nothing else in it would go: its
        // last chat, or the whole work tree workspace.
        let default_remove = match action {
            ConfirmAction::DeleteSession { wi, .. } => self
                .conv
                .workspaces
                .get(wi)
                .is_some_and(|w| w.worktree.is_some() && w.sessions.len() == 1),
            ConfirmAction::DeleteWorkspace { wi, .. } => self
                .conv
                .workspaces
                .get(wi)
                .is_some_and(|w| w.worktree.is_some()),
            _ => false,
        };
        if let Some(flag) = remove_worktree_flag(&mut action) {
            *flag = default_remove;
        }
        self.conv.confirm_prompt = Some(action);
    }

    /// True while the confirmation modal is up — used to suppress Enter-to-send in
    /// the composer, which would otherwise fire together with Enter-to-confirm.
    pub(crate) fn confirm_prompt_open(&self) -> bool {
        self.conv.confirm_prompt.is_some() || self.conv.editor.prompt.is_some()
    }

    pub(crate) fn render_confirm_prompt(&mut self, ctx: &egui::Context) {
        let Some(action) = self.conv.confirm_prompt.clone() else {
            return;
        };

        let (title, body, note, confirm_label) = self.confirm_texts(&action);
        let option_label = self.confirm_option_label(&action);
        let mut option_value = matches!(
            action,
            ConfirmAction::DeleteSession {
                remove_worktree: true,
                ..
            } | ConfirmAction::DeleteWorkspace {
                remove_worktree: true,
                ..
            }
        );
        let outcome = confirm_modal(
            ctx,
            "app_confirm_prompt",
            &title,
            &body,
            note.as_deref(),
            option_label
                .as_deref()
                .map(|label| (label, &mut option_value)),
            &confirm_label,
        );
        if let Some(flag) = self
            .conv
            .confirm_prompt
            .as_mut()
            .and_then(remove_worktree_flag)
        {
            *flag = option_value && option_label.is_some();
        }
        match outcome {
            ModalOutcome::Open => {}
            ModalOutcome::Cancelled => self.conv.confirm_prompt = None,
            ModalOutcome::Confirmed => {
                if let Some(action) = self.conv.confirm_prompt.take() {
                    self.execute_confirmed(ctx, action);
                }
            }
        }
    }

    /// Checkbox offered next to the action, if any: deleting a chat in a work tree can take
    /// the work tree with it, unless another of its chats is still running; so can removing
    /// the work tree workspace (the sidebar only offers that while none of its chats run).
    fn confirm_option_label(&self, action: &ConfirmAction) -> Option<String> {
        if let ConfirmAction::DeleteWorkspace { wi, .. } = action {
            let info = self.conv.workspaces.get(*wi)?.worktree.as_ref()?;
            return Some(format!(
                "Also delete the work tree folder and branch \u{201c}{}\u{201d}",
                info.branch
            ));
        }
        let ConfirmAction::DeleteSession { wi, si, .. } = action else {
            return None;
        };
        let workspace = self.conv.workspaces.get(*wi)?;
        let info = workspace.worktree.as_ref()?;
        let others = workspace.sessions.len().saturating_sub(1);
        if (0..workspace.sessions.len()).any(|i| i != *si && self.session_row_is_running(*wi, i)) {
            return None;
        }
        Some(match others {
            0 => format!("Also remove the work tree \u{201c}{}\u{201d}", info.branch),
            1 => format!(
                "Also remove the work tree \u{201c}{}\u{201d} and its other chat",
                info.branch
            ),
            n => format!(
                "Also remove the work tree \u{201c}{}\u{201d} and its {n} other chats",
                info.branch
            ),
        })
    }

    fn confirm_texts(&self, action: &ConfirmAction) -> (String, String, Option<String>, String) {
        match action {
            ConfirmAction::DeleteSession {
                wi,
                si,
                remove_worktree,
            } => {
                let name = self
                    .conv
                    .workspaces
                    .get(*wi)
                    .and_then(|w| w.sessions.get(*si))
                    .map(|s| s.title.clone())
                    .unwrap_or_else(|| "this chat".to_string());
                let body = if *remove_worktree {
                    format!(
                        "\u{201c}{name}\u{201d} and its history will be deleted, together with \
                         the work tree folder and branch, including changes that were not merged."
                    )
                } else {
                    format!("\u{201c}{name}\u{201d} and its history will be deleted.")
                };
                (
                    "Delete chat?".to_string(),
                    body,
                    Some("This action cannot be undone.".to_string()),
                    "Delete".to_string(),
                )
            }
            ConfirmAction::DeleteWorkspace {
                wi,
                remove_worktree,
            } => {
                let name = self
                    .conv
                    .workspaces
                    .get(*wi)
                    .map(|w| crate::theme::workspace_sidebar_label(&w.root_path))
                    .unwrap_or_else(|| "this workspace".to_string());
                if *remove_worktree {
                    return (
                        "Remove workspace?".to_string(),
                        format!(
                            "\u{201c}{name}\u{201d} will be removed from the sidebar, and its \
                             work tree folder and branch deleted, including changes that were \
                             not merged."
                        ),
                        Some("This action cannot be undone.".to_string()),
                        "Remove".to_string(),
                    );
                }
                (
                    "Remove workspace?".to_string(),
                    format!(
                        "\u{201c}{name}\u{201d} will be removed from the sidebar. Files on \
                         disk are kept; re-add the folder to see its chats again."
                    ),
                    None,
                    "Remove".to_string(),
                )
            }
            ConfirmAction::RemoveWorktree { wi } => {
                let branch = self
                    .conv
                    .workspaces
                    .get(*wi)
                    .and_then(|w| w.worktree.as_ref())
                    .map(|w| w.branch.clone())
                    .unwrap_or_default();
                (
                    "Remove work tree?".to_string(),
                    format!(
                        "The work tree folder and branch \u{201c}{branch}\u{201d} will be \
                         deleted, including changes that were not merged."
                    ),
                    Some("This action cannot be undone.".to_string()),
                    "Remove".to_string(),
                )
            }
            ConfirmAction::GitDiscard { paths } => {
                let body = if let [path] = paths.as_slice() {
                    format!("This will permanently discard changes in {path}.")
                } else {
                    format!(
                        "This will permanently discard changes in {} files.",
                        paths.len()
                    )
                };
                (
                    "Discard changes?".to_string(),
                    body,
                    Some("This action cannot be undone.".to_string()),
                    "Discard".to_string(),
                )
            }
            ConfirmAction::GitForceDeleteBranch { name } => (
                "Delete unmerged branch?".to_string(),
                format!(
                    "\u{201c}{name}\u{201d} has commits that are not in the current branch \
                     or its upstream. Deleting it drops them."
                ),
                Some("This action cannot be undone.".to_string()),
                "Delete".to_string(),
            ),
            ConfirmAction::DeleteLocalModel { id } => (
                "Delete model?".to_string(),
                format!("{id} will be deleted from disk."),
                None,
                "Delete".to_string(),
            ),
            ConfirmAction::DeleteRemoteModel { id, .. } => (
                "Delete remote model?".to_string(),
                format!("{id} will be deleted from the SSH host."),
                None,
                "Delete".to_string(),
            ),
            ConfirmAction::DeleteVoiceModel { id } => (
                "Delete voice model?".to_string(),
                format!("{id} will be deleted from disk."),
                None,
                "Delete".to_string(),
            ),
            ConfirmAction::ReplaceInFiles { matches, files } => (
                "Replace in files?".to_string(),
                format!(
                    "Replace {matches} {} in {files} {}. Files open in the editor are changed \
                     there and left unsaved; the others are written to disk.",
                    if *matches == 1 { "match" } else { "matches" },
                    if *files == 1 { "file" } else { "files" },
                ),
                Some("Files written to disk cannot be restored with Undo.".to_string()),
                "Replace".to_string(),
            ),
        }
    }

    fn execute_confirmed(&mut self, ctx: &egui::Context, action: ConfirmAction) {
        match action {
            ConfirmAction::DeleteSession {
                wi,
                si,
                remove_worktree,
            } => {
                // Sessions can only be deleted from the active workspace (the sidebar
                // enforces this); guard anyway in case state moved under the modal.
                if wi == self.conv.active_workspace {
                    self.delete_session(si);
                    if remove_worktree {
                        self.remove_worktree(wi);
                    }
                }
            }
            ConfirmAction::DeleteWorkspace {
                wi,
                remove_worktree: true,
            } => self.remove_worktree(wi),
            ConfirmAction::DeleteWorkspace { wi, .. } => self.delete_workspace(wi),
            ConfirmAction::RemoveWorktree { wi } => self.remove_worktree(wi),
            ConfirmAction::GitDiscard { paths } => {
                self.request(crate::git::GitOp::Discard(paths));
            }
            ConfirmAction::GitForceDeleteBranch { name } => {
                self.request(crate::git::GitOp::DeleteBranch { name, force: true });
            }
            ConfirmAction::DeleteLocalModel { id } => {
                if let Err(e) = crate::local_models::remove_downloaded(&id) {
                    self.conv.local_models.runtime_status = Some(format!("Delete failed: {e}"));
                }
                self.conv.local_models.downloaded = crate::local_models::load_manifest().models;
            }
            ConfirmAction::DeleteRemoteModel { id, path } => {
                self.spawn_remote_model_delete(ctx, id, path);
            }
            ConfirmAction::DeleteVoiceModel { id } => {
                if let Err(e) = crate::voice_models::remove_downloaded(&id) {
                    self.conv.voice_ui.download_error = Some(format!("Delete failed: {e}"));
                }
                self.conv.voice_ui.downloaded = crate::voice_models::load_manifest().models;
                if self.conv.settings.dictation.model_id.as_deref() == Some(id.as_str()) {
                    self.conv.settings.dictation.model_id = None;
                }
            }
            ConfirmAction::ReplaceInFiles { .. } => self.replace_in_files(),
        }
    }
}

/// The "also remove the work tree" flag of actions that offer it.
fn remove_worktree_flag(action: &mut ConfirmAction) -> Option<&mut bool> {
    match action {
        ConfirmAction::DeleteSession {
            remove_worktree, ..
        }
        | ConfirmAction::DeleteWorkspace {
            remove_worktree, ..
        } => Some(remove_worktree),
        _ => None,
    }
}
