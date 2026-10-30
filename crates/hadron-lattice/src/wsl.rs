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
/// (e.g. `\\wsl.localhost\Ubuntu\home\user`), falling back to `\\wsl.localhost\<distro>\home`.
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

/// Given a workspace or field path on Windows WSL UNC (`\\wsl.localhost\<distro>\home\<user>\...`),
/// infer the in-distro user's `~/.hadron` directory UNC path
/// (`\\wsl.localhost\<distro>\home\<user>\.hadron`).
fn wsl_user_subpath_for_field(field_path: &Path, sub: &str) -> Option<PathBuf> {
    let (distro, linux_path) = parse_wsl_unc_path(field_path)?;
    let path_str = linux_path.to_string_lossy();
    let mut parts = path_str.split('/').filter(|p| !p.is_empty());
    let first = parts.next()?;
    let linux_sub = match first {
        "home" => {
            let user = parts.next()?;
            if sub.is_empty() {
                format!("/home/{user}/.hadron")
            } else {
                format!("/home/{user}/.hadron/{sub}")
            }
        }
        "root" => {
            if sub.is_empty() {
                "/root/.hadron".to_string()
            } else {
                format!("/root/.hadron/{sub}")
            }
        }
        _ => return None,
    };
    Some(format_wsl_unc_path(&distro, Path::new(&linux_sub)))
}

/// Given a workspace or field path on Windows WSL UNC (`\\wsl.localhost\<distro>\home\<user>\...`),
/// infer the in-distro user's `~/.hadron` directory UNC path
/// (`\\wsl.localhost\<distro>\home\<user>\.hadron`).
pub fn wsl_user_hadron_dir_for_field(field_path: &Path) -> Option<PathBuf> {
    wsl_user_subpath_for_field(field_path, "")
}

/// Given a workspace or field path on Windows WSL UNC (`\\wsl.localhost\<distro>\home\<user>\...`),
/// infer the in-distro user's `~/.hadron/team.json` UNC path
/// (`\\wsl.localhost\<distro>\home\<user>\.hadron\team.json`).
pub fn wsl_catalogue_path_for_field(field_path: &Path) -> Option<PathBuf> {
    wsl_user_subpath_for_field(field_path, "team.json")
}

/// Given a workspace or field path on Windows WSL UNC (`\\wsl.localhost\<distro>\home\<user>\...`),
/// infer the in-distro user's `~/.hadron/themes` directory UNC path
/// (`\\wsl.localhost\<distro>\home\<user>\.hadron\themes`).
pub fn wsl_themes_dir_for_field(field_path: &Path) -> Option<PathBuf> {
    wsl_user_subpath_for_field(field_path, "themes")
}

/// Given a workspace or field path on Windows WSL UNC (`\\wsl.localhost\<distro>\home\<user>\...`),
/// infer the in-distro user's `~/.hadron/skills` directory UNC path
/// (`\\wsl.localhost\<distro>\home\<user>\.hadron\skills`).
pub fn wsl_skills_dir_for_field(field_path: &Path) -> Option<PathBuf> {
    wsl_user_subpath_for_field(field_path, "skills")
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

/// Resolve the full path of `program` inside `distro` using the user's login `PATH`
/// (including `$HOME/.nvm/versions/node/*/bin`, `$HOME/.local/bin`, `$HOME/.cargo/bin`,
/// `$HOME/.gemini/antigravity-cli/bin`, `$HOME/.grok/bin`, `$HOME/.bun/bin`, `$HOME/.deno/bin`,
/// `$HOME/.volta/bin`, `/snap/bin`), or return `None` if `program` is not installed inside `distro`.
///
/// Results are cached per `(distro, program)` for the lifetime of the process.
pub fn resolve_wsl_program_path(distro: &str, program: &str) -> Option<String> {
    if program.starts_with('/') {
        return Some(program.to_string());
    }
    #[cfg(windows)]
    {
        use std::collections::HashMap;
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        use std::sync::{Mutex, OnceLock};

        static CACHE: OnceLock<Mutex<HashMap<(String, String), Option<String>>>> = OnceLock::new();
        let map = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let key = (distro.to_string(), program.to_string());
        if let Some(cached) = map.lock().unwrap().get(&key) {
            return cached.clone();
        }

        let resolved = Command::new("wsl.exe")
            .args([
                "-d",
                distro,
                "--exec",
                "sh",
                "-c",
                "NVM_BIN=$(ls -d \"$HOME\"/.nvm/versions/node/*/bin 2>/dev/null | sort -V | tail -n 1); export PATH=\"${NVM_BIN:+$NVM_BIN:}$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.gemini/antigravity-cli/bin:$HOME/.grok/bin:$HOME/.bun/bin:$HOME/.deno/bin:$HOME/.volta/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin\"; command -v \"$1\"",
                "sh",
                program,
            ])
            .creation_flags(0x08000000)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| decode_wsl_output(&o.stdout).into_iter().next())
            .filter(|s| s.starts_with('/') && !s.starts_with("/mnt/"));

        map.lock().unwrap().insert(key, resolved.clone());
        resolved
    }
    #[cfg(not(windows))]
    {
        let _ = distro;
        Some(program.to_string())
    }
}

