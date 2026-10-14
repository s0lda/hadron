use std::io;
use std::path::{Path, PathBuf};

/// Canonicalizes a path using `dunce` and strips Windows extended-length UNC prefixes
/// (`\\?\UNC\` -> `\\` and `\\?\` -> ``).
pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    let canon = dunce::canonicalize(path)?;
    if let Some(s) = canon.to_str() {
        let stripped = strip_unc_prefix(s);
        if stripped != s {
            return Ok(PathBuf::from(stripped));
        }
    }
    Ok(canon)
}

/// Simplifies a path using `dunce` to strip Windows extended length UNC prefixes (`\\?\`).
pub fn simplified(path: &Path) -> &Path {
    dunce::simplified(path)
}

/// Strip `\\?\UNC\`, `//?/UNC/`, `\\?\`, or `//?/` extended-length prefix from a path string.
///
/// Converting `\\?\UNC\server\share` preserves the leading `\\` (`\\server\share`) rather than
/// corrupting the path into a relative `UNC\server\share`.
pub fn strip_unc_prefix(s: &str) -> String {
    if let Some(stripped) = s
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| s.strip_prefix(r"\\?\unc\"))
    {
        format!(r"\\{stripped}")
    } else if let Some(stripped) = s
        .strip_prefix("//?/UNC/")
        .or_else(|| s.strip_prefix("//?/unc/"))
    {
        format!("//{stripped}")
    } else if let Some(stripped) = s.strip_prefix(r"\\?\") {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_unc_prefix_preserves_network_share() {
        assert_eq!(
            strip_unc_prefix(r"\\?\UNC\wsl.localhost\Ubuntu\home\Jake\dev\hadron"),
            r"\\wsl.localhost\Ubuntu\home\Jake\dev\hadron"
        );
        assert_eq!(
            strip_unc_prefix("//?/UNC/wsl.localhost/Ubuntu/home/Jake"),
            "//wsl.localhost/Ubuntu/home/Jake"
        );
        assert_eq!(
            strip_unc_prefix(r"\\?\C:\Users\Jake\repo"),
            r"C:\Users\Jake\repo"
        );
        assert_eq!(
            strip_unc_prefix(r"\\wsl.localhost\Ubuntu\home\Jake"),
            r"\\wsl.localhost\Ubuntu\home\Jake"
        );
    }
}
