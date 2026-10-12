use super::*;
use std::time::{Duration, Instant};

fn wait_for(mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn foreground_job_tracks_running_commands() {
    let ctx = egui::Context::default();
    let mut term = TerminalSession::spawn(&ctx, ".", 24, 80, WindowsTerminal::default()).unwrap();
    assert!(
        wait_for(|| !term.has_foreground_job()),
        "idle shell reads as busy"
    );
    term.send(b"sleep 30\n");
    assert!(wait_for(|| term.has_foreground_job()), "sleep not detected");
    // The foreground group switches to the job before its command execs, and an
    // interrupt landing in that window can be lost while the child still holds the
    // shell's signal setup. Keep pressing Ctrl+C like a user would until it lands.
    let mut last_interrupt: Option<Instant> = None;
    assert!(
        wait_for(|| {
            if !term.has_foreground_job() {
                return true;
            }
            if last_interrupt.is_none_or(|t| t.elapsed() >= Duration::from_millis(500)) {
                term.send(&[0x03]);
                last_interrupt = Some(Instant::now());
            }
            false
        }),
        "Ctrl+C left it busy"
    );
}
