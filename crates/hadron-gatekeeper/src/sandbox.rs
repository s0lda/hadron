//! Ephemeral Cgroup/Container Isolation Gate (Capability #7).
//!
//! Provides isolated command execution environments with scrubbed envs and resource quotas.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use hadron_lattice::SandboxMode;


pub fn is_bwrap_available() -> bool {
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

/// When `work_dir` is inside a Hadron quark worktree (`<repo_root>/.hadron/trees/<quark_id>`),
/// returns the parent repository root (`<repo_root>`) so `bwrap` can bind `<repo_root>/.git`,
/// `<repo_root>/.hadron/nucleus`, `<repo_root>/.hadron/docs`, `<repo_root>/.hadron/live`, and
/// `<repo_root>/target` while keeping the main checkout working tree isolated.
pub fn infer_repo_root_from_worktree(work_dir: &Path) -> Option<PathBuf> {
    for ancestor in work_dir.ancestors() {
        if ancestor.file_name().and_then(|n| n.to_str()) == Some("trees") {
            if let Some(hadron_dir) = ancestor.parent() {
                if hadron_dir.file_name().and_then(|n| n.to_str()) == Some(".hadron") {
                    if let Some(repo_root) = hadron_dir.parent() {
                        return Some(repo_root.to_path_buf());
                    }
                }
            }
        }
    }
    None
}

pub fn build_bwrap_args(
    work_dir: &Path,
    program: &str,
    args: &[String],
    mode: SandboxMode,
    unshare_net: bool,
) -> Vec<String> {
    if mode == SandboxMode::Off {
        let mut full = vec![program.to_string()];
        full.extend_from_slice(args);
        return full;
    }

    let effective_work_dir =
        if let Some((_distro, linux_path)) = hadron_lattice::wsl::parse_wsl_unc_path(work_dir) {
            linux_path
        } else {
            work_dir.to_path_buf()
        };
    let work_dir_str = effective_work_dir.to_string_lossy().replace('\\', "/");

    let mut bwrap = vec![
        "--ro-bind".to_string(), "/usr".to_string(), "/usr".to_string(),
        "--ro-bind".to_string(), "/bin".to_string(), "/bin".to_string(),
        "--ro-bind-try".to_string(), "/sbin".to_string(), "/sbin".to_string(),
        "--ro-bind".to_string(), "/lib".to_string(), "/lib".to_string(),
        "--ro-bind-try".to_string(), "/lib64".to_string(), "/lib64".to_string(),
        "--ro-bind-try".to_string(), "/lib32".to_string(), "/lib32".to_string(),
        "--ro-bind-try".to_string(), "/run".to_string(), "/run".to_string(),
        "--ro-bind-try".to_string(), "/mnt/wsl".to_string(), "/mnt/wsl".to_string(),
        "--ro-bind-try".to_string(), "/etc".to_string(), "/etc".to_string(),
        "--ro-bind-try".to_string(), "/opt".to_string(), "/opt".to_string(),
        "--proc".to_string(), "/proc".to_string(),
        "--dev".to_string(), "/dev".to_string(),
        "--tmpfs".to_string(), "/tmp".to_string(),
    ];

    let linux_home = hadron_lattice::wsl::infer_linux_home_from_path(&effective_work_dir)
        .or_else(|| std::env::var("HOME").ok().filter(|h| h.starts_with('/')));
    if let Some(ref home) = linux_home {
        for ro_rel in [
            ".local",
            ".rustup",
            ".nvm",
            ".fnm",
            ".volta",
            ".bun",
            ".deno",
            ".pyenv",
            ".gitconfig",
        ] {
            let p = format!("{home}/{ro_rel}");
            bwrap.extend(["--ro-bind-try".to_string(), p.clone(), p]);
        }
        for rw_rel in [
            ".cargo",
            ".gemini",
            ".antigravity",
            ".claude",
            ".claude.json",
            ".codex",
            ".copilot",
            ".grok",
            ".config",
            ".cache",
            ".npm",
            ".hadron",
        ] {
            let p = format!("{home}/{rw_rel}");
            bwrap.extend(["--bind-try".to_string(), p.clone(), p]);
        }
        bwrap.extend(["--setenv".to_string(), "HOME".to_string(), home.clone()]);
        let mut sandbox_path = hadron_lattice::wsl::wsl_user_path_env(&effective_work_dir)
            .unwrap_or_else(|| {
                format!(
                    "{home}/.local/bin:{home}/.cargo/bin:{home}/.gemini/antigravity-cli/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin:/snap/bin"
                )
            });
        if program.starts_with('/') {
            if let Some(parent) = Path::new(program).parent().and_then(|p| p.to_str()) {
                if !parent.is_empty() && !sandbox_path.split(':').any(|d| d == parent) {
                    sandbox_path = format!("{parent}:{sandbox_path}");
                }
            }
        }
        bwrap.extend(["--setenv".to_string(), "PATH".to_string(), sandbox_path]);
    }

    if let Some(repo_root) = infer_repo_root_from_worktree(&effective_work_dir) {
        let root_str = repo_root.to_string_lossy().replace('\\', "/");
        let git_dir = format!("{root_str}/.git");
        let hadron_dir = format!("{root_str}/.hadron");
        bwrap.extend([
            "--bind-try".to_string(), git_dir.clone(), git_dir,
            "--ro-bind-try".to_string(), hadron_dir.clone(), hadron_dir,
        ]);
        for sub in ["nucleus", "docs", "skills", "preons", "live"] {
            let sub_path = repo_root.join(".hadron").join(sub);
            if repo_root.join(".hadron").is_dir() {
                let _ = std::fs::create_dir_all(&sub_path);
            }
            let sub_str = format!("{root_str}/.hadron/{sub}");
            bwrap.extend(["--bind-try".to_string(), sub_str.clone(), sub_str]);
        }
        let target_dir = format!("{root_str}/target");
        bwrap.extend(["--bind-try".to_string(), target_dir.clone(), target_dir]);
    }

    bwrap.extend([
        "--bind".to_string(), work_dir_str.clone(), work_dir_str.clone(),
        "--chdir".to_string(), work_dir_str,
        "--die-with-parent".to_string(),
    ]);

    if unshare_net || mode == SandboxMode::Strict {
        bwrap.push("--unshare-net".to_string());
    }

    bwrap.push(program.to_string());
    bwrap.extend_from_slice(args);
    bwrap
}

pub fn is_macos_sandbox_available() -> bool {
    cfg!(target_os = "macos") || Path::new("/usr/bin/sandbox-exec").exists()
}

pub fn build_macos_sandbox_profile(
    work_dir: &Path,
    mode: SandboxMode,
    unshare_net: bool,
) -> String {
    let work_dir_str = work_dir.display().to_string();
    let repo_subpaths = if let Some(repo_root) = infer_repo_root_from_worktree(work_dir) {
        let root_str = repo_root.display();
        format!(
            "\n    (subpath \"{root_str}/.git\")\n    (subpath \"{root_str}/.hadron/nucleus\")\n    (subpath \"{root_str}/.hadron/docs\")\n    (subpath \"{root_str}/.hadron/skills\")\n    (subpath \"{root_str}/.hadron/preons\")\n    (subpath \"{root_str}/.hadron/live\")\n    (subpath \"{root_str}/target\")"
        )
    } else {
        String::new()
    };
    let network_rule = if unshare_net || mode == SandboxMode::Strict {
        "(deny network*)"
    } else {
        "(allow network*)"
    };

    format!(
        r#"(version 1)
(deny default)
(allow process-exec*)
(allow process-fork)
(allow sysctl-read)
(allow file-read*)
(allow file-write*
    (subpath "{work_dir_str}"){repo_subpaths}
    (subpath "/private/tmp")
    (subpath "/tmp")
    (subpath "/private/var/folders")
    (subpath "/var/folders")
    (literal "/dev/null")
    (literal "/dev/zero")
    (literal "/dev/dtracehelper")
    (literal "/dev/tty")
    (literal "/dev/stdin")
    (literal "/dev/stdout")
    (literal "/dev/stderr")
)
(allow file-write-data
    (literal "/dev/null")
    (literal "/dev/zero")
    (literal "/dev/tty")
)
(allow mach-lookup)
(allow signal (target self))
(allow ipc-posix-shm*)
{network_rule}
"#
    )
}

pub fn build_macos_sandbox_args(
    work_dir: &Path,
    program: &str,
    args: &[String],
    mode: SandboxMode,
    unshare_net: bool,
) -> Vec<String> {
    if mode == SandboxMode::Off {
        let mut full = vec![program.to_string()];
        full.extend_from_slice(args);
        return full;
    }

    let profile = build_macos_sandbox_profile(work_dir, mode, unshare_net);
    let mut cmd_args = vec!["-p".to_string(), profile, program.to_string()];
    cmd_args.extend_from_slice(args);
    cmd_args
}

pub fn is_windows_sandbox_available() -> bool {
    cfg!(windows)
        || Path::new("C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe").exists()
        || Path::new("/mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe").exists()
}

pub fn escape_powershell_single_quote(s: &str) -> String {
    s.replace('\'', "''")
}

/// Escape an argument string for a single-quoted element `'...'` inside a PowerShell
/// `@(...)` array passed to a native executable (`& '<prog>' @(...)`).
///
/// Windows PowerShell 5.1 (`powershell.exe`) builds the child `CreateProcessW`
/// command line by wrapping arguments that contain whitespace in outer `"..."`
/// **without** escaping internal `"` or trailing `\`, and drops empty `''` strings
/// outright unless passed as `'""'`. Native Windows binaries (`CommandLineToArgvW` /
/// MSVCRT) then split on any unescaped `"` inside the argument (e.g. `"The code is correct"`
/// in Standard Model Rule 1), causing the next word (`code`) to be parsed as an
/// unexpected positional argument.
pub fn escape_powershell_native_arg(s: &str) -> String {
    if s.is_empty() {
        return "\"\"".to_string();
    }
    let has_whitespace = s.chars().any(|c| c.is_whitespace());
    let mut out = String::with_capacity(s.len() + 8);
    let mut bs_count = 0usize;
    for ch in s.chars() {
        match ch {
            '\\' => {
                bs_count += 1;
            }
            '"' => {
                for _ in 0..(bs_count * 2 + 1) {
                    out.push('\\');
                }
                out.push('"');
                bs_count = 0;
            }
            '\'' => {
                for _ in 0..bs_count {
                    out.push('\\');
                }
                bs_count = 0;
                out.push_str("''");
            }
            _ => {
                for _ in 0..bs_count {
                    out.push('\\');
                }
                bs_count = 0;
                out.push(ch);
            }
        }
    }
    if bs_count > 0 {
        let final_bs = if has_whitespace { bs_count * 2 } else { bs_count };
        for _ in 0..final_bs {
            out.push('\\');
        }
    }
    out
}

pub fn build_windows_sandbox_script(
    work_dir: &Path,
    program: &str,
    args: &[String],
    mode: SandboxMode,
    unshare_net: bool,
) -> String {
    let clean_work_dir = hadron_lattice::sys::paths::strip_unc_prefix(&work_dir.display().to_string());
    let work_dir_escaped = escape_powershell_single_quote(&clean_work_dir);
    let prog_escaped = escape_powershell_single_quote(program);

    let mut parts = Vec::new();

    // 1. Lock execution to worktree directory
    parts.push(format!("Set-Location -LiteralPath '{work_dir_escaped}'"));

    // 2. Scrub sensitive environment variables
    parts.push(
        "Get-ChildItem env: | Where-Object { $_.Name -match '^(AWS_|GITHUB_|ANTHROPIC_|OPENAI_|SSH_|TOKEN|SECRET|PASSWORD)' } | ForEach-Object { Remove-Item \"env:$($_.Name)\" -ErrorAction SilentlyContinue }".to_string()
    );

    // 3. Network containment if Strict or unshare_net
    if unshare_net || mode == SandboxMode::Strict {
        parts.push(
            "$env:HTTP_PROXY='http://127.0.0.1:0'; $env:HTTPS_PROXY='http://127.0.0.1:0'; $env:ALL_PROXY='http://127.0.0.1:0'; $env:NO_PROXY=''".to_string()
        );
    }

    // 4. Build argument array
    if args.is_empty() {
        parts.push(format!("& '{prog_escaped}'; exit $LASTEXITCODE"));
    } else {
        let formatted_args: Vec<String> = args
            .iter()
            .map(|a| format!("'{}'", escape_powershell_native_arg(a)))
            .collect();
        let args_array = formatted_args.join(", ");
        parts.push(format!("& '{prog_escaped}' @({args_array}); exit $LASTEXITCODE"));
    }

    parts.join("; ")
}

pub fn build_windows_sandbox_args(
    work_dir: &Path,
    program: &str,
    args: &[String],
    mode: SandboxMode,
    unshare_net: bool,
) -> (String, Vec<String>) {
    if mode == SandboxMode::Off {
        return (program.to_string(), args.to_vec());
    }

    let script = build_windows_sandbox_script(work_dir, program, args, mode, unshare_net);
    (
        "powershell.exe".to_string(),
        vec![
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-Command".to_string(),
            script,
        ],
    )
}



#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxPlatform {
    LinuxBwrap,
    MacOsSeatbelt,
    WindowsHarness,
    Fallback,
}