/// Check whether Bubblewrap (`bwrap`) is installed and functional inside `distro`.
///
/// Cached per `distro` for the lifetime of the process.
pub fn is_wsl_bwrap_available(distro: &str) -> bool {
    #[cfg(windows)]
    {
        use std::collections::HashMap;
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        use std::sync::{Mutex, OnceLock};

        static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
        let map = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(&cached) = map.lock().unwrap().get(distro) {
            return cached;
        }

        let Some(bwrap_path) = resolve_wsl_program_path(distro, "bwrap") else {
            map.lock().unwrap().insert(distro.to_string(), false);
            return false;
        };

        let ok = Command::new("wsl.exe")
            .args([
                "-d",
                distro,
                "--exec",
                &bwrap_path,
                "--ro-bind",
                "/",
                "/",
                "true",
            ])
            .creation_flags(0x08000000)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        map.lock().unwrap().insert(distro.to_string(), ok);
        ok
    }
    #[cfg(not(windows))]
    {
        let _ = distro;
        use std::process::Command;
        use std::sync::OnceLock;
        static AVAILABLE: OnceLock<bool> = OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            Path::new("/usr/bin/bwrap").exists()
                && Command::new("/usr/bin/bwrap")
                    .args(["--ro-bind", "/", "/", "true"])
                    .status()
                    .map(|s| s.success())
                    .unwrap_or(false)
        })
    }
}

/// Whether `program` is an explicit Windows host path (drive-letter `C:\...` / `C:/...`
/// or UNC `\\...`). Such paths always live on the Windows host and can never be
/// `execvpe`'d verbatim inside a Linux WSL distribution.
pub fn is_windows_host_path(program: &str) -> bool {
    let bytes = program.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return true;
    }
    program.starts_with(r"\\")
}

/// Check whether `program` exists inside `distro` (checking standard system and user bin directories)
/// or on the Windows host, and return `true` if `program` should be executed via `wsl.exe` inside `distro`,
/// or `false` if it is only installed on the Windows host (e.g. `agy.exe` on Windows when opening a WSL repo,
/// or a Windows-host venv interpreter like `C:\Users\...\.hadron\bridges\agy\venv\Scripts\python.exe`).
///
/// Results are cached per `(distro, program)` for the lifetime of the process.
pub fn should_run_in_wsl(distro: &str, program: &str) -> bool {
    if is_windows_host_path(program) {
        return false;
    }
    #[cfg(windows)]
    {
        use std::collections::HashMap;
        use std::os::windows::process::CommandExt;
        use std::process::Command;
        use std::sync::{Mutex, OnceLock};

        static CACHE: OnceLock<Mutex<HashMap<(String, String), bool>>> = OnceLock::new();
        let map = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        let key = (distro.to_string(), program.to_string());
        if let Some(&cached) = map.lock().unwrap().get(&key) {
            return cached;
        }

        let in_wsl = resolve_wsl_program_path(distro, program).is_some();
        let on_host = Path::new(program).exists()
            || Command::new("where.exe")
                .arg(program)
                .creation_flags(0x08000000)
                .status()
                .map(|s| s.success())
                .unwrap_or(false);

        let decision = decide_wsl_vs_host(in_wsl, on_host);
        map.lock().unwrap().insert(key, decision);
        decision
    }
    #[cfg(not(windows))]
    {
        let _ = (distro, program);
        true
    }
}

/// Pure decision helper for unit tests: given whether a binary exists in WSL and whether it
/// exists on the Windows host, returns `true` when execution should route through `wsl.exe`.
pub fn decide_wsl_vs_host(in_wsl: bool, on_host: bool) -> bool {
    if in_wsl {
        true
    } else {
        !on_host
    }
}

/// Resolve the in-distro user home directory (e.g. `/home/user` or `/root`) from a Linux path,
/// falling back to `None` if `linux_path` is not under `/home/<user>` or `/root`.
pub fn infer_linux_home_from_path(linux_path: &Path) -> Option<String> {
    let s = linux_path.to_string_lossy();
    let mut parts = s.split('/').filter(|p| !p.is_empty());
    match parts.next()? {
        "home" => {
            let user = parts.next()?;
            Some(format!("/home/{user}"))
        }
        "root" => Some("/root".to_string()),
        _ => None,
    }
}

