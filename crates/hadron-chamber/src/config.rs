//! Persistent chamber layout preferences — rail collapse state and panel widths.
//! Stored at `~/.hadron/chamber.json` (cross-platform), loaded on start and saved
//! whenever the layout changes, so the user's workspace is preserved across
//! sessions. Pure (no GPUI) so it is unit-tested.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A customizable identity for chat/roster display — the human's, or a quark's.
/// All fields are optional overrides; unset fields fall back to code defaults
/// (id-derived name, hue-derived color, initials avatar) at render time.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    /// Shown name; falls back to the actor id (or "You" for the human).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// Accent + avatar color as `#rrggbb`; falls back to the actor's hue.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Path to an avatar image; when set, it wins over color + initials.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowBoundsPrefs {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The persisted chamber state: rail layout, plus per-actor display identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamberPrefs {
    #[serde(default = "default_false")]
    pub roster_collapsed: bool,
    #[serde(default = "default_false")]
    pub inspector_collapsed: bool,
    #[serde(default = "default_roster_width")]
    pub roster_width: f32,
    #[serde(default = "default_inspector_width")]
    pub inspector_width: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_bounds: Option<WindowBoundsPrefs>,
    /// The human's own chat identity.
    #[serde(default)]
    pub human: Identity,
    /// Per-quark identity overrides, keyed by quark id.
    #[serde(default)]
    pub quarks: BTreeMap<String, Identity>,
}

fn default_false() -> bool {
    false
}
fn default_roster_width() -> f32 {
    245.0
}
fn default_inspector_width() -> f32 {
    300.0
}

impl Default for ChamberPrefs {
    fn default() -> Self {
        ChamberPrefs {
            roster_collapsed: default_false(),
            inspector_collapsed: default_false(),
            roster_width: default_roster_width(),
            inspector_width: default_inspector_width(),
            window_bounds: None,
            human: Identity::default(),
            quarks: BTreeMap::new(),
        }
    }
}

/// Resolve the on-disk preferences path: `~/.hadron/chamber.json` (cross-platform).
/// `None` if the home directory can't be resolved.
pub fn config_path() -> Option<PathBuf> {
    Some(hadron_lattice::user_hadron_dir()?.join("chamber.json"))
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
        let mut quarks = BTreeMap::new();
        quarks.insert(
            "claude".to_string(),
            Identity {
                display_name: Some("Claude".into()),
                color: Some("#a855f7".into()),
                image_path: None,
            },
        );
        let prefs = ChamberPrefs {
            roster_collapsed: true,
            inspector_collapsed: false,
            roster_width: 180.5,
            inspector_width: 320.0,
            window_bounds: None,
            human: Identity {
                display_name: Some("Jake".into()),
                color: Some("#ec4899".into()),
                image_path: Some("/tmp/me.png".into()),
            },
            quarks,
        };
        let json = serde_json::to_string(&prefs).unwrap();
        let back: ChamberPrefs = serde_json::from_str(&json).unwrap();
        assert_eq!(prefs, back);
    }

    #[test]
    fn old_config_without_identities_keeps_layout() {
        // A config written by a pre-identity binary has only the four layout
        // keys. Loading it must preserve them (not reset via unwrap_or_default)
        // and yield empty identities.
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(
            &path,
            r#"{"roster_collapsed":true,"inspector_collapsed":true,"roster_width":175.0,"inspector_width":333.0}"#,
        )
        .unwrap();
        let prefs = load_from(&path);
        assert!(prefs.roster_collapsed);
        assert!(prefs.inspector_collapsed);
        assert_eq!(prefs.roster_width, 175.0);
        assert_eq!(prefs.inspector_width, 333.0);
        assert_eq!(prefs.human, Identity::default());
        assert!(prefs.quarks.is_empty());
    }

    #[test]
    fn window_bounds_round_trip_on_disk() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        let prefs = ChamberPrefs {
            window_bounds: Some(WindowBoundsPrefs {
                x: 120.0,
                y: 64.0,
                width: 1600.0,
                height: 1000.0,
            }),
            ..Default::default()
        };
        save_to(&path, &prefs).unwrap();
        assert_eq!(load_from(&path), prefs);
    }

    #[test]
    fn config_without_window_bounds_loads_as_unset() {
        // A config written by a pre-persistence binary must still parse — it just
        // has no remembered geometry, so the window centers on its default size.
        let dir = tempdir().unwrap();
        let path = dir.path().join("chamber.json");
        std::fs::write(&path, r#"{"roster_collapsed":false,"roster_width":240.0}"#).unwrap();
        assert_eq!(load_from(&path).window_bounds, None);
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
        let prefs = ChamberPrefs {
            roster_collapsed: true,
            ..Default::default()
        };
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
