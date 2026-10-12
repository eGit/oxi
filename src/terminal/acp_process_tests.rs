use super::*;

#[test]
fn truncation_keeps_unicode_boundaries_and_accepts_split_reads() {
    let process = TerminalProcess::new(5);
    process.append("aaș".as_bytes());
    process.append("țbb".as_bytes());
    assert_eq!(process.output()["output"], "țbb");
    assert_eq!(process.output()["truncated"], true);
    let process = TerminalProcess::new(10);
    process.append(&[0xc8]);
    process.append(&[0x99]);
    assert_eq!(process.output()["output"], "ș");
}

#[cfg(unix)]
#[test]
fn command_pty_captures_output_exit_and_can_be_interrupted() {
    let ctx = egui::Context::default();
    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", "printf terminal-output; exit 7"]);
    let (terminal, process) = TerminalSession::spawn_command(&ctx, command, 1024).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while process.exit_status().is_none() || terminal.is_alive() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert_eq!(process.output()["output"], "terminal-output");
    assert_eq!(process.exit_status().unwrap()["exitCode"], 7);
    let mut command = CommandBuilder::new("/bin/sh");
    command.args(["-c", "exec sleep 30"]);
    let (_terminal, process) = TerminalSession::spawn_command(&ctx, command, 1024).unwrap();
    process.kill().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while process.exit_status().is_none() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
