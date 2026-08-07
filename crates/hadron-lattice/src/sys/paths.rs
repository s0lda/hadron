use std::path::{Path, PathBuf};

/// Normalizes path representations across POSIX and Windows separators.
pub fn normalize_path_str(path: &str) -> String {
    #[cfg(windows)]
    {
        path.replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path.to_string()
    }
}

/// Returns a normalized `PathBuf`.
pub fn normalize_path(path: &Path) -> PathBuf {
    PathBuf::from(normalize_path_str(&path.to_string_lossy()))
}
