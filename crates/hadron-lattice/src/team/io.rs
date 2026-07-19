use std::path::{Path, PathBuf};

use super::Team;

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
    // Fall back to walking up the directory tree looking for a project `.hadron/team.json`.
    let mut current = field_path.parent();
    while let Some(dir) = current {
        let repo_config = dir.join(".hadron").join("team.json");
        if repo_config.exists() {
            return Some(repo_config);
        }
        current = dir.parent();
    }
    team_config_path()
}

/// Parse a team from JSON text, **keeping the error**.
///
/// The one parser. The daemon re-reads `team.json` while the swarm is live and must
/// tell "the human seated nobody" apart from "I could not parse this" — a distinction
/// [`load_team`] cannot make, since it maps both to an empty team. If a malformed read
/// answered "empty team", a `team.json` caught mid-write would silently unseat the
/// entire swarm.
///
/// It takes text rather than a path on purpose: the daemon detects change by comparing
/// the file's bytes, and must parse *those* bytes. Re-reading the path to parse it
/// would be a second read of a file that may have changed in between.
pub fn parse_team(text: &str) -> std::io::Result<Team> {
    let mut team: Team = serde_json::from_str(text).map_err(std::io::Error::other)?;
    for seat in &mut team.quarks {
        seat.normalize_vendor();
    }
    Ok(team)
}

/// Load a team from an explicit path. Missing or malformed → an empty team, so
/// a fresh install (or a viewer with no config) degrades to "no annotations"
/// rather than an error.
///
/// The lossy wrapper over [`parse_team`]: one parser, two error policies.
pub fn load_team(path: &Path) -> Team {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_team(&text).unwrap_or_default(),
        Err(_) => Team::default(),
    }
}

/// Save a team to an explicit path, creating the directory if it does not exist.
/// The chamber calls this when the human seats a provider in Settings.
///
/// The write is **atomic**: the JSON goes to a temp file in the *same directory*
/// (rename is only atomic within one filesystem) and is then renamed over the
/// target. A plain `fs::write` truncates first, so a reader — and the daemon now
/// polls this file to re-seat the live swarm — can catch it empty or half-written.
/// [`try_load_team`] would reject that torn read anyway; this stops it happening at
/// all. Both layers stay: the parse guard is what makes a *crashed* save safe, and
/// the rename is what makes a *concurrent* save safe.
pub fn save_team(path: &Path, team: &Team) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(team).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}
