//! The evidence tools oxi's MCP server offers ACP agents (see [`super`]).

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::capture::{self, ScreenTarget};
use super::{Claim, EvidenceItem, ItemKind, Origin, Phase, TurnMarker};

/// Evidence storage and command limits belonging to a single agent run.
#[derive(Clone, Debug)]
pub struct Context {
    pub dir: PathBuf,
    pub execution: super::exec::Execution,
}

/// Commands must use the same approval rules as an ordinary shell tool call.
pub fn command_args(name: &str, args: &Value) -> Option<Value> {
    if !matches!(name, CAPTURE | RUN) {
        return None;
    }
    let command = [&args["command"], &args["during"]]
        .into_iter()
        .filter_map(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" && ");
    (!command.is_empty()).then(|| json!({ "command": command }))
}

pub fn native_definitions() -> Vec<Value> {
    definitions()
        .into_iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool["name"],
                    "description": tool["description"],
                    "parameters": tool["inputSchema"],
                }
            })
        })
        .collect()
}

pub const CAPTURE: &str = "evidence_capture";
pub const ADD: &str = "evidence_add";
pub const RUN: &str = "evidence_run";
pub const BASE_CHECKOUT: &str = "evidence_base_checkout";
pub const SUBMIT: &str = "evidence_submit";
pub const NAMES: [&str; 5] = [CAPTURE, ADD, RUN, BASE_CHECKOUT, SUBMIT];

const DEFAULT_RUN_SECS: u64 = 600;
const MAX_RUN_SECS: u64 = 1800;
/// Output tail returned to the agent (the full tail is kept in the item).
const REPLY_OUTPUT_CHARS: usize = 3000;

/// Read by the agent in the server's `initialize` reply.
pub const INSTRUCTIONS: &str = "The evidence_* tools let you prove a change works: oxi itself captures before/after screenshots or recordings and runs checks, then shows them to the user next to your reply. Use them when the user's message starts with [Evidence mode] or asks you to show or prove a result.";

/// The connection folder, when the app enabled evidence for this server.
pub fn conn_dir() -> Option<PathBuf> {
    std::env::var_os(super::DIR_ENV)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

pub fn definitions() -> Vec<Value> {
    let phase = json!({
        "type": "string",
        "enum": ["before", "after"],
        "description": "\"before\": the state before your change (capture it first). \"after\": the changed result."
    });
    let label = json!({
        "type": "string",
        "description": "What it shows, e.g. \"Login page\" or \"Settings > Theme\". Use the same label for the before and after of one thing."
    });
    vec![
        json!({
            "name": CAPTURE,
            "description": "Capture what the user would see, made by oxi itself: a page (`url`, headless Chrome), an app window (`window`: part of the app name or window title, macOS), the whole `screen`, or an image/video a command writes (`command` + `output`, e.g. a Playwright script or a UI snapshot test). Give `seconds` (1-30) for a recording instead of a still; `during` is a shell command run while recording (clicks, typing, a script) so the recording shows the interaction. Returns the evidence id to cite in evidence_submit.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "phase": phase,
                    "label": label,
                    "url": { "type": "string", "description": "Page to screenshot, usually the running dev server." },
                    "window": { "type": "string", "description": "App name or window title to capture (macOS); the app must be running with its window on screen." },
                    "screen": { "type": "boolean", "description": "Capture the main screen." },
                    "command": { "type": "string", "description": "Shell command (run in the workspace) that writes `output`." },
                    "output": { "type": "string", "description": "Image or video file `command` writes, relative to the workspace." },
                    "seconds": { "type": "integer", "description": "Record this long instead of taking a still (window or screen)." },
                    "during": { "type": "string", "description": "Shell command run while recording, to drive the app." },
                    "width": { "type": "integer", "description": "Viewport width for `url` (default 1280)." },
                    "height": { "type": "integer", "description": "Viewport height for `url` (default 800)." },
                    "wait_ms": { "type": "integer", "description": "For `url`: time to let the page settle (default 2000). For window/screen: delay before capturing." }
                },
                "required": ["phase", "label"]
            }
        }),
        json!({
            "name": ADD,
            "description": "Register an image or video you produced yourself (e.g. with your own browser automation). Prefer evidence_capture: oxi can only vouch for a file you add if it was written during this turn.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "phase": phase,
                    "label": label,
                    "path": { "type": "string", "description": "Image or video file, relative to the workspace or absolute." }
                },
                "required": ["phase", "label", "path"]
            }
        }),
        json!({
            "name": RUN,
            "description": "Run a check (tests, build, linter, a curl against the running app) through oxi. oxi records the exit code and output, so a passing check is proof the user can trust. Returns the evidence id and the result.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "label": { "type": "string", "description": "What it checks, e.g. \"Unit tests\"." },
                    "command": { "type": "string", "description": "Shell command, run in the workspace." },
                    "timeout_secs": { "type": "integer", "description": "Default 600, at most 1800." }
                },
                "required": ["label", "command"]
            }
        }),
        json!({
            "name": BASE_CHECKOUT,
            "description": "A copy of the workspace as it was when this turn started (before your edits), for capturing \"before\" after you already changed things. Run the old version from that folder (on another port if needed) and capture it with phase \"before\". Ignored folders (build output, node_modules) are not copied; node_modules and virtualenvs are linked from the workspace.",
            "inputSchema": { "type": "object", "properties": {} }
        }),
        json!({
            "name": SUBMIT,
            "description": "Finish the evidence: a one-paragraph summary of what changed and what you verified, and your claims, each citing the evidence ids that prove it. oxi marks a claim proven only when everything it cites was captured or run by oxi and every cited check passed. Call it last; calling again replaces the previous submission.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "summary": { "type": "string" },
                    "claims": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "text": { "type": "string", "description": "One behaviour, stated plainly, e.g. \"A wrong password shows an inline error\"." },
                                "evidence": { "type": "array", "items": { "type": "string" }, "description": "Evidence ids, e.g. [\"e1\", \"e3\"]." }
                            },
                            "required": ["text", "evidence"]
                        }
                    }
                },
                "required": ["summary", "claims"]
            }
        }),
    ]
}

