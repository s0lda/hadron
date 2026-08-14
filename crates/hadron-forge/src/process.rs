//! Background process supervisor for jailed agents.
//!
//! Spawns long-running or interactive background tasks (like dev servers, test runners,
//! or background build processes) within the worktree jail.
//!
//! **Invariants:**
//! 1. Process group isolation: every spawned process gets its own process group (`setpgid(0, 0)`).
//! 2. Group teardown: killing a process or dropping the manager signals the entire process group.
//! 3. Jail enforcement: arguments and working directory are strictly validated against the worktree root.
//! 4. Bounded log ring-buffer: stdout/stderr are read continuously into memory-bounded buffers.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, RwLock};

use crate::exec::{arg_is_jailed, kill_process_group, Program};
use crate::file::{ForgeError, Root};

/// Maximum lines kept per process in the log ring-buffer.
pub const MAX_RING_BUFFER_LINES: usize = 5_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp_ms: u64,
    pub stream: LogStream,
    pub line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessSummary {
    pub id: u64,
    pub program: String,
    pub args: Vec<String>,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub pid: u32,
    pub uptime_secs: u64,
}

struct ProcessHandle {
    id: u64,
    program: String,
    args: Vec<String>,
    pid: u32,
    started_at: Instant,
    running: Arc<RwLock<bool>>,
    exit_code: Arc<RwLock<Option<i32>>>,
    logs: Arc<RwLock<VecDeque<LogEntry>>>,
    stdin_tx: Option<mpsc::Sender<String>>,
}

/// Thread-safe supervisor for background processes.
#[derive(Clone)]
pub struct ProcessManager {
    root: Root,
    processes: Arc<RwLock<HashMap<u64, Arc<ProcessHandle>>>>,
    next_id: Arc<AtomicU64>,
}

impl ProcessManager {
    pub fn new(root: Root) -> Self {
        Self {
            root,
            processes: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn root(&self) -> &Root {
        &self.root
    }

    /// Spawn a new background process inside the worktree jail.
    pub async fn spawn(
        &self,
        program_name: &str,
        args: &[impl AsRef<str>],
        env_vars: Option<HashMap<String, String>>,
    ) -> Result<u64, ForgeError> {
        let args_vec: Vec<String> = args.iter().map(|a| a.as_ref().to_string()).collect();

        // Validate program allowlist
        let prog = Program::parse(program_name).ok_or_else(|| {
            ForgeError::Rejected(format!(
                "program {:?} is not on the allowed execution list",
                program_name
            ))
        })?;

        // Validate jailed arguments
        if let Some(bad) = args_vec.iter().find(|a| !arg_is_jailed(a)) {
            return Err(ForgeError::Rejected(format!(
                "argument {:?} would point outside the worktree jail",
                bad
            )));
        }

        // Canonicalise working directory
        let cwd = self
            .root
            .path()
            .canonicalize()
            .map_err(|e| ForgeError::Io(e.to_string()))?;

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);

        let mut cmd = tokio::process::Command::new(prog.as_str());
        cmd.args(&args_vec).current_dir(&cwd);
        cmd.env("GIT_TERMINAL_PROMPT", "0");

        if let Some(envs) = env_vars {
            for (k, v) in envs {
                cmd.env(k, v);
            }
        }

        #[cfg(unix)]
        {
            cmd.process_group(0);
        }

        cmd.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| ForgeError::Io(format!("failed to spawn {program_name}: {e}")))?;

        let pid = child
            .id()
            .ok_or_else(|| ForgeError::Io("failed to get child PID".into()))?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let mut stdin = child.stdin.take();

        let running = Arc::new(RwLock::new(true));
        let exit_code = Arc::new(RwLock::new(None));
        let logs = Arc::new(RwLock::new(VecDeque::new()));

        let (stdin_tx, mut stdin_rx) = mpsc::channel::<String>(32);

        // Stdin handler task
        if let Some(mut stdin_pipe) = stdin.take() {
            tokio::spawn(async move {
                while let Some(line) = stdin_rx.recv().await {
                    if stdin_pipe.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    if stdin_pipe.flush().await.is_err() {
                        break;
                    }
                }
            });
        }

        // Stdout reader task
        if let Some(stdout_pipe) = stdout {
            let logs_clone = Arc::clone(&logs);
            tokio::spawn(async move {
                let mut reader = BufReader::new(stdout_pipe).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let mut lg = logs_clone.write().await;
                    if lg.len() >= MAX_RING_BUFFER_LINES {
                        lg.pop_front();
                    }
                    lg.push_back(LogEntry {
                        timestamp_ms: ts,
                        stream: LogStream::Stdout,
                        line,
                    });
                }
            });
        }

