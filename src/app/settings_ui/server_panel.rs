//! Settings → Server: run the oxi server on this machine and pair phones with it.
//!
//! Everything here goes through `remote::service`, the same code as `oxi server`. Those calls
//! talk to launchd/systemd/the registry and to the daemon, so they run on a worker thread and
//! report back through `ServerPanel::rx`.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, RichText, Ui};

use crate::remote::service::{self, Status};
use crate::theme::*;
use crate::ui::chrome::{
    card_frame, field_hint, ghost_button_widget, hairline, settings_caption, settings_card_header,
    settings_section_title, toggle,
};

use super::super::OxiApp;

/// Re-read the status this often while the page is open (phones come and go).
const REFRESH_EVERY: Duration = Duration::from_secs(5);

pub enum ServerMsg {
    Status(Status),
    Done(Result<String, String>),
}

#[derive(Default)]
pub struct ServerPanel {
    status: Option<Status>,
    checked_at: Option<Instant>,
    /// What runs now ("Installing…"), while a worker is busy.
    busy: Option<&'static str>,
    rx: Option<mpsc::Receiver<ServerMsg>>,
    message: Option<Result<String, String>>,
    qr: Option<(String, egui::TextureHandle)>,
    show_link: bool,
}

impl ServerPanel {
    fn poll(&mut self, ctx: &egui::Context) {
        let Some(rx) = &self.rx else {
            if self.checked_at.is_none_or(|t| t.elapsed() > REFRESH_EVERY) {
                self.refresh(ctx);
            }
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(ServerMsg::Status(status)) => {
                    self.status = Some(status);
                    self.checked_at = Some(Instant::now());
                }
                Ok(ServerMsg::Done(result)) => {
                    self.message = Some(result);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(150));
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.busy = None;
                    ctx.request_repaint();
                    return;
                }
            }
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) {
        self.checked_at = Some(Instant::now());
        self.run(ctx, None, || Ok(None));
    }

    /// Run `job` on a worker, then read the status again. `Ok(Some(text))` is shown as a note.
    fn run(
        &mut self,
        ctx: &egui::Context,
        label: Option<&'static str>,
        job: impl FnOnce() -> Result<Option<String>, String> + Send + 'static,
    ) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        self.busy = label;
        if label.is_some() {
            self.message = None;
        }
        self.rx = Some(rx);
        std::thread::Builder::new()
            .name("oxi-server-settings".into())
            .spawn(move || {
                match job() {
                    Ok(Some(text)) => drop(tx.send(ServerMsg::Done(Ok(text)))),
                    Ok(None) => {}
                    Err(e) => drop(tx.send(ServerMsg::Done(Err(e)))),
                }
                let _ = tx.send(ServerMsg::Status(service::status()));
                ctx.request_repaint();
            })
            .ok();
    }

    fn qr_texture(&mut self, ctx: &egui::Context, link: &str) -> Option<egui::TextureHandle> {
        if let Some((cached, texture)) = &self.qr
            && cached == link
        {
            return Some(texture.clone());
        }
        let (width, modules) = service::qr_modules(link)?;
        let quiet = 3;
        let size = width + 2 * quiet;
        let mut pixels = vec![Color32::WHITE; size * size];
        for y in 0..width {
            for x in 0..width {
                if modules[y * width + x] {
                    pixels[(y + quiet) * size + x + quiet] = Color32::BLACK;
                }
            }
        }
        let image = egui::ColorImage::new([size, size], pixels);
        let texture = ctx.load_texture("oxi-pair-qr", image, egui::TextureOptions::NEAREST);
        self.qr = Some((link.to_string(), texture.clone()));
        Some(texture)
    }
}

