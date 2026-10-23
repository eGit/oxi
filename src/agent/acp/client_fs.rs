//! ACP client filesystem request handlers.

use serde_json::Value;

pub(super) fn fs_read_text(params: &Value) -> Result<String, String> {
    let path = params
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or("fs/read_text_file: missing path")?;
    // An open editor tab with unsaved changes is what the user sees as the file.
    let content = match crate::open_buffers::unsaved(std::path::Path::new(path)) {
        Some(text) => text.to_string(),
        None => std::fs::read_to_string(path).map_err(|e| format!("read {path} failed: {e}"))?,
    };
    let line = params.get("line").and_then(|v| v.as_u64());
    let limit = params.get("limit").and_then(|v| v.as_u64());
    if line.is_none() && limit.is_none() {
        return Ok(content);
    }
    // `line` is 1-based; `limit` is a line count.
    let start = line.map(|l| l.saturating_sub(1) as usize).unwrap_or(0);
    let mut out: Vec<&str> = content.lines().skip(start).collect();
    if let Some(lim) = limit {
        out.truncate(lim as usize);
    }
    Ok(out.join("\n"))
}

pub(super) fn fs_write_text(params: &Value) -> Result<(), String> {
    let path = params
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or("fs/write_text_file: missing path")?;
    let content = params
        .get("content")
        .and_then(|c| c.as_str())
        .unwrap_or_default();
    if let Some(parent) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    crate::open_buffers::write_text(std::path::Path::new(path), content)
        .map_err(|e| format!("write {path} failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_buffers::{self, OpenTab};
    use serde_json::json;

    #[test]
    fn reads_follow_successful_writes_before_the_ui_applies_them() {
        let _guard = open_buffers::TEST_LOCK.lock().unwrap();
        let path = std::env::temp_dir().join(format!("oxi-buffer-write-{}", rand::random::<u64>()));
        std::fs::write(&path, "disk").unwrap();
        let publish = |unsaved| {
            open_buffers::publish(std::iter::once(OpenTab {
                path: &path,
                revision: 1,
                unsaved,
            }));
        };
        publish(Some("user draft"));
        let read = json!({"path": path});
        fs_write_text(&json!({"path": path, "content": "user draft + first edit"})).unwrap();
        assert_eq!(fs_read_text(&read).unwrap(), "user draft + first edit");
        // A frame can still publish the old tab after the worker finished its write.
        publish(None);
        assert_eq!(fs_read_text(&read).unwrap(), "user draft + first edit");
        fs_write_text(&json!({"path": path, "content": "user draft + both edits"})).unwrap();
        assert_eq!(fs_read_text(&read).unwrap(), "user draft + both edits");
        assert_eq!(
            open_buffers::take_agent_writes(),
            vec![(path.clone(), "user draft + both edits".into())]
        );
        // Draining the queue must not briefly expose the old draft either.
        assert_eq!(fs_read_text(&read).unwrap(), "user draft + both edits");
        publish(None);
        assert_eq!(fs_read_text(&read).unwrap(), "user draft + both edits");
        open_buffers::publish(std::iter::empty());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn failed_writes_keep_the_unsaved_buffer_and_do_not_queue_an_edit() {
        let _guard = open_buffers::TEST_LOCK.lock().unwrap();
        let path =
            std::env::temp_dir().join(format!("oxi-buffer-failed-{}", rand::random::<u64>()));
        // Replacing a file by a directory forces a write error on every platform.
        std::fs::create_dir(&path).unwrap();
        open_buffers::publish(std::iter::once(OpenTab {
            path: &path,
            revision: 1,
            unsaved: Some("user draft"),
        }));
        assert!(fs_write_text(&json!({"path": path, "content": "not saved"})).is_err());
        assert_eq!(fs_read_text(&json!({"path": path})).unwrap(), "user draft");
        assert!(!open_buffers::has_agent_write(&path));
        assert!(open_buffers::take_agent_writes().is_empty());
        open_buffers::publish(std::iter::empty());
        std::fs::remove_dir(path).unwrap();
    }
}
