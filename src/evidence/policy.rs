//! Live mode selection and workspace-scoped proof freshness.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::EvidenceReport;

/// Shared with the composer so toggling during a response takes effect at its next boundary.
#[derive(Clone, Debug, Default)]
pub struct Mode(Arc<AtomicBool>);

impl From<bool> for Mode {
    fn from(enabled: bool) -> Self {
        Self(Arc::new(AtomicBool::new(enabled)))
    }
}

impl Mode {
    pub fn enabled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
    pub fn set(&self, enabled: bool) {
        self.0.store(enabled, Ordering::SeqCst);
    }
    pub fn toggle(&self) -> bool {
        !self.0.fetch_xor(true, Ordering::SeqCst)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceState {
    pub root: String,
    pub tree: String,
}

impl WorkspaceState {
    pub fn from_snapshot(snapshot: &crate::git::checkpoint::Snapshot) -> Self {
        Self {
            root: snapshot.repo.to_string_lossy().into_owned(),
            tree: snapshot.tree.clone(),
        }
    }

    pub fn capture(cwd: &Path) -> Result<Self, String> {
        if let Ok(snapshot) = crate::git::checkpoint::snapshot(cwd) {
            return Ok(Self::from_snapshot(&snapshot));
        }
        // Non-git workspaces still need change detection, without following symlinks out of them.
        let root = cwd.canonicalize().map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let walker = ignore::WalkBuilder::new(&root)
            .hidden(false)
            .require_git(false)
            .sort_by_file_path(|a, b| a.cmp(b))
            .filter_entry(|entry| {
                entry.depth() == 0
                    || !matches!(
                        entry.file_name().to_str(),
                        Some(".git" | "target" | "node_modules" | ".venv" | "venv")
                    )
            })
            .build();
        for entry in walker {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let relative = path
                .strip_prefix(&root)
                .map_err(|e| e.to_string())?
                .to_string_lossy();
            hash.update((relative.len() as u64).to_le_bytes());
            hash.update(relative.as_bytes());
            let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
            hash.update([u8::from(metadata.permissions().readonly())]);
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                hash.update((metadata.permissions().mode() & 0o111).to_le_bytes());
            }
            if path.is_symlink() {
                hash.update(b"l");
                let target = std::fs::read_link(path).map_err(|e| e.to_string())?;
                let target = target.to_string_lossy();
                hash.update((target.len() as u64).to_le_bytes());
                hash.update(target.as_bytes());
            } else if path.is_file() {
                hash.update(b"f");
                hash.update(metadata.len().to_le_bytes());
                use std::io::Read;
                let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
                let mut bytes = [0; 64 * 1024];
                loop {
                    let n = file.read(&mut bytes).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    hash.update(&bytes[..n]);
                }
            } else {
                hash.update(b"d");
            }
        }
        Ok(Self {
            root: root.to_string_lossy().into_owned(),
            tree: format!(
                "sha256:{}",
                hash.finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            ),
        })
    }
}

#[derive(Clone, Debug, Default)]
pub struct Policy {
    pub mode: Mode,
    pub previous: Arc<Vec<EvidenceReport>>,
    pub pending_changes: bool,
}

pub struct Decision {
    pub report: EvidenceReport,
    pub problems: Vec<String>,
}

impl Policy {
    pub fn from_history(mode: Mode, history: &[crate::model::ChatMessage]) -> Self {
        use crate::model::AssistantBlock;
        let mut policy = Self {
            mode,
            ..Default::default()
        };
        for message in history {
            if message.changes.as_ref().is_some_and(|c| c.before != c.after)
                || message.blocks.iter().any(|b| matches!(b, AssistantBlock::Tool {name, metadata, is_error: Some(false), ..}
                    if matches!(name.as_str(), "write" | "edit" | "delete" | "move" | "mkdir")
                        || metadata.as_ref().is_some_and(|m| matches!(m.kind.as_str(), "edit" | "delete" | "move"))))
            { policy.pending_changes = true; }
            if let Some(report) = &message.evidence {
                if report
                    .workspace
                    .as_ref()
                    .is_some_and(|state| report.valid_for(state))
                {
                    policy.pending_changes = false;
                }
                Arc::make_mut(&mut policy.previous).push((**report).clone());
            }
        }
        policy
    }

