//! Background check for a newer GitHub release, following the app's spawn/drain
//! pattern (see `drain_models` / `drain_ssh_test`): a worker thread with its own tokio
//! runtime sends one message over an mpsc channel, drained each frame.

use eframe::egui;

use crate::update::{
    APP_VERSION, ReleaseInfo, can_self_update, download_and_install, fetch_latest_release, is_newer,
};

use super::OxiApp;
use super::state::{UpdateInstall, UpdateInstallMsg, UpdateMsg};
use super::task_runner::spawn_async_task;

/// A window left open for days still notices new releases.
const RECHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(6 * 60 * 60);

impl OxiApp {
    /// Run the update check at app start and every [`RECHECK_EVERY`] after (plus explicit
    /// re-runs from the About panel's button with `force`). Failures are shown only in About.
    pub(crate) fn ensure_update_checked(&mut self, ctx: &egui::Context, force: bool) {
        let due = self
            .conv
            .update
            .last_check
            .is_none_or(|at| at.elapsed() >= RECHECK_EVERY);
        if self.conv.update.started && !force && !due {
            return;
        }
        if self.conv.update.checking {
            return;
        }
        self.conv.update.started = true;
        self.conv.update.checking = true;
        self.conv.update.last_check = Some(std::time::Instant::now());

        let (tx, rx) = std::sync::mpsc::channel::<UpdateMsg>();
        self.conv.update.rx = Some(rx);
        let ctx = ctx.clone();
        let err_tx = tx.clone();
        let err_ctx = ctx.clone();
        spawn_async_task(
            move |err| {
                let _ = err_tx.send(UpdateMsg(Err(err)));
                err_ctx.request_repaint();
            },
            move |rt| {
                let r = rt.block_on(async {
                    let client = crate::runtime::http_client();
                    fetch_latest_release(&client).await
                });
                let _ = tx.send(UpdateMsg(r));
                ctx.request_repaint();
            },
        );
    }

    /// Drain the update-check result. Mirrors [`Self::drain_models`].
    pub(crate) fn drain_update_check(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.conv.update.rx.take() else {
            return;
        };
        let mut repainted = false;
        loop {
            match rx.try_recv() {
                Ok(UpdateMsg(result)) => {
                    self.conv.update.checking = false;
                    self.conv.update.result = Some(result);
                    if self.conv.settings.auto_update {
                        self.install_update(ctx, false);
                    }
                    repainted = true;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    self.conv.update.rx = Some(rx);
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if repainted {
            ctx.request_repaint();
        }
    }

    /// The latest release, when it is strictly newer than the running binary.
    pub(crate) fn update_available(&self) -> Option<&ReleaseInfo> {
        match &self.conv.update.result {
            Some(Ok(info)) if is_newer(&info.version, APP_VERSION) => Some(info),
            _ => None,
        }
    }

    /// Download and install the newer release found by the last check, unless that version is
    /// already installed or installing. A failed version is retried only from About (`force`).
    pub(crate) fn install_update(&mut self, ctx: &egui::Context, force: bool) {
        let Some(info) = self.update_available().cloned() else {
            return;
        };
        if !can_self_update() {
            return;
        }
        match &self.conv.update.install {
            UpdateInstall::Installing(_) => return,
            UpdateInstall::Ready { version, .. } if *version == info.version => return,
            UpdateInstall::Failed { version, .. } if *version == info.version && !force => {
                return;
            }
            _ => {}
        }
        self.conv.update.install = UpdateInstall::Installing(info.version.clone());

        let (tx, rx) = std::sync::mpsc::channel::<UpdateInstallMsg>();
        self.conv.update.install_rx = Some(rx);
        let ctx = ctx.clone();
        let err_tx = tx.clone();
        let err_ctx = ctx.clone();
        let err_version = info.version.clone();
        spawn_async_task(
            move |err| {
                let _ = err_tx.send(UpdateInstallMsg(err_version, Err(err)));
                err_ctx.request_repaint();
            },
            move |rt| {
                let r = rt.block_on(async {
                    let client = crate::runtime::http_client();
                    download_and_install(&client, &info).await
                });
                let _ = tx.send(UpdateInstallMsg(info.version, r));
                ctx.request_repaint();
            },
        );
    }

    /// Drain the install result.
    pub(crate) fn drain_update_install(&mut self) {
        let Some(rx) = self.conv.update.install_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(UpdateInstallMsg(version, result)) => {
                self.conv.update.install_rx = None;
                self.conv.update.install = match result {
                    Ok(path) => {
                        log::info!("installed oxi v{version} at {}", path.display());
                        UpdateInstall::Ready { version, path }
                    }
                    Err(error) => {
                        log::warn!("installing oxi v{version} failed: {error}");
                        UpdateInstall::Failed { version, error }
                    }
                };
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.conv.update.install_rx = None;
            }
        }
    }

    /// Quit through the normal close path (unsaved-changes prompt included), then start the
    /// installed update.
    pub(crate) fn restart_to_update(&mut self, ctx: &egui::Context) {
        if let UpdateInstall::Ready { path, .. } = &self.conv.update.install {
            crate::update::request_relaunch(path.clone());
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