/// Handle one evidence tool call: the text for the agent, or an error.
pub fn call(name: &str, args: &Value) -> Result<String, String> {
    let conn = conn_dir().ok_or("Evidence is not available in this session.")?;
    Context {
        dir: conn,
        execution: Default::default(),
    }
    .call(name, args)
}

impl Context {
    pub fn call(&self, name: &str, args: &Value) -> Result<String, String> {
        self.execution.check_cancelled()?;
        let conn = &self.dir;
        let marker = super::current_turn(conn).ok_or(
            "oxi has not started an evidence turn yet; try again after the user's next message.",
        )?;
        let turn_dir = super::turn_dir(conn, &marker.turn);
        std::fs::create_dir_all(&turn_dir).map_err(|e| e.to_string())?;
        match name {
            CAPTURE => capture_tool(args, &marker, &turn_dir, &self.execution),
            ADD => add_tool(args, &marker, &turn_dir, &self.execution),
            RUN => run_tool(args, &marker, &turn_dir, &self.execution),
            BASE_CHECKOUT => base_checkout(&marker, &turn_dir),
            SUBMIT => submit_tool(args, conn, &marker, &turn_dir),
            _ => Err(format!("Unknown evidence tool `{name}`.")),
        }
    }
}

fn str_arg<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args[key].as_str().map(str::trim).filter(|s| !s.is_empty())
}

fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    str_arg(args, key).ok_or_else(|| format!("Missing `{key}`."))
}

fn phase_arg(args: &Value) -> Result<Phase, String> {
    match required(args, "phase")? {
        "before" => Ok(Phase::Before),
        "after" => Ok(Phase::After),
        other => Err(format!(
            "`phase` must be \"before\" or \"after\", not \"{other}\"."
        )),
    }
}

