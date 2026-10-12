//! Integration tests over the library's public API (`oxi::settings`, `oxi::fsutil`).
//!
//! These run as a separate crate from the unit tests inline in `src/`, so they only pass
//! if the public surface stays usable from outside the binary: provider catalog
//! invariants every settings/UI call site relies on, plus crash-safe file writes.

use std::collections::HashSet;

use oxi::settings::{Billing, LlmProviderKind, ProviderConfig};

#[test]
fn provider_catalog_slugs_are_unique_and_nonempty() {
    let mut slugs = HashSet::new();
    for kind in LlmProviderKind::ALL {
        assert!(!kind.slug().is_empty(), "{kind:?} has an empty slug");
        assert!(slugs.insert(kind.slug()), "duplicate slug {}", kind.slug());
        assert!(!kind.label().is_empty(), "{kind:?} has an empty label");
        assert!(
            !kind.default_model_id().is_empty(),
            "{kind:?} has an empty default model"
        );
    }
}

#[test]
fn acp_providers_report_acp_and_their_default_commands() {
    for kind in [
        LlmProviderKind::ClaudeCodeAcp,
        LlmProviderKind::CursorAcp,
        LlmProviderKind::CodexAcp,
        LlmProviderKind::MuseCodeAcp,
    ] {
        assert!(kind.is_acp(), "{kind:?} should be an ACP provider");
        let cfg = ProviderConfig::new(kind);
        assert!(cfg.is_acp());
        assert_eq!(cfg.effective_acp_command(), cfg.default_acp_command());
        assert!(!cfg.effective_acp_command().is_empty());
    }
    // The muse-acp adapter must stay on the scoped BrokkAi package: the unscoped
    // `muse-acp` on npm is an unrelated project.
    assert_eq!(
        ProviderConfig::new(LlmProviderKind::MuseCodeAcp).default_acp_command(),
        "npx -y @brokkai/muse-acp"
    );
    assert!(!LlmProviderKind::MetaMuse.is_acp());
}

#[test]
fn meta_muse_defaults_and_billing() {
    let cfg = ProviderConfig::new(LlmProviderKind::MetaMuse);
    assert_eq!(cfg.effective_base_url(), "https://api.meta.ai/v1");
    assert_eq!(cfg.model_id, "muse-wonder-1.0");
    assert_eq!(
        Billing::default_for(LlmProviderKind::MetaMuse),
        Billing::PayPerUse
    );
    assert_eq!(
        Billing::default_for(LlmProviderKind::MuseCodeAcp),
        Billing::Subscription
    );
}

#[test]
fn muse_code_acp_key_goes_to_meta_api_key_env() {
    let mut cfg = ProviderConfig::new(LlmProviderKind::MuseCodeAcp);
    assert!(
        cfg.acp_env().is_empty(),
        "no key configured, no env expected"
    );
    cfg.api_key = "  test-key  ".to_string();
    assert_eq!(
        cfg.acp_env(),
        vec![("META_API_KEY".to_string(), "test-key".to_string())]
    );
}

/// Removes the temp dir on drop, including when the test panics, so a failing run
/// leaves nothing behind in `temp_dir()`.
struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn write_atomic_roundtrips_and_replaces() {
    let dir = TempDir(std::env::temp_dir().join(format!("oxi-it-{}", std::process::id())));
    let path = dir.0.join("nested").join("state.json");
    oxi::fsutil::write_atomic(&path, b"{\"v\":1}").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"{\"v\":1}");
    oxi::fsutil::write_atomic(&path, b"{\"v\":2}").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"{\"v\":2}");
}
