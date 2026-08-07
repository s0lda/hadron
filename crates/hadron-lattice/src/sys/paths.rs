use std::io;
use std::path::{Path, PathBuf};

/// Canonicalizes a path using `dunce` to strip Windows extended length UNC prefixes (`\\?\`).
pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// Simplifies a path using `dunce` to strip Windows extended length UNC prefixes (`\\?\`).
pub fn simplified(path: &Path) -> &Path {
    dunce::simplified(path)
}

/// Strip `\\?\` or `//?/` UNC prefix from a string representation if present.
pub fn strip_unc_prefix(s: &str) -> String {
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else if let Some(stripped) = s.strip_prefix("//?/") {
        stripped.to_string()
    } else {
        s.to_string()
    }
}

/// Normalizes path representations across POSIX and Windows separators without UNC prefixes.
pub fn normalize_path_str(path: &str) -> String {
    let clean = strip_unc_prefix(path);
    #[cfg(windows)]
    {
        clean.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        clean
    }
}

/// Returns a normalized `PathBuf` without UNC prefixes.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from(normalize_path_str(&path.to_string_lossy()))
}
