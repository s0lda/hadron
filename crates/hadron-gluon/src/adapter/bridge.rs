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

/// `~/.hadron/bridges/agy` — where the script and its venv live.
fn bridge_dir() -> anyhow::Result<PathBuf> {
    hadron_lattice::user_hadron_dir()
        .map(|home| home.join("bridges").join("agy"))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "could not resolve the user's home directory — neither $HOME nor \
                 %USERPROFILE% is set"
            )
        })
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

/// Create the venv and install `google-antigravity` into it. Blocking and
/// potentially slow — callers MUST run this off the seating/dispatch path and off
/// the UI thread (see task 1c in
/// `.hadron/docs/plans/2026-07-28-shippable-bridge-and-self-update.md`).
/// A no-op returning the existing interpreter if already provisioned.
pub fn provision_venv() -> anyhow::Result<PathBuf> {
    let python = venv_python()?;
    if python.exists() {
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
    install.args(["-m", "pip", "install", "--quiet", "google-antigravity"]);
    run_bounded(install, "pip install google-antigravity")?;

    if !python.exists() {
        anyhow::bail!(
            "venv provisioning finished but {} still does not exist",
            python.display()
        );
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
}
