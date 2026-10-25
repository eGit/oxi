//! "Evidence" card under an agent turn (see [`crate::evidence`]): before/after captures side by
//! side (recordings play inline as GIFs), the checks oxi ran, and the agent's claims marked
//! proven, unproven or contradicted. "Re-run checks" runs the checks again without the agent.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, FontId, RichText, Sense, Ui};

use super::OxiApp;
use crate::evidence::exec::CmdResult;
use crate::evidence::{ClaimStatus, EvidenceItem, EvidenceReport, ItemKind};
use crate::theme::*;

/// Per-report view state, keyed by the report's turn id.
#[derive(Default)]
pub(crate) struct EvidenceCardCache {
    entries: HashMap<String, CardEntry>,
}

#[derive(Default)]
struct CardEntry {
    /// Checks whose output is unfolded.
    open_outputs: HashSet<String>,
    rerun: Option<Arc<Mutex<Rerun>>>,
}

#[derive(Default)]
struct Rerun {
    running: bool,
    results: HashMap<String, CmdResult>,
}

const CARD_INSET: f32 = 6.0;
const HEADER_H: f32 = 26.0;
const THUMB_MAX_H: f32 = 300.0;
/// Below this card width the before and after captures stack instead of sitting side by side.
const SIDE_BY_SIDE_MIN_W: f32 = 460.0;

enum CardAction {
    Rerun,
    ToggleOutput(String),
    OpenImage(PathBuf),
    OpenFile(PathBuf),
}

/// Pixel size of a capture, read from its header once.
fn image_size(path: &Path) -> Option<egui::Vec2> {
    thread_local! {
        static SIZES: std::cell::RefCell<HashMap<PathBuf, Option<egui::Vec2>>> =
            std::cell::RefCell::new(HashMap::new());
    }
    SIZES.with(|sizes| {
        *sizes
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| {
                image::image_dimensions(path)
                    .ok()
                    .map(|(w, h)| egui::vec2(w as f32, h as f32))
            })
    })
}

/// Fit `size` into `width` × [`THUMB_MAX_H`], keeping its aspect ratio.
fn fit(size: egui::Vec2, width: f32) -> egui::Vec2 {
    let scale = (width / size.x).min(THUMB_MAX_H / size.y).min(1.0);
    size * scale
}

fn file_uri(path: &Path) -> String {
    #[cfg(windows)]
    {
        format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        format!("file://{}", path.display())
    }
}

fn status_look(status: ClaimStatus) -> (&'static str, egui::Color32, &'static str) {
    match status {
        ClaimStatus::Attested => (
            ICON_CHECK_CIRCLE,
            c_success(),
            "Proven: everything it cites was captured or run by oxi, and the checks passed",
        ),
        ClaimStatus::Declared => (
            ICON_WARNING,
            c_warning_fg(),
            "Unproven: nothing it cites is evidence oxi made or ran itself",
        ),
        ClaimStatus::Contradicted => (
            ICON_CLOSE,
            c_danger(),
            "Contradicted: a check it cites failed",
        ),
    }
}

fn icon(glyph: &str, size: f32, color: egui::Color32) -> RichText {
    RichText::new(glyph)
        .font(FontId::new(size, icon_font()))
        .color(color)
}

fn seconds(ms: u64) -> String {
    if ms < 10_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{}s", ms / 1000)
    }
}

