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
/// - Verbatim UNC variants: `\\?\UNC\wsl.localhost\<distro>\...`, `\\?\UNC\wsl$\<distro>\...`
/// - Forward-slash variants: `//wsl.localhost/...`, `//wsl$/...`, `//?/UNC/wsl.localhost/...`
pub fn parse_wsl_unc_path(path: &Path) -> Option<(String, PathBuf)> {
    let s = path.to_str()?;
    let normalized = s.replace('/', "\\");

    // Strip optional Windows verbatim prefixes (`\\?\UNC\` or `\\?\`) before matching WSL host.
    let without_verbatim = if let Some(s) = normalized
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| normalized.strip_prefix(r"\\?\unc\"))
    {
        format!(r"\\{s}")
    } else if let Some(s) = normalized.strip_prefix(r"\\?\") {
        if s.starts_with(r"\\") {
            s.to_string()
        } else {
            format!(r"\\{s}")
        }
    } else {
        normalized
    };

    let rest = if let Some(stripped) = without_verbatim.strip_prefix(r"\\wsl.localhost\") {
        stripped
    } else if let Some(stripped) = without_verbatim.strip_prefix(r"\\wsl$\") {
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

/// Parse raw stdout from `wsl.exe -d <distro> -- sh -c 'printf %s "$HOME"'` into a
/// Windows WSL UNC path (`\\wsl.localhost\<distro>\<home>`), falling back to `/home`.
pub fn parse_wsl_home_output(distro: &str, bytes: &[u8]) -> PathBuf {
    if let Some(first) = decode_wsl_output(bytes).into_iter().next() {
        let trimmed = first.trim();
        if trimmed.starts_with('/') && trimmed.len() > 1 {
            return format_wsl_unc_path(distro, Path::new(trimmed));
        }
    }
    format_wsl_unc_path(distro, Path::new("/home"))
}

/// Resolve the default user's `$HOME` inside `distro` as a Windows WSL UNC path
/// (e.g. `\\wsl.localhost\Ubuntu\home\Jake`), falling back to `\\wsl.localhost\<distro>\home`.
pub fn resolve_wsl_home_unc_path(distro: &str) -> PathBuf {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        if let Ok(output) = Command::new("wsl.exe")
            .args(["-d", distro, "--exec", "sh", "-c", "printf %s \"$HOME\""])
            .creation_flags(0x08000000)
            .output()
        {
            if output.status.success() {
                return parse_wsl_home_output(distro, &output.stdout);
            }
        }
    }
    let _ = distro;
    parse_wsl_home_output(distro, b"")
}

/// Given a WSL UNC workspace path (e.g. `\\wsl.localhost\Ubuntu\home\Jake\dev\hadron`),
/// infer the in-distro user's `~/.hadron/team.json` UNC path
/// (`\\wsl.localhost\Ubuntu\home\Jake\.hadron\team.json`).
pub fn wsl_catalogue_path_for_field(field_path: &Path) -> Option<PathBuf> {
    let (distro, linux_path) = parse_wsl_unc_path(field_path)?;
    let path_str = linux_path.to_string_lossy();
    let mut parts = path_str.split('/').filter(|p| !p.is_empty());
    let first = parts.next()?;
    let user_home = match first {
        "home" => {
            let user = parts.next()?;
            PathBuf::from(format!("/home/{user}/.hadron/team.json"))
        }
        "root" => PathBuf::from("/root/.hadron/team.json"),
        _ => return None,
    };
    Some(format_wsl_unc_path(&distro, &user_home))
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
        .filter(|line| !line.is_empty() && !line.starts_with("wsl: ") && !line.starts_with("WSL: "))
        .collect()
}

/// Query installed WSL distributions on the local machine.
///
/// Runs `wsl.exe --list --quiet` on Windows (cached once per process), safely decoding UTF-16LE / UTF-8 output.
/// Returns an empty vector on non-Windows platforms or if `wsl.exe` is unavailable.
pub fn list_installed_wsl_distros() -> Vec<String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        use std::sync::OnceLock;
        static DISTROS: OnceLock<Vec<String>> = OnceLock::new();
        DISTROS
            .get_or_init(|| {
                match Command::new("wsl.exe")
                    .args(["--list", "--quiet"])
                    .creation_flags(0x08000000)
                    .output()
                {
                    Ok(output) if output.status.success() => decode_wsl_output(&output.stdout),
                    _ => Vec::new(),
                }
            })
            .clone()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Translate a command argument or environment variable value when executing inside `distro`:
/// if `val` is a WSL UNC path belonging to `distro`, returns its in-distro Linux path (`/home/...`);
/// otherwise strips any extended-length UNC prefix.
pub fn translate_arg_for_wsl(distro: &str, val: &str) -> String {
    if let Some((arg_distro, linux_path)) = parse_wsl_unc_path(Path::new(val)) {
        if arg_distro.eq_ignore_ascii_case(distro) {
            return linux_path.to_string_lossy().replace('\\', "/");
        }
    }
    crate::sys::paths::strip_unc_prefix(val)
}

/// Strip `wsl: ...` host diagnostic lines (such as `wsl: Failed to mount Z:\, see dmesg for more details.`)
/// and carriage returns / null bytes from command output captured via `wsl.exe`.
pub fn strip_wsl_noise(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for line in raw.lines() {
        let clean = line.trim_end_matches(['\r', '\0']);
        let trimmed_start = clean.trim_start_matches('\0');
        if trimmed_start.starts_with("wsl: ") || trimmed_start.starts_with("WSL: ") {
            continue;
        }
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(trimmed_start);
    }
    out
}

/// Build a `WSLENV` value that instructs `wsl.exe` on Windows to forward the given
/// environment variable names into the Linux subprocess environment without exposing
/// their values on the command line.
pub fn wslenv_for_keys<'a>(keys: impl IntoIterator<Item = &'a str>) -> String {
    let mut entries: Vec<String> = std::env::var("WSLENV")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .into_iter()
        .collect();
    for k in keys {
        let k = k.trim();
        if !k.is_empty() && k != "WSLENV" && k != "PATH" {
            entries.push(format!("{k}/u"));
        }
    }
    entries.join(":")
}

/// Wrap a command and its arguments for execution inside a WSL distribution via `wsl.exe`.
///
/// Uses `--exec` (rather than `--`) so `wsl.exe` invokes `execvp` directly with the
/// discrete argument vector instead of concatenating arguments into `/bin/bash -c`,
/// which would fail on parentheses (`%(refname:short)`), pipes (`|`), globs (`quark/*`),
/// or multi-line prompt strings.
///
/// Returns `(program, args)` where `program` is `"wsl.exe"` and args are:
/// `["-d", <distro>, "--cd", <linux_cwd>, "--exec", <program>, ...<args>]`
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
        "--exec".to_string(),
        program.to_string(),
    ];
    wsl_args.extend(args.iter().map(|a| translate_arg_for_wsl(distro, a)));
    ("wsl.exe".to_string(), wsl_args)
}

