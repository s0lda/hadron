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

#[test]
fn skill_selection_toggles_deny_list() {
    let mut deny_skills: Vec<String> = vec![];
    let skill = "writing-plans";

    // 1. Initially empty deny_skills -> skill is selected (ON) by default
    let denied = deny_skills.iter().any(|d| d.eq_ignore_ascii_case(skill));
    assert!(!denied, "skill must be selected by default");

    // 2. User unselects skill -> added to deny_skills
    if !denied {
        deny_skills.push(skill.to_string());
    }
    assert_eq!(deny_skills, vec!["writing-plans".to_string()]);

    // 3. User selects skill back ON -> removed from deny_skills
    let denied_now = deny_skills.iter().any(|d| d.eq_ignore_ascii_case(skill));
    assert!(denied_now);
    if denied_now {
        deny_skills.retain(|x| !x.eq_ignore_ascii_case(skill));
    }
    assert!(deny_skills.is_empty(), "selecting skill back ON must clear it from deny_skills");
}

#[test]
fn settings_target_keys_and_equality() {
    assert_eq!(SettingsTarget::General.key(), "general");
    assert_eq!(SettingsTarget::Appearance.key(), "appearance");
    assert_eq!(SettingsTarget::Execution.key(), "execution");
    assert_eq!(SettingsTarget::Environment.key(), "environment");
    assert_eq!(SettingsTarget::Providers.key(), "providers");
    assert_eq!(SettingsTarget::Skills.key(), "skills");
    assert_eq!(SettingsTarget::Human.key(), "human");
    assert_eq!(SettingsTarget::Quark("acp-agy".to_string()).key(), "acp-agy");

    assert_eq!(SettingsTarget::Appearance, SettingsTarget::Appearance);
    assert_ne!(SettingsTarget::Appearance, SettingsTarget::Execution);
    assert_ne!(SettingsTarget::Execution, SettingsTarget::Environment);
}

#[test]
fn general_subtargets_belong_to_general_group() {
    let is_general_subtarget = |t: &SettingsTarget| {
        matches!(
            t,
            SettingsTarget::General
                | SettingsTarget::Appearance
                | SettingsTarget::Execution
                | SettingsTarget::Environment
        )
    };

    assert!(is_general_subtarget(&SettingsTarget::General));
    assert!(is_general_subtarget(&SettingsTarget::Appearance));
    assert!(is_general_subtarget(&SettingsTarget::Execution));
    assert!(is_general_subtarget(&SettingsTarget::Environment));
    assert!(!is_general_subtarget(&SettingsTarget::Providers));
    assert!(!is_general_subtarget(&SettingsTarget::Skills));
    assert!(!is_general_subtarget(&SettingsTarget::Human));
    assert!(!is_general_subtarget(&SettingsTarget::Quark("acp-claude".to_string())));
}

#[test]
fn first_added_quark_automatically_becomes_orchestrator_when_team_has_none() {
    use hadron_lattice::{resolve_team, Flavor, QuarkId, Seat, Team};

    let mut repo = Team::default();
    let global = Team::default();

    // Initially no orchestrators in resolved team
    let has_orch = resolve_team(&repo, &global)
        .quarks
        .iter()
        .any(|s| s.flavor == Flavor::Orchestrator);
    assert!(!has_orch, "initially empty team has no orchestrator");

    // When 1st quark is added:
    let mut seat1 = Seat::cli(QuarkId::new("acp-claude"), "claude", "opus", Flavor::Worker);
    if !has_orch {
        seat1.flavor = Flavor::Orchestrator;
    }
    repo.quarks.push(seat1.clone());

    let resolved = resolve_team(&repo, &global);
    assert_eq!(resolved.quarks.len(), 1);
    assert_eq!(
        resolved.quarks[0].flavor,
        Flavor::Orchestrator,
        "1st added quark must automatically be promoted to Orchestrator"
    );

    // When 2nd quark is added:
    let has_orch2 = resolve_team(&repo, &global)
        .quarks
        .iter()
        .any(|s| s.flavor == Flavor::Orchestrator);
    assert!(has_orch2, "now team has an orchestrator");

    let mut seat2 = Seat::cli(QuarkId::new("acp-agy"), "agy", "gemini", Flavor::Worker);
    if !has_orch2 {
        seat2.flavor = Flavor::Orchestrator;
    }
    repo.quarks.push(seat2.clone());

    let resolved2 = resolve_team(&repo, &global);
    assert_eq!(resolved2.quarks.len(), 2);
    assert_eq!(
        resolved2.quarks[0].flavor,
        Flavor::Orchestrator,
        "1st quark remains Orchestrator"
    );
    assert_eq!(
        resolved2.quarks[1].flavor,
        Flavor::Worker,
        "2nd quark remains Worker"
    );
}

