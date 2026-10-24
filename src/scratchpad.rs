//! Storage shared by the global scratchpad editor and agent tools. The app and oxi's MCP
//! server (a separate process per ACP agent) both write it, so writers also hold a lock file.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

static STORAGE_LOCK: Mutex<()> = Mutex::new(());
pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;

pub(crate) fn path() -> PathBuf {
    crate::settings::AppSettings::config_path()
        .parent()
        .map_or_else(
            || PathBuf::from("scratchpad.md"),
            |dir| dir.join("scratchpad.md"),
        )
}

/// Held across a read-modify-write of the scratchpad at `path`.
pub(crate) struct StorageGuard {
    _thread: MutexGuard<'static, ()>,
    // Unlocked when the file is closed. Best effort: without it only this process is serialized.
    _process: Option<std::fs::File>,
}

pub(crate) fn lock(path: &Path) -> StorageGuard {
    let thread = STORAGE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let process = path.parent().and_then(|dir| {
        std::fs::create_dir_all(dir).ok()?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path.with_extension("lock"))
            .ok()?;
        file.lock().ok()?;
        Some(file)
    });
    StorageGuard {
        _thread: thread,
        _process: process,
    }
}

/// Caller holds the storage lock across reads and writes.
pub(crate) fn read(path: &Path) -> Result<String, String> {
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error.to_string()),
    }
}

/// Read under the storage lock, for callers outside a read-modify-write.
pub(crate) fn read_locked(path: &Path) -> Result<String, String> {
    let _guard = lock(path);
    read(path)
}

fn write(path: &Path, content: &str) -> Result<(), String> {
    if content.len() > MAX_BYTES {
        return Err("Scratchpad exceeds the 2 MiB text editor limit.".into());
    }
    let parent = path
        .parent()
        .ok_or("Scratchpad path has no parent directory.")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    crate::fsutil::write_atomic(path, content.as_bytes())
}

/// Save the editor's `content`, edited from `baseline`. What an agent wrote meanwhile is merged
/// in (both sides kept where they touch the same lines), so typing never loses to an agent nor
/// the other way round. Returns the text now on disk, which the editor adopts.
pub(crate) fn save(path: &Path, baseline: &str, content: &str) -> Result<String, String> {
    let _guard = lock(path);
    let disk = read(path)?;
    let merged = if disk == baseline {
        content.to_string()
    } else {
        crate::merge3::merge(baseline, content, &disk, true).ok_or(
            "Scratchpad changed on disk and is too large to merge. Your unsaved text was preserved; copy it before reloading the scratchpad.",
        )?
    };
    if merged != disk {
        write(path, &merged)?;
    }
    Ok(merged)
}

pub(crate) fn update(path: &Path, mode: &str, content: &str) -> Result<(String, String), String> {
    if !matches!(mode, "append" | "rewrite") {
        return Err("Scratchpad mode must be `append` or `rewrite`.".into());
    }
    let _guard = lock(path);
    let before = read(path)?;
    let after = if mode == "append" {
        format!("{before}{content}")
    } else {
        content.to_string()
    };
    write(path, &after)?;
    Ok((before, after))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "oxi-scratchpad-{}-{}.md",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn append_rewrite_and_clear() {
        let path = test_path();
        assert_eq!(
            update(&path, "append", "Salut\n").unwrap(),
            (String::new(), "Salut\n".into())
        );
        assert_eq!(
            update(&path, "append", "notițe").unwrap().1,
            "Salut\nnotițe"
        );
        assert_eq!(update(&path, "rewrite", "nou").unwrap().1, "nou");
        update(&path, "rewrite", "").unwrap();
        assert_eq!(read(&path).unwrap(), "");
        let _ = std::fs::remove_file(path.with_extension("lock"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn invalid_mode_and_io_errors_preserve_content() {
        let path = test_path();
        update(&path, "rewrite", "original").unwrap();
        assert!(update(&path, "delete", "bad").is_err());
        assert_eq!(read(&path).unwrap(), "original");
        assert!(update(&path.join("child"), "append", "bad").is_err());
        assert!(update(&path, "rewrite", &"x".repeat(MAX_BYTES + 1)).is_err());
        assert_eq!(read(&path).unwrap(), "original");
        let _ = std::fs::remove_file(path.with_extension("lock"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn autosave_merges_agent_changes() {
        let path = test_path();
        save(&path, "", "first\nsecond\n").unwrap();
        update(&path, "append", "agent\n").unwrap();
        let merged = save(&path, "first\nsecond\n", "first edited\nsecond\n").unwrap();
        assert_eq!(merged, "first edited\nsecond\nagent\n");
        assert_eq!(read(&path).unwrap(), merged);
        // Both sides at the same spot: keep both, typing first.
        update(&path, "append", "more agent\n").unwrap();
        let merged = save(&path, &merged, &format!("{merged}typing")).unwrap();
        assert!(merged.ends_with("agent\ntyping\nmore agent\n"), "{merged}");
        let _ = std::fs::remove_file(path.with_extension("lock"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn concurrent_appends_are_not_lost() {
        let path = test_path();
        std::thread::scope(|scope| {
            for _ in 0..12 {
                scope.spawn(|| {
                    update(&path, "append", "x").unwrap();
                });
            }
        });
        assert_eq!(read(&path).unwrap(), "x".repeat(12));
        let _ = std::fs::remove_file(path.with_extension("lock"));
        std::fs::remove_file(path).unwrap();
    }
}
