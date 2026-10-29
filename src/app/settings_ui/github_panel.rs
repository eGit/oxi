//! Git identity and GitHub authentication settings.

use super::super::OxiApp;
use crate::ui::chrome::{
    CONTROL_W, ghost_button, setting_row, setting_switch, settings_section, settings_section_title,
    settings_text_field_width,
};
use eframe::egui::{self, Ui};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

/// `git --version` results keyed by the configured executable path. Probed on a worker thread so
/// typing a path never blocks the UI on process spawns.
#[derive(Clone)]
enum GitProbe {
    Pending,
    Done(Result<String, String>),
}

fn git_probe(ctx: &egui::Context, configured: &str) -> GitProbe {
    static PROBES: OnceLock<Mutex<HashMap<String, GitProbe>>> = OnceLock::new();
    let probes = PROBES.get_or_init(Default::default);
    let key = configured.trim().to_owned();
    let Ok(mut cache) = probes.lock() else {
        return GitProbe::Pending;
    };
    if let Some(state) = cache.get(&key) {
        return state.clone();
    }
    cache.insert(key.clone(), GitProbe::Pending);
    drop(cache);
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let result = crate::git::system_git_version(&key);
        if let Ok(mut cache) = probes.lock() {
            cache.insert(key, GitProbe::Done(result));
        }
        ctx.request_repaint();
    });
    GitProbe::Pending
}

impl OxiApp {
    pub(super) fn render_settings_github_panel(&mut self, ui: &mut Ui) {
        settings_section_title(
            ui,
            "Git & GitHub",
            Some("Who commits are from, and how push, pull and fetch sign in."),
        );

        settings_section(
            ui,
            "Commit identity",
            Some("Leave both empty to use the repository's or your global Git config."),
            |ui| {
                setting_row(ui, "Author name", None, |ui| {
                    settings_text_field_width(
                        ui,
                        &mut self.conv.settings.git_author_name,
                        "Your name",
                        CONTROL_W + 40.0,
                    )
                });
                setting_row(ui, "Author email", None, |ui| {
                    settings_text_field_width(
                        ui,
                        &mut self.conv.settings.git_author_email,
                        "you@example.com",
                        CONTROL_W + 40.0,
                    )
                });
            },
        );

        settings_section(
            ui,
            "GitHub authentication",
            Some(
                "A fine-grained personal access token with Contents: Read and write on your repositories. It is kept in the OS keychain.",
            ),
            |ui| {
                setting_row(
                    ui,
                    "GitHub username",
                    Some("Your account name, not your email."),
                    |ui| {
                        settings_text_field_width(
                            ui,
                            &mut self.conv.settings.github_username,
                            "octocat",
                            CONTROL_W + 40.0,
                        )
                    },
                );
                let token_set = !self.conv.settings.github_token.trim().is_empty();
                let help = if self.conv.settings.git_use_system_cli {
                    "Not used while system Git handles push, pull and fetch."
                } else if token_set {
                    "Configured."
                } else {
                    "Organization repositories may also need SSO authorization."
                };
                setting_row(ui, "Personal access token", Some(help), |ui| {
                    if token_set {
                        if ghost_button(ui, "Clear", true).clicked() {
                            self.conv.settings.github_token.clear();
                        }
                    } else if ghost_button(ui, "Create on GitHub", false).clicked() {
                        let _ = webbrowser::open(
                            "https://github.com/settings/personal-access-tokens/new?name=oxi&description=Native%20Git%20push%20from%20oxi&contents=write",
                        );
                    }
                    ui.add_space(6.0);
                    crate::ui::chrome::settings_password_field_width(
                        ui,
                        &mut self.conv.settings.github_token,
                        "github_pat_\u{2026}",
                        CONTROL_W - 40.0,
                    )
                });
            },
        );

        settings_section(ui, "System Git", None, |ui| {
            setting_switch(
                ui,
                &mut self.conv.settings.git_use_system_cli,
                "Use system Git for push, pull, and fetch",
                Some(
                    "For remotes that reject tokens, like company servers: uses your SSH keys and credential manager. Status, diffs and commits still run on the built-in engine.",
                ),
            );
            if !self.conv.settings.git_use_system_cli {
                return;
            }
            let probe = git_probe(ui.ctx(), &self.conv.settings.git_executable);
            let help = match &probe {
                GitProbe::Pending => "Detecting Git\u{2026}".to_owned(),
                GitProbe::Done(Ok(version)) => version.clone(),
                GitProbe::Done(Err(error)) => format!("Not found: {error}"),
            };
            setting_row(ui, "Git executable", Some(&help), |ui| {
                settings_text_field_width(
                    ui,
                    &mut self.conv.settings.git_executable,
                    "git on PATH",
                    CONTROL_W + 40.0,
                )
            });
        });
    }
}
