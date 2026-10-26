use super::*;
use crate::model::{RouteNote, ToolLocation, ToolMetadata, ToolStatus};

fn chat_msg(role: MsgRole, text: &str, blocks: Vec<AssistantBlock>) -> ChatMessage {
    ChatMessage {
        role,
        text: text.to_string(),
        is_summary: false,
        attachments: vec![],
        blocks,
        streaming: false,
        started_at: None,
        worked_duration: None,
        route: None,
        changes: None,
        evidence: None,
    }
}

fn answer_by(provider: LlmProviderKind, text: &str) -> ChatMessage {
    let mut m = chat_msg(
        MsgRole::Assistant,
        "",
        vec![AssistantBlock::Answer(text.into())],
    );
    m.route = Some(Box::new(RouteNote::direct(provider, "m", "")));
    m
}

fn tool(name: &str, args: &str, output: &str, failed: bool) -> AssistantBlock {
    AssistantBlock::Tool {
        tool_call_id: "1".into(),
        name: name.into(),
        args_summary: Some(args.into()),
        output: output.into(),
        diff: None,
        is_error: Some(failed),
        full_output_path: None,
        output_truncated: false,
        metadata: None,
    }
}

#[test]
fn keeps_turns_and_skips_thinking() {
    let chat = vec![
        chat_msg(MsgRole::User, "fix the bug", vec![]),
        chat_msg(
            MsgRole::Assistant,
            "",
            vec![
                AssistantBlock::Thinking("secret".into()),
                AssistantBlock::Answer("Done.".into()),
            ],
        ),
    ];
    let t = acp_history_transcript(&chat);
    assert_eq!(t, "User: fix the bug\n\nAssistant: Done.");
}

#[test]
fn drops_oldest_turns_over_budget() {
    let big = "x".repeat(ACP_HISTORY_MAX_CHARS - 10);
    let chat = vec![
        chat_msg(MsgRole::User, "old", vec![]),
        chat_msg(MsgRole::User, &big, vec![]),
    ];
    let t = acp_history_transcript(&chat);
    assert!(!t.contains("old"));
    assert!(t.starts_with("User: xxx"));
}

#[test]
fn keeps_tool_results_and_names_the_agent() {
    let mut reply = answer_by(LlmProviderKind::CodexAcp, "Tests fail.");
    reply.blocks.insert(
        0,
        tool("bash", "cargo test", "test a ... FAILED\n1 failed", true),
    );
    let chat = vec![chat_msg(MsgRole::User, "run tests", vec![]), reply];
    let t = acp_history_transcript(&chat);
    assert!(t.contains(
        "Assistant (Codex (ACP) · m): [tool bash: cargo test] failed\ntest a ... FAILED"
    ));
}

#[test]
fn older_tool_output_is_trimmed_from_the_front_but_errors_are_not() {
    let long = format!("{}END", "y".repeat(5_000));
    let mut chat = vec![chat_msg(
        MsgRole::Assistant,
        "",
        vec![
            tool("read", "a", &long, false),
            tool("bash", "b", &long, true),
        ],
    )];
    for _ in 0..RECENT_ASSISTANT_TURNS {
        chat.push(chat_msg(
            MsgRole::Assistant,
            "",
            vec![AssistantBlock::Answer("ok".into())],
        ));
    }
    let t = acp_history_transcript(&chat);
    let read = t.split("[tool bash").next().unwrap();
    assert!(read.len() < TOOL_OUTPUT_OLDER_CHARS + 50 && read.contains("END"));
    let bash = t.split("[tool bash").nth(1).unwrap();
    assert!(bash.len() > TOOL_OUTPUT_RECENT_CHARS && bash.contains("END"));
}

