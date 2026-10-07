//! The vendored `agy` ACP bridge: a Python script embedded in the binary via
//! `include_str!` so it ships with `cargo install` regardless of whether the
//! source repository is present at runtime, materialized to `~/.hadron/bridges/agy`
//! plus a dedicated venv holding `google-antigravity`.
//!
//! See `.hadron/nucleus/notes/anchoring-a-boot-command-does-not-ship-it.md`: the
//! script used to live only in the repository and the venv only in git-ignored
//! `scripts/venv`, so neither reached an installed build. This module is what makes
//! [`USER_HOME_TOKEN`](super::registry::USER_HOME_TOKEN)-anchored boot commands
//! actually resolvable.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Current spec version for the Antigravity Python bridge dependencies and venv configuration.
/// Bump this whenever minimum package versions or required dependencies change.
pub const BRIDGE_SPEC_VERSION: u32 = 2;

/// Package specifications required inside the bridge's venv.
pub const REQUIRED_PACKAGES: &[&str] = &[
    "google-antigravity>=0.1.21",
    "google-genai>=2.28.0",
];

/// The bridge script, baked into the binary at compile time. `include_str!`
/// resolves relative to *this* file — kept as a flat `bridge.rs` (not
/// `bridge/mod.rs`) so this path doesn't need a third `../` (the landmine
/// `notes/a-fix-that-guards-the-preset-does-not-guard-the-seat.md`-adjacent lesson
/// about `include_str!`'s including-file-relative resolution already covers).
const AGY_ACP_PY: &str = include_str!("../../scripts/agy_acp.py");

/// Timeout for each provisioning subprocess (`python3 -m venv`, `pip install`).
/// Generous: a cold `pip install` against a slow mirror can legitimately take
/// minutes, and this must never run on the seating/dispatch path (task 1c) where
/// a shorter bound would matter — here it only bounds an explicit, off-UI-thread
/// provisioning action.
const PROVISION_DEADLINE: Duration = Duration::from_secs(300);

/// Manifest tracking the installed state of the bridge virtual environment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BridgeManifest {
    pub spec_version: u32,
    pub installed_packages: Vec<String>,
    pub last_updated_secs: u64,
    pub script_hash: String,
}

/// Operational status and freshness of the bridge environment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BridgeHealth {
    Unprovisioned,
    Outdated { installed_spec: u32, target_spec: u32 },
    UpToDate { spec_version: u32 },
}

