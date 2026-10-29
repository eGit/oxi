//! Settings → Voice: local speech-to-text dictation (see [`crate::voice_engine`],
//! [`crate::voice_models`]). Unlike the local-LLM HF panel there's a small fixed model
//! catalog instead of a HuggingFace search UI, since there's one canonical upstream repo.

use eframe::egui::{self, RichText, Ui};

use crate::app::task_runner::spawn_async_task;
use crate::theme::*;
use crate::ui::chrome::{
    alert_banner, ghost_button, ghost_button_icon, ghost_button_icon_enabled, setting_row,
    setting_switch, settings_section, settings_section_title, settings_text_field_width,
};
use crate::voice_models::{self, VOICE_MODEL_CATALOG, VoiceModelCatalogEntry, VoiceModelMsg};

use super::super::OxiApp;
use super::layout::active_pill;

impl OxiApp {
    pub(super) fn render_settings_voice_panel(&mut self, ui: &mut Ui) {
        settings_section_title(
            ui,
            "Voice",
            Some("Dictate into the composer. Speech is transcribed on this machine with Whisper."),
        );

        settings_section(ui, "Dictation", None, |ui| {
            setting_switch(
                ui,
                &mut self.conv.settings.dictation.enabled,
                "Enable dictation",
                Some("Shows the mic button in the composer. No model is loaded until you dictate."),
            );
            if !self.conv.settings.dictation.enabled {
                return;
            }
            setting_row(
                ui,
                "Language",
                Some("A code like en or ro, or auto to detect it."),
                |ui| {
                    let mut language = self.conv.settings.dictation.language.clone();
                    if settings_text_field_width(ui, &mut language, "auto", 72.0).changed() {
                        self.conv.settings.dictation.language = language;
                    }
                },
            );
            setting_row(
                ui,
                "Keep model loaded after use",
                Some("Faster next time, but holds the model in memory."),
                |ui| {
                    crate::ui::chrome::toggle(
                        ui,
                        &mut self.conv.settings.dictation.keep_loaded,
                        "",
                    );
                    if self.conv.settings.dictation.keep_loaded {
                        ui.add_space(8.0);
                        if ghost_button(ui, "Unload now", false)
                            .on_hover_text("Free the Whisper model from memory now.")
                            .clicked()
                        {
                            self.voice.unload();
                        }
                    }
                },
            );
        });

        if !self.conv.settings.dictation.enabled {
            return;
        }
        settings_section(
            ui,
            "Whisper model",
            Some("Larger models are more accurate and slower."),
            |ui| {
                for entry in VOICE_MODEL_CATALOG.iter() {
                    let downloaded = self
                        .conv
                        .voice_ui
                        .downloaded
                        .iter()
                        .find(|m| m.id == entry.id)
                        .cloned();
                    let is_active =
                        self.conv.settings.dictation.model_id.as_deref() == Some(entry.id);
                    let downloading =
                        self.conv.voice_ui.downloading_id.as_deref() == Some(entry.id);
                    let size = format!("~{} MB", entry.approx_mb);

                    setting_row(ui, entry.label, Some(&size), |ui| {
                        if let Some(m) = &downloaded {
                            if ghost_button_icon(ui, ICON_TRASH, "Delete", true).clicked() {
                                self.request_confirm(
                                    crate::app::state::ConfirmAction::DeleteVoiceModel {
                                        id: m.id.clone(),
                                    },
                                );
                            }
                            if is_active {
                                active_pill(ui, "Active");
                            } else if ghost_button(ui, "Make active", false).clicked() {
                                self.conv.settings.dictation.model_id = Some(m.id.clone());
                            }
                        } else if downloading {
                            let text = match self.conv.voice_ui.download_progress {
                                Some((done, Some(total))) if total > 0 => format!(
                                    "{:.0}% ({}/{})",
                                    done as f64 * 100.0 / total as f64,
                                    fmt_bytes(done),
                                    fmt_bytes(total)
                                ),
                                Some((done, _)) => fmt_bytes(done),
                                None => "…".to_string(),
                            };
                            ui.label(RichText::new(text).size(FS_TINY).color(c_text_muted()));
                        } else if ghost_button_icon_enabled(
                            ui,
                            ICON_DOWNLOAD,
                            "Download",
                            false,
                            self.conv.voice_ui.downloading_id.is_none(),
                        )
                        .clicked()
                        {
                            self.spawn_voice_download(ui.ctx(), entry);
                        }
                    });
                }

                if let Some(e) = self.conv.voice_ui.download_error.clone() {
                    ui.add_space(6.0);
                    alert_banner(ui, &e, true);
                }
                if let Some(e) = self.conv.voice_ui.error.clone() {
                    ui.add_space(8.0);
                    alert_banner(ui, &format!("Dictation error: {e}"), true);
                }
            },
        );
    }

    fn spawn_voice_download(
        &mut self,
        ctx: &egui::Context,
        entry: &'static VoiceModelCatalogEntry,
    ) {
        self.conv.voice_ui.downloading_id = Some(entry.id.to_string());
        self.conv.voice_ui.download_progress = None;
        self.conv.voice_ui.download_error = None;
        let (tx, rx) = std::sync::mpsc::channel();
        self.conv.voice_model_rx = Some(rx);
        let err_tx = tx.clone();
        let err_ctx = ctx.clone();
        let work_ctx = ctx.clone();
        spawn_async_task(
            move |err| {
                let _ = err_tx.send(VoiceModelMsg::DownloadDone(Err(err)));
                err_ctx.request_repaint();
            },
            move |rt| {
                let client = crate::runtime::http_client();
                let r = rt.block_on(voice_models::download_model(&client, entry, tx.clone()));
                let _ = tx.send(VoiceModelMsg::DownloadDone(r));
                work_ctx.request_repaint();
            },
        );
    }

    /// Drain in-flight voice-model download progress, called once per frame.
    pub(crate) fn drain_voice_models(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.conv.voice_model_rx.take() else {
            return;
        };
        let mut keep = true;
        loop {
            match rx.try_recv() {
                Ok(VoiceModelMsg::DownloadProgress {
                    id,
                    downloaded,
                    total,
                }) => {
                    self.conv.voice_ui.downloading_id = Some(id);
                    self.conv.voice_ui.download_progress = Some((downloaded, total));
                    ctx.request_repaint();
                }
                Ok(VoiceModelMsg::DownloadDone(r)) => {
                    self.conv.voice_ui.downloading_id = None;
                    match r {
                        Ok(m) => {
                            self.conv.voice_ui.downloaded = voice_models::load_manifest().models;
                            if self.conv.settings.dictation.model_id.is_none() {
                                self.conv.settings.dictation.model_id = Some(m.id);
                            }
                        }
                        Err(e) => self.conv.voice_ui.download_error = Some(e),
                    }
                    ctx.request_repaint();
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    keep = false;
                    break;
                }
            }
        }
        if keep {
            self.conv.voice_model_rx = Some(rx);
        }
    }
}
