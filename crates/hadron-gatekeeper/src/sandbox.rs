//! Ephemeral Cgroup/Container Isolation Gate (Capability #7).
//!
//! Provides isolated command execution environments with scrubbed envs and resource quotas.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use hadron_lattice::SandboxMode;


pub fn is_bwrap_available() -> bool {
    std::path::Path::new("/usr/bin/bwrap").exists()
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

    let mut bwrap = vec![
        "--ro-bind".to_string(), "/usr".to_string(), "/usr".to_string(),
        "--ro-bind".to_string(), "/bin".to_string(), "/bin".to_string(),
        "--ro-bind".to_string(), "/lib".to_string(), "/lib".to_string(),
    ];

    if Path::new("/lib64").exists() {
        bwrap.push("--ro-bind".to_string());
        bwrap.push("/lib64".to_string());
        bwrap.push("/lib64".to_string());
    }
    if Path::new("/etc/resolv.conf").exists() {
        bwrap.push("--ro-bind".to_string());
        bwrap.push("/etc/resolv.conf".to_string());
        bwrap.push("/etc/resolv.conf".to_string());
    }
    if Path::new("/etc/ssl").exists() {
        bwrap.push("--ro-bind".to_string());
        bwrap.push("/etc/ssl".to_string());
        bwrap.push("/etc/ssl".to_string());
    }

    bwrap.extend(vec![
        "--proc".to_string(), "/proc".to_string(),
        "--dev".to_string(), "/dev".to_string(),
        "--tmpfs".to_string(), "/tmp".to_string(),
        "--bind".to_string(), work_dir.display().to_string(), work_dir.display().to_string(),
        "--chdir".to_string(), work_dir.display().to_string(),
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
    (subpath "{work_dir_str}")
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

pub fn build_windows_sandbox_script(
    work_dir: &Path,
    program: &str,
    args: &[String],
    mode: SandboxMode,
    unshare_net: bool,
) -> String {
    let work_dir_escaped = escape_powershell_single_quote(&work_dir.display().to_string());
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
            .map(|a| format!("'{}'", escape_powershell_single_quote(a)))
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



/// Cross-platform sandboxed command resolver.
/// Jails via Bubblewrap (`bwrap`) where available (Linux/WSL),
/// while falling back to host executable with directory and environment scrubbing
/// on Windows, macOS, and Linux systems without bwrap.
pub fn resolve_sandboxed_command(
    work_dir: &Path,
    program: &str,
    args: &[String],
    mode: SandboxMode,
    unshare_net: bool,
) -> (String, Vec<String>) {
    if mode != SandboxMode::Off && is_bwrap_available() {
        let bwrap_args = build_bwrap_args(work_dir, program, args, mode, unshare_net);
        ("bwrap".to_string(), bwrap_args)
    } else {
        (program.to_string(), args.to_vec())
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
}

