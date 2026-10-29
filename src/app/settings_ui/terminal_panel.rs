//! Embedded terminal settings: a section at the end of Settings → Editor & terminal.

use eframe::egui::Ui;

use crate::ui::chrome::settings_section;

use super::super::OxiApp;

impl OxiApp {
    pub(super) fn render_settings_terminal_section(&mut self, ui: &mut Ui) {
        settings_section(ui, "Terminal", None, |ui| {
            #[cfg(windows)]
            {
                use crate::settings::WindowsTerminal;
                use crate::ui::select_menu::SelectOption;
                let current = self.conv.settings.windows_terminal;
                let wsl_available = crate::terminal::wsl_available();
                // Only offered when installed: picking it would start a shell that can't run.
                let choices: Vec<WindowsTerminal> = WindowsTerminal::ALL
                    .into_iter()
                    .filter(|terminal| *terminal != WindowsTerminal::Wsl || wsl_available)
                    .collect();
                let picked = crate::ui::chrome::setting_select(
                    ui,
                    "windows_terminal",
                    "Shell",
                    Some("Used the next time the terminal starts."),
                    current.label(),
                    || {
                        choices
                            .iter()
                            .map(|terminal| {
                                SelectOption::new(terminal.label(), *terminal == current)
                            })
                            .collect()
                    },
                );
                if let Some(terminal) = picked
                    .and_then(|i| choices.get(i))
                    .copied()
                    .filter(|terminal| *terminal != current)
                {
                    self.conv.settings.windows_terminal = terminal;
                    // Ensure the next opened/restarted panel uses the selected shell.
                    self.terminals.clear();
                    self.active_terminal = 0;
                }
            }

            // Only Windows has a choice to make; elsewhere the row just says which shell runs.
            #[cfg(not(windows))]
            {
                use eframe::egui::RichText;

                use crate::theme::{FS_SMALL, c_text_muted};
                let shell = std::env::var("SHELL").unwrap_or_default();
                let shell = if shell.is_empty() {
                    "login shell".to_owned()
                } else {
                    shell
                };
                crate::ui::chrome::setting_row(
                    ui,
                    "Shell",
                    Some("Your login shell. Change it to use a different one."),
                    |ui| {
                        ui.label(
                            RichText::new(shell)
                                .size(FS_SMALL)
                                .color(c_text_muted())
                                .monospace(),
                        )
                    },
                );
            }
        });
    }
}
