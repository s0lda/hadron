//! Windows Subsystem for Linux (WSL) UNC path translation, distro discovery,
//! and cross-boundary command execution wrapping.

use std::path::{Path, PathBuf};

/// Check if a given path is a Windows WSL UNC path (`\\wsl.localhost\...` or `\\wsl$\...`).
pub fn is_wsl_unc_path(path: &Path) -> bool {
    parse_wsl_unc_path(path).is_some()
}

/// Parse a Windows WSL UNC path into its target Linux distribution name and
/// the in-distro Linux path (with forward slashes).
///
/// Supported UNC formats:
/// - `\\wsl.localhost\<distro>\<linux_path>`
/// - `\\wsl$\<distro>\<linux_path>`
/// - Forward-slash variants: `//wsl.localhost/...` and `//wsl$/...`
pub fn parse_wsl_unc_path(path: &Path) -> Option<(String, PathBuf)> {
    let s = path.to_str()?;
    let normalized = s.replace('/', "\\");

    let rest = if let Some(stripped) = normalized.strip_prefix(r"\\wsl.localhost\") {
        stripped
    } else if let Some(stripped) = normalized.strip_prefix(r"\\wsl$\") {
        stripped
    } else {
        return None;
    };

    if rest.is_empty() {
        return None;
    }

    // Split on first backslash after the prefix to extract the distribution name
    let (distro, linux_part) = match rest.find('\\') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => (rest, ""),
    };

    if distro.is_empty() {
        return None;
    }

    let mut linux_path_str = linux_part.replace('\\', "/");
    if !linux_path_str.starts_with('/') {
        linux_path_str.insert(0, '/');
    }

    Some((distro.to_string(), PathBuf::from(linux_path_str)))
}