/// Build an augmented Linux `PATH` string that prepends the user's `~/.nvm/versions/node/*/bin`
/// (when discoverable), `~/.local/bin`, `~/.cargo/bin`, `~/.gemini/antigravity-cli/bin`,
/// `~/.grok/bin`, `~/.bun/bin`, `~/.deno/bin`, and `~/.volta/bin` so `wsl.exe --exec` and `bwrap`
/// can find user-installed tools (like `agy`, `cargo`, `claude`, `codex`, `copilot`, `node`, `npx`)
/// without invoking a shell that mangles arguments.
pub fn wsl_user_path_env(linux_cwd: &Path) -> Option<String> {
    let home = infer_linux_home_from_path(linux_cwd)?;
    let mut dirs = Vec::new();
    let nvm_nodes = Path::new(&home).join(".nvm/versions/node");
    if let Ok(entries) = std::fs::read_dir(&nvm_nodes) {
        let mut versions: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path().join("bin")))
            .filter(|p| p.is_dir())
            .collect();
        versions.sort();
        for v in versions.into_iter().rev() {
            dirs.push(v.to_string_lossy().replace('\\', "/"));
        }
    }
    dirs.extend([
        format!("{home}/.local/bin"),
        format!("{home}/.cargo/bin"),
        format!("{home}/.gemini/antigravity-cli/bin"),
        format!("{home}/.grok/bin"),
        format!("{home}/.bun/bin"),
        format!("{home}/.deno/bin"),
        format!("{home}/.volta/bin"),
        "/usr/local/sbin".to_string(),
        "/usr/local/bin".to_string(),
        "/usr/sbin".to_string(),
        "/usr/bin".to_string(),
        "/sbin".to_string(),
        "/bin".to_string(),
        "/snap/bin".to_string(),
    ]);
    Some(dirs.join(":"))
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
    let resolved_prog = resolve_wsl_program_path(distro, program).unwrap_or_else(|| program.to_string());
    let mut wsl_args = vec![
        "-d".to_string(),
        distro.to_string(),
        "--cd".to_string(),
        linux_cwd.to_string_lossy().replace('\\', "/"),
        "--exec".to_string(),
        resolved_prog,
    ];
    wsl_args.extend(args.iter().map(|a| translate_arg_for_wsl(distro, a)));
    ("wsl.exe".to_string(), wsl_args)
}

