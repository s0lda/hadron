//! Persistent chamber layout preferences — rail collapse state and panel widths.
//! Stored at `$XDG_CONFIG_HOME/hadron/chamber.json` (fallback `~/.config/...`),
//! loaded on start and saved whenever the layout changes, so the user's
//! workspace is preserved across sessions. Pure (no GPUI) so it is unit-tested.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The persisted layout state of the chamber's collapsible rails.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamberPrefs {
    pub roster_collapsed: bool,
    pub inspector_collapsed: bool,
    pub roster_width: f32,
    pub inspector_width: f32,
}

impl Default for ChamberPrefs {
    fn default() -> Self {
        ChamberPrefs {
            roster_collapsed: false,
            inspector_collapsed: false,
            roster_width: 240.0,
            inspector_width: 300.0,
        }
    }
}

/// Resolve the on-disk preferences path: `$XDG_CONFIG_HOME/hadron/chamber.json`,
/// falling back to `$HOME/.config/hadron/chamber.json`. `None` if neither is set.
pub fn config_path() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME") {
        if !dir.is_empty() {
            return Some(PathBuf::from(dir).join("hadron").join("chamber.json"));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config").join("hadron").join("chamber.json"))
}

/// Read preferences from an explicit path; missing or malformed → defaults.
pub fn load_from(path: &Path) -> ChamberPrefs {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => ChamberPrefs::default(),
    }
}

/// Write preferences to an explicit path, creating parent dirs as needed.
pub fn save_to(path: &Path, prefs: &ChamberPrefs) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(prefs).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
}

/// Load from the resolved config path (defaults if unresolved/missing).
pub fn load() -> ChamberPrefs {
    config_path().map(|p| load_from(&p)).unwrap_or_default()
}

/// Save to the resolved config path (no-op if the path can't be resolved).
pub fn save(prefs: &ChamberPrefs) -> std::io::Result<()> {
    match config_path() {
        Some(p) => save_to(&p, prefs),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prefs_round_trip() {
        let prefs = ChamberPrefs {
            roster_collapsed: true,
            inspector_collapsed: false,
            roster_width: 180.5,
            inspector_width: 320.0,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: ChamberPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(prefs, back);
    }

    #[test]
    fn load_from_missing_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert_eq!(load_from(&path), ChamberPrefs::default());
    }

    #[test]
    fn save_then_load_from_round_trips_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sub").join("chamber.json"); // parent created by save
        let prefs = ChamberPrefs { roster_collapsed: true, ..Default::default() };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path), prefs);
    }

    #[test]
    fn malformed_file_falls_back_to_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(&path, "{ not json").unwrap();
        assert_eq!(load_from(&path), ChamberPrefs::default());
    }
}
