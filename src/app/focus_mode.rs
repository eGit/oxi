//! Distraction Free mode, as in Sublime (Ctrl+Cmd+Shift+F on macOS, Shift+F11 elsewhere): the
//! sidebar, Git panel, terminal and status bar step aside and the chat (or editor) has the window.
//! The same shortcut brings back exactly the panels that were open.
//!
//! Only the panels' on-screen state changes; the saved layout in settings is left alone, so
//! quitting in this mode still restores the usual layout on the next start.

use super::OxiApp;

/// Panels that were open when Distraction Free mode started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PanelsBefore {
    sidebar: bool,
    git: bool,
    terminal: bool,
}

impl OxiApp {
    pub(crate) fn distraction_free(&self) -> bool {
        self.distraction_free.is_some()
    }

    pub(crate) fn toggle_distraction_free(&mut self) {
        match self.distraction_free.take() {
            Some(before) => {
                // A panel opened meanwhile stays open; the rest come back as they were.
                self.conv.sidebar.open |= before.sidebar;
                self.conv.git_ui.open |= before.git;
                self.conv.terminal_panel.open |= before.terminal;
            }
            None => {
                self.distraction_free = Some(PanelsBefore {
                    sidebar: self.conv.sidebar.open,
                    git: self.conv.git_ui.open,
                    terminal: self.conv.terminal_panel.open,
                });
                self.conv.sidebar.open = false;
                self.conv.git_ui.open = false;
                self.conv.terminal_panel.open = false;
                let shortcut = if cfg!(target_os = "macos") {
                    "⌃⇧⌘F"
                } else {
                    "Shift+F11"
                };
                self.notify_composer(format!("Distraction Free · {shortcut} to leave"));
            }
        }
        self.focus_active_view_next_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaving_restores_the_panels_and_keeps_ones_opened_meanwhile() {
        crate::secrets::use_mock_store();
        let mut app = OxiApp::new();
        app.conv.sidebar.open = true;
        app.conv.git_ui.open = true;
        app.conv.terminal_panel.open = false;
        let saved_git = app.conv.settings.git_open;

        app.toggle_distraction_free();
        assert!(app.distraction_free());
        assert!(!app.conv.sidebar.open && !app.conv.git_ui.open);
        assert_eq!(
            app.conv.settings.git_open, saved_git,
            "the saved layout is untouched"
        );

        app.conv.terminal_panel.open = true;
        app.toggle_distraction_free();
        assert!(!app.distraction_free());
        assert!(app.conv.sidebar.open && app.conv.git_ui.open);
        assert!(
            app.conv.terminal_panel.open,
            "opened while focused, so it stays"
        );
    }
}
