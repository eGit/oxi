//! Evidence orchestration shared by every native provider protocol.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::Value;

use crate::agent::dispatch::DispatchParams;
use crate::agent::events::AgentEvent;
use crate::evidence::tools::Context;
use crate::evidence::{self, TurnMarker};

pub(super) async fn begin(
    cwd: &Path,
    cancel: Arc<AtomicBool>,
    timeout_cap_secs: u32,
    policy: evidence::Policy,
) -> Result<Context, String> {
    let cwd = cwd.to_path_buf();
    tokio::task::spawn_blocking(move || {
        let execution = evidence::exec::Execution {
            cancel: Some(cancel),
            timeout_cap: Some(Duration::from_secs(timeout_cap_secs.max(1).into())),
        };
        execution.check_cancelled()?;
        // Keep the baseline separate from native tools' per-file undo journal.
        let baseline = crate::git::checkpoint::snapshot(&cwd).ok();
        let state = baseline
            .as_ref()
            .map(evidence::WorkspaceState::from_snapshot)
            .or_else(|| evidence::WorkspaceState::capture(&cwd).ok());
        let context = Context {
            dir: evidence::new_conn_dir(),
            execution,
            policy: Some(policy),
        };
        evidence::begin_turn(
            &context.dir,
            &TurnMarker {
                baseline: state,
                turn: "turn".into(),
                started_ms: evidence::now_ms(),
                cwd: cwd.to_string_lossy().into_owned(),
                repo: baseline
                    .as_ref()
                    .map(|s| s.repo.to_string_lossy().into_owned()),
                base_tree: baseline.map(|s| s.tree),
            },
        )?;
        Ok(context)
    })
    .await
    .map_err(|error| error.to_string())?
}

pub(super) async fn run_provider_loop(
    p: DispatchParams<'_>,
    messages: &mut Vec<Value>,
    tools: &[Value],
) -> Result<(), String> {
    let Some(context) = p.env.evidence.as_ref() else {
        return crate::agent::dispatch::run_provider_loop(p, messages, tools).await;
    };
    let marker = evidence::current_turn(&context.dir).ok_or("Missing evidence turn marker.")?;
    let tx = p.tx;
    let result = crate::agent::dispatch::run_provider_loop(p, messages, tools).await;
    // Retain partial proof on errors or cancellation, before the terminal event.
    let decision = context
        .policy
        .as_ref()
        .map(|p| p.inspect(&context.dir, &marker.turn));
    let report = decision.map_or_else(
        || evidence::collect(&context.dir, &marker.turn, true),
        |d| d.report,
    );
    if report.requested || !report.is_empty() {
        let _ = tx.send(AgentEvent::Evidence(Box::new(report)));
    }
    result
}

#[cfg(test)]
mod tests;
