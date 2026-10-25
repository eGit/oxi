//! A live tool-heavy turn must only grow at its tail: blocks already on screen keep their
//! position while thinking, tool calls and bash output stream in. With the transcript stuck to
//! the bottom, any reflow above the tail moves the whole visible history (seen as flicker).

use super::*;

type Step = Box<dyn Fn(&mut Vec<AssistantBlock>)>;

fn tool(id: &str, name: &str, args: &str) -> AssistantBlock {
    AssistantBlock::Tool {
        tool_call_id: id.into(),
        name: name.into(),
        args_summary: Some(args.into()),
        output: String::new(),
        diff: None,
        is_error: None,
        full_output_path: None,
        output_truncated: false,
        metadata: None,
    }
}

/// Render the run for a couple of frames; return the y of every painted text containing `needle`.
fn text_ys(ctx: &egui::Context, frame: &mut u32, msg: &ChatMessage, needle: &str) -> Vec<f32> {
    let mut ys = Vec::new();
    for _ in 0..2 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 5000.0),
            )),
            time: Some(f64::from(*frame) / 60.0),
            ..Default::default()
        };
        *frame += 1;
        let out = ctx.run_ui(raw, |ui| {
            render_assistant_message_run(ui, 0, std::slice::from_ref(msg), None);
        });
        ys = out
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::epaint::Shape::Text(text) if text.galley.text().contains(needle) => {
                    Some(text.pos.y)
                }
                _ => None,
            })
            .collect();
    }
    ys
}

fn check_stream(thinking: &str) {
    let ctx = egui::Context::default();
    crate::theme::apply_theme(&ctx, "dark");
    let mut frame = 0;
    let mut msg = ChatMessage {
        role: MsgRole::Assistant,
        text: String::new(),
        is_summary: false,
        attachments: vec![],
        blocks: vec![AssistantBlock::Thinking(thinking.into())],
        streaming: true,
        started_at: Some(std::time::Instant::now()),
        worked_duration: None,
        route: None,
        changes: None,
        evidence: None,
    };
    let first_line = thinking.lines().next().unwrap();
    let thinking_y = text_ys(&ctx, &mut frame, &msg, first_line)[0];

    let mut first_pill_y = None;
    for (index, name) in ["read", "grep", "read", "bash", "read", "edit"]
        .into_iter()
        .enumerate()
    {
        let args = match name {
            "bash" => r#"{"command":"cargo test retry"}"#.to_owned(),
            _ => format!(r#"{{"path":"src/file{index}.rs"}}"#),
        };
        msg.blocks.push(tool(&index.to_string(), name, &args));
        let mut steps: Vec<Step> = Vec::new();
        if name == "bash" {
            for lines in 1..6 {
                steps.push(Box::new(move |blocks| {
                    if let Some(AssistantBlock::Tool { output, .. }) = blocks.last_mut() {
                        *output = (0..lines).map(|i| format!("line {i}\n")).collect();
                    }
                }));
            }
        }
        steps.push(Box::new(|blocks| {
            if let Some(AssistantBlock::Tool {
                output, is_error, ..
            }) = blocks.last_mut()
            {
                output.push_str("ok");
                *is_error = Some(false);
            }
        }));
        steps.push(Box::new(|blocks| {
            blocks.push(AssistantBlock::Thinking("Now the next file.".into()));
        }));
        for (step, apply) in steps.iter().enumerate() {
            let before = text_ys(&ctx, &mut frame, &msg, first_line)[0];
            apply(&mut msg.blocks);
            let label = format!("{name}#{index} step {step}");
            let after = text_ys(&ctx, &mut frame, &msg, first_line)[0];
            assert_eq!(before, after, "first thinking body moved at {label}");
            assert_eq!(after, thinking_y, "first thinking body moved at {label}");
            let pill_y = text_ys(&ctx, &mut frame, &msg, "src/file0.rs")[0];
            assert_eq!(
                *first_pill_y.get_or_insert(pill_y),
                pill_y,
                "first tool pill moved at {label}"
            );
        }
    }
}

#[test]
fn tool_heavy_stream_keeps_earlier_blocks_in_place() {
    check_stream("I should look at how the config is loaded and where retries are handled.");
}

#[test]
fn long_thinking_keeps_its_height_when_the_next_tool_starts() {
    check_stream(
        &"I should look at how the config is loaded and where retries are handled.\n".repeat(25),
    );
}

#[test]
fn streaming_commentary_keeps_activity_visible_below_the_answer_until_finished() {
    for activity in [
        vec![],
        vec![AssistantBlock::Thinking("Checking the code.".into())],
        {
            let mut completed = tool("read", "read", r#"{"path":"src/main.rs"}"#);
            if let AssistantBlock::Tool { is_error, .. } = &mut completed {
                *is_error = Some(false);
            }
            vec![completed]
        },
    ] {
        let ctx = egui::Context::default();
        crate::theme::apply_theme(&ctx, "dark");
        let mut frame = 0;
        let mut msg = ChatMessage::streaming_assistant(None);
        msg.blocks = activity;
        msg.blocks
            .push(AssistantBlock::Answer("I will check the next file.".into()));

        let answer_y = text_ys(&ctx, &mut frame, &msg, "I will check the next file.")[0];
        for _ in 0..3 {
            let status_ys = text_ys(&ctx, &mut frame, &msg, "Working");
            assert!(
                status_ys.iter().any(|&y| y > answer_y),
                "a running turn must keep an activity indicator at its visible tail"
            );
            assert_eq!(
                text_ys(&ctx, &mut frame, &msg, "I will check the next file.")[0],
                answer_y,
                "the idle activity indicator must not move the answer"
            );
        }

        msg.finish_streaming();
        assert!(text_ys(&ctx, &mut frame, &msg, "Working").is_empty());
        assert!(!text_ys(&ctx, &mut frame, &msg, "I will check the next file.").is_empty());
    }
}
