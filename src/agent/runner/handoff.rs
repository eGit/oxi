//! Plain-text transcripts that carry a chat across ACP agents: the whole conversation for an
//! agent that had to start a blank session, or just the turns it missed (another agent spoke)
//! for one that resumed its own session.

use std::path::{Path, PathBuf};

use crate::model::{AssistantBlock, ChatMessage, MsgRole, ToolMetadata};
use crate::settings::LlmProviderKind;

/// Upper bound on the replayed transcript; older turns are dropped first.
pub(super) const ACP_HISTORY_MAX_CHARS: usize = 200_000;
/// Tool output kept per call in the latest assistant turns, and for failed calls anywhere.
const TOOL_OUTPUT_RECENT_CHARS: usize = 2_000;
/// Tool output kept per call in older turns.
const TOOL_OUTPUT_OLDER_CHARS: usize = 300;
/// How many trailing assistant turns count as recent.
const RECENT_ASSISTANT_TURNS: usize = 3;

/// Transcript of earlier turns for an agent that had to start a blank session (it could not
/// resume its own), so it still knows the conversation. Keeps user text, compaction summaries,
/// answers, tool calls with their (trimmed) results, and the files the conversation changed;
/// drops thinking.
pub(super) fn acp_history_transcript(chat: &[ChatMessage]) -> String {
    transcript(chat, false)
}

/// Turns `provider` has not seen: everything after its own last answer. Empty when it answered
/// last or never answered in this chat (a resumed session from before agents were recorded
/// already holds the conversation). The other agent's answers already sum up what it read, so
/// read-only tools keep only their call and other tools a short tail.
pub(super) fn acp_missed_transcript(chat: &[ChatMessage], provider: LlmProviderKind) -> String {
    // A reply another agent finished after `provider` ran out of quota mid-turn: `provider` saw
    // the request but not the rest of the reply.
    let ran_out = format!("{}:", provider.label());
    let Some(start) = chat
        .iter()
        .rposition(|m| {
            m.role == MsgRole::Assistant
                && m.route.as_deref().is_some_and(|r| {
                    r.provider == provider
                        || r.interrupted_providers.contains(&provider)
                        || r.failover_from
                            .as_deref()
                            .is_some_and(|f| f.starts_with(&ran_out))
                })
        })
        .map(|i| match chat[i].route.as_deref() {
            Some(r) if r.provider == provider => i + 1,
            _ => i,
        })
    else {
        return String::new();
    };
    let missed = &chat[start..];
    // Only the user's own follow-ups (e.g. a failed turn) are not worth a recap.
    if !missed.iter().any(|m| m.role == MsgRole::Assistant) {
        return String::new();
    }
    transcript(missed, true)
}

/// Append where the other agents in this chat keep their own notes (Claude Code's auto-memory,
/// Codex's memories), so the agent can look something up when the transcript lacks it. Only
/// paths that exist, and nothing when there is no transcript to hand over.
pub(super) fn with_agent_notes(
    transcript: String,
    chat: &[ChatMessage],
    provider: LlmProviderKind,
    cwd: &Path,
) -> String {
    if transcript.is_empty() {
        return transcript;
    }
    let mut others: Vec<LlmProviderKind> = Vec::new();
    for kind in chat
        .iter()
        .filter_map(|m| m.route.as_deref().map(|r| r.provider))
    {
        if kind != provider && !others.contains(&kind) {
            others.push(kind);
        }
    }
    let notes: Vec<String> = others
        .into_iter()
        .filter_map(|kind| {
            let path = notes_index(kind, cwd).filter(|p| p.is_file())?;
            Some(format!(
                "{}: {}",
                kind.label().trim_end_matches(" (ACP)"),
                path.display()
            ))
        })
        .collect();
    if notes.is_empty() {
        return transcript;
    }
    format!(
        "{transcript}\n\n[Notes the other agents keep across sessions: {}. Each is an index with details in the files beside it; grep or read only the part you need when the conversation lacks context. They can be out of date, so check them against the code.]",
        notes.join("; ")
    )
}

/// The memory index an agent keeps for `cwd`, if it has a known place.
fn notes_index(kind: LlmProviderKind, cwd: &Path) -> Option<PathBuf> {
    let home_or = |var: &str, dir: &str| {
        std::env::var_os(var)
            .map(PathBuf::from)
            .or_else(|| dirs::home_dir().map(|h| h.join(dir)))
    };
    match kind {
        LlmProviderKind::ClaudeCodeAcp => Some(
            home_or("CLAUDE_CONFIG_DIR", ".claude")?
                .join("projects")
                .join(claude_project_slug(cwd))
                .join("memory")
                .join("MEMORY.md"),
        ),
        LlmProviderKind::CodexAcp => Some(
            home_or("CODEX_HOME", ".codex")?
                .join("memories")
                .join("memory_summary.md"),
        ),
        _ => None,
    }
}

/// Claude Code's per-project directory name: the path with every other character as `-`.
fn claude_project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Tools that only look things up; their output is not worth repeating in a recap.
fn read_only(name: &str, metadata: Option<&ToolMetadata>) -> bool {
    match metadata {
        Some(meta) => matches!(meta.kind.as_str(), "read" | "search" | "fetch" | "think"),
        None => crate::agent::approval::allowed_in_plan_mode(name),
    }
}