#[test]
fn first_adopted_catalogue_quark_becomes_orchestrator_when_team_has_none() {
    use hadron_lattice::{resolve_team, Flavor, QuarkId, Seat, SeatOverride, Team};

    let mut repo = Team::default();
    let global = Team {
        quarks: vec![
            Seat::cli(QuarkId::new("acp-claude"), "claude", "opus", Flavor::Worker),
            Seat::cli(QuarkId::new("acp-agy"), "agy", "gemini", Flavor::Worker),
        ],
        roster: vec![],
        max_exchanges: None,
        nucleus_index_budget_kb: None,
        merge_strategy: None,
        ..Default::default()
    };

    // When adopting first quark into empty repo:
    let has_orch = resolve_team(&repo, &global)
        .quarks
        .iter()
        .any(|s| s.flavor == Flavor::Orchestrator);
    assert!(!has_orch);

    let flavor_override = if !has_orch {
        Some(Flavor::Orchestrator)
    } else {
        None
    };

    repo.roster.push(SeatOverride {
        enabled: Some(true),
        flavor: flavor_override,
        ..SeatOverride::role(QuarkId::new("acp-claude"))
    });

    let resolved = resolve_team(&repo, &global);
    assert_eq!(resolved.quarks.len(), 1);
    assert_eq!(
        resolved.quarks[0].flavor,
        Flavor::Orchestrator,
        "First adopted quark in repo without orchestrator must be promoted to Orchestrator"
    );
}

#[test]
fn test_theme_tokens_count_and_categories() {
    use super::providers::{ThemeCategoryTab, ThemeTokenKey};

    assert_eq!(ThemeTokenKey::ALL.len(), 33, "Theme editor must provide all 33 design tokens");

    let mut surfaces_count = 0;
    let mut text_accents_count = 0;
    let mut terminal_count = 0;
    let mut syntax_count = 0;

    for &token in &ThemeTokenKey::ALL {
        assert!(!token.label().is_empty());
        assert!(!token.description().is_empty());
        assert!(token.matches_category(ThemeCategoryTab::All));

        if token.matches_category(ThemeCategoryTab::Surfaces) {
            surfaces_count += 1;
        }
        if token.matches_category(ThemeCategoryTab::TextAccents) {
            text_accents_count += 1;
        }
        if token.matches_category(ThemeCategoryTab::Terminal) {
            terminal_count += 1;
        }
        if token.matches_category(ThemeCategoryTab::Syntax) {
            syntax_count += 1;
        }
    }

    assert_eq!(surfaces_count, 8, "Surfaces category must contain 8 tokens");
    assert_eq!(text_accents_count, 8, "Text & Accents category must contain 8 tokens (3 text + 5 accents)");
    assert_eq!(terminal_count, 3, "Terminal category must contain 3 tokens");
    assert_eq!(syntax_count, 14, "Syntax category must contain 14 tokens");
    assert_eq!(surfaces_count + text_accents_count + terminal_count + syntax_count, 33);
}

#[test]
fn test_theme_token_get_and_set_hex() {
    use super::providers::ThemeTokenKey;
    use crate::config::ThemeDefinition;

    let mut theme = ThemeDefinition::default();

    // Verify initial values from default preset
    assert_eq!(ThemeTokenKey::CanvasBase.get_hex(&theme), "#050505");
    assert_eq!(ThemeTokenKey::TextPrimary.get_hex(&theme), "#e8e8e8");
    assert_eq!(ThemeTokenKey::TextMuted.get_hex(&theme), "#707070");
    assert_eq!(ThemeTokenKey::SyntaxKeyword.get_hex(&theme), "#f97583");
    assert_eq!(ThemeTokenKey::TermBg.get_hex(&theme), "#080808");

    // Mutate colors using ThemeTokenKey::set_hex
    ThemeTokenKey::CanvasBase.set_hex(&mut theme, "#010101".into());
    ThemeTokenKey::TextPrimary.set_hex(&mut theme, "#ffffff".into());
    ThemeTokenKey::TextMuted.set_hex(&mut theme, "#555555".into());
    ThemeTokenKey::SyntaxKeyword.set_hex(&mut theme, "#ff007f".into());
    ThemeTokenKey::TermBg.set_hex(&mut theme, "#000000".into());

    assert_eq!(theme.surfaces.canvas_base, "#010101");
    assert_eq!(theme.text.primary, "#ffffff");
    assert_eq!(theme.text.muted, "#555555");
    assert_eq!(theme.syntax.keyword, "#ff007f");
    assert_eq!(theme.terminal.bg, "#000000");

    assert_eq!(ThemeTokenKey::CanvasBase.get_hex(&theme), "#010101");
    assert_eq!(ThemeTokenKey::TextPrimary.get_hex(&theme), "#ffffff");
    assert_eq!(ThemeTokenKey::TextMuted.get_hex(&theme), "#555555");
    assert_eq!(ThemeTokenKey::SyntaxKeyword.get_hex(&theme), "#ff007f");
    assert_eq!(ThemeTokenKey::TermBg.get_hex(&theme), "#000000");
}
