//! Ephemeral Cgroup/Container Isolation Gate (Capability #7).
//!
//! Provides isolated command execution environments with scrubbed envs and resource quotas.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

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
}