/// Format a distribution name and in-distro Linux path into a canonical Windows WSL UNC path:
/// `\\wsl.localhost\<distro>\<linux_path>`
pub fn format_wsl_unc_path(distro: &str, linux_path: &Path) -> PathBuf {
    let s = linux_path.to_string_lossy();
    let clean = s.trim_start_matches('/').replace('/', "\\");
    if clean.is_empty() {
        PathBuf::from(format!(r"\\wsl.localhost\{distro}\"))
    } else {
        PathBuf::from(format!(r"\\wsl.localhost\{distro}\{clean}"))
    }
}

/// Decode raw bytes from CLI command output (e.g. `wsl.exe -l -q`) into strings,
/// handling UTF-8, UTF-16LE (with or without BOM), and stripping null characters / carriage returns.
pub fn decode_wsl_output(bytes: &[u8]) -> Vec<String> {
    let decoded = if bytes.len() >= 2 && bytes[0] == 0xFF && bytes[1] == 0xFE {
        // UTF-16LE BOM
        let u16_chars: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&u16_chars)
    } else if bytes.iter().filter(|&&b| b == 0).count() > bytes.len() / 4 {
        // Likely UTF-16LE without BOM
        let u16_chars: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16_lossy(&u16_chars)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    };

    decoded
        .lines()
        .map(|line| line.trim_matches(|c: char| c == '\r' || c == '\0' || c.is_whitespace()).to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

/// Query installed WSL distributions on the local machine.
///
/// Runs `wsl.exe --list --quiet` on Windows, safely decoding UTF-16LE / UTF-8 output.
/// Returns an empty vector on non-Windows platforms or if `wsl.exe` is unavailable.
pub fn list_installed_wsl_distros() -> Vec<String> {
    #[cfg(windows)]
    {
        use std::process::Command;
        match Command::new("wsl.exe").args(["--list", "--quiet"]).output() {
            Ok(output) if output.status.success() => decode_wsl_output(&output.stdout),
            _ => Vec::new(),
        }
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Wrap a command and its arguments for execution inside a WSL distribution via `wsl.exe`.
///
/// Returns `(program, args)` where `program` is `"wsl.exe"` and args are:
/// `["-d", <distro>, "--cd", <linux_cwd>, "--", <program>, ...<args>]`
pub fn wrap_wsl_command(
    distro: &str,
    linux_cwd: &Path,
    program: &str,
    args: &[String],
) -> (String, Vec<String>) {
    let mut wsl_args = vec![
        "-d".to_string(),
        distro.to_string(),
        "--cd".to_string(),
        linux_cwd.to_string_lossy().replace('\\', "/"),
        "--".to_string(),
        program.to_string(),
    ];
    wsl_args.extend(args.iter().cloned());
    ("wsl.exe".to_string(), wsl_args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wsl_unc_path_localhost() {
        let path = Path::new(r"\\wsl.localhost\Ubuntu\home\jake\repo");
        let parsed = parse_wsl_unc_path(path);
        assert_eq!(
            parsed,
            Some(("Ubuntu".to_string(), PathBuf::from("/home/jake/repo")))
        );

        // Forward-slash variant
        let forward = Path::new("//wsl.localhost/Ubuntu/home/jake/repo");
        assert_eq!(
            parse_wsl_unc_path(forward),
            Some(("Ubuntu".to_string(), PathBuf::from("/home/jake/repo")))
        );
    }

    #[test]
    fn test_parse_wsl_unc_dollar() {
        let path = Path::new(r"\\wsl$\Debian\var\log");
        let parsed = parse_wsl_unc_path(path);
        assert_eq!(
            parsed,
            Some(("Debian".to_string(), PathBuf::from("/var/log")))
        );

        // Forward-slash variant
        let forward = Path::new("//wsl$/Debian/var/log");
        assert_eq!(
            parse_wsl_unc_path(forward),
            Some(("Debian".to_string(), PathBuf::from("/var/log")))
        );
    }

    #[test]
    fn test_parse_non_wsl_paths() {
        assert_eq!(parse_wsl_unc_path(Path::new(r"C:\Users\Jake")), None);
        assert_eq!(parse_wsl_unc_path(Path::new("/home/jake")), None);
        assert_eq!(parse_wsl_unc_path(Path::new(r"\\server\share\folder")), None);
        assert_eq!(parse_wsl_unc_path(Path::new(".")), None);

        assert!(!is_wsl_unc_path(Path::new(r"C:\Users\Jake")));
        assert!(is_wsl_unc_path(Path::new(r"\\wsl.localhost\Ubuntu\home\jake")));
        assert!(is_wsl_unc_path(Path::new(r"\\wsl$\Debian\var\log")));

        // Verify list_installed_wsl_distros executes safely without panicking
        let _ = list_installed_wsl_distros();
    }

    #[test]
    fn test_format_wsl_unc_path() {
        let formatted = format_wsl_unc_path("Ubuntu", Path::new("/home/jake/repo"));
        assert_eq!(
            formatted,
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\jake\repo")
        );

        // Round trip
        let parsed = parse_wsl_unc_path(&formatted).unwrap();
        assert_eq!(parsed.0, "Ubuntu");
        assert_eq!(parsed.1, PathBuf::from("/home/jake/repo"));
    }

    #[test]
    fn test_wrap_wsl_command() {
        let (prog, args) = wrap_wsl_command(
            "Ubuntu",
            Path::new("/home/jake"),
            "cargo",
            &["test".to_string()],
        );
        assert_eq!(prog, "wsl.exe");
        assert_eq!(
            args,
            vec![
                "-d".to_string(),
                "Ubuntu".to_string(),
                "--cd".to_string(),
                "/home/jake".to_string(),
                "--".to_string(),
                "cargo".to_string(),
                "test".to_string(),
            ]
        );
    }

    #[test]
    fn test_decode_wsl_output_utf8_and_utf16le() {
        // UTF-8 sample
        let utf8_bytes = b"Ubuntu\r\nDebian\r\ndocker-desktop\r\n";
        let distros = decode_wsl_output(utf8_bytes);
        assert_eq!(distros, vec!["Ubuntu", "Debian", "docker-desktop"]);

        // UTF-16LE without BOM
        let utf16_str = "Ubuntu\r\nDebian\r\n";
        let mut utf16_bytes = Vec::new();
        for c in utf16_str.encode_utf16() {
            utf16_bytes.extend_from_slice(&c.to_le_bytes());
        }
        let distros_u16 = decode_wsl_output(&utf16_bytes);
        assert_eq!(distros_u16, vec!["Ubuntu", "Debian"]);
    }
}
