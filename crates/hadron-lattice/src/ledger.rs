//! SQLite database path resolution and WSL UNC redirection.
//!
//! Windows 9P (`\\wsl.localhost\...` or `\\wsl$\...`) rejects Win32 `LockFileEx`
//! byte-range locks with `SQLITE_BUSY` (`Error code 5: database is locked`).
//! SQLite paths on WSL UNC locations are deterministically redirected to
//! host-local storage under `~/.hadron/ledgers/`.

use std::path::{Path, PathBuf};

/// Compute a deterministic host-local SQLite path under `~/.hadron/ledgers/` for
/// workspaces on filesystems that do not support Win32 `LockFileEx` (such as Windows
/// 9P `\\wsl.localhost\...` mounts, where SQLite fails with `Error code 5: database is locked`).
pub fn host_local_ledger_path(original_path: &Path) -> Option<PathBuf> {
    let base = crate::user_hadron_dir()?.join("ledgers");
    let norm = crate::sys::paths::strip_unc_prefix(&original_path.to_string_lossy())
        .replace('\\', "/")
        .to_ascii_lowercase();
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in norm.as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Some(base.join(format!("ledger-{hash:016x}.db")))
}

/// Resolve the actual SQLite database path to open for `db_path`.
/// If `db_path` is a WSL UNC path (e.g. `\\wsl.localhost\...`), routes to the
/// host-local ledger under `~/.hadron/ledgers/` to bypass 9P byte-range lock failures.
pub fn resolve_sqlite_db_path(db_path: &Path) -> PathBuf {
    if crate::wsl::is_wsl_unc_path(db_path) {
        if let Some(local) = host_local_ledger_path(db_path) {
            return local;
        }
    }
    db_path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_sqlite_db_path_preserves_local_paths() {
        let local = Path::new("/tmp/test-repo/.hadron/ledger.db");
        assert_eq!(resolve_sqlite_db_path(local), local);

        let win_local = Path::new(r"C:\Users\Jake\dev\hadron\.hadron\ledger.db");
        assert_eq!(resolve_sqlite_db_path(win_local), win_local);
    }

    #[test]
    fn resolve_sqlite_db_path_redirects_wsl_unc_deterministically() {
        let unc1 = Path::new(r"\\wsl.localhost\Ubuntu\home\Jake\dev\hadron\.hadron\ledger.db");
        let resolved = resolve_sqlite_db_path(unc1);
        assert_ne!(resolved, unc1);
        assert!(resolved.to_string_lossy().contains("ledgers"));
        assert!(resolved.to_string_lossy().ends_with(".db"));

        // Verbatim UNC yields identical deterministic path
        let unc_verbatim = Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\Jake\dev\hadron\.hadron\ledger.db");
        assert_eq!(resolve_sqlite_db_path(unc_verbatim), resolved);

        // Forward slash variation
        let unc_forward = Path::new("//wsl.localhost/Ubuntu/home/Jake/dev/hadron/.hadron/ledger.db");
        assert_eq!(resolve_sqlite_db_path(unc_forward), resolved);
    }
}