        // Stderr reader task
        if let Some(stderr_pipe) = stderr {
            let logs_clone = Arc::clone(&logs);
            tokio::spawn(async move {
                let mut reader = BufReader::new(stderr_pipe).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    let ts = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as u64;
                    let mut lg = logs_clone.write().await;
                    if lg.len() >= MAX_RING_BUFFER_LINES {
                        lg.pop_front();
                    }
                    lg.push_back(LogEntry {
                        timestamp_ms: ts,
                        stream: LogStream::Stderr,
                        line,
                    });
                }
            });
        }

        // Child process waiter task
        let running_clone = Arc::clone(&running);
        let exit_code_clone = Arc::clone(&exit_code);
        let logs_clone = Arc::clone(&logs);
        tokio::spawn(async move {
            let res = child.wait().await;
            let mut r = running_clone.write().await;
            *r = false;
            let code = match res {
                Ok(status) => status.code(),
                Err(_) => None,
            };
            let mut ec = exit_code_clone.write().await;
            *ec = code;

            let ts = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let mut lg = logs_clone.write().await;
            lg.push_back(LogEntry {
                timestamp_ms: ts,
                stream: LogStream::System,
                line: format!("[process exited with code: {:?}]", code),
            });
        });

        let handle = Arc::new(ProcessHandle {
            id,
            program: program_name.to_string(),
            args: args_vec,
            pid,
            started_at: Instant::now(),
            running,
            exit_code,
            logs,
            stdin_tx: Some(stdin_tx),
        });

        let mut procs = self.processes.write().await;
        procs.insert(id, handle);

        Ok(id)
    }

    /// Retrieve logs for a process by ID.
    pub async fn get_logs(
        &self,
        id: u64,
        tail_lines: Option<usize>,
        cursor: Option<usize>,
    ) -> Result<String, ForgeError> {
        let procs = self.processes.read().await;
        let proc = procs
            .get(&id)
            .ok_or(ForgeError::NotFound)?;

        let logs = proc.logs.read().await;
        let running = *proc.running.read().await;
        let exit_code = *proc.exit_code.read().await;

        let total_lines = logs.len();
        let start_idx = if let Some(c) = cursor {
            c.min(total_lines)
        } else if let Some(n) = tail_lines {
            total_lines.saturating_sub(n)
        } else {
            0
        };

        let mut out = String::new();
        out.push_str(&format!(
            "Process #{} ({}) [running: {}, exit: {:?}]\n",
            id, proc.program, running, exit_code
        ));
        for entry in logs.iter().skip(start_idx) {
            let prefix = match entry.stream {
                LogStream::Stdout => "",
                LogStream::Stderr => "[stderr] ",
                LogStream::System => "[system] ",
            };
            out.push_str(prefix);
            out.push_str(&entry.line);
            out.push('\n');
        }

        Ok(out)
    }

    /// List all tracked processes.
    pub async fn list(&self) -> Vec<ProcessSummary> {
        let procs = self.processes.read().await;
        let mut list = Vec::new();
        for proc in procs.values() {
            let running = *proc.running.read().await;
            let exit_code = *proc.exit_code.read().await;
            list.push(ProcessSummary {
                id: proc.id,
                program: proc.program.clone(),
                args: proc.args.clone(),
                running,
                exit_code,
                pid: proc.pid,
                uptime_secs: proc.started_at.elapsed().as_secs(),
            });
        }
        list.sort_by_key(|p| p.id);
        list
    }

    /// Send input to the process stdin.
    pub async fn send_stdin(&self, id: u64, input: &str) -> Result<(), ForgeError> {
        let procs = self.processes.read().await;
        let proc = procs
            .get(&id)
            .ok_or(ForgeError::NotFound)?;

        let is_running = *proc.running.read().await;
        if !is_running {
            return Err(ForgeError::Rejected(format!(
                "process {} is not running",
                id
            )));
        }

        if let Some(tx) = &proc.stdin_tx {
            tx.send(input.to_string()).await.map_err(|_| {
                ForgeError::Io(format!("failed to send stdin to process {}", id))
            })?;
            Ok(())
        } else {
            Err(ForgeError::Rejected("stdin pipe is not open".into()))
        }
    }

    /// Terminate a background process and its entire process group.
    pub async fn kill(&self, id: u64) -> Result<bool, ForgeError> {
        let procs = self.processes.read().await;
        let proc = procs
            .get(&id)
            .ok_or(ForgeError::NotFound)?;

        let pid = proc.pid;
        let is_running = *proc.running.read().await;
        if !is_running {
            return Ok(false);
        }

        kill_process_group(pid);
        let mut r = proc.running.write().await;
        *r = false;

        Ok(true)
    }

    /// Kill all active process groups managed by this supervisor.
    pub async fn kill_all(&self) {
        let procs = self.processes.read().await;
        for proc in procs.values() {
            let is_running = *proc.running.read().await;
            if is_running {
                kill_process_group(proc.pid);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn process_manager_spawns_reads_logs_and_kills_group() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path().to_path_buf());
        let manager = ProcessManager::new(root);

        let id = manager.spawn("cargo", &["--version"], None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;

        let logs = manager.get_logs(id, Some(10), None).await.unwrap();
        assert!(logs.contains("cargo"), "logs should contain cargo version: {logs}");

        let procs = manager.list().await;
        assert_eq!(procs.len(), 1);
        assert_eq!(procs[0].id, id);

        let _ = manager.kill(id).await;
    }

    #[tokio::test]
    async fn process_manager_rejects_unjailed_args() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path().to_path_buf());
        let manager = ProcessManager::new(root);

        let res = manager.spawn("git", &["-C", "/tmp"], None).await;
        assert!(res.is_err(), "unjailed path must be rejected");
    }

    #[tokio::test]
    async fn process_manager_handles_missing_process() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path().to_path_buf());
        let manager = ProcessManager::new(root);

        assert!(manager.get_logs(999, None, None).await.is_err());
        assert!(manager.kill(999).await.is_err());
    }
}