    pub fn inspect(&self, dir: &Path, turn: &str) -> Decision {
        let mut report = super::collect(dir, turn, false);
        if !self.mode.enabled() && report.is_empty() {
            return Decision {
                report,
                problems: Vec::new(),
            };
        }
        let marker = super::current_turn(dir);
        let current = marker
            .as_ref()
            .and_then(|m| WorkspaceState::capture(Path::new(&m.cwd)).ok());
        if report.submitted {
            report.stale = current
                .as_ref()
                .is_none_or(|state| !report.valid_for(state));
        }
        if !self.mode.enabled() {
            return Decision {
                report,
                problems: Vec::new(),
            };
        }
        // An earlier report covers this exact code state. Leave its existing card in place.
        let covered = report.is_empty()
            && current
                .as_ref()
                .is_some_and(|state| self.previous.iter().any(|r| r.valid_for(state)));
        let changed = marker
            .as_ref()
            .and_then(|m| m.baseline.as_ref())
            .zip(current.as_ref())
            .is_none_or(|(before, after)| before != after);
        let needed = !covered && (changed || self.pending_changes);
        report.requested = needed;
        let mut problems = if needed {
            report.problems()
        } else {
            Vec::new()
        };
        if needed && let Some(current) = &current {
            if report.submitted && report.workspace.as_ref() != Some(current) {
                problems.push("The workspace changed after the last evidence submission. Verify the new state and submit fresh evidence.".into());
            }
            for item in report
                .items
                .iter()
                .filter(|item| report.claims.iter().any(|c| c.evidence.contains(&item.id)))
            {
                if item.phase != super::Phase::Before
                    && item
                        .workspace
                        .as_ref()
                        .is_some_and(|state| state != current)
                {
                    problems.push(format!("Evidence {} belongs to an older workspace state. Capture or run it again and cite the new id instead.",item.id));
                }
            }
        }
        Decision { report, problems }
    }
}

impl EvidenceReport {
    pub fn complete(&self) -> bool {
        self.submitted
            && !self.stale
            && !self.items.is_empty()
            && !self.claims.is_empty()
            && self.problems().is_empty()
    }