/// Construct a `std::process::Command` for `program` in `cwd` that automatically routes
/// through `wsl.exe -d <distro> --cd <linux_cwd> --exec <program>` when `cwd` is a Windows
/// WSL UNC path (`\\wsl.localhost\<distro>\...` or `\\wsl$\<distro>\...`).
pub fn wsl_aware_command(cwd: &Path, program: &str) -> std::process::Command {
    if let Some((distro, linux_cwd)) = parse_wsl_unc_path(cwd) {
        let mut c = std::process::Command::new("wsl.exe");
        c.args([
            "-d",
            &distro,
            "--cd",
            &linux_cwd.to_string_lossy().replace('\\', "/"),
            "--exec",
            program,
        ]);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        c
    } else {
        let mut c = std::process::Command::new(program);
        c.current_dir(cwd);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            c.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        c
    }
}

/// Construct a `std::process::Command` for `git` in `cwd`, routing through `wsl.exe` when
/// `cwd` is a Windows WSL UNC path so git runs natively on ext4 instead of across 9P.
pub fn git_command(cwd: &Path) -> std::process::Command {
    wsl_aware_command(cwd, "git")
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

        // Windows canonicalized verbatim UNC variants (`\\?\UNC\...`)
        let verbatim = Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\jake\repo");
        assert_eq!(
            parse_wsl_unc_path(verbatim),
            Some(("Ubuntu".to_string(), PathBuf::from("/home/jake/repo")))
        );
        let verbatim_dollar = Path::new(r"\\?\UNC\wsl$\Debian\var\log");
        assert_eq!(
            parse_wsl_unc_path(verbatim_dollar),
            Some(("Debian".to_string(), PathBuf::from("/var/log")))
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
        assert!(is_wsl_unc_path(Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\jake")));
        assert!(is_wsl_unc_path(Path::new(r"\\wsl$\Debian\var\log")));

        // Verify list_installed_wsl_distros executes safely without panicking
        let _ = list_installed_wsl_distros();
    }

    #[test]
    fn test_parse_wsl_home_and_catalogue_path() {
        assert_eq!(
            parse_wsl_home_output("Ubuntu", b"/home/Jake\n"),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\Jake")
        );
        assert_eq!(
            parse_wsl_home_output("Ubuntu", b""),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home")
        );
        assert_eq!(
            resolve_wsl_home_unc_path("Ubuntu"),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home")
        );

        let field = Path::new(r"\\wsl.localhost\Ubuntu\home\Jake\dev\hadron");
        assert_eq!(
            wsl_catalogue_path_for_field(field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\Jake\.hadron\team.json"
            ))
        );
        let verbatim_field = Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\Jake\dev\hadron");
        assert_eq!(
            wsl_catalogue_path_for_field(verbatim_field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\Jake\.hadron\team.json"
            ))
        );
        assert_eq!(
            wsl_catalogue_path_for_field(Path::new(r"C:\Users\Jake\dev\hadron")),
            None
        );
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
                "--exec".to_string(),
                "cargo".to_string(),
                "test".to_string(),
            ]
        );
    }

    #[test]
    fn test_strip_wsl_noise() {
        let noisy = "origin/main\r\nwsl: Failed to mount Z:\\, see dmesg for more details.\r\n";
        assert_eq!(strip_wsl_noise(noisy), "origin/main");

        let only_noise = "wsl: Failed to mount Z:\\, see dmesg for more details.\n";
        assert_eq!(strip_wsl_noise(only_noise), "");

        let status = " M src/main.rs\r\nwsl: Failed to mount Z:\\\r\n?? new.txt\n";
        assert_eq!(strip_wsl_noise(status), " M src/main.rs\n?? new.txt");
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

    #[test]
    fn test_git_command_routes_wsl_unc_to_wsl_exe() {
        let unc = Path::new(r"\\wsl.localhost\Ubuntu\home\jake\dev\hadron");
        let cmd = git_command(unc);
        assert_eq!(cmd.get_program(), "wsl.exe");
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            vec!["-d", "Ubuntu", "--cd", "/home/jake/dev/hadron", "--exec", "git"]
        );

        let local = Path::new("/home/jake/dev/hadron");
        let local_cmd = git_command(local);
        assert_eq!(local_cmd.get_program(), "git");
        assert_eq!(local_cmd.get_current_dir(), Some(local));
    }
}
