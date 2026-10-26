//! Running a shell command for evidence: the exit code and the tail of its output, with a
//! timeout that kills everything the command started.

use std::io::Read as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Output kept per command (the end matters: test summaries and errors print last).
const TAIL_BYTES: usize = 12 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CmdResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub output: String,
    pub duration_ms: u64,
}

impl CmdResult {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out
    }
}

pub fn shell(command: &str) -> Command {
    #[cfg(unix)]
    {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(command);
        crate::agent::tools::sanitize_bash_env(&mut c);
        c
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command).creation_flags(CREATE_NO_WINDOW);
        crate::agent::tools::sanitize_bash_env(&mut c);
        c
    }
}

/// Run `command` in `cwd` through the shell, stdout and stderr merged in arrival order.
pub fn run(command: &str, cwd: &Path, timeout: Duration) -> Result<CmdResult, String> {
    Execution::default().run(command, cwd, timeout)
}

/// Per-run limits, passed explicitly so concurrent conversations remain independent.
#[derive(Clone, Debug, Default)]
pub struct Execution {
    pub cancel: Option<Arc<AtomicBool>>,
    pub timeout_cap: Option<Duration>,
}

impl Execution {
    pub fn check_cancelled(&self) -> Result<(), String> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            Err("Evidence capture or check cancelled.".into())
        } else {
            Ok(())
        }
    }

    pub fn wait(&self, duration: Duration) -> Result<(), String> {
        let start = Instant::now();
        while start.elapsed() < duration {
            self.check_cancelled()?;
            std::thread::sleep(
                duration
                    .saturating_sub(start.elapsed())
                    .min(Duration::from_millis(50)),
            );
        }
        self.check_cancelled()
    }

    pub fn run(&self, command: &str, cwd: &Path, timeout: Duration) -> Result<CmdResult, String> {
        let mut c = shell(command);
        c.current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.run_child(c, timeout)
    }

    pub fn run_child(&self, mut c: Command, timeout: Duration) -> Result<CmdResult, String> {
        self.check_cancelled()?;
        let timeout = self.timeout_cap.map_or(timeout, |cap| timeout.min(cap));
        crate::agent::tools::isolate_process_group(&mut c);
        let start = Instant::now();
        let mut child = c
            .spawn()
            .map_err(|e| format!("Could not start the command: {e}"))?;
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let mut readers = Vec::new();
        let streams: [Option<Box<dyn std::io::Read + Send>>; 2] = [
            child
                .stdout
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
            child
                .stderr
                .take()
                .map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
        ];
        for mut stream in streams.into_iter().flatten() {
            let tx = tx.clone();
            readers.push(std::thread::spawn(move || {
                let mut buf = [0u8; 8192];
                while let Ok(n) = stream.read(&mut buf) {
                    if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }));
        }
        drop(tx);
        let mut output: Vec<u8> = Vec::new();
        let keep = |chunk: Vec<u8>, output: &mut Vec<u8>| {
            output.extend_from_slice(&chunk);
            if output.len() > TAIL_BYTES * 2 {
                output.drain(..output.len() - TAIL_BYTES);
            }
        };
        let mut timed_out = false;
        let status = loop {
            if let Err(error) = self.check_cancelled() {
                crate::agent::tools::terminate_child_tree(&mut child);
                return Err(error);
            }
            while let Ok(chunk) = rx.try_recv() {
                keep(chunk, &mut output);
            }
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if start.elapsed() >= timeout => {
                    timed_out = true;
                    crate::agent::tools::terminate_child_tree(&mut child);
                    break None;
                }
                Ok(None) => {
                    if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
                        keep(chunk, &mut output);
                    }
                }
                Err(e) => {
                    crate::agent::tools::terminate_child_tree(&mut child);
                    return Err(e.to_string());
                }
            }
        };
        // Background children may hold the pipes open; take what arrives promptly and move on.
        let drain_until = Instant::now() + Duration::from_millis(500);
        while let Ok(chunk) = rx.recv_timeout(drain_until.saturating_duration_since(Instant::now()))
        {
            keep(chunk, &mut output);
        }
        let output = if output.len() > TAIL_BYTES {
            let cut = output.len() - TAIL_BYTES;
            format!("…\n{}", String::from_utf8_lossy(&output[cut..]))
        } else {
            String::from_utf8_lossy(&output).into_owned()
        };
        Ok(CmdResult {
            exit_code: status.and_then(|s| s.code()),
            timed_out,
            output: strip_ansi(&output),
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}

/// Colour codes and cursor moves are noise in a stored output tail.
fn strip_ansi(text: &str) -> String {
    static ANSI: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07").expect("valid regex")
    });
    ANSI.replace_all(text, "").replace('\r', "")
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn records_exit_code_output_and_timeouts() {
        let cwd = std::env::temp_dir();
        let ok = run("echo hi; echo err >&2", &cwd, Duration::from_secs(10)).unwrap();
        assert!(ok.passed());
        assert!(ok.output.contains("hi") && ok.output.contains("err"));
        let failed = run(
            "printf '\\033[31mred\\033[0m'; exit 3",
            &cwd,
            Duration::from_secs(10),
        )
        .unwrap();
        assert_eq!(failed.exit_code, Some(3));
        assert_eq!(failed.output, "red");
        let slow = run("sleep 5", &cwd, Duration::from_millis(200)).unwrap();
        assert!(slow.timed_out && !slow.passed());
        assert!(slow.duration_ms < 3000);
    }

    #[test]
    fn cancellation_stops_a_running_command_and_its_children() {
        let cancel = Arc::new(AtomicBool::new(false));
        let execution = Execution {
            cancel: Some(cancel.clone()),
            timeout_cap: None,
        };
        let dir = super::super::tests::tempfile_dir("cancel");
        let cwd = dir.clone();
        let worker = std::thread::spawn(move || {
            execution.run(
                "echo started > started; sleep 2; echo leaked > leaked",
                &cwd,
                Duration::from_secs(30),
            )
        });
        let start = Instant::now();
        while !dir.join("started").exists() && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(dir.join("started").exists());
        cancel.store(true, Ordering::SeqCst);
        assert!(worker.join().unwrap().unwrap_err().contains("cancelled"));
        std::thread::sleep(Duration::from_millis(2200));
        assert!(
            !dir.join("leaked").exists(),
            "cancelled shell continued executing"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn execution_caps_the_tool_timeout() {
        let execution = Execution {
            cancel: None,
            timeout_cap: Some(Duration::from_millis(100)),
        };
        let result = execution
            .run("sleep 5", &std::env::temp_dir(), Duration::from_secs(30))
            .unwrap();
        assert!(result.timed_out);
        assert!(result.duration_ms < 3000);
    }
}