    pub fn valid_for(&self, state: &WorkspaceState) -> bool {
        self.workspace.as_ref() == Some(state)
            && self.complete()
            && self.items.iter().any(|item| {
                item.phase != super::Phase::Before
                    && self.claims.iter().any(|c| c.evidence.contains(&item.id))
            })
            && self
                .items
                .iter()
                .filter(|item| {
                    item.phase != super::Phase::Before
                        && self.claims.iter().any(|c| c.evidence.contains(&item.id))
                })
                .all(|item| item.workspace.as_ref() == Some(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::evidence::{self, TurnMarker};
    use serde_json::json;
    use std::path::PathBuf;

    struct Fixture {
        root: PathBuf,
        cwd: PathBuf,
        dir: PathBuf,
        policy: Policy,
    }
    impl Fixture {
        fn new(git: bool) -> Self {
            let root =
                std::env::temp_dir().join(format!("oxi-evidence-policy-{}", rand::random::<u64>()));
            let cwd = root.join("work");
            let dir = root.join("proof");
            std::fs::create_dir_all(&cwd).unwrap();
            if git {
                git2::Repository::init(&cwd).unwrap();
            }
            std::fs::write(cwd.join("code.txt"), "before").unwrap();
            evidence::begin_turn(
                &dir,
                &TurnMarker {
                    baseline: Some(WorkspaceState::capture(&cwd).unwrap()),
                    turn: "turn".into(),
                    started_ms: 0,
                    cwd: cwd.to_string_lossy().into_owned(),
                    repo: None,
                    base_tree: None,
                },
            )
            .unwrap();
            Self {
                root,
                cwd,
                dir,
                policy: Policy {
                    mode: true.into(),
                    ..Default::default()
                },
            }
        }
        fn inspect(&self) -> Decision {
            self.policy.inspect(&self.dir, "turn")
        }
        fn proof(&self) -> EvidenceReport {
            let context = evidence::tools::Context {
                dir: self.dir.clone(),
                execution: Default::default(),
                policy: None,
            };
            context
                .call(
                    "evidence_run",
                    &json!({"label":"Check","command":"echo verified"}),
                )
                .unwrap();
            context.call("evidence_submit",&json!({"summary":"Verified","claims":[{"text":"Check passes","evidence":["e1"]}]})).unwrap();
            self.inspect().report
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn informational_turns_do_not_request_proof_in_git_or_plain_folders() {
        for git in [false, true] {
            let fixture = Fixture::new(git);
            let decision = fixture.inspect();
            assert!(!decision.report.requested);
            assert!(decision.problems.is_empty());
        }
    }

    #[test]
    fn enabling_or_disabling_during_a_turn_changes_its_final_decision() {
        let fixture = Fixture::new(false);
        let running = fixture.policy.clone();
        fixture.policy.mode.set(false);
        std::fs::write(fixture.cwd.join("code.txt"), "after").unwrap();
        assert!(!running.inspect(&fixture.dir, "turn").report.requested);
        fixture.policy.mode.set(true);
        let decision = running.inspect(&fixture.dir, "turn");
        assert!(decision.report.requested && !decision.problems.is_empty());
        fixture.policy.mode.set(false);
        assert!(!running.inspect(&fixture.dir, "turn").report.requested);
    }

    #[test]
    fn persisted_proof_is_reused_only_for_the_same_workspace_state() {
        for git in [false, true] {
            let mut fixture = Fixture::new(git);
            let report = fixture.proof();
            assert!(report.valid_for(&WorkspaceState::capture(&fixture.cwd).unwrap()));
            let restored: EvidenceReport =
                serde_json::from_slice(&serde_json::to_vec(&report).unwrap()).unwrap();
            fixture.policy.previous = Arc::new(vec![restored]);
            fixture.policy.pending_changes = true;
            let next_dir = fixture.root.join("next");
            let mut marker = evidence::current_turn(&fixture.dir).unwrap();
            marker.baseline = Some(WorkspaceState::capture(&fixture.cwd).unwrap());
            evidence::begin_turn(&next_dir, &marker).unwrap();
            fixture.dir = next_dir;
            assert!(
                !fixture.inspect().report.requested,
                "the existing card already covers this state"
            );
            std::fs::write(fixture.cwd.join("code.txt"), "new change").unwrap();
            assert!(
                fixture.inspect().report.requested,
                "old proof must not cover a new change"
            );
        }
    }

    #[test]
    fn resubmitting_an_old_check_cannot_make_it_fresh_again() {
        let fixture = Fixture::new(false);
        fixture.proof();
        std::fs::write(fixture.cwd.join("code.txt"), "new change").unwrap();
        let context = evidence::tools::Context {
            dir: fixture.dir.clone(),
            execution: Default::default(),
            policy: None,
        };
        context.call("evidence_submit",&json!({"summary":"Relabelled","claims":[{"text":"Check passes","evidence":["e1"]}]})).unwrap();
        let stale = fixture.inspect();
        assert!(stale.report.stale);
        assert_eq!(stale.report.tally(), (0, 1, 0));
        assert!(
            stale
                .problems
                .iter()
                .any(|p| p.contains("older workspace state"))
        );
        context
            .call(
                "evidence_run",
                &json!({"label":"New check","command":"echo verified"}),
            )
            .unwrap();
        context
            .call(
                "evidence_submit",
                &json!({"summary":"Fresh","claims":[{"text":"Check passes","evidence":["e2"]}]}),
            )
            .unwrap();
        let fresh = fixture.inspect();
        assert!(fresh.problems.is_empty());
        assert!(
            fresh
                .report
                .valid_for(&WorkspaceState::capture(&fixture.cwd).unwrap()),
            "unused stale items must not invalidate new proof"
        );
    }

    #[test]
    fn earlier_unverified_agent_edits_are_pending_but_covered_edits_are_not() {
        use crate::model::{AssistantBlock, ChatMessage};
        let fixture = Fixture::new(false);
        let mut message = ChatMessage::streaming_assistant(None);
        message.blocks.push(AssistantBlock::Tool {
            name: "edit".into(),
            tool_call_id: "edit-1".into(),
            args_summary: None,
            output: "Edited".into(),
            diff: None,
            is_error: Some(false),
            full_output_path: None,
            output_truncated: false,
            metadata: None,
        });
        let policy = Policy::from_history(true.into(), &[message.clone()]);
        assert!(policy.inspect(&fixture.dir, "turn").report.requested);
        message.evidence = Some(Box::new(fixture.proof()));
        let policy = Policy::from_history(true.into(), &[message]);
        assert!(!policy.pending_changes);
    }

    #[test]
    fn generated_and_ignored_files_do_not_trigger_plain_folder_verification() {
        let fixture = Fixture::new(false);
        std::fs::write(fixture.cwd.join(".gitignore"), "generated.txt\n").unwrap();
        let before = WorkspaceState::capture(&fixture.cwd).unwrap();
        std::fs::create_dir_all(fixture.cwd.join("target")).unwrap();
        std::fs::write(fixture.cwd.join("target/output"), "build").unwrap();
        std::fs::write(fixture.cwd.join("generated.txt"), "generated").unwrap();
        assert_eq!(before, WorkspaceState::capture(&fixture.cwd).unwrap());
    }
}