impl OxiApp {
    pub(super) fn render_settings_server_panel(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let panel = &mut self.conv.settings_page.server;
        panel.poll(&ctx);

        settings_section_title(
            ui,
            "Server & phone",
            Some(
                "Run your agents on this machine and use them from your phone or another computer.",
            ),
        );

        let Some(status) = panel.status.clone() else {
            ui.label(
                RichText::new("Checking…")
                    .size(FS_SMALL)
                    .color(c_text_muted()),
            );
            return;
        };
        let busy = panel.busy.is_some();

        // ---- the server itself ------------------------------------------------------------
        card_frame().show(ui, |ui| {
            settings_card_header(
                ui,
                "Run the oxi server",
                Some("Keeps agents running in the background, even with this window closed."),
            );
            let mut on = status.service;
            let response = ui
                .add_enabled_ui(!busy, |ui| {
                    toggle(ui, &mut on, "Start at login and keep running")
                })
                .inner;
            if response.clicked() {
                let label = if on { "Installing…" } else { "Removing…" };
                panel.run(&ctx, Some(label), move || {
                    if on {
                        service::install()
                    } else {
                        service::uninstall()
                    }
                    .map(Some)
                });
            }
            field_hint(ui, service_hint());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let (dot, text) = match &status.daemon {
                    Some(d) => (
                        c_success(),
                        format!(
                            "Running · oxi {} · {} connected",
                            d["version"].as_str().unwrap_or("?"),
                            status.clients()
                        ),
                    ),
                    None => (c_text_faint(), "Not running".to_string()),
                };
                status_dot(ui, dot);
                ui.label(RichText::new(text).size(FS_SMALL).color(c_text_muted()));
                if status.running() {
                    ui.add_space(8.0);
                    if ui
                        .add_enabled(!busy, ghost_button_widget("Restart", false))
                        .on_hover_text("Restart the server. Running turns are stopped.")
                        .clicked()
                    {
                        panel.run(&ctx, Some("Restarting…"), || {
                            service::restart().map(|()| None)
                        });
                    }
                }
            });
        });

        ui.add_space(12.0);

        // ---- phones -------------------------------------------------------------------------
        card_frame().show(ui, |ui| {
            settings_card_header(
                ui,
                "Phone access",
                Some(
                    "Pair the oxi Android app. Phones connect over Tailscale with a private token.",
                ),
            );
            let mut phones = status.phone.is_some();
            let response = ui
                .add_enabled_ui(!busy, |ui| {
                    toggle(ui, &mut phones, "Allow phones to connect")
                })
                .inner;
            if response.clicked() {
                if phones {
                    panel.run(&ctx, Some("Turning on…"), || {
                        service::enable_phones(false).map(|_| None)
                    });
                } else {
                    panel.run(&ctx, Some("Turning off…"), || {
                        service::disable_phones().map(|()| None)
                    });
                }
            }
            let Some(config) = status.phone.clone() else {
                field_hint(
                    ui,
                    "Off: the server accepts no connections from other devices.",
                );
                return;
            };
            let mut lan = config.lan;
            let response = ui
                .add_enabled_ui(!busy, |ui| {
                    toggle(ui, &mut lan, "Also allow phones on my local network")
                })
                .inner;
            if response.clicked() {
                panel.run(&ctx, Some("Saving…"), move || {
                    service::set_phone_lan(lan).map(|()| None)
                });
            }
            field_hint(
                ui,
                "Local network traffic is not encrypted; Tailscale's is. Leave this off unless \
                 the phone can't use Tailscale.",
            );

            ui.add_space(10.0);
            hairline(ui);
            ui.add_space(10.0);
            settings_caption(ui, "Pair a phone");
            ui.add_space(6.0);
            match status.pair_link() {
                None => {
                    ui.label(
                        RichText::new(
                            "No Tailscale address found on this machine. Install and start \
                             Tailscale (tailscale.com/download) on it and on the phone, or allow \
                             the local network above.",
                        )
                        .size(FS_SMALL)
                        .color(c_warning_fg()),
                    );
                }
                Some(link) => {
                    ui.horizontal_top(|ui| {
                        if let Some(texture) = panel.qr_texture(&ctx, &link) {
                            ui.add(
                                egui::Image::new(&texture)
                                    .fit_to_exact_size(egui::vec2(176.0, 176.0)),
                            );
                            ui.add_space(14.0);
                        }
                        ui.vertical(|ui| {
                            let steps = [
                                "1. Install the oxi app on the phone.",
                                "2. Tap Add server → Scan QR code.",
                                "3. Point the camera at this code.",
                            ];
                            for step in steps {
                                ui.label(RichText::new(step).size(FS_SMALL).color(c_text()));
                                ui.add_space(2.0);
                            }
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(format!(
                                    "{} · {}:{}",
                                    status.name,
                                    status.pair_host.as_deref().unwrap_or("?"),
                                    config.port
                                ))
                                .size(FS_TINY)
                                .color(c_text_faint()),
                            );
                            ui.add_space(8.0);
                            ui.horizontal_wrapped(|ui| {
                                if ui
                                    .add(ghost_button_widget("Copy pair link", false))
                                    .clicked()
                                {
                                    ui.ctx().copy_text(link.clone());
                                    panel.message = Some(Ok("Pair link copied.".into()));
                                }
                                if ui
                                    .add_enabled(!busy, ghost_button_widget("New token", true))
                                    .on_hover_text("Phones paired before have to pair again.")
                                    .clicked()
                                {
                                    panel.run(&ctx, Some("Making a new token…"), || {
                                        service::enable_phones(true).map(|_| {
                                            Some("New token: pair your phones again.".into())
                                        })
                                    });
                                }
                            });
                            if panel.show_link {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(&link)
                                        .size(FS_TINY)
                                        .monospace()
                                        .color(c_text_muted()),
                                );
                            } else if ui
                                .link(
                                    RichText::new("Show link")
                                        .size(FS_TINY)
                                        .color(c_text_faint()),
                                )
                                .clicked()
                            {
                                panel.show_link = true;
                            }
                        });
                    });
                }
            }
            if !status.service {
                ui.add_space(8.0);
                field_hint(
                    ui,
                    "Tip: turn on “Start at login” above, or phones can't connect once the \
                     server stops 15 minutes after its last client.",
                );
            }
        });

        ui.add_space(12.0);
        card_frame().show(ui, |ui| {
            settings_card_header(
                ui,
                "From the terminal",
                Some("The same controls, for a machine you reach over SSH."),
            );
            for line in [
                "oxi server install     # start at login",
                "oxi server phone       # allow phones, print the QR code",
                "oxi server             # status",
            ] {
                ui.label(
                    RichText::new(line)
                        .size(FS_SMALL)
                        .monospace()
                        .color(c_text_muted()),
                );
            }
        });

        if let Some(label) = panel.busy {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(label).size(FS_SMALL).color(c_text_muted()));
            });
        } else if let Some(message) = &panel.message {
            ui.add_space(10.0);
            let (text, color) = match message {
                Ok(text) => (text.as_str(), c_text_muted()),
                Err(e) => (e.as_str(), c_error_fg()),
            };
            ui.label(RichText::new(text).size(FS_SMALL).color(color));
        }
    }
}

fn service_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "Installs the dev.oxi.server LaunchAgent."
    } else if cfg!(windows) {
        "Starts the server when you sign in to Windows."
    } else {
        "Installs the oxi-server systemd user service."
    }
}

fn status_dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.left_center() + egui::vec2(4.0, 0.0), 4.0, color);
}
