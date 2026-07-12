//! The team roster config: the seats the human has added. A **seat** is one
//! quark instance = an id bound to a provider (backing CLI) running a model.
//! Stored as `team.json` and read by both the daemon (to instantiate adapters)
//! and the chamber (to make each roster row legible: `id · provider · model`).
//!
//! Pure and offline: this only parses the config. Spawning adapters from it is
//! the daemon's job; annotating the roster is the chamber's.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Flavor, QuarkId};

/// One seat: an identity bound to a provider (CLI/vendor) and a model. Same
/// provider with a different model is a different seat (independent trust).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    pub id: QuarkId,
    /// The backing CLI/vendor, e.g. "claude", "agy".
    pub provider: String,
    /// The model this seat runs, e.g. "opus-4.8", "gemini-3-pro".
    pub model: String,
    pub flavor: Flavor,
}

/// The full team: every seat the human has added.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Team {
    #[serde(default)]
    pub quarks: Vec<Seat>,
}

impl Team {
    /// Look up a seat by quark id.
    pub fn get(&self, id: &QuarkId) -> Option<&Seat> {
        self.quarks.iter().find(|s| &s.id == id)
    }

    /// Whether the team has any seats.
    pub fn is_empty(&self) -> bool {
        self.quarks.is_empty()
    }
}

/// The user's home directory, cross-platform: `$HOME` on Unix, `%USERPROFILE%`
/// on Windows. `None` if neither is set.
fn home_dir() -> Option<PathBuf> {
    for var in ["HOME", "USERPROFILE"] {
        if let Some(v) = std::env::var_os(var) {
            if !v.is_empty() {
                return Some(PathBuf::from(v));
            }
        }
    }
    None
}

/// The user-level Hadron directory: `~/.hadron` (i.e. `<home>/.hadron`), the same
/// dot-folder convention as a project's `.hadron/`. Cross-platform. `None` if the
/// home directory can't be resolved. All global Hadron state (chamber prefs, the
/// default team) lives here.
pub fn user_hadron_dir() -> Option<PathBuf> {
    Some(home_dir()?.join(".hadron"))
}

/// The canonical global `team.json` location: `~/.hadron/team.json`. Both the
/// daemon (to seat quarks when no project team is found) and the chamber (to
/// annotate the roster) resolve the same file here.
pub fn team_config_path() -> Option<PathBuf> {
    Some(user_hadron_dir()?.join("team.json"))
}

/// Resolve which `team.json` describes the team working a given field: the
/// project-level `team.json` sitting next to the field (the `.hadron/` convention)
/// if present, else the global `~/.hadron/team.json`. Both the daemon (to seat)
/// and the chamber (to annotate the roster) must resolve the SAME team, so they
/// share this — otherwise the chamber shows legibility for a team the daemon
/// never seated.
pub fn team_for_field(field_path: &Path) -> Option<PathBuf> {
    if let Some(sibling) = field_path.parent().map(|d| d.join("team.json")) {
        if sibling.exists() {
            return Some(sibling);
        }
    }
    team_config_path()
}

/// Load a team from an explicit path. Missing or malformed → an empty team, so
/// a fresh install (or a viewer with no config) degrades to "no annotations"
/// rather than an error.
pub fn load_team(path: &Path) -> Team {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Team::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn seat(id: &str, provider: &str, model: &str, flavor: Flavor) -> Seat {
        Seat { id: QuarkId::new(id), provider: provider.into(), model: model.into(), flavor }
    }

    #[test]
    fn team_round_trips() {
        let team = Team {
            quarks: vec![
                seat("opus", "claude", "opus-4.8", Flavor::Orchestrator),
                seat("agy", "agy", "gemini-3-pro", Flavor::Worker),
            ],
        };
        let json = serde_json::to_string(&team).unwrap();
        let back: Team = serde_json::from_str(&json).unwrap();
        assert_eq!(team, back);
    }

    #[test]
    fn lookup_finds_a_seat_by_id() {
        let team = Team { quarks: vec![seat("agy", "agy", "gemini-3-pro", Flavor::Worker)] };
        let s = team.get(&QuarkId::new("agy")).unwrap();
        assert_eq!(s.provider, "agy");
        assert_eq!(s.model, "gemini-3-pro");
        assert!(team.get(&QuarkId::new("nope")).is_none());
    }

    #[test]
    fn missing_or_malformed_file_is_an_empty_team() {
        let dir = tempdir().unwrap();
        assert!(load_team(&dir.path().join("nope.json")).is_empty());
        let bad = dir.path().join("team.json");
        std::fs::write(&bad, "{ not json").unwrap();
        assert!(load_team(&bad).is_empty());
    }

    #[test]
    fn team_for_field_prefers_the_sibling_then_global() {
        let dir = tempdir().unwrap();
        let hadron = dir.path().join(".hadron");
        std::fs::create_dir_all(&hadron).unwrap();
        let field = hadron.join("field.jsonl");
        // No sibling team.json yet → falls back to the global path (env-dependent,
        // but never the sibling).
        let sibling = hadron.join("team.json");
        assert_ne!(team_for_field(&field), Some(sibling.clone()));
        // Once a sibling exists, it wins.
        std::fs::write(&sibling, "{}").unwrap();
        assert_eq!(team_for_field(&field), Some(sibling));
    }

    #[test]
    fn global_paths_live_under_user_hadron_dir() {
        // Whatever the home resolves to in this env, ~/.hadron is the root and
        // the global team.json sits directly inside it.
        if let Some(dir) = user_hadron_dir() {
            assert!(dir.ends_with(".hadron"), "user dir is <home>/.hadron");
            assert_eq!(team_config_path(), Some(dir.join("team.json")));
        }
    }

    #[test]
    fn tolerates_unknown_keys_like_the_template_note() {
        // The shipped team.example.json carries a leading "_note" comment key.
        // A silent parse failure would degrade to an empty team (mock quarks), so
        // pin that the extra key is ignored and the quarks still load.
        let with_note = r#"{
            "_note": "provider = backing CLI; agy model is a display name",
            "quarks": [
                { "id": "opus", "provider": "claude", "model": "opus", "flavor": "orchestrator" },
                { "id": "agy",  "provider": "agy",    "model": "Gemini 3.1 Pro (High)", "flavor": "worker" }
            ]
        }"#;
        let team: Team = serde_json::from_str(with_note).unwrap();
        assert_eq!(team.quarks.len(), 2);
        assert_eq!(team.get(&QuarkId::new("agy")).unwrap().model, "Gemini 3.1 Pro (High)");
    }

    #[test]
    fn loads_a_written_team() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        std::fs::write(
            &path,
            r#"{"quarks":[{"id":"opus","provider":"claude","model":"opus-4.8","flavor":"orchestrator"}]}"#,
        )
        .unwrap();
        let team = load_team(&path);
        assert_eq!(team.quarks.len(), 1);
        assert_eq!(team.get(&QuarkId::new("opus")).unwrap().model, "opus-4.8");
    }
}
