use std::path::{Path, PathBuf};

use super::Team;

/// The user's home directory, cross-platform: `$HOME` on Unix, `%USERPROFILE%`
/// on Windows. `None` if neither is set.
pub fn home_dir() -> Option<PathBuf> {
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

/// Returns `true` if `path` points to a user-level global `~/.hadron/team.json`
/// (either the local host's `team_config_path()` or a Linux/WSL user home
/// `/home/<user>/.hadron/team.json` / `/root/.hadron/team.json`, including Windows
/// WSL UNC paths like `\\wsl.localhost\<distro>\home\<user>\.hadron\team.json`).
///
/// This prevents directory walk-ups from mistaking a user's home `~/.hadron/team.json`
/// for a repository's `.hadron/team.json` when opening a repository inside `~` that
/// has no repo-level `team.json` yet.
pub fn is_global_team_config_path(path: &Path) -> bool {
    if let Some(global) = team_config_path() {
        if path == global {
            return true;
        }
    }
    if let Some(wsl_cat) = crate::wsl::wsl_catalogue_path_for_field(path) {
        if path == wsl_cat {
            return true;
        }
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|p| !p.is_empty()).collect();
    // Match `/home/<user>/.hadron/team.json` or `//wsl.localhost/<distro>/home/<user>/.hadron/team.json`
    if parts.len() == 4
        && parts[0] == "home"
        && parts[2] == ".hadron"
        && parts[3] == "team.json"
    {
        return true;
    }
    // Match `/root/.hadron/team.json`
    if parts.len() == 3
        && parts[0] == "root"
        && parts[1] == ".hadron"
        && parts[2] == "team.json"
    {
        return true;
    }
    if let Some((_distro, linux_path)) = crate::wsl::parse_wsl_unc_path(path) {
        let lp = linux_path.to_string_lossy();
        let lparts: Vec<&str> = lp.split('/').filter(|p| !p.is_empty()).collect();
        if (lparts.len() == 4
            && lparts[0] == "home"
            && lparts[2] == ".hadron"
            && lparts[3] == "team.json")
            || (lparts.len() == 3
                && lparts[0] == "root"
                && lparts[1] == ".hadron"
                && lparts[2] == "team.json")
        {
            return true;
        }
    }
    false
}

/// Resolve the **repository-scoped** `.hadron/team.json` for a given `field_path`,
/// returning `None` if the repository has no `.hadron/team.json` of its own.
///
/// Never returns the user-level global `~/.hadron/team.json` (nor a WSL home
/// `/home/<user>/.hadron/team.json`), and stops walking up once it hits a `.git`
/// boundary.
pub fn repo_team_for_field(field_path: &Path) -> Option<PathBuf> {
    if let Some(sibling) = field_path.parent().map(|d| d.join("team.json")) {
        if sibling.exists() && !is_global_team_config_path(&sibling) {
            return Some(sibling);
        }
    }
    let mut current = field_path.parent();
    while let Some(dir) = current {
        let repo_config = dir.join(".hadron").join("team.json");
        if repo_config.exists() && !is_global_team_config_path(&repo_config) {
            return Some(repo_config);
        }
        if dir.join(".git").exists() {
            break;
        }
        current = dir.parent();
    }
    None
}

/// Resolve which `team.json` describes the team working a given field: the
/// project-level `team.json` sitting next to the field (the `.hadron/` convention)
/// if present, else the global `~/.hadron/team.json`. Both the daemon (to seat)
/// and the chamber (to annotate the roster) must resolve the SAME team, so they
/// share this — otherwise the chamber shows legibility for a team the daemon
/// never seated.
pub fn team_for_field(field_path: &Path) -> Option<PathBuf> {
    repo_team_for_field(field_path).or_else(team_config_path)
}

/// Resolve the `team.json` path for a given repository root. Checks `<repo_root>/.hadron/team.json`,
/// then falls back to `team_config_path()` (`~/.hadron/team.json`).
pub fn team_for_repo(repo_root: &Path) -> Option<PathBuf> {
    let repo_team = repo_root.join(".hadron").join("team.json");
    if repo_team.exists() {
        return Some(repo_team);
    }
    team_config_path()
}

/// Load the configured team for a repository root, falling back to global or default team.
pub fn load_team_for_repo(repo_root: &Path) -> Team {
    team_for_repo(repo_root)
        .map(|p| load_team(&p))
        .unwrap_or_default()
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

/// When operating on a WSL UNC workspace from Windows (`\\wsl.localhost\<distro>\home\<user>\...`),
/// merge any seat definitions from the WSL user's `~/.hadron/team.json` into `catalogue`
/// (preserving existing host catalogue seats on ID collision) so repo roster overrides
/// referencing WSL-configured seats do not get dropped as orphans.
pub fn merge_wsl_catalogue(catalogue: &mut Team, workspace_or_field_path: &Path) {
    if let Some(wsl_cat_path) = crate::wsl::wsl_catalogue_path_for_field(workspace_or_field_path) {
        if wsl_cat_path.exists() {
            let wsl_cat = load_team(&wsl_cat_path);
            for seat in wsl_cat.quarks {
                if catalogue.get(&seat.id).is_none() {
                    catalogue.quarks.push(seat);
                }
            }
        }
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
