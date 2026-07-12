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

/// The canonical `team.json` location: `$XDG_CONFIG_HOME/hadron/team.json`,
/// falling back to `$HOME/.config/hadron/team.json`. `None` if neither is set.
/// Both the daemon (to seat quarks) and the chamber (to annotate the roster)
/// resolve the same file here.
pub fn team_config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("hadron").join("team.json"));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("hadron").join("team.json"))
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
