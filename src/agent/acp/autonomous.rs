//! Agent turns nobody prompted. Claude Code wakes itself when a background command finishes, a
//! scheduled wake-up fires or a peer messages it, and streams that cycle as ordinary
//! `session/update`s after the user's turn has already ended. Those updates are held here until
//! the UI attaches a turn to show them (see [`super::AcpManager::attach`]); before, they were
//! dropped and the chat looked stuck on the agent's "I'll report back when it finishes".

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use serde_json::Value;

/// Keys of the sessions holding updates that no turn has shown yet.
static PENDING: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Wakes the UI when a session starts holding updates.
static WAKER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// More held updates than this drops the oldest, so a cycle nobody attaches to stays bounded.
const MAX_HELD: usize = 20_000;

/// Install the UI wake-up. Only the first call takes effect.
pub fn set_waker(wake: impl Fn() + Send + Sync + 'static) {
    if WAKER.get().is_none() {
        let _ = WAKER.set(Box::new(wake));
    }
}

/// Sessions (by ACP session key) whose agent is running a turn of its own.
pub fn pending() -> Vec<String> {
    PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The UI has attached a turn for `session_key`, or has no chat for it any more.
pub fn claim(session_key: &str) {
    PENDING
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|key| key != session_key);
}

fn announce(session_key: &str) {
    {
        let mut pending = PENDING.lock().unwrap_or_else(|e| e.into_inner());
        if pending.iter().any(|key| key == session_key) {
            return;
        }
        pending.push(session_key.to_owned());
    }
    wake();
}

/// Wake the UI to look at what agents reported between turns.
pub(super) fn wake() {
    if let Some(wake) = WAKER.get() {
        wake();
    }
}

/// Updates one connection received with no turn in flight, oldest first.
pub(super) struct Held {
    session_key: String,
    /// Off while the session is set up: `session/load` replays the whole history as updates.
    armed: AtomicBool,
    updates: Mutex<VecDeque<Value>>,
    /// `session/request_permission` requests (id, params) of such a turn, unanswered until a
    /// turn shows them: the agent waits on them.
    permissions: Mutex<Vec<(Value, Value)>>,
}

impl Held {
    pub(super) fn new(session_key: &str) -> Self {
        Self {
            session_key: session_key.to_owned(),
            armed: AtomicBool::new(false),
            updates: Mutex::new(VecDeque::new()),
            permissions: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn session_key(&self) -> &str {
        &self.session_key
    }

    /// Start holding updates; the session is set up.
    pub(super) fn arm(&self) {
        self.armed.store(true, Ordering::SeqCst);
    }

    /// Keep the `session/update` params that arrived between turns. Only what the agent says or
    /// does starts a held cycle; anything else (usage, a late status) is kept only behind it.
    pub(super) fn hold(&self, params: &Value) {
        if !self.armed.load(Ordering::SeqCst) {
            return;
        }
        let starts_cycle = matches!(
            params["update"]["sessionUpdate"].as_str(),
            Some(
                "agent_message_chunk"
                    | "agent_thought_chunk"
                    | "tool_call"
                    | "tool_call_update"
                    | "plan"
            )
        );
        {
            let mut updates = self.updates.lock().unwrap_or_else(|e| e.into_inner());
            if !starts_cycle && updates.is_empty() {
                return;
            }
            if updates.len() == MAX_HELD {
                updates.pop_front();
            }
            updates.push_back(params.clone());
        }
        if starts_cycle {
            announce(&self.session_key);
        }
    }

    /// Keep a permission request that arrived between turns for the turn that shows them.
    /// Returns false (the caller declines it) while the session is still being set up.
    pub(super) fn hold_permission(&self, id: Value, params: Value) -> bool {
        if !self.armed.load(Ordering::SeqCst) {
            return false;
        }
        self.permissions
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((id, params));
        announce(&self.session_key);
        true
    }

    /// Nothing is waiting for a turn to show it.
    pub(super) fn is_empty(&self) -> bool {
        self.updates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_empty()
            && self
                .permissions
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .is_empty()
    }

    /// Take the held permission requests, oldest first. Taken after [`Self::drain`].
    pub(super) fn drain_permissions(&self) -> Vec<(Value, Value)> {
        std::mem::take(&mut *self.permissions.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Take the held updates for a turn that shows them, which also settles the announcement.
    pub(super) fn drain(&self) -> Vec<Value> {
        let updates = self
            .updates
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
            .collect();
        claim(&self.session_key);
        updates
    }
}

/// Whether `update` closes a turn the agent started itself: Claude Code's adapter reports each
/// cycle's usage tagged with what woke it.
pub(super) fn ends_cycle(update: &Value) -> bool {
    update["sessionUpdate"].as_str() == Some("usage_update")
        && matches!(
            update["_meta"]["_claude/origin"]["kind"].as_str(),
            Some("task-notification" | "peer" | "coordinator" | "observer")
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn update(kind: &str) -> Value {
        json!({ "sessionId": "s", "update": { "sessionUpdate": kind } })
    }

    #[test]
    fn holds_a_cycle_only_once_armed_and_announces_it() {
        let held = Held::new("autonomous-test-key");
        held.hold(&update("agent_message_chunk"));
        assert!(held.drain().is_empty(), "history replay during setup");

        held.arm();
        held.hold(&update("usage_update"));
        assert!(held.drain().is_empty(), "usage alone is not a cycle");

        held.hold(&update("agent_message_chunk"));
        held.hold(&update("usage_update"));
        assert!(pending().contains(&"autonomous-test-key".to_owned()));
        assert_eq!(held.drain().len(), 2);
        assert!(!pending().contains(&"autonomous-test-key".to_owned()));
    }

    #[test]
    fn a_tagged_usage_update_ends_the_cycle() {
        let tagged = json!({
            "sessionUpdate": "usage_update",
            "_meta": { "_claude/origin": { "kind": "task-notification" } }
        });
        assert!(ends_cycle(&tagged));
        assert!(!ends_cycle(&json!({ "sessionUpdate": "usage_update" })));
        assert!(!ends_cycle(
            &json!({ "sessionUpdate": "agent_message_chunk" })
        ));
    }
}
