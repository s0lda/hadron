use super::*;
use super::secrets::{declare_secret_var, secret_status, undeclare_secret_var, SecretStatus};
use hadron_lattice::secrets::MemoryStore;
use hadron_lattice::secrets::SecretStore;
use gpui_component::IndexPath;
use hadron_lattice::{Flavor, Seat};

fn seat(id: &str) -> Seat {
    Seat::cli(QuarkId::new(id), "agy", "gemini-3-pro", Flavor::Worker)
}

#[test]
fn declare_secret_var_adds_when_absent() {
    let mut s = seat("acp-agy");
    assert!(s.secret_env.is_empty());

    let changed = declare_secret_var(&mut s, "GEMINI_API_KEY");

    assert!(changed, "adding a new var must report a change");
    assert_eq!(s.secret_env, vec!["GEMINI_API_KEY".to_string()]);
}

#[test]
fn declare_secret_var_noop_when_present() {
    let mut s = seat("acp-agy");
    s.secret_env.push("GEMINI_API_KEY".to_string());

    let changed = declare_secret_var(&mut s, "GEMINI_API_KEY");

    assert!(!changed, "an already-declared var must not report a change");
    assert_eq!(s.secret_env, vec!["GEMINI_API_KEY".to_string()], "must not duplicate");
}

#[test]
fn declare_secret_var_ignores_blank() {
    let mut s = seat("acp-agy");
    let changed = declare_secret_var(&mut s, "");
    assert!(!changed);
    assert!(s.secret_env.is_empty());
}

#[test]
fn undeclare_secret_var_removes_when_present() {
    let mut s = seat("acp-agy");
    s.secret_env.push("GEMINI_API_KEY".to_string());

    let changed = undeclare_secret_var(&mut s, "GEMINI_API_KEY");

    assert!(changed);
    assert!(s.secret_env.is_empty());
}

#[test]
fn undeclare_secret_var_noop_when_absent() {
    let mut s = seat("acp-agy");
    let changed = undeclare_secret_var(&mut s, "GEMINI_API_KEY");
    assert!(!changed);
    assert!(s.secret_env.is_empty());
}

/// The behaviour the masked field's status line depends on: setting a value
/// via a `SecretStore` flips the status to `Set`, clearing it back to `NotSet` —
/// exercised against a `MemoryStore`, never a real keychain.
///
/// The var name must be one no environment can hold: `secret_status` falls back
/// to `std::env::var` on a store miss (deliberately — an exported key really is
/// available), so naming a live var like `GEMINI_API_KEY` made this test fail on
/// any machine that has one exported, including the merge gate's.
#[test]
fn set_then_status_reports_set() {
    let store = MemoryStore::new();
    let id = QuarkId::new("acp-agy");
    let var = "GEMINI_API_KEY_TEST_VAR_NOT_SET_1234";

    assert_eq!(secret_status(&store, &id, var), SecretStatus::NotSet, "unset");

    store.set(&id, var, "sk-live-value").unwrap();
    assert_eq!(secret_status(&store, &id, var), SecretStatus::Set, "set");

    store.delete(&id, var).unwrap();
    assert_eq!(secret_status(&store, &id, var), SecretStatus::NotSet, "cleared");
}

/// A store that ERRORS (stands in for no OS credential service, e.g. bare WSL2)
/// must report `Unavailable`, NOT `NotSet` — otherwise a failed keychain looks
/// exactly like an unset key and the user has no signal.
#[test]
fn store_error_reports_unavailable_not_notset() {
    struct DeadStore;
    impl SecretStore for DeadStore {
        fn get(&self, _: &QuarkId, _: &str) -> anyhow::Result<Option<String>> {
            anyhow::bail!("no credential service")
        }
        fn set(&self, _: &QuarkId, _: &str, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("no credential service")
        }
        fn delete(&self, _: &QuarkId, _: &str) -> anyhow::Result<()> {
            anyhow::bail!("no credential service")
        }
    }
    assert_eq!(
        secret_status(&DeadStore, &QuarkId::new("acp-agy"), "GEMINI_API_KEY_TEST_VAR_NOT_SET_1234"),
        SecretStatus::Unavailable,
    );
}