fn new_item(
    id: String,
    kind: ItemKind,
    phase: Phase,
    label: &str,
    origin: Origin,
    cwd: &str,
) -> EvidenceItem {
    EvidenceItem {
        id,
        kind,
        phase,
        label: label.to_string(),
        origin,
        file: None,
        preview: None,
        fresh: false,
        source: String::new(),
        command: None,
        cwd: Some(cwd.to_string()),
        exit_code: None,
        timed_out: false,
        output: String::new(),
        duration_ms: 0,
    }
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn capture_tool(
    args: &Value,
    marker: &TurnMarker,
    turn_dir: &Path,
    execution: &super::exec::Execution,
) -> Result<String, String> {
    let phase = phase_arg(args)?;
    let label = required(args, "label")?;
    let cwd = Path::new(&marker.cwd);
    let id = super::next_id(turn_dir);
    let stem = turn_dir.join(&id);
    let seconds = args["seconds"].as_u64().unwrap_or(0);
    let wait_ms = args["wait_ms"].as_u64();
    let targets = [
        str_arg(args, "url").is_some(),
        str_arg(args, "window").is_some(),
        args["screen"].as_bool() == Some(true),
        str_arg(args, "command").is_some(),
    ];
    if targets.iter().filter(|t| **t).count() != 1 {
        return Err(
            "Give exactly one of `url`, `window`, `screen: true`, or `command` + `output`.".into(),
        );
    }
    let mut command_result = None;
    let captured = if let Some(url) = str_arg(args, "url") {
        if seconds > 0 {
            return Err("Recordings are made of a window or the screen. For a page, open it in a visible browser and capture that window, or record it with your own browser automation through `command` + `output`.".into());
        }
        let width = args["width"].as_u64().unwrap_or(1280).clamp(320, 3840) as u32;
        let height = args["height"].as_u64().unwrap_or(800).clamp(240, 4000) as u32;
        capture::url_with_execution(
            url,
            &stem.with_extension("png"),
            width,
            height,
            wait_ms.unwrap_or(2000),
            execution,
        )?
    } else if let Some(command) = str_arg(args, "command") {
        let output = required(args, "output")?;
        let (captured, result) = capture::command_output_with_execution(
            command,
            output,
            cwd,
            Duration::from_secs(DEFAULT_RUN_SECS),
            &stem,
            execution,
        )?;
        command_result = Some((command.to_string(), result));
        captured
    } else {
        if let Some(ms) = wait_ms {
            execution.wait(Duration::from_millis(ms.min(30_000)))?;
        }
        let target = match str_arg(args, "window") {
            Some(window) => ScreenTarget::Window(window),
            None => ScreenTarget::Screen,
        };
        capture::screen_with_execution(
            target,
            &stem,
            seconds,
            str_arg(args, "during"),
            cwd,
            execution,
        )?
    };
    let mut item = new_item(
        id.clone(),
        captured.kind,
        phase,
        label,
        Origin::Oxi,
        &marker.cwd,
    );
    item.file = Some(path_string(&captured.file));
    item.preview = captured.preview.as_deref().map(path_string);
    item.source = captured.source;
    if let Some((command, result)) = command_result {
        item.command = Some(command);
        item.exit_code = result.exit_code;
        item.duration_ms = result.duration_ms;
    }
    super::save_item(turn_dir, &item)?;
    Ok(format!(
        "Captured {} {} \"{label}\" as {id}: {}",
        match phase {
            Phase::Before => "before",
            _ => "after",
        },
        match item.kind {
            ItemKind::Video => "recording",
            _ => "image",
        },
        item.file.as_deref().unwrap_or_default()
    ))
}

fn add_tool(
    args: &Value,
    marker: &TurnMarker,
    turn_dir: &Path,
    execution: &super::exec::Execution,
) -> Result<String, String> {
    let phase = phase_arg(args)?;
    let label = required(args, "label")?;
    let path = Path::new(&marker.cwd).join(required(args, "path")?);
    let kind = capture::kind_of(&path)
        .ok_or("`path` must be an image or video (png, jpg, gif, webp, mp4, mov, webm).")?;
    let modified = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    let modified_ms = modified
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default();
    let id = super::next_id(turn_dir);
    let file = capture::copy_into(&path, &turn_dir.join(&id))?;
    let mut item = new_item(id.clone(), kind, phase, label, Origin::Agent, &marker.cwd);
    item.fresh = modified_ms >= marker.started_ms;
    item.preview = (kind == ItemKind::Video)
        .then(|| capture::video_preview_with_execution(&file, execution))
        .flatten()
        .map(|p| path_string(&p));
    item.file = Some(path_string(&file));
    item.source = format!("Supplied by the agent · {}", path.display());
    super::save_item(turn_dir, &item)?;
    Ok(if item.fresh {
        format!("Added {id}.")
    } else {
        format!(
            "Added {id}. It was written before this turn, so oxi cannot vouch for it; capture it with evidence_capture instead if you can."
        )
    })
}

fn run_tool(
    args: &Value,
    marker: &TurnMarker,
    turn_dir: &Path,
    execution: &super::exec::Execution,
) -> Result<String, String> {
    let label = required(args, "label")?;
    let command = required(args, "command")?;
    let timeout = args["timeout_secs"]
        .as_u64()
        .unwrap_or(DEFAULT_RUN_SECS)
        .clamp(1, MAX_RUN_SECS);
    let result = execution.run(
        command,
        Path::new(&marker.cwd),
        Duration::from_secs(timeout),
    )?;
    let id = super::next_id(turn_dir);
    let mut item = new_item(
        id.clone(),
        ItemKind::Check,
        Phase::Check,
        label,
        Origin::Oxi,
        &marker.cwd,
    );
    item.command = Some(command.to_string());
    item.exit_code = result.exit_code;
    item.timed_out = result.timed_out;
    item.output = result.output.clone();
    item.duration_ms = result.duration_ms;
    super::save_item(turn_dir, &item)?;
    let verdict = if result.timed_out {
        format!("timed out after {timeout}s")
    } else {
        match result.exit_code {
            Some(0) => "passed (exit code 0)".to_string(),
            Some(code) => format!("failed (exit code {code})"),
            None => "was killed".to_string(),
        }
    };
    let output = result.output.trim();
    let tail_start = output
        .char_indices()
        .rev()
        .nth(REPLY_OUTPUT_CHARS)
        .map_or(0, |(i, _)| i);
    Ok(format!(
        "{id}: \"{label}\" {verdict} in {:.1}s.\n{}",
        result.duration_ms as f64 / 1000.0,
        &output[tail_start..]
    ))
}

fn base_checkout(marker: &TurnMarker, turn_dir: &Path) -> Result<String, String> {
    let (Some(repo), Some(tree)) = (&marker.repo, &marker.base_tree) else {
        return Err(
            "No snapshot of the starting state exists (the workspace is not a git repository)."
                .into(),
        );
    };
    let dest = turn_dir.join("base");
    if !dest.is_dir() {
        crate::git::checkpoint::checkout_to(Path::new(repo), tree, &dest)?;
        for name in ["node_modules", ".venv", "venv"] {
            let source = Path::new(repo).join(name);
            if source.is_dir() {
                link_dir(&source, &dest.join(name));
            }
        }
    }
    let cwd = Path::new(&marker.cwd);
    let sub = cwd.strip_prefix(repo).unwrap_or(Path::new(""));
    let folder = dest.join(sub);
    Ok(format!(
        "The workspace as it was when this turn started is at:\n{}\nRun the old version from there (use another port, or `CARGO_TARGET_DIR` / a separate build folder so the current build is not overwritten), capture it with phase \"before\", then stop it. Do not edit files there.",
        folder.display()
    ))
}

#[cfg(unix)]
fn link_dir(source: &Path, link: &Path) {
    let _ = std::os::unix::fs::symlink(source, link);
}

#[cfg(windows)]
fn link_dir(source: &Path, link: &Path) {
    let _ = std::os::windows::fs::symlink_dir(source, link);
}

fn submit_tool(
    args: &Value,
    conn: &Path,
    marker: &TurnMarker,
    turn_dir: &Path,
) -> Result<String, String> {
    let summary = str_arg(args, "summary").unwrap_or_default();
    let claims: Vec<Claim> = args["claims"]
        .as_array()
        .map(|claims| {
            claims
                .iter()
                .filter_map(|c| {
                    let text = c["text"].as_str()?.trim();
                    (!text.is_empty()).then(|| Claim {
                        text: text.to_string(),
                        evidence: c["evidence"]
                            .as_array()
                            .map(|ids| {
                                ids.iter()
                                    .filter_map(|id| id.as_str().map(|s| s.trim().to_string()))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    super::save_submission(turn_dir, summary, &claims)?;
    let report = super::collect(conn, &marker.turn, true);
    let (attested, declared, contradicted) = report.tally();
    let problems = report.problems();
    let mut text =
        format!("Submitted: {attested} proven, {declared} unproven, {contradicted} contradicted.");
    if problems.is_empty() {
        text.push_str(" The evidence is complete.");
    } else {
        text.push_str("\nStill missing:\n");
        for problem in problems {
            text.push_str("- ");
            text.push_str(&problem);
            text.push('\n');
        }
    }
    Ok(text)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn checks_and_submission_flow_through_the_store() {
        let conn = super::super::tests::tempfile_dir("tools");
        let cwd = conn.join("work");
        std::fs::create_dir_all(&cwd).unwrap();
        let marker = TurnMarker {
            turn: "t1".into(),
            started_ms: super::super::now_ms(),
            cwd: path_string(&cwd),
            repo: None,
            base_tree: None,
        };
        super::super::begin_turn(&conn, &marker).unwrap();
        let turn_dir = super::super::turn_dir(&conn, "t1");
        let ok = run_tool(
            &json!({"label": "Unit tests", "command": "echo 3 passed"}),
            &marker,
            &turn_dir,
            &Default::default(),
        )
        .unwrap();
        assert!(ok.starts_with("e1: \"Unit tests\" passed"), "{ok}");
        let failed = run_tool(
            &json!({"label": "Lint", "command": "exit 2"}),
            &marker,
            &turn_dir,
            &Default::default(),
        )
        .unwrap();
        assert!(failed.contains("failed (exit code 2)"), "{failed}");
        std::fs::write(cwd.join("shot.png"), b"png").unwrap();
        let added = add_tool(
            &json!({"phase": "after", "label": "Home", "path": "shot.png"}),
            &marker,
            &turn_dir,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(added, "Added e3.");
        let submitted = submit_tool(
            &json!({"summary": "Done", "claims": [
                {"text": "Tests pass", "evidence": ["e1"]},
                {"text": "Lint is clean", "evidence": ["e2"]},
            ]}),
            &conn,
            &marker,
            &turn_dir,
        )
        .unwrap();
        assert!(
            submitted.starts_with("Submitted: 1 proven, 0 unproven, 1 contradicted."),
            "{submitted}"
        );
        assert!(submitted.contains("\"Home\" has an after capture but no before"));
        assert!(base_checkout(&marker, &turn_dir).is_err());
    }
}