impl OxiApp {
    pub(crate) fn render_evidence(&mut self, ui: &mut Ui, report: &EvidenceReport) {
        if report.is_empty() && !report.requested {
            return;
        }
        let entry = self
            .conv
            .transcript
            .evidence
            .entries
            .entry(report.turn.clone())
            .or_default();
        let (running, rerun_results) = match &entry.rerun {
            Some(rerun) => {
                let rerun = rerun.lock().unwrap_or_else(|e| e.into_inner());
                (rerun.running, rerun.results.clone())
            }
            None => (false, HashMap::new()),
        };
        let open_outputs = entry.open_outputs.clone();
        let has_checks = report.checks().any(|c| c.command.is_some());
        let (proven, unproven, contradicted) = report.tally();

        let mut action = None;
        egui::Frame::new()
            .fill(c_bg_elevated())
            .stroke(egui::Stroke::new(1.0, c_border()))
            .corner_radius(RADIUS_PANEL)
            .inner_margin(egui::Margin {
                left: 12,
                right: CARD_INSET as i8,
                top: CARD_INSET as i8,
                bottom: 10,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.set_min_height(HEADER_H);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(icon(ICON_CAMERA, FS_SMALL, c_text_muted()));
                    ui.label(RichText::new("Evidence").size(FS_SMALL).color(c_text()));
                    for (count, word, color) in [
                        (proven, "proven", c_success()),
                        (unproven, "unproven", c_warning_fg()),
                        (contradicted, "contradicted", c_danger()),
                    ] {
                        if count > 0 {
                            ui.label(
                                RichText::new(format!("{count} {word}"))
                                    .size(FS_TINY)
                                    .color(color),
                            );
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if has_checks {
                            if running {
                                ui.add_space(6.0);
                                small_spinner(ui);
                                ui.label(
                                    RichText::new("Running checks…")
                                        .size(FS_TINY)
                                        .color(c_text_muted()),
                                );
                            } else if crate::ui::chrome::ghost_button_icon(
                                ui,
                                ICON_REFRESH,
                                "Re-run checks",
                                false,
                            )
                            .on_hover_text(
                                "Run this response's checks again now, without the agent",
                            )
                            .clicked()
                            {
                                action = Some(CardAction::Rerun);
                            }
                        }
                    });
                });
                ui.add_space(2.0);
                if report.stale {
                    ui.label(
                        RichText::new("The workspace changed after this evidence was produced. Fresh verification is needed.")
                            .size(FS_SMALL).color(c_warning_fg()),
                    );
                }
                if report.requested && report.items.is_empty() && !report.submitted {
                    ui.label(
                        RichText::new("The agent gave no evidence for this response.")
                            .size(FS_SMALL)
                            .color(c_warning_fg()),
                    );
                }
                if !report.summary.is_empty() {
                    ui.add(
                        egui::Label::new(
                            RichText::new(&report.summary)
                                .size(FS_SMALL)
                                .color(c_text()),
                        )
                        .wrap(),
                    );
                    ui.add_space(4.0);
                }
                for pair in report.pairs() {
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(pair.label)
                            .size(FS_SMALL)
                            .color(c_text_strong()),
                    );
                    ui.add_space(2.0);
                    let width = ui.available_width() - CARD_INSET;
                    let slots = [("Before", pair.before), ("After", pair.after)];
                    if width >= SIDE_BY_SIDE_MIN_W {
                        let column_w = (width - 10.0) / 2.0;
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 10.0;
                            for (phase, item) in slots {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(column_w, 0.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(column_w);
                                        if let Some(a) = capture_slot(ui, phase, item, column_w) {
                                            action = Some(a);
                                        }
                                    },
                                );
                            }
                        });
                    } else {
                        for (phase, item) in slots {
                            if let Some(a) = capture_slot(ui, phase, item, width) {
                                action = Some(a);
                            }
                            ui.add_space(6.0);
                        }
                    }
                }
                let checks: Vec<&EvidenceItem> = report.checks().collect();
                if !checks.is_empty() {
                    ui.add_space(8.0);
                    section_title(ui, "Checks");
                    for check in checks {
                        let rerun = rerun_results.get(&check.id);
                        let open = open_outputs.contains(&check.id);
                        if let Some(a) = check_row(ui, check, rerun, open) {
                            action = Some(a);
                        }
                    }
                }
                if !report.claims.is_empty() {
                    ui.add_space(8.0);
                    section_title(ui, "Claims");
                    for claim in &report.claims {
                        let (glyph, color, hover) = status_look(report.claim_status(claim));
                        ui.horizontal_top(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(icon(glyph, FS_SMALL, color)).on_hover_text(hover);
                            let mut job = egui::text::LayoutJob::default();
                            job.append(
                                &claim.text,
                                0.0,
                                egui::TextFormat::simple(FontId::proportional(FS_SMALL), c_text()),
                            );
                            if !claim.evidence.is_empty() {
                                job.append(
                                    &format!("  {}", claim.evidence.join(" · ")),
                                    0.0,
                                    egui::TextFormat::simple(
                                        FontId::monospace(FS_TINY),
                                        c_text_faint(),
                                    ),
                                );
                            }
                            ui.add(egui::Label::new(job).wrap());
                        });
                    }
                }
            });
        ui.add_space(12.0);

        match action {
            Some(CardAction::Rerun) => self.rerun_evidence_checks(ui.ctx(), report),
            Some(CardAction::ToggleOutput(id)) => {
                if let Some(entry) = self.conv.transcript.evidence.entries.get_mut(&report.turn)
                    && !entry.open_outputs.remove(&id)
                {
                    entry.open_outputs.insert(id);
                }
            }
            Some(CardAction::OpenImage(path)) => match std::fs::read(&path) {
                Ok(bytes) => crate::ui::image_viewer::open(ui.ctx(), &bytes),
                Err(e) => self.notify_composer(format!("Cannot open the capture: {e}")),
            },
            Some(CardAction::OpenFile(path)) => crate::os_open::open_path(&path),
            None => {}
        }
    }

    fn rerun_evidence_checks(&mut self, ctx: &egui::Context, report: &EvidenceReport) {
        let checks: Vec<(String, String, PathBuf)> = report
            .checks()
            .filter_map(|c| {
                Some((
                    c.id.clone(),
                    c.command.clone()?,
                    PathBuf::from(c.cwd.clone()?),
                ))
            })
            .collect();
        let state = Arc::new(Mutex::new(Rerun {
            running: true,
            results: HashMap::new(),
        }));
        let entry = self
            .conv
            .transcript
            .evidence
            .entries
            .entry(report.turn.clone())
            .or_default();
        entry.rerun = Some(state.clone());
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            for (id, command, cwd) in checks {
                let result = crate::evidence::exec::run(
                    &command,
                    &cwd,
                    std::time::Duration::from_secs(1800),
                )
                .unwrap_or_else(|e| CmdResult {
                    exit_code: None,
                    timed_out: false,
                    output: e,
                    duration_ms: 0,
                });
                state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .results
                    .insert(id, result);
                ctx.request_repaint();
            }
            state.lock().unwrap_or_else(|e| e.into_inner()).running = false;
            ctx.request_repaint();
        });
    }
}