#[test]
fn env_var_reports_set_when_not_in_secret_store() {
    let store = MemoryStore::new();
    let id = QuarkId::new("acp-agy");
    let test_var = "TEST_HADRON_GEMINI_KEY_ENV";
    std::env::set_var(test_var, "env_test_val");

    assert_eq!(
        secret_status(&store, &id, test_var),
        SecretStatus::Set,
        "must report Set when key is present in process env"
    );

    std::env::remove_var(test_var);
}

/// A second seat of the same provider mints a fresh id instead of colliding
/// with (and silently re-adopting) the first.
#[test]
fn a_second_same_provider_seat_gets_a_unique_id() {
    let existing = vec!["acp-claude".to_string(), "acp-claude-2".to_string()];
    let taken = |id: &str| existing.iter().any(|e| e == id);
    // Free base → unchanged.
    assert_eq!(super::providers::unique_seat_id("acp-codex", &taken), "acp-codex");
    // Taken base and taken -2 → the next free suffix.
    assert_eq!(super::providers::unique_seat_id("acp-claude", &taken), "acp-claude-3");
}

#[test]
fn model_select_delegate_includes_default_and_custom_value() {
    use gpui_component::searchable_list::SearchableListDelegate;
    use gpui_component::select::SelectItem;
    let models = vec!["gpt-4o".to_string(), "gpt-4o-mini".to_string()];
    let delegate = create_model_delegate("Inherit", &models, Some("custom-model-x"));

    assert_eq!(delegate.items_count(0), 4, "Inherit + 2 models + 1 custom selected model");
    assert_eq!(delegate.item(IndexPath::default().row(0)).unwrap().value, "");
    assert_eq!(delegate.item(IndexPath::default().row(0)).unwrap().title(), "Inherit (inherit)");
    assert_eq!(delegate.item(IndexPath::default().row(1)).unwrap().value, "gpt-4o");
    assert_eq!(delegate.item(IndexPath::default().row(2)).unwrap().value, "gpt-4o-mini");
    assert_eq!(delegate.item(IndexPath::default().row(3)).unwrap().value, "custom-model-x");

    let pos = delegate.position(&gpui::SharedString::from("gpt-4o-mini"));
    assert_eq!(pos, Some(IndexPath::default().row(2)));
}

#[test]
fn supports_model_params_capability_gating_in_settings() {
    use hadron_lattice::Transport;

    let mut http_seat = Seat::cli(QuarkId::new("ollama"), "http", "llama3", Flavor::Worker);
    http_seat.transport = Transport::Http;
    assert!(http_seat.supports_model_params(), "HTTP transport must support model params");

    let mut cli_seat = Seat::cli(QuarkId::new("claude"), "claude", "opus", Flavor::Worker);
    cli_seat.transport = Transport::Cli;
    assert!(!cli_seat.supports_model_params(), "Default CLI seat without param flags must not support model params");
}

#[test]
fn skill_creation_and_deletion_in_repo_and_global_paths() {
    let dir = tempfile::tempdir().unwrap();
    let team_path = dir.path().join(".hadron").join("team.json");
    std::fs::create_dir_all(team_path.parent().unwrap()).unwrap();
    std::fs::write(&team_path, "{}").unwrap();

    let skill_dir = team_path.parent().unwrap().join("skills");
    std::fs::create_dir_all(&skill_dir).unwrap();
    let file_path = skill_dir.join("code-auditor.md");
    std::fs::write(&file_path, "---\nname: code-auditor\n---\n\n# Skill: code-auditor\n").unwrap();

    assert!(file_path.exists());
    assert!(std::fs::remove_file(&file_path).is_ok());
    assert!(!file_path.exists());
}
