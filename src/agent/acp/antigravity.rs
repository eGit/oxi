//! Official native Antigravity ACP distribution (pinned to the reviewed registry release).
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use tokio::sync::Mutex;

use crate::settings::ProviderConfig;

const VERSION: &str = "1.4.0";

pub(super) fn needs_authentication(init: &serde_json::Value, error: &str) -> bool {
    init["agentInfo"]["name"].as_str() == Some("antigravity-acp")
        && error.contains("Authentication required")
}

pub(super) fn auth_method(env: &[(String, String)]) -> &'static str {
    if env
        .iter()
        .any(|(key, value)| key == "GEMINI_API_KEY" && !value.trim().is_empty())
        || std::env::var("GEMINI_API_KEY").is_ok_and(|key| !key.trim().is_empty())
    {
        "gemini-api-key"
    } else {
        "oauth-personal"
    }
}

pub(super) fn is_default(command: &str) -> bool {
    command.trim() == ProviderConfig::DEFAULT_ANTIGRAVITY_ACP_COMMAND
}

fn distribution(os: &str, arch: &str) -> Result<(&'static str, &'static str), String> {
    match (os, arch) {
        ("macos", "aarch64") => Ok(("macos", "darwin-arm64")),
        ("macos", "x86_64") => Ok(("macos", "darwin-x86_64")),
        ("linux", "aarch64") => Ok(("linux", "linux-arm64")),
        ("linux", "x86_64") => Ok(("linux", "linux-x86_64")),
        ("windows", "aarch64") => Ok(("windows", "windows-arm64")),
        ("windows", "x86_64") => Ok(("windows", "windows-x86_64")),
        _ => Err(format!(
            "Antigravity has no official ACP binary for {os}/{arch}. Set a custom Agent command in Settings."
        )),
    }
}

fn root() -> PathBuf {
    crate::app_dirs::data_dir()
        .join("acp")
        .join("antigravity")
        .join(VERSION)
}

fn binary(root: &Path) -> PathBuf {
    root.join(if cfg!(windows) {
        "agy_acp_server.exe"
    } else {
        "agy_acp_server.par"
    })
}

pub(super) fn is_ready() -> bool {
    binary(&root()).is_file()
}

pub(super) async fn resolve() -> Result<String, String> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = LOCK.get_or_init(|| Mutex::new(())).lock().await;
    let root = root();
    if !binary(&root).is_file() {
        let (folder, target) = distribution(std::env::consts::OS, std::env::consts::ARCH)?;
        let url = format!(
            "https://dl.google.com/agy-extensions/releases/{folder}/agy-acp-server-{VERSION}-{target}.zip"
        );
        let client = reqwest::Client::builder()
            // ~110 MB: bound stalls, not the whole transfer, so slow links still finish.
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60))
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| e.to_string())?;
        let response = client
            .get(&url)
            .send()
            .await
            .map_err(|e| {
                format!(
                    "Could not download Antigravity ACP: {e}. Retry or set a custom Agent command."
                )
            })?
            .error_for_status()
            .map_err(|e| format!("Antigravity ACP download failed: {e}"))?;
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        let install_root = root.clone();
        tokio::task::spawn_blocking(move || install(&bytes, &install_root))
            .await
            .map_err(|e| e.to_string())??;
    }
    let command = super::install::shell_quote(&binary(&root));
    Ok(if cfg!(target_os = "linux") {
        format!("{command} --uid=")
    } else {
        command
    })
}

fn install(bytes: &[u8], root: &Path) -> Result<(), String> {
    let parent = root
        .parent()
        .ok_or("Antigravity install has no parent directory")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let staging = parent.join(format!(
        ".staging-{}-{}",
        std::process::id(),
        rand::random::<u64>()
    ));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| {
        extract(bytes, &staging)?;
        if !binary(&staging).is_file() {
            return Err("Antigravity archive is missing the ACP server".to_string());
        }
        // A leftover folder without the server (interrupted or edited install) would block the rename.
        if root.exists() {
            std::fs::remove_dir_all(root).map_err(|e| e.to_string())?;
        }
        std::fs::rename(&staging, root)
            .map_err(|e| format!("Could not install Antigravity ACP: {e}"))
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn extract(bytes: &[u8], root: &Path) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = entry
            .enclosed_name()
            .ok_or("Unsafe path in Antigravity archive")?;
        if entry.is_symlink() {
            return Err("Symlink in Antigravity archive".into());
        }
        let path = root.join(path);
        if entry.is_dir() {
            std::fs::create_dir_all(&path).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // The server and its localharness companion both need execution permission.
            let mode = entry.unix_mode().unwrap_or(0o755) & 0o777;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn distributions_cover_official_registry_platforms() {
        for os in ["macos", "linux", "windows"] {
            for arch in ["aarch64", "x86_64"] {
                assert!(distribution(os, arch).is_ok());
            }
        }
        assert!(distribution("linux", "riscv64").is_err());
        assert!(distribution("freebsd", "x86_64").is_err());
        assert!(is_default(ProviderConfig::DEFAULT_ANTIGRAVITY_ACP_COMMAND));
        assert!(!is_default("/custom/agy_acp_server.par --debug"));
    }

    #[test]
    fn native_install_keeps_companion_and_rejects_traversal() {
        fn archive(names: &[&str]) -> Vec<u8> {
            let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
            for name in names {
                zip.start_file(
                    *name,
                    zip::write::SimpleFileOptions::default().unix_permissions(0o755),
                )
                .unwrap();
                zip.write_all(b"fixture").unwrap();
            }
            zip.finish().unwrap().into_inner()
        }
        let root = std::env::temp_dir().join(format!("oxi-agy-install-{}", rand::random::<u64>()));
        let server = if cfg!(windows) {
            "agy_acp_server.exe"
        } else {
            "agy_acp_server.par"
        };
        install(&archive(&[server, "localharness_external"]), &root).unwrap();
        assert!(binary(&root).is_file());
        assert!(root.join("localharness_external").is_file());
        assert!(extract(&archive(&["../escape"]), &root).is_err());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_ne!(
                std::fs::metadata(binary(&root))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o111,
                0
            );
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
