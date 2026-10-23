//! The user's login-shell environment, for subprocesses oxi launches.
//!
//! An app started from Finder, the Dock or a Linux desktop launcher inherits only the session's
//! minimal environment: none of the `PATH` edits or exports (`JAVA_HOME`, nvm, `~/.local/bin`, …)
//! that live in `~/.profile`, `~/.bashrc`, `~/.zshrc` or fish's config. The integrated terminal
//! starts a login shell and sees them, so without this the agent's `bash` tool, ACP agents, MCP
//! servers and system git would disagree with it about which tools exist.
//!
//! The environment is captured once by running the user's `$SHELL` as an interactive login shell
//! and is applied per command (never to oxi's own process, where `set_var` would race other
//! threads). When oxi was started from a terminal, its environment already is the shell's and
//! nothing is captured.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Variables to set on child processes; empty when nothing needs adding.
static VARS: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Start capturing on a background thread so the first subprocess does not wait for it.
pub(crate) fn prefetch() {
    std::thread::Builder::new()
        .name("oxi-shell-env".into())
        .spawn(|| {
            vars();
        })
        .ok();
}

/// The captured variables (blocks until the capture finishes, bounded by its timeout).
pub(crate) fn vars() -> &'static HashMap<String, String> {
    VARS.get_or_init(resolve)
}

/// Set the login-shell environment on `cmd`. Call before any per-command `env`/`env_remove`, so
/// those still win.
pub(crate) fn apply(cmd: &mut std::process::Command) {
    for (key, value) in vars() {
        cmd.env(key, value);
    }
}

/// [`apply`] for async callers: waits for the capture off the runtime's worker threads.
pub(crate) async fn apply_async(cmd: &mut tokio::process::Command) {
    for (key, value) in vars_async().await {
        cmd.env(key, value);
    }
}

pub(crate) async fn vars_async() -> &'static HashMap<String, String> {
    if let Some(vars) = VARS.get() {
        return vars;
    }
    tokio::task::spawn_blocking(vars)
        .await
        .unwrap_or_else(|_| VARS.get_or_init(HashMap::new))
}

/// The `PATH` child processes get, when it differs from oxi's own.
pub(crate) async fn path() -> Option<String> {
    vars_async().await.get("PATH").cloned()
}

#[cfg(windows)]
fn resolve() -> HashMap<String, String> {
    // GUI apps on Windows get the full user environment from the registry.
    HashMap::new()
}

#[cfg(not(windows))]
fn resolve() -> HashMap<String, String> {
    use std::io::IsTerminal;

    if cfg!(test) || launched_from_terminal(std::io::stderr().is_terminal()) {
        return HashMap::new();
    }
    let Some(captured) = capture() else {
        return HashMap::new();
    };
    let current: HashMap<String, String> = std::env::vars().collect();
    diff_env(captured, &current)
}

#[cfg(not(windows))]
fn launched_from_terminal(stderr_is_tty: bool) -> bool {
    stderr_is_tty || std::env::var_os("TERM").is_some_and(|t| !t.is_empty() && t != "dumb")
}

#[cfg(not(windows))]
const MARK: &str = "__OXI_ENV__";

/// Run `$SHELL -ilc 'env -0'` (interactive + login, so both profile and rc files run) and parse
/// the NUL-separated output between markers, which skips anything the rc files print.
#[cfg(not(windows))]
fn capture() -> Option<Vec<(String, String)>> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".to_string());
    // Same syntax in sh, bash, zsh and fish.
    let script = format!("printf '{MARK}'; command env -0; printf '{MARK}'");
    let mut child = Command::new(&shell)
        .args(["-ilc", &script])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    // A slow or hung rc file must not leave agents without a shell forever.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                log::warn!("login shell environment from {shell} timed out");
                return None;
            }
        }
    }
    let out = reader.join().ok()?;
    parse_env_output(&out)
}

#[cfg(not(windows))]
fn parse_env_output(out: &[u8]) -> Option<Vec<(String, String)>> {
    let text = String::from_utf8_lossy(out);
    let start = text.find(MARK)? + MARK.len();
    let len = text[start..].find(MARK)?;
    let vars = text[start..start + len]
        .split('\0')
        .filter_map(|entry| {
            let (key, value) = entry.split_once('=')?;
            (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect::<Vec<_>>();
    (!vars.is_empty()).then_some(vars)
}

/// Variables from the login shell that are new or different from oxi's own, minus the ones that
/// describe that shell itself. `PATH` keeps oxi's own entries after the shell's.
#[cfg(not(windows))]
fn diff_env(
    captured: Vec<(String, String)>,
    current: &HashMap<String, String>,
) -> HashMap<String, String> {
    const SHELL_LOCAL: &[&str] = &["_", "PWD", "OLDPWD", "SHLVL", "TERM", "PS1", "PS2"];
    let mut out = HashMap::new();
    for (key, value) in captured {
        if SHELL_LOCAL.contains(&key.as_str()) {
            continue;
        }
        let value = if key == "PATH" {
            merge_path(&value, current.get("PATH").map_or("", String::as_str))
        } else {
            value
        };
        if current.get(&key) != Some(&value) {
            out.insert(key, value);
        }
    }
    out
}

#[cfg(not(windows))]
fn merge_path(login: &str, current: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    login
        .split(':')
        .chain(current.split(':'))
        .filter(|p| !p.is_empty() && seen.insert(*p))
        .collect::<Vec<_>>()
        .join(":")
}

#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;

    #[test]
    fn parses_between_markers_and_skips_rc_noise() {
        let out =
            b"welcome!\n__OXI_ENV__JAVA_HOME=/opt/jdk\0PATH=/a:/b\0EMPTY=\0X=a=b\0__OXI_ENV__";
        let vars = parse_env_output(out).unwrap();
        assert_eq!(
            vars,
            vec![
                ("JAVA_HOME".into(), "/opt/jdk".into()),
                ("PATH".into(), "/a:/b".into()),
                ("EMPTY".into(), "".into()),
                ("X".into(), "a=b".into()),
            ]
        );
        assert!(parse_env_output(b"no markers").is_none());
    }

    #[test]
    fn keeps_only_changes_and_merges_path() {
        let current: HashMap<String, String> = [
            ("PATH", "/usr/bin:/oxi/bin"),
            ("HOME", "/home/u"),
            ("DISPLAY", ":0"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let captured = [
            ("PATH", "/home/u/.local/bin:/usr/bin"),
            ("HOME", "/home/u"),
            ("JAVA_HOME", "/opt/jdk"),
            ("PWD", "/home/u"),
            ("SHLVL", "1"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let vars = diff_env(captured, &current);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars["PATH"], "/home/u/.local/bin:/usr/bin:/oxi/bin");
        assert_eq!(vars["JAVA_HOME"], "/opt/jdk");
    }
}