/// Construct a `std::process::Command` for `program` in `cwd` that automatically routes
/// through `wsl.exe -d <distro> --cd <linux_cwd> --exec <program>` when `cwd` is a Windows
/// WSL UNC path (`\\wsl.localhost\<distro>\...` or `\\wsl$\<distro>\...`).
pub fn wsl_aware_command(cwd: &Path, program: &str) -> std::process::Command {
    if let Some((distro, linux_cwd)) = parse_wsl_unc_path(cwd) {
        let resolved_prog = resolve_wsl_program_path(&distro, program).unwrap_or_else(|| program.to_string());
        let mut c = std::process::Command::new("wsl.exe");
        c.args([
            "-d",
            &distro,
            "--cd",
            &linux_cwd.to_string_lossy().replace('\\', "/"),
            "--exec",
            &resolved_prog,
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
        let path = Path::new(r"\\wsl.localhost\Ubuntu\home\user\repo");
        let parsed = parse_wsl_unc_path(path);
        assert_eq!(
            parsed,
            Some(("Ubuntu".to_string(), PathBuf::from("/home/user/repo")))
        );

        // Forward-slash variant
        let forward = Path::new("//wsl.localhost/Ubuntu/home/user/repo");
        assert_eq!(
            parse_wsl_unc_path(forward),
            Some(("Ubuntu".to_string(), PathBuf::from("/home/user/repo")))
        );

        // Windows canonicalized verbatim UNC variants (`\\?\UNC\...`)
        let verbatim = Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\user\repo");
        assert_eq!(
            parse_wsl_unc_path(verbatim),
            Some(("Ubuntu".to_string(), PathBuf::from("/home/user/repo")))
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
        assert_eq!(parse_wsl_unc_path(Path::new(r"C:\Users\user")), None);
        assert_eq!(parse_wsl_unc_path(Path::new("/home/user")), None);
        assert_eq!(parse_wsl_unc_path(Path::new(r"\\server\share\folder")), None);
        assert_eq!(parse_wsl_unc_path(Path::new(".")), None);

        assert!(!is_wsl_unc_path(Path::new(r"C:\Users\user")));
        assert!(is_wsl_unc_path(Path::new(r"\\wsl.localhost\Ubuntu\home\user")));
        assert!(is_wsl_unc_path(Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\user")));
        assert!(is_wsl_unc_path(Path::new(r"\\wsl$\Debian\var\log")));

        // Verify list_installed_wsl_distros executes safely without panicking
        let _ = list_installed_wsl_distros();
    }

    #[test]
    fn test_parse_wsl_home_and_catalogue_path() {
        assert_eq!(
            parse_wsl_home_output("Ubuntu", b"/home/user\n"),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\user")
        );
        assert_eq!(
            parse_wsl_home_output("Ubuntu", b""),
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home")
        );
        assert_eq!(
            resolve_wsl_home_unc_path("NonExistentTestDistro"),
            PathBuf::from(r"\\wsl.localhost\NonExistentTestDistro\home")
        );

        let field = Path::new(r"\\wsl.localhost\Ubuntu\home\user\project");
        assert_eq!(
            wsl_catalogue_path_for_field(field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\user\.hadron\team.json"
            ))
        );
        assert_eq!(
            wsl_themes_dir_for_field(field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\user\.hadron\themes"
            ))
        );
        assert_eq!(
            wsl_skills_dir_for_field(field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\user\.hadron\skills"
            ))
        );
        let verbatim_field = Path::new(r"\\?\UNC\wsl.localhost\Ubuntu\home\user\project");
        assert_eq!(
            wsl_catalogue_path_for_field(verbatim_field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\user\.hadron\team.json"
            ))
        );
        assert_eq!(
            wsl_themes_dir_for_field(verbatim_field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\user\.hadron\themes"
            ))
        );
        assert_eq!(
            wsl_skills_dir_for_field(verbatim_field),
            Some(PathBuf::from(
                r"\\wsl.localhost\Ubuntu\home\user\.hadron\skills"
            ))
        );
        assert_eq!(
            wsl_catalogue_path_for_field(Path::new(r"C:\Users\user\project")),
            None
        );
        assert_eq!(
            wsl_themes_dir_for_field(Path::new(r"C:\Users\user\project")),
            None
        );
        assert_eq!(
            wsl_skills_dir_for_field(Path::new(r"C:\Users\user\project")),
            None
        );
    }

    #[test]
    fn test_format_wsl_unc_path() {
        let formatted = format_wsl_unc_path("Ubuntu", Path::new("/home/user/repo"));
        assert_eq!(
            formatted,
            PathBuf::from(r"\\wsl.localhost\Ubuntu\home\user\repo")
        );

        // Round trip
        let parsed = parse_wsl_unc_path(&formatted).unwrap();
        assert_eq!(parsed.0, "Ubuntu");
        assert_eq!(parsed.1, PathBuf::from("/home/user/repo"));
    }

    #[test]
    fn test_wrap_wsl_command() {
        let (prog, args) = wrap_wsl_command(
            "Ubuntu",
            Path::new("/home/user"),
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
                "/home/user".to_string(),
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
        let unc = Path::new(r"\\wsl.localhost\Ubuntu\home\user\project");
        let cmd = git_command(unc);
        assert_eq!(cmd.get_program(), "wsl.exe");
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(
            args,
            vec!["-d", "Ubuntu", "--cd", "/home/user/project", "--exec", "git"]
        );

        let local = Path::new("/home/user/project");
        let local_cmd = git_command(local);
        assert_eq!(local_cmd.get_program(), "git");
        assert_eq!(local_cmd.get_current_dir(), Some(local));
    }

    #[test]
    fn test_decide_wsl_vs_host_and_user_path() {
        // When present in WSL, run in WSL
        assert!(decide_wsl_vs_host(true, true));
        assert!(decide_wsl_vs_host(true, false));
        // When NOT in WSL, but present on Windows host, run on Windows host
        assert!(!decide_wsl_vs_host(false, true));
        // When neither has it, default to WSL so the WSL error surfaces cleanly
        assert!(decide_wsl_vs_host(false, false));

        assert_eq!(
            infer_linux_home_from_path(Path::new("/home/user/project")),
            Some("/home/user".to_string())
        );
        assert_eq!(
            infer_linux_home_from_path(Path::new("/root/project")),
            Some("/root".to_string())
        );
        assert_eq!(infer_linux_home_from_path(Path::new("/var/log")), None);

        let path_env = wsl_user_path_env(Path::new("/home/user/project")).unwrap();
        assert!(path_env.contains("/home/user/.local/bin"));
        assert!(path_env.contains("/home/user/.cargo/bin"));
        assert!(path_env.contains("/home/user/.gemini/antigravity-cli/bin"));

        assert!(is_windows_host_path(
            r"C:\Users\Komputer\.hadron/bridges/agy/venv/Scripts/python.exe"
        ));
        assert!(is_windows_host_path(r"\\server\share\tool.exe"));
        assert!(!is_windows_host_path("npx"));
        assert!(!is_windows_host_path("/home/user/.hadron/bridges/agy/venv/bin/python"));
        assert!(!should_run_in_wsl(
            "Ubuntu-24.04",
            r"C:\Users\Komputer\.hadron/bridges/agy/venv/Scripts/python.exe"
        ));
    }
}