#[test]
fn lists_changed_files_first() {
    let mut edit = tool("Edit src/a.rs", "{}", "", false);
    if let AssistantBlock::Tool { metadata, .. } = &mut edit {
        *metadata = Some(Box::new(ToolMetadata {
            title: "Edit".into(),
            kind: "edit".into(),
            name: None,
            locations: vec![ToolLocation {
                path: "src/a.rs".into(),
                line: None,
            }],
            status: ToolStatus::Completed,
        }));
    }
    let chat = vec![
        chat_msg(MsgRole::Assistant, "", vec![edit]),
        chat_msg(
            MsgRole::Assistant,
            "",
            vec![tool("write", r#"{"path":"b.rs","content":"x"}"#, "", false)],
        ),
    ];
    let t = acp_history_transcript(&chat);
    assert!(t.starts_with("[Files changed in this conversation: src/a.rs, b.rs]\n\n"));
}

#[test]
fn missed_turns_start_after_the_agents_own_last_answer() {
    let chat = vec![
        chat_msg(MsgRole::User, "implement", vec![]),
        answer_by(LlmProviderKind::ClaudeCodeAcp, "implemented"),
        chat_msg(MsgRole::User, "review it", vec![]),
        answer_by(LlmProviderKind::CodexAcp, "found a bug"),
    ];
    let t = acp_missed_transcript(&chat, LlmProviderKind::ClaudeCodeAcp);
    assert_eq!(
        t,
        "User: review it\n\nAssistant (Codex (ACP) · m): found a bug"
    );
    assert!(acp_missed_transcript(&chat, LlmProviderKind::CodexAcp).is_empty());
    assert!(acp_missed_transcript(&chat, LlmProviderKind::CursorAcp).is_empty());
}

#[test]
fn no_recap_when_only_the_user_spoke_since() {
    let chat = vec![
        answer_by(LlmProviderKind::ClaudeCodeAcp, "done"),
        chat_msg(MsgRole::User, "retry", vec![]),
    ];
    assert!(acp_missed_transcript(&chat, LlmProviderKind::ClaudeCodeAcp).is_empty());
}

#[test]
fn recap_drops_read_only_output_and_trims_the_rest() {
    let mut read = tool("read", "src/a.rs", &"file body ".repeat(100), false);
    if let AssistantBlock::Tool { metadata, .. } = &mut read {
        *metadata = Some(Box::new(ToolMetadata {
            title: "Read src/a.rs".into(),
            kind: "read".into(),
            name: None,
            locations: vec![],
            status: ToolStatus::Completed,
        }));
    }
    let mut reply = answer_by(LlmProviderKind::CodexAcp, "Looks fine.");
    reply.blocks.insert(0, read);
    reply
        .blocks
        .insert(1, tool("bash", "cargo test", &"x".repeat(5_000), false));
    let chat = vec![
        answer_by(LlmProviderKind::ClaudeCodeAcp, "implemented"),
        chat_msg(MsgRole::User, "review it", vec![]),
        reply,
    ];
    let t = acp_missed_transcript(&chat, LlmProviderKind::ClaudeCodeAcp);
    assert!(t.contains("[tool read: src/a.rs]") && !t.contains("file body"));
    assert!(t.contains("[tool bash: cargo test]"));
    assert!(t.len() < 600, "{t}");
    // A blank session still gets the full recent output.
    assert!(acp_history_transcript(&chat).contains("file body"));
}

#[test]
fn claude_project_dir_matches_claude_code() {
    assert_eq!(
        claude_project_slug(Path::new("/Users/manu/Projects/oxi")),
        "-Users-manu-Projects-oxi"
    );
    assert_eq!(claude_project_slug(Path::new("/a/b.c d")), "-a-b-c-d");
}

#[test]
fn agent_notes_only_name_other_agents() {
    let chat = vec![answer_by(LlmProviderKind::ClaudeCodeAcp, "done")];
    let cwd = Path::new("/nowhere");
    // Nothing to hand over, nothing to point at.
    assert_eq!(
        with_agent_notes(String::new(), &chat, LlmProviderKind::CodexAcp, cwd),
        ""
    );
    // The agent's own notes are not pointed at.
    assert_eq!(
        with_agent_notes("recap".into(), &chat, LlmProviderKind::ClaudeCodeAcp, cwd),
        "recap"
    );
}

#[test]
fn an_agent_that_ran_out_mid_turn_gets_the_rest_of_that_reply() {
    let mut reply = answer_by(LlmProviderKind::CodexAcp, "finished it");
    if let Some(route) = reply.route.as_deref_mut() {
        route.failover_from = Some("Claude Code (ACP): usage limit reached".into());
    }
    let chat = vec![chat_msg(MsgRole::User, "implement", vec![]), reply];
    let t = acp_missed_transcript(&chat, LlmProviderKind::ClaudeCodeAcp);
    assert!(t.contains("finished it"), "{t}");
    assert!(!t.contains("User: implement"));
}

#[test]
fn multi_agent_chain_survives_reload_and_recaps_each_interrupted_agent() {
    let mut reply = answer_by(LlmProviderKind::CursorAcp, "finished all patches");
    let route = reply.route.as_deref_mut().unwrap();
    route.failover_from = Some("Codex (ACP): usage limit reached".into());
    route.interrupted_providers = vec![LlmProviderKind::ClaudeCodeAcp, LlmProviderKind::CodexAcp];
    // The chain is part of the persisted route, not only transient runner state.
    let json = serde_json::to_string(route).unwrap();
    *route = serde_json::from_str(&json).unwrap();
    let chat = vec![chat_msg(MsgRole::User, "implement", vec![]), reply];
    for provider in [LlmProviderKind::ClaudeCodeAcp, LlmProviderKind::CodexAcp] {
        let recap = acp_missed_transcript(&chat, provider);
        assert!(
            recap.contains("finished all patches"),
            "{provider:?}: {recap}"
        );
    }
    assert!(acp_missed_transcript(&chat, LlmProviderKind::CursorAcp).is_empty());
}