fn section_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).size(FS_TINY).color(c_text_muted()));
    ui.add_space(2.0);
}

/// One side of a before/after pair.
fn capture_slot(
    ui: &mut Ui,
    phase: &str,
    item: Option<&EvidenceItem>,
    width: f32,
) -> Option<CardAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(RichText::new(phase).size(FS_TINY).color(c_text_muted()));
        if let Some(item) = item {
            if item.kind == ItemKind::Video {
                ui.label(RichText::new("recording").size(FS_TINY).color(c_text_faint()));
            }
            if !item.attested() {
                ui.label(icon(ICON_WARNING, FS_TINY, c_warning_fg())).on_hover_text(
                    "Supplied by the agent and not written during this turn, so oxi cannot vouch for it",
                );
            }
        }
    });
    let Some(item) = item else {
        placeholder(ui, width, &format!("No {} capture", phase.to_lowercase()));
        return None;
    };
    let Some(file) = item.file.as_deref().map(PathBuf::from) else {
        placeholder(ui, width, "No file");
        return None;
    };
    if !file.is_file() {
        placeholder(ui, width, "The capture file is gone");
        return None;
    }
    let shown = match item.kind {
        ItemKind::Video => item
            .preview
            .as_deref()
            .map(PathBuf::from)
            .filter(|p| p.is_file()),
        _ => Some(file.clone()),
    };
    match shown {
        Some(shown) => {
            // An explicit size keeps the card's height known before the image has decoded.
            let size = image_size(&shown)
                .map_or_else(|| egui::vec2(width, width * 0.6), |s| fit(s, width));
            let response = ui
                .add(
                    egui::Image::from_uri(file_uri(&shown))
                        .fit_to_exact_size(size)
                        .corner_radius(RADIUS_CHIP)
                        .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::ZoomIn)
                .on_hover_text(&item.source);
            if response.clicked() {
                let still = item.kind == ItemKind::Image
                    && !file
                        .extension()
                        .is_some_and(|e| e.eq_ignore_ascii_case("gif"));
                action = Some(if still {
                    CardAction::OpenImage(file.clone())
                } else {
                    CardAction::OpenFile(file.clone())
                });
            }
        }
        None => {
            let response = placeholder(ui, width, "Recording · click to play")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(&item.source);
            if response.clicked() {
                action = Some(CardAction::OpenFile(file.clone()));
            }
        }
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add(
            egui::Label::new(
                RichText::new(format!("{} · {}", item.id, item.source))
                    .size(FS_TINY)
                    .color(c_text_faint()),
            )
            .truncate(),
        );
        if item.kind == ItemKind::Video
            && ui
                .add(
                    egui::Label::new(RichText::new("Play").size(FS_TINY).color(c_accent()))
                        .sense(Sense::click()),
                )
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Open the full recording")
                .clicked()
        {
            action = Some(CardAction::OpenFile(file.clone()));
        }
    });
    action
}