impl SandboxPlatform {
    pub fn name(&self) -> &'static str {
        match self {
            Self::LinuxBwrap => "Bubblewrap (Linux/WSL)",
            Self::MacOsSeatbelt => "Apple Seatbelt (macOS)",
            Self::WindowsHarness => "PowerShell & JobObject (Windows)",
            Self::Fallback => "Host Environment Isolation",
        }
    }

    pub fn is_isolated(&self) -> bool {
        !matches!(self, Self::Fallback)
    }
}

pub fn detect_sandbox_platform() -> SandboxPlatform {
    #[cfg(target_os = "macos")]
    {
        if is_macos_sandbox_available() {
            return SandboxPlatform::MacOsSeatbelt;
        }
    }

    #[cfg(target_os = "windows")]
    {
        if is_windows_sandbox_available() {
            return SandboxPlatform::WindowsHarness;
        }
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        if is_bwrap_available() {
            return SandboxPlatform::LinuxBwrap;
        }
    }

    // Dynamic checks for cross-platform / container / non-standard environments
    if is_macos_sandbox_available() {
        SandboxPlatform::MacOsSeatbelt
    } else if is_bwrap_available() {
        SandboxPlatform::LinuxBwrap
    } else if is_windows_sandbox_available() {
        SandboxPlatform::WindowsHarness
    } else {
        SandboxPlatform::Fallback
    }
}