/// Compute a SHA256 hex digest of the embedded bridge script.
pub fn compute_script_hash() -> String {
    let mut hasher = Sha256::new();
    hasher.update(AGY_ACP_PY.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// `~/.hadron/bridges/agy` — where the script and its venv live.
pub fn bridge_dir() -> anyhow::Result<PathBuf> {
    hadron_lattice::user_hadron_dir()
        .map(|home| home.join("bridges").join("agy"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "could not resolve the user's home directory — neither $HOME nor \
                 %USERPROFILE% is set"
            )
        })
}

/// Path to `~/.hadron/bridges/agy/manifest.json`.
pub fn manifest_path() -> anyhow::Result<PathBuf> {
    Ok(bridge_dir()?.join("manifest.json"))
}

/// Read the existing manifest from disk if present and parseable.
pub fn read_manifest() -> Option<BridgeManifest> {
    let path = manifest_path().ok()?;
    let content = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Persist the bridge manifest to `~/.hadron/bridges/agy/manifest.json`.
pub fn write_manifest(manifest: &BridgeManifest) -> anyhow::Result<()> {
    let path = manifest_path()?;
    let content = serde_json::to_string_pretty(manifest)
        .context("serializing bridge manifest")?;
    std::fs::write(&path, content)
        .with_context(|| format!("writing manifest to {}", path.display()))?;
    Ok(())
}

/// Pure evaluation of bridge health given provisioning state and recorded manifest spec version.
pub fn evaluate_health(is_provisioned: bool, manifest_spec: Option<u32>) -> BridgeHealth {
    if !is_provisioned {
        return BridgeHealth::Unprovisioned;
    }
    match manifest_spec {
        Some(v) if v >= BRIDGE_SPEC_VERSION => BridgeHealth::UpToDate { spec_version: v },
        Some(v) => BridgeHealth::Outdated {
            installed_spec: v,
            target_spec: BRIDGE_SPEC_VERSION,
        },
        None => BridgeHealth::Outdated {
            installed_spec: 0,
            target_spec: BRIDGE_SPEC_VERSION,
        },
    }
}

/// Check whether the bridge environment is provisioned and running the current spec version.
pub fn check_bridge_health() -> BridgeHealth {
    evaluate_health(is_provisioned(), read_manifest().map(|m| m.spec_version))
}

/// Quick check whether the bridge is provisioned and matches the target spec version.
pub fn is_bridge_up_to_date() -> bool {
    matches!(check_bridge_health(), BridgeHealth::UpToDate { .. })
}

/// Write [`AGY_ACP_PY`] to `{hadron}/bridges/agy/agy_acp.py` if it is missing or its
/// contents differ from what's embedded — comparing bytes, not mtime, so an
/// upgrade always refreshes a stale copy instead of trusting a timestamp a package
/// manager may not have touched. Cheap enough to call on every seat build.
pub fn materialize_script() -> anyhow::Result<PathBuf> {
    let dir = bridge_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating bridge directory {}", dir.display()))?;
    let path = dir.join("agy_acp.py");
    let stale = std::fs::read_to_string(&path).map(|existing| existing != AGY_ACP_PY).unwrap_or(true);
    if stale {
        std::fs::write(&path, AGY_ACP_PY)
            .with_context(|| format!("writing bridge script to {}", path.display()))?;
    }

    // Materialize requirements.txt alongside agy_acp.py for clear documentation and manual tooling.
    let reqs_path = dir.join("requirements.txt");
    let reqs_content = REQUIRED_PACKAGES.join("\n") + "\n";
    let reqs_stale = std::fs::read_to_string(&reqs_path).map(|existing| existing != reqs_content).unwrap_or(true);
    if reqs_stale {
        let _ = std::fs::write(&reqs_path, reqs_content);
    }

    Ok(path)
}

/// The venv's python interpreter path, whether or not it exists yet.
pub fn venv_python() -> anyhow::Result<PathBuf> {
    let dir = bridge_dir()?.join("venv");
    if cfg!(windows) {
        Ok(dir.join("Scripts").join("python.exe"))
    } else {
        Ok(dir.join("bin").join("python"))
    }
}

/// Whether the venv is already provisioned — a pure existence check, cheap enough
/// for the seating path (unlike [`provision_venv`], which is not).
pub fn is_provisioned() -> bool {
    venv_python().map(|p| p.exists()).unwrap_or(false)
}

/// Upgrade installed packages in the bridge venv to satisfy [`REQUIRED_PACKAGES`]
/// and record the fresh [`BridgeManifest`]. Must be executed off the UI thread.
pub fn upgrade_venv() -> anyhow::Result<BridgeManifest> {
    let python = venv_python()?;
    if !python.exists() {
        provision_venv()?;
    }
    let mut install = Command::new(&python);
    install.args(["-m", "pip", "install", "--quiet", "--upgrade"]);
    for pkg in REQUIRED_PACKAGES {
        install.arg(pkg);
    }
    run_bounded(install, "pip install --upgrade packages")?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let manifest = BridgeManifest {
        spec_version: BRIDGE_SPEC_VERSION,
        installed_packages: REQUIRED_PACKAGES.iter().map(|s| s.to_string()).collect(),
        last_updated_secs: now,
        script_hash: compute_script_hash(),
    };
    write_manifest(&manifest)?;
    Ok(manifest)
}

/// Create the venv and install required packages into it. Blocking and
/// potentially slow — callers MUST run this off the seating/dispatch path and off
/// the UI thread (see task 1c in
/// `.hadron/docs/plans/2026-07-28-shippable-bridge-and-self-update.md`).
/// A no-op returning the existing interpreter if already provisioned.
pub fn provision_venv() -> anyhow::Result<PathBuf> {
    let python = venv_python()?;
    if python.exists() {
        if !is_bridge_up_to_date() {
            let _ = upgrade_venv();
        }
        return Ok(python);
    }
    let dir = bridge_dir()?;
    let venv_dir = dir.join("venv");
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating bridge directory {}", dir.display()))?;

    let python_cmd = if cfg!(windows) {
        if is_valid_python("py") {
            "py"
        } else if is_valid_python("python") {
            "python"
        } else if is_valid_python("python3") {
            "python3"
        } else {
            anyhow::bail!(
                "Python 3 is required for the Antigravity bridge but was not found on PATH \
                 (tried 'py', 'python', 'python3'). Please install Python 3."
            );
        }
    } else {
        if is_valid_python("python3") {
            "python3"
        } else if is_valid_python("python") {
            "python"
        } else {
            anyhow::bail!(
                "Python 3 is required for the Antigravity bridge but was not found on PATH \
                 (tried 'python3', 'python'). Please install Python 3."
            );
        }
    };
    let mut make_venv = Command::new(python_cmd);
    make_venv.args(["-m", "venv", "--clear"]).arg(&venv_dir);
    run_bounded(make_venv, &format!("{python_cmd} -m venv"))?;

    let mut install = Command::new(&python);
    install.args(["-m", "pip", "install", "--quiet"]);
    for pkg in REQUIRED_PACKAGES {
        install.arg(pkg);
    }
    run_bounded(install, "pip install packages")?;

    if !python.exists() {
        anyhow::bail!(
            "venv provisioning finished but {} still does not exist",
            python.display()
        );
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let manifest = BridgeManifest {
        spec_version: BRIDGE_SPEC_VERSION,
        installed_packages: REQUIRED_PACKAGES.iter().map(|s| s.to_string()).collect(),
        last_updated_secs: now,
        script_hash: compute_script_hash(),
    };
    let _ = write_manifest(&manifest);

    Ok(python)
}

/// Ensure the bridge script is materialized, venv is provisioned, and packages are up to date.
/// Must only be invoked from background tasks off the UI thread.
pub fn ensure_bridge_ready() -> anyhow::Result<PathBuf> {
    let _ = materialize_script()?;
    let python = venv_python()?;
    if !python.exists() {
        provision_venv()?;
    } else if !is_bridge_up_to_date() {
        upgrade_venv()?;
    }
    Ok(python)
}

/// Run `cmd` bounded by [`PROVISION_DEADLINE`], process-group-killed on expiry —
/// the same shape `snapshot::git_with_env` uses for `git`, reused here via
/// `hadron_forge::exec::run_bounded` rather than a second bounded-subprocess
/// helper (rule 2/3: reuse, one home for the pattern).
fn run_bounded(cmd: Command, label: &str) -> anyhow::Result<()> {
    let out = hadron_forge::exec::run_bounded(cmd, PROVISION_DEADLINE, label)
        .with_context(|| format!("failed to spawn {label} — is Python installed and on PATH?"))?;
    if out.timed_out {
        anyhow::bail!("{label} timed out after {PROVISION_DEADLINE:?} and was killed");
    }
    if !out.success() {
        let stderr = out.stderr_lossy().trim().to_string();
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("process exited with code {:?}", out.code)
        };
        anyhow::bail!("{label} failed: {detail}");
    }
    Ok(())
}

/// Verify that `cmd` is an actual Python executable and not a Microsoft Store stub / alias.
fn is_valid_python(cmd: &str) -> bool {
    let Ok(out) = Command::new(cmd).arg("--version").output() else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    if combined.contains("Microsoft Store") || combined.contains("App execution aliases") {
        return false;
    }
    combined.contains("Python")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_script_is_not_empty_and_looks_like_the_bridge() {
        assert!(AGY_ACP_PY.contains("google.antigravity"));
    }

    #[test]
    fn test_script_hash_is_stable_and_sha256() {
        let hash = compute_script_hash();
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, compute_script_hash());
    }

    #[test]
    fn test_required_packages_contain_minimum_versions() {
        assert!(REQUIRED_PACKAGES.iter().any(|p| p.starts_with("google-antigravity>=")));
        assert!(REQUIRED_PACKAGES.iter().any(|p| p.starts_with("google-genai>=")));
    }

    #[test]
    fn test_bridge_manifest_serialization_roundtrip() {
        let manifest = BridgeManifest {
            spec_version: 2,
            installed_packages: vec!["google-antigravity>=0.1.21".into(), "google-genai>=2.28.0".into()],
            last_updated_secs: 1728320000,
            script_hash: "abcd1234efgh5678".into(),
        };
        let serialized = serde_json::to_string(&manifest).expect("serialize");
        let deserialized: BridgeManifest = serde_json::from_str(&serialized).expect("deserialize");
        assert_eq!(manifest, deserialized);
    }

    #[test]
    fn test_bridge_health_states() {
        assert_eq!(
            evaluate_health(false, None),
            BridgeHealth::Unprovisioned
        );
        assert_eq!(
            evaluate_health(true, None),
            BridgeHealth::Outdated {
                installed_spec: 0,
                target_spec: BRIDGE_SPEC_VERSION,
            }
        );
        assert_eq!(
            evaluate_health(true, Some(1)),
            BridgeHealth::Outdated {
                installed_spec: 1,
                target_spec: BRIDGE_SPEC_VERSION,
            }
        );
        assert_eq!(
            evaluate_health(true, Some(BRIDGE_SPEC_VERSION)),
            BridgeHealth::UpToDate {
                spec_version: BRIDGE_SPEC_VERSION,
            }
        );
        assert_eq!(
            evaluate_health(true, Some(BRIDGE_SPEC_VERSION + 1)),
            BridgeHealth::UpToDate {
                spec_version: BRIDGE_SPEC_VERSION + 1,
            }
        );
    }
}