fn placeholder(ui: &mut Ui, width: f32, text: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, 120.0_f32.min(THUMB_MAX_H)),
        Sense::click(),
    );
    ui.painter().rect(
        rect,
        RADIUS_CHIP,
        c_bg_elevated_2(),
        egui::Stroke::new(1.0, c_border_subtle()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::proportional(FS_TINY),
        c_text_muted(),
    );
    response
}

fn check_row(
    ui: &mut Ui,
    check: &EvidenceItem,
    rerun: Option<&CmdResult>,
    open: bool,
) -> Option<CardAction> {
    let mut action = None;
    let (passed, timed_out, exit_code, duration_ms, output) = match rerun {
        Some(r) => (
            r.passed(),
            r.timed_out,
            r.exit_code,
            r.duration_ms,
            r.output.as_str(),
        ),
        None => (
            check.passed(),
            check.timed_out,
            check.exit_code,
            check.duration_ms,
            check.output.as_str(),
        ),
    };
    let (glyph, color) = if passed {
        (ICON_CHECK_CIRCLE, c_success())
    } else {
        (ICON_CLOSE, c_danger())
    };
    let verdict = if timed_out {
        "timed out".to_string()
    } else {
        match exit_code {
            Some(0) => "passed".to_string(),
            Some(code) => format!("exit {code}"),
            None => "killed".to_string(),
        }
    };
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(icon(
            if open {
                ICON_ANGLE_DOWN
            } else {
                ICON_CHEVRON_RIGHT
            },
            FS_TINY,
            c_text_muted(),
        ));
        ui.label(icon(glyph, FS_SMALL, color));
        ui.label(RichText::new(&check.label).size(FS_SMALL).color(c_text()));
        ui.label(
            RichText::new(format!("{verdict} · {}", seconds(duration_ms)))
                .size(FS_TINY)
                .color(c_text_muted()),
        );
        if rerun.is_some() {
            ui.label(RichText::new("re-run").size(FS_TINY).color(c_accent()));
        }
        if let Some(command) = &check.command {
            ui.add(
                egui::Label::new(
                    RichText::new(command)
                        .font(FontId::monospace(FS_TINY))
                        .color(c_text_faint()),
                )
                .truncate(),
            );
        }
    });
    let click = ui
        .interact(
            row.response.rect,
            row.response.id.with("toggle"),
            Sense::click(),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if click.clicked() {
        action = Some(CardAction::ToggleOutput(check.id.clone()));
    }
    if open {
        let tail: Vec<&str> = output.lines().rev().take(40).collect();
        let text = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
        egui::Frame::new()
            .fill(c_md_code_block_bg())
            .corner_radius(RADIUS_ROW)
            .inner_margin(egui::Margin::same(8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width() - CARD_INSET);
                ui.add(
                    egui::Label::new(
                        RichText::new(if text.trim().is_empty() {
                            "(no output)"
                        } else {
                            &text
                        })
                        .font(FontId::monospace(FS_TINY))
                        .color(c_text()),
                    )
                    .wrap(),
                );
            });
    }
    action
}