/// `recap`: the turns another agent answered, for one that resumes its own session.
fn transcript(chat: &[ChatMessage], recap: bool) -> String {
    let assistant_turns = chat.iter().filter(|m| m.role == MsgRole::Assistant).count();
    let mut seen_assistant = 0;
    let mut turns: Vec<String> = Vec::new();
    for m in chat {
        match m.role {
            MsgRole::User if m.is_summary => {
                turns.push(format!(
                    "[Summary of earlier conversation]\n{}",
                    m.text.trim()
                ));
            }
            MsgRole::User => {
                let mut t = format!("User: {}", m.user_prompt_text().trim());
                let images = m
                    .attachments
                    .iter()
                    .filter(|a| matches!(a, crate::model::UserAttachment::Image { .. }))
                    .count();
                if images > 0 {
                    t.push_str(&format!("\n[{images} image(s) attached]"));
                }
                turns.push(t);
            }
            MsgRole::Assistant => {
                seen_assistant += 1;
                let recent = assistant_turns - seen_assistant < RECENT_ASSISTANT_TURNS;
                let parts: Vec<String> = m
                    .blocks
                    .iter()
                    .filter_map(|b| block_text(b, recent, recap))
                    .collect();
                if !parts.is_empty() {
                    let who = m
                        .route
                        .as_deref()
                        .map(|r| format!("Assistant ({} · {})", r.provider.label(), r.model))
                        .unwrap_or_else(|| "Assistant".to_string());
                    turns.push(format!("{who}: {}", parts.join("\n")));
                }
            }
        }
    }
    let files = changed_files(chat);
    let header = if files.is_empty() {
        String::new()
    } else {
        format!("[Files changed in this conversation: {}]", files.join(", "))
    };
    let budget = ACP_HISTORY_MAX_CHARS.saturating_sub(header.len());
    let mut total = 0;
    let mut start = turns.len();
    while start > 0 && total + turns[start - 1].len() <= budget {
        start -= 1;
        total += turns[start].len() + 2;
    }
    let body = turns[start..].join("\n\n");
    if header.is_empty() || body.is_empty() {
        body
    } else {
        format!("{header}\n\n{body}")
    }
}

fn block_text(block: &AssistantBlock, recent: bool, recap: bool) -> Option<String> {
    match block {
        AssistantBlock::Answer(a) if !a.trim().is_empty() => Some(a.trim().to_string()),
        AssistantBlock::Tool {
            name,
            args_summary,
            output,
            diff,
            is_error,
            metadata,
            ..
        } => {
            let failed = *is_error == Some(true);
            let mut t = match args_summary {
                Some(args) => format!("[tool {name}: {args}]"),
                None => format!("[tool {name}]"),
            };
            if failed {
                t.push_str(" failed");
            }
            let limit = if failed {
                TOOL_OUTPUT_RECENT_CHARS
            } else if recap && read_only(name, metadata.as_deref()) {
                0
            } else if recent && !recap {
                TOOL_OUTPUT_RECENT_CHARS
            } else {
                TOOL_OUTPUT_OLDER_CHARS
            };
            let output = if limit == 0 { "" } else { output.trim() };
            if !output.is_empty() {
                t.push('\n');
                t.push_str(&tail(output, limit));
            }
            if recent && let Some(diff) = diff.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
                t.push('\n');
                t.push_str(&head(diff, TOOL_OUTPUT_RECENT_CHARS));
            }
            Some(t)
        }
        _ => None,
    }
}

/// The end of `s` (test summaries and errors come last), at most `max` bytes.
fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = s.len() - max;
    while !s.is_char_boundary(cut) {
        cut += 1;
    }
    format!("[…]{}", &s[cut..])
}

fn head(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}[…]", &s[..cut])
}

/// Paths written by edit-like tools, in first-touched order.
fn changed_files(chat: &[ChatMessage]) -> Vec<String> {
    let mut files: Vec<String> = Vec::new();
    let mut add = |path: &str| {
        if !path.is_empty() && !files.iter().any(|f| f == path) {
            files.push(path.to_string());
        }
    };
    for b in chat.iter().flat_map(|m| &m.blocks) {
        let AssistantBlock::Tool {
            name,
            args_summary,
            metadata,
            is_error,
            ..
        } = b
        else {
            continue;
        };
        if *is_error == Some(true) {
            continue;
        }
        match metadata {
            Some(meta) if matches!(meta.kind.as_str(), "edit" | "delete" | "move") => {
                for location in &meta.locations {
                    add(&location.path);
                }
            }
            Some(_) => {}
            None if matches!(name.as_str(), "edit" | "write") => {
                if let Some(path) = args_summary
                    .as_deref()
                    .and_then(|a| serde_json::from_str::<serde_json::Value>(a).ok())
                    .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(str::to_owned))
                {
                    add(&path);
                }
            }
            None => {}
        }
    }
    files
}

#[cfg(test)]
#[path = "handoff/tests.rs"]
mod tests;
