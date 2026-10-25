//! `oxi server`: keep the headless daemon running on this machine for apps that connect over
//! SSH.
//!
//! On macOS the daemon has to run inside the user's login session: started from an SSH login
//! it can't open the keychain, where oxi keeps API keys and agents such as Claude Code keep
//! their sign-in. A LaunchAgent puts it there, keeps it up, and restarts it after
//! `daemon.stop` (how a bridge replaces a daemon running an older build). Bridges started over
//! SSH then ask launchd for the daemon instead of starting one themselves.
//!
//! `install` also links `~/.local/bin/oxi-server` to this binary; the app runs that first.

use std::path::PathBuf;

pub const SUBCOMMAND: &str = "server";
pub const LABEL: &str = "dev.oxi.server";

pub fn run() -> i32 {
    crate::logging::init();
    match std::env::args().nth(2).as_deref() {
        Some("install") => report(install()),
        Some("uninstall") => report(uninstall()),
        Some("status") | None => status(),
        Some(other) => {
            eprintln!(
                "oxi server: unknown command {other}\nusage: oxi server [install|uninstall|status]"
            );
            2
        }
    }
}

fn report(result: Result<String, String>) -> i32 {
    match result {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(e) => {
            eprintln!("oxi server: {e}");
            1
        }
    }
}

/// The link the app's SSH command tries first.
fn server_link() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".local/bin/oxi-server"))
}

fn link_this_binary() -> Result<PathBuf, String> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("this binary: {e}"))?;
    let link = server_link().ok_or("no home folder")?;
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    std::os::unix::fs::symlink(&exe, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    #[cfg(not(unix))]
    std::fs::copy(&exe, &link).map_err(|e| format!("{}: {e}", link.display()))?;
    Ok(exe)
}

#[cfg(target_os = "macos")]
fn plist_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(format!("Library/LaunchAgents/{LABEL}.plist")))
}

#[cfg(target_os = "macos")]
fn uid() -> u32 {
    unsafe { libc::getuid() }
}

#[cfg(target_os = "macos")]
fn launchctl(args: &[&str]) -> std::io::Result<std::process::Output> {
    std::process::Command::new("launchctl").args(args).output()
}

/// Whether the LaunchAgent is installed, so a bridge should ask launchd for the daemon.
pub fn managed() -> bool {
    #[cfg(target_os = "macos")]
    {
        plist_path().is_some_and(|p| p.exists())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// Ask launchd to start the daemon now (it may have exited after `daemon.stop`).
pub fn kickstart() -> bool {
    #[cfg(target_os = "macos")]
    {
        let target = format!("gui/{}/{LABEL}", uid());
        launchctl(&["kickstart", &target]).is_ok_and(|o| o.status.success())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

#[cfg(target_os = "macos")]
fn install() -> Result<String, String> {
    let exe = link_this_binary()?;
    let plist = plist_path().ok_or("no home folder")?;
    let log = crate::app_dirs::config_dir().join("server.log");
    let escape = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let home = dirs::home_dir().unwrap_or_default();
    let body = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>{headless}</string>
        <string>--daemon</string>
        <string>--keep</string>
    </array>
    <key>EnvironmentVariables</key>
    <dict>
        <key>{secrets_env}</key>
        <string>1</string>
    </dict>
    <key>WorkingDirectory</key>
    <string>{home}</string>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ThrottleInterval</key>
    <integer>5</integer>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>StandardErrorPath</key>
    <string>{log}</string>
</dict>
</plist>
"#,
        exe = escape(&exe.to_string_lossy()),
        headless = crate::headless::SUBCOMMAND,
        secrets_env = crate::secrets::SERVER_FILE_ENV,
        home = escape(&home.to_string_lossy()),
        log = escape(&log.to_string_lossy()),
    );
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let target = format!("gui/{}", uid());
    let service = format!("{target}/{LABEL}");
    // Replace a loaded copy, so the new binary path takes effect.
    let _ = launchctl(&["bootout", &service]);
    // A daemon started by a bridge (not by launchd) would hold the lock; stop it.
    crate::headless::stop_running_daemon();
    std::fs::write(&plist, body).map_err(|e| format!("{}: {e}", plist.display()))?;
    let out = launchctl(&["bootstrap", &target, &plist.to_string_lossy()])
        .map_err(|e| format!("launchctl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "launchctl bootstrap failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(format!(
        "oxi server installed: {LABEL} runs {} at login.\n\
         On its first start it copies oxi's keys from the keychain into a file only you can \
         read, so updates don't stop it at a keychain prompt: if macOS asks, choose Allow.",
        exe.display()
    ))
}

#[cfg(target_os = "macos")]
fn uninstall() -> Result<String, String> {
    let service = format!("gui/{}/{LABEL}", uid());
    let _ = launchctl(&["bootout", &service]);
    if let Some(plist) = plist_path() {
        let _ = std::fs::remove_file(plist);
    }
    if let Some(link) = server_link() {
        let _ = std::fs::remove_file(link);
    }
    Ok("oxi server removed.".into())
}

#[cfg(not(target_os = "macos"))]
fn install() -> Result<String, String> {
    let exe = link_this_binary()?;
    Ok(format!(
        "Linked ~/.local/bin/oxi-server to {}. Keep `{} headless --daemon --keep` running \
         (for example as a systemd user service) so apps can connect.",
        exe.display(),
        exe.display()
    ))
}

#[cfg(not(target_os = "macos"))]
fn uninstall() -> Result<String, String> {
    if let Some(link) = server_link() {
        let _ = std::fs::remove_file(link);
    }
    Ok("oxi server link removed.".into())
}

fn status() -> i32 {
    println!(
        "LaunchAgent: {}",
        if managed() {
            "installed"
        } else {
            "not installed"
        }
    );
    crate::headless::print_status()
}
