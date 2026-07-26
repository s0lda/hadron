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
    /// Kill the auto-spawned `hadron-gluon` daemon when the chamber window closes.
    /// Default `true` — the chamber spawns the daemon for you, so closing the window
    /// is the natural end of the swarm; a daemon left running after its only viewer
    /// is gone keeps burning tokens with nobody reading them. Turn it off in Settings
    /// to keep a headless swarm alive across chamber restarts.
    #[serde(default = "default_true")]
    pub close_gluon_on_exit: bool,
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
    /// Which program opens a source file the human clicks — both a chat message's
    /// `file://` link and the file tree's "Open in editor". Defaults to
    /// [`crate::sys::EditorChoice::System`], which is the pre-existing behaviour
    /// (the desktop's own association, i.e. whatever `xdg-open` picks).
    #[serde(default)]
    pub editor: crate::sys::EditorChoice,
}

fn default_true() -> bool {
    true
}
fn default_false() -> bool {
    false
}
fn default_roster_width() -> f32 {
    // Wide enough for effort + mode tags beside the name/model column, trimmed
    // ~11% from the previous 450 default (Jake's request).
    400.0
}
fn default_inspector_width() -> f32 {
    300.0
}

impl ChamberPrefs {
    /// Move per-quark identity (colour/name/avatar) to a renamed id, so the taxonomy
    /// migration does not reset a quark's appearance. Reads the SAME map as the team.json
    /// rename (`hadron_lattice::legacy_id_renames`) — passed in so the SSOT stays in lattice.
    pub fn rename_quark_ids(&mut self, renames: &[(&str, &str)]) {
        for (old, new) in renames {
            if let Some(identity) = self.quarks.remove(*old) {
                self.quarks.entry(new.to_string()).or_insert(identity);
            }
        }
    }
}

impl Default for ChamberPrefs {
    fn default() -> Self {
        ChamberPrefs {
            roster_collapsed: default_false(),
            inspector_collapsed: default_false(),
            close_gluon_on_exit: default_true(),
            roster_width: default_roster_width(),
            inspector_width: default_inspector_width(),
            window_bounds: None,
            human: Identity::default(),
            quarks: BTreeMap::new(),
            editor: crate::sys::EditorChoice::default(),
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
        Ok(text) => {
            let mut prefs: ChamberPrefs = serde_json::from_str(&text).unwrap_or_default();
            // One-time migration: a chamber.json still pinned at an old default
            // (≤410, 450 or 500) is bumped to the new default so the trimmed roster
            // applies without a manual edit. Any other width was deliberately chosen
            // by the user and is untouched.
            if prefs.roster_width <= 410.0
                || prefs.roster_width == 450.0
                || prefs.roster_width == 500.0
            {
                prefs.roster_width = default_roster_width();
            }
            prefs
        }
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
            close_gluon_on_exit: false,
            roster_width: 180.5,
            inspector_width: 320.0,
            window_bounds: None,
            human: Identity {
                display_name: Some("Jake".into()),
                color: Some("#ec4899".into()),
                image_path: Some("/tmp/me.png".into()),
            },
            quarks,
            editor: crate::sys::EditorChoice::Zed,
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
        // Wait, 175.0 <= 410.0, so this would migrate to 500.0! Let's update this assertion as well.
        assert_eq!(prefs.roster_width, default_roster_width());
        assert_eq!(prefs.inspector_width, 333.0);
        assert_eq!(prefs.human, Identity::default());
        assert!(prefs.quarks.is_empty());
    }

    #[test]
    fn close_gluon_on_exit_defaults_to_true_and_round_trips() {
        let prefs = ChamberPrefs::default();
        assert!(prefs.close_gluon_on_exit, "default must be true");

        let json = serde_json::to_string(&prefs).unwrap();
        assert!(json.contains("\"close_gluon_on_exit\":true"));

        // The opposite value must still survive a round trip — the default is a
        // starting point, not a forced value.
        let custom = ChamberPrefs {
            close_gluon_on_exit: false,
            ..Default::default()
        };
        let json_custom = serde_json::to_string(&custom).unwrap();
        let back: ChamberPrefs = serde_json::from_str(&json_custom).unwrap();
        assert!(!back.close_gluon_on_exit);
    }

    /// A config written before the key existed gets the NEW default, but a config
    /// that says `false` on purpose keeps saying `false`. Changing a default must
    /// never overwrite a deliberate choice (unlike `roster_width`, which migrates
    /// a *stale default* — not a user's own value).
    #[test]
    fn an_explicit_close_gluon_on_exit_false_survives_the_default_flip() {
        let dir = tempdir().unwrap();

        let absent = dir.path().join("absent.json");
        std::fs::write(&absent, r#"{"roster_collapsed":false}"#).unwrap();
        assert!(load_from(&absent).close_gluon_on_exit, "absent key → new default");

        let explicit = dir.path().join("explicit.json");
        std::fs::write(&explicit, r#"{"close_gluon_on_exit":false}"#).unwrap();
        assert!(!load_from(&explicit).close_gluon_on_exit, "an explicit false is kept");
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
        // 240.0 is <= 410.0, so it migrates to 500.0. Let's make sure it's 500.0.
        assert_eq!(load_from(&path).roster_width, default_roster_width());
        assert_eq!(load_from(&path).window_bounds, None);
    }

    #[test]
    fn load_from_missing_returns_default() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nope.json");
        assert_eq!(load_from(&path), ChamberPrefs::default());
    }

    #[test]
    fn a_stored_410_roster_width_migrates_to_the_new_default() {
        let dir = tempdir().unwrap();
        // A chamber.json pinned at any old default (410, 450, 500) is bumped on load...
        for old_default in ["410.0", "450.0", "500.0"] {
            let old = dir.path().join(format!("old-{old_default}.json"));
            std::fs::write(&old, format!(r#"{{"roster_width":{old_default}}}"#)).unwrap();
            assert_eq!(load_from(&old).roster_width, default_roster_width());
        }
        // ...but a width the user chose themselves is preserved exactly.
        let chosen = dir.path().join("chosen.json");
        std::fs::write(&chosen, r#"{"roster_width":460.0}"#).unwrap();
        assert_eq!(load_from(&chosen).roster_width, 460.0);
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

    #[test]
    fn rename_quark_ids_moves_identity_to_the_new_key() {
        let mut prefs = ChamberPrefs::default();
        prefs.quarks.insert("agy".to_string(), Identity::default());
        prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
        assert!(prefs.quarks.contains_key("cli-agy"), "identity moved to the new id");
        assert!(!prefs.quarks.contains_key("agy"), "old key gone");
        // Idempotent: a second run finds nothing to move.
        prefs.rename_quark_ids(hadron_lattice::legacy_id_renames());
        assert!(prefs.quarks.contains_key("cli-agy"));
    }
}