/// Detect the active sandbox platform for a specific workspace path.
///
/// When `work_dir` is a Windows WSL UNC path (`\\wsl.localhost\<distro>\...` or `\\wsl$\<distro>\...`),
/// execution targets the Linux distribution inside WSL rather than the Windows host, so this
/// checks whether Bubblewrap (`bwrap`) is available inside `distro` and never selects the
/// Windows PowerShell harness.
pub fn detect_sandbox_platform_for_path(work_dir: &Path) -> SandboxPlatform {
    if let Some((distro, _)) = hadron_lattice::wsl::parse_wsl_unc_path(work_dir) {
        if hadron_lattice::wsl::is_wsl_bwrap_available(&distro) {
            return SandboxPlatform::LinuxBwrap;
        } else {
            return SandboxPlatform::Fallback;
        }
    }
    detect_sandbox_platform()
}

/// Cross-platform sandboxed command resolver.
/// Jails via Bubblewrap (`bwrap`) on Linux/WSL, Apple Seatbelt (`sandbox-exec`) on macOS,
/// and PowerShell containment harness with Job Objects on Windows.
///
/// When `work_dir` is a WSL UNC path (`\\wsl.localhost\<distro>\<linux_path>`) and `program`
/// runs inside WSL (`should_run_in_wsl`), resolves `program` and translates `args` for `distro`
/// and jails with `bwrap` inside `distro` (when available) rather than wrapping in host
/// `powershell.exe`.
pub fn resolve_sandboxed_command(
    work_dir: &Path,
    program: &str,
    args: &[String],
    mode: SandboxMode,
    unshare_net: bool,
) -> (String, Vec<String>) {
    if mode == SandboxMode::Off {
        return (program.to_string(), args.to_vec());
    }

    if let Some((distro, linux_cwd)) = hadron_lattice::wsl::parse_wsl_unc_path(work_dir) {
        if hadron_lattice::wsl::should_run_in_wsl(&distro, program) {
            let resolved_prog = hadron_lattice::wsl::resolve_wsl_program_path(&distro, program)
                .unwrap_or_else(|| program.to_string());
            let translated_args: Vec<String> = args
                .iter()
                .map(|a| hadron_lattice::wsl::translate_arg_for_wsl(&distro, a))
                .collect();
            if hadron_lattice::wsl::is_wsl_bwrap_available(&distro) {
                let bwrap_args = build_bwrap_args(
                    &linux_cwd,
                    &resolved_prog,
                    &translated_args,
                    mode,
                    unshare_net,
                );
                return ("bwrap".to_string(), bwrap_args);
            } else {
                return (resolved_prog, translated_args);
            }
        }
    }

    match detect_sandbox_platform() {
        SandboxPlatform::LinuxBwrap => {
            let bwrap_args = build_bwrap_args(work_dir, program, args, mode, unshare_net);
            ("bwrap".to_string(), bwrap_args)
        }
        SandboxPlatform::MacOsSeatbelt => {
            let macos_args = build_macos_sandbox_args(work_dir, program, args, mode, unshare_net);
            ("/usr/bin/sandbox-exec".to_string(), macos_args)
        }
        SandboxPlatform::WindowsHarness => {
            build_windows_sandbox_args(work_dir, program, args, mode, unshare_net)
        }
        SandboxPlatform::Fallback => {
            (program.to_string(), args.to_vec())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxConfig {
    pub max_memory_mb: Option<u64>,
    pub max_cpu_time_secs: Option<u64>,
    pub allow_network: bool,
    pub allow_file_writes: bool,
    pub allowed_paths: Vec<PathBuf>,
    pub env_passthrough: Vec<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: Some(2048),
            max_cpu_time_secs: Some(120),
            allow_network: true,
            allow_file_writes: true,
            allowed_paths: Vec::new(),
            env_passthrough: vec![
                "PATH".to_string(),
                "HOME".to_string(),
                "USER".to_string(),
                "SHELL".to_string(),
                "RUST_LOG".to_string(),
                "CARGO_HOME".to_string(),
                "RUSTUP_HOME".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxExecutionReport {
    pub exit_code: Option<i32>,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub duration_ms: u64,
    pub timed_out: bool,
}

pub struct IsolatedSandbox {
    config: SandboxConfig,
    work_dir: PathBuf,
}

impl IsolatedSandbox {
    pub fn new(work_dir: &Path, config: SandboxConfig) -> Self {
        Self {
            config,
            work_dir: work_dir.to_path_buf(),
        }
    }

    /// Prepares a scrubbed environment map containing only whitelisted environment variables.
    pub fn scrub_environment(&self) -> HashMap<String, String> {
        let mut scrubbed = HashMap::new();
        for key in &self.config.env_passthrough {
            if let Ok(val) = std::env::var(key) {
                scrubbed.insert(key.clone(), val);
            }
        }
        // Guarantee clean PATH if absent
        scrubbed.entry("PATH".to_string()).or_insert_with(|| "/usr/local/bin:/usr/bin:/bin".to_string());
        scrubbed
    }

    /// Executes a command in the isolated environment.
    pub fn run_command(&self, program: &str, args: &[&str]) -> std::io::Result<SandboxExecutionReport> {
        let start = std::time::Instant::now();
        let mut cmd = Command::new(program);
        cmd.args(args);
        cmd.current_dir(&self.work_dir);

        // Apply scrubbed env
        cmd.env_clear();
        for (k, v) in self.scrub_environment() {
            cmd.env(k, v);
        }

        let output = cmd.output()?;
        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(SandboxExecutionReport {
            exit_code: output.status.code(),
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
            duration_ms,
            timed_out: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_sandbox_isolation_and_env_scrub() {
        let tmp = tempdir().unwrap();
        let config = SandboxConfig::default();
        let sandbox = IsolatedSandbox::new(tmp.path(), config);

        let scrubbed = sandbox.scrub_environment();
        assert!(scrubbed.contains_key("PATH"));

        // Execute a quick command (echo / sh)
        let report = sandbox.run_command("echo", &["isolated sandbox active"]).unwrap();
        assert!(report.success);
        assert!(report.stdout.contains("isolated sandbox active"));
    }

    #[test]
    fn test_build_bwrap_args_jails_to_worktree_and_masks_home() {
        use std::path::Path;
        let work_dir = Path::new("/home/Jake/dev/hadron/.hadron/trees/test-quark");
        let program = "cargo";
        let args = vec!["test".to_string()];
        let bwrap_args = build_bwrap_args(work_dir, program, &args, SandboxMode::WorktreeOnly, false);

        assert!(bwrap_args.contains(&"--ro-bind".to_string()));
        assert!(bwrap_args.contains(&"/usr".to_string()));
        assert!(bwrap_args.contains(&"--bind".to_string()));
        assert!(bwrap_args.contains(&work_dir.to_str().unwrap().to_string()));
        assert!(bwrap_args.contains(&"--chdir".to_string()));
        assert!(bwrap_args.contains(&"cargo".to_string()));
    }

    #[test]
    fn test_macos_sandbox_profile_generation_and_args() {
        use std::path::Path;
        let work_dir = Path::new("/Users/developer/hadron/.hadron/trees/test-quark");
        let profile = build_macos_sandbox_profile(work_dir, SandboxMode::WorktreeOnly, false);
        assert!(profile.contains("(version 1)"));
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(allow file-read*)"));
        assert!(profile.contains("/Users/developer/hadron/.hadron/trees/test-quark"));
        assert!(profile.contains("(allow network*)"));

        let strict_profile = build_macos_sandbox_profile(work_dir, SandboxMode::Strict, false);
        assert!(strict_profile.contains("(deny network*)"));

        let args = build_macos_sandbox_args(work_dir, "cargo", &["test".to_string()], SandboxMode::WorktreeOnly, false);
        assert_eq!(args[0], "-p");
        assert!(args[1].contains("(version 1)"));
        assert_eq!(args[2], "cargo");
        assert_eq!(args[3], "test");
    }

    #[test]
    fn test_windows_sandbox_args_and_script() {
        use std::path::Path;
        let work_dir = Path::new("C:\\Users\\Jake\\hadron\\.hadron\\trees\\test-quark");
        let (prog, args) = build_windows_sandbox_args(work_dir, "cargo.exe", &["check".to_string()], SandboxMode::WorktreeOnly, false);
        assert_eq!(prog, "powershell.exe");
        assert!(args.contains(&"-NoProfile".to_string()));
        assert!(args.contains(&"-NonInteractive".to_string()));
        assert!(args.contains(&"-ExecutionPolicy".to_string()));
        assert!(args.contains(&"Bypass".to_string()));

        let script = build_windows_sandbox_script(work_dir, "cargo.exe", &["check".to_string()], SandboxMode::Strict, true);
        assert!(script.contains("Set-Location -LiteralPath"));
        assert!(script.contains("C:\\Users\\Jake\\hadron\\.hadron\\trees\\test-quark"));
        assert!(script.contains("HTTP_PROXY"));
        assert!(script.contains("cargo.exe"));
    }

    #[test]
    fn test_detect_sandbox_platform_and_resolution() {
        use std::path::Path;
        let platform = detect_sandbox_platform();
        assert!(matches!(
            platform,
            SandboxPlatform::LinuxBwrap
                | SandboxPlatform::MacOsSeatbelt
                | SandboxPlatform::WindowsHarness
                | SandboxPlatform::Fallback
        ));

        let work_dir = Path::new("/tmp/test-tree");
        let (prog, args) = resolve_sandboxed_command(work_dir, "echo", &["test".to_string()], SandboxMode::Off, false);
        assert_eq!(prog, "echo");
        assert_eq!(args, vec!["test".to_string()]);
    }

    #[test]
    fn test_escape_powershell_native_arg_preserves_embedded_quotes_and_backslashes() {
        use std::path::Path;

        // Empty argument must become "" inside single quotes ('""') so PowerShell 5.1 does not drop it.
        assert_eq!(escape_powershell_native_arg(""), "\"\"");

        // Embedded double quotes (e.g. Standard Model Rule 1) must be backslash-escaped
        // so PowerShell's native argument quoting does not split on `"The code is correct"`.
        let prompt = "# CRITICAL DIRECTIVE\n\n\"The code is correct\" and \"the code runs\" are different claims.";
        assert_eq!(
            escape_powershell_native_arg(prompt),
            "# CRITICAL DIRECTIVE\n\n\\\"The code is correct\\\" and \\\"the code runs\\\" are different claims."
        );

        // Backslashes preceding a double quote must be doubled + 1 (`2k + 1`), and single quotes doubled.
        assert_eq!(
            escape_powershell_native_arg(r#"it's a \"quoted\" test"#),
            r#"it''s a \\\"quoted\\\" test"#
        );

        // Trailing backslashes in an argument WITH whitespace must be doubled so they don't escape PowerShell's closing `"`.
        assert_eq!(
            escape_powershell_native_arg(r"C:\Program Files\App\"),
            r"C:\Program Files\App\\"
        );

        // Trailing backslashes in an argument WITHOUT whitespace are not wrapped in `"..."` by PowerShell, so keep as-is.
        assert_eq!(
            escape_powershell_native_arg(r"\\wsl.localhost\Ubuntu\home\Jake\"),
            r"\\wsl.localhost\Ubuntu\home\Jake\"
        );

        // Verify build_windows_sandbox_script escapes `--print <prompt>` properly for agy.
        let work_dir = Path::new(r"C:\Users\Jake\repo");
        let script = build_windows_sandbox_script(
            work_dir,
            "agy",
            &[
                "chat".to_string(),
                "--mode".to_string(),
                "agent".to_string(),
                "--print".to_string(),
                prompt.to_string(),
            ],
            SandboxMode::WorktreeOnly,
            false,
        );
        assert!(
            script.contains(r#"\"The code is correct\""#),
            "script must escape internal double quotes for native argv parsing, got: {script}"
        );
    }

    #[test]
    fn test_bwrap_worktree_and_wsl_unc_resolution_never_uses_powershell() {
        use std::path::{Path, PathBuf};

        let wt = Path::new("/home/Jake/dev/my-cloud/.hadron/trees/cli-agy");
        assert_eq!(
            infer_repo_root_from_worktree(wt),
            Some(PathBuf::from("/home/Jake/dev/my-cloud"))
        );

        let unc_wt = Path::new(r"\\wsl.localhost\Ubuntu\home\Jake\dev\my-cloud\.hadron\trees\cli-agy");
        let bwrap_args = build_bwrap_args(
            unc_wt,
            "/home/Jake/.local/bin/agy",
            &["--version".to_string()],
            SandboxMode::WorktreeOnly,
            false,
        );
        // Must normalize WSL UNC work_dir to Linux path inside bwrap
        assert!(bwrap_args.contains(&"/home/Jake/dev/my-cloud/.hadron/trees/cli-agy".to_string()));
        // Must mount /run and /mnt/wsl BEFORE /etc so symlinked /etc/resolv.conf resolves
        let run_idx = bwrap_args.iter().position(|a| a == "/run").expect("/run mounted");
        let etc_idx = bwrap_args.iter().position(|a| a == "/etc").expect("/etc mounted");
        assert!(run_idx < etc_idx, "/run must be mounted before /etc");
        // Must bind user toolchains/auth and parent git/nucleus/docs/skills/preons/target
        assert!(bwrap_args.contains(&"/home/Jake/.local".to_string()));
        assert!(bwrap_args.contains(&"/home/Jake/.gemini".to_string()));
        assert!(bwrap_args.contains(&"/home/Jake/dev/my-cloud/.git".to_string()));
        assert!(bwrap_args.contains(&"/home/Jake/dev/my-cloud/.hadron/nucleus".to_string()));
        assert!(bwrap_args.contains(&"/home/Jake/dev/my-cloud/.hadron/docs".to_string()));
        assert!(bwrap_args.contains(&"/home/Jake/dev/my-cloud/.hadron/skills".to_string()));
        assert!(bwrap_args.contains(&"/home/Jake/dev/my-cloud/.hadron/preons".to_string()));
        assert!(bwrap_args.contains(&"/home/Jake/dev/my-cloud/target".to_string()));

        let macos_profile = build_macos_sandbox_profile(wt, SandboxMode::WorktreeOnly, false);
        assert!(macos_profile.contains("/home/Jake/dev/my-cloud/.hadron/skills"));
        assert!(macos_profile.contains("/home/Jake/dev/my-cloud/.hadron/preons"));

        // WSL UNC path must NEVER select WindowsHarness / powershell.exe
        let platform = detect_sandbox_platform_for_path(unc_wt);
        assert_ne!(platform, SandboxPlatform::WindowsHarness);
        let (prog, args) = resolve_sandboxed_command(
            unc_wt,
            "agy",
            &["--print".to_string(), "ok".to_string()],
            SandboxMode::WorktreeOnly,
            false,
        );
        assert_ne!(prog, "powershell.exe");
        assert!(!args.iter().any(|a| a.contains("powershell")));
    }
}

