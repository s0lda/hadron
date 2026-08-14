//! Interactive PTY session supervisor for jailed agents.
//!
//! Spawns interactive CLI tools, TUI apps, REPLs, and terminal prompts within
//! real pseudo-terminals (`openpty`), preserving ANSI terminal sequences, window resizing,
//! and raw control keystrokes.
//!
//! **Invariants:**
//! 1. Process group isolation: every PTY child runs in its own process group (`setpgid(0, 0)`).
//! 2. Group teardown: terminating a PTY session signals the entire process group (`kill(-pgid)`).
//! 3. Jail enforcement: arguments and working directory are strictly validated against `Root`.
//! 4. Bounded log ring-buffer: output is read continuously into a memory-bounded buffer.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock as StdRwLock};
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::exec::{arg_is_jailed, kill_process_group, Program};
use crate::file::{ForgeError, Root};

pub const DEFAULT_PTY_COLS: u16 = 80;
pub const DEFAULT_PTY_ROWS: u16 = 24;
pub const MAX_PTY_BUFFER_BYTES: usize = 2 * 1024 * 1024; // 2 MB

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtySummary {
    pub id: u64,
    pub program: String,
    pub args: Vec<String>,
    pub cols: u16,
    pub rows: u16,
    pub running: bool,
    pub exit_code: Option<i32>,
    pub pid: u32,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PtyReadResult {
    pub id: u64,
    pub output: String,
    pub total_bytes: usize,
    pub running: bool,
    pub exit_code: Option<i32>,
}

struct PtySession {
    id: u64,
    program: String,
    args: Vec<String>,
    pid: u32,
    cols: Arc<StdRwLock<u16>>,
    rows: Arc<StdRwLock<u16>>,
    started_at: Instant,
    running: Arc<StdRwLock<bool>>,
    exit_code: Arc<StdRwLock<Option<i32>>>,
    buffer: Arc<StdRwLock<Vec<u8>>>,
    #[cfg(unix)]
    master_fd: std::os::unix::io::RawFd,
}

#[derive(Clone)]
pub struct PtyManager {
    root: Root,
    sessions: Arc<RwLock<HashMap<u64, Arc<PtySession>>>>,
    next_id: Arc<AtomicU64>,
}

impl PtyManager {
    pub fn new(root: Root) -> Self {
        Self {
            root,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn root(&self) -> &Root {
        &self.root
    }

    /// Spawn a new interactive PTY session.
    pub async fn spawn(
        &self,
        program_name: &str,
        args: &[impl AsRef<str>],
        cols: Option<u16>,
        rows: Option<u16>,
        env_vars: Option<HashMap<String, String>>,
    ) -> Result<u64, ForgeError> {
        let args_vec: Vec<String> = args.iter().map(|a| a.as_ref().to_string()).collect();

        let prog = Program::parse(program_name).ok_or_else(|| {
            ForgeError::Rejected(format!(
                "program {:?} is not on the allowed execution list",
                program_name
            ))
        })?;

        if let Some(bad) = args_vec.iter().find(|a| !arg_is_jailed(a)) {
            return Err(ForgeError::Rejected(format!(
                "argument {:?} would point outside the worktree jail",
                bad
            )));
        }

        let cwd = self
            .root
            .path()
            .canonicalize()
            .map_err(|e| ForgeError::Io(e.to_string()))?;

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let terminal_cols = cols.unwrap_or(DEFAULT_PTY_COLS);
        let terminal_rows = rows.unwrap_or(DEFAULT_PTY_ROWS);

        #[cfg(unix)]
        {
            use std::os::unix::io::{FromRawFd, RawFd};
            use std::os::unix::process::CommandExt;

            let mut master_fd: RawFd = -1;
            let mut slave_fd: RawFd = -1;
            let ws = libc::winsize {
                ws_row: terminal_rows,
                ws_col: terminal_cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };

            let res = unsafe {
                libc::openpty(
                    &mut master_fd,
                    &mut slave_fd,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    &ws,
                )
            };
            if res != 0 {
                return Err(ForgeError::Io(format!(
                    "failed to open pseudo-terminal: errno {}",
                    std::io::Error::last_os_error()
                )));
            }

            let slave_file = unsafe { std::fs::File::from_raw_fd(slave_fd) };
            let slave_stdin = slave_file
                .try_clone()
                .map_err(|e| ForgeError::Io(e.to_string()))?;
            let slave_stdout = slave_file
                .try_clone()
                .map_err(|e| ForgeError::Io(e.to_string()))?;
            let slave_stderr = slave_file;

            let mut cmd = std::process::Command::new(prog.as_str());
            cmd.args(&args_vec).current_dir(&cwd);
            cmd.env("TERM", "xterm-256color");
            cmd.env("GIT_TERMINAL_PROMPT", "0");

            if let Some(envs) = env_vars {
                for (k, v) in envs {
                    cmd.env(k, v);
                }
            }

            cmd.stdin(slave_stdin)
                .stdout(slave_stdout)
                .stderr(slave_stderr);

            unsafe {
                cmd.pre_exec(|| {
                    libc::setpgid(0, 0);
                    Ok(())
                });
            }

            let mut child = cmd.spawn().map_err(|e| {
                unsafe { libc::close(master_fd) };
                ForgeError::Io(format!("failed to spawn PTY child {program_name}: {e}"))
            })?;

            let pid = child.id();
            let running = Arc::new(StdRwLock::new(true));
            let exit_code = Arc::new(StdRwLock::new(None));
            let buffer = Arc::new(StdRwLock::new(Vec::new()));

            // Background thread to read master fd output
            let buffer_clone = Arc::clone(&buffer);
            let running_clone = Arc::clone(&running);
            let exit_code_clone = Arc::clone(&exit_code);

            // Duplicate master fd for reading
            let read_master_fd = unsafe { libc::dup(master_fd) };

            std::thread::spawn(move || {
                let mut read_buf = [0u8; 4096];
                loop {
                    let n = unsafe {
                        libc::read(
                            read_master_fd,
                            read_buf.as_mut_ptr() as *mut libc::c_void,
                            read_buf.len(),
                        )
                    };
                    if n <= 0 {
                        break;
                    }
                    let chunk = &read_buf[..n as usize];
                    if let Ok(mut b) = buffer_clone.write() {
                        b.extend_from_slice(chunk);
                        if b.len() > MAX_PTY_BUFFER_BYTES {
                            let drain_len = b.len() - MAX_PTY_BUFFER_BYTES;
                            b.drain(..drain_len);
                        }
                    }
                }
                unsafe { libc::close(read_master_fd) };

                let status = child.wait();
                if let Ok(mut r) = running_clone.write() {
                    *r = false;
                }
                if let Ok(mut ec) = exit_code_clone.write() {
                    *ec = status.ok().and_then(|s| s.code());
                }
            });

            let session = Arc::new(PtySession {
                id,
                program: program_name.to_string(),
                args: args_vec,
                pid,
                cols: Arc::new(StdRwLock::new(terminal_cols)),
                rows: Arc::new(StdRwLock::new(terminal_rows)),
                started_at: Instant::now(),
                running,
                exit_code,
                buffer,
                master_fd,
            });

            let mut sessions = self.sessions.write().await;
            sessions.insert(id, session);

            Ok(id)
        }

        #[cfg(not(unix))]
        {
            Err(ForgeError::Rejected(
                "PTY sessions are only supported on Unix platforms".into(),
            ))
        }
    }

    /// Write input or control characters to the PTY.
    pub async fn write(&self, id: u64, data: &str) -> Result<usize, ForgeError> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(&id).ok_or(ForgeError::NotFound)?;

        let is_running = *session.running.read().unwrap_or_else(|e| e.into_inner());
        if !is_running {
            return Err(ForgeError::Rejected(format!(
                "PTY session {id} is no longer running"
            )));
        }

        #[cfg(unix)]
        {
            let bytes = data.as_bytes();
            let n = unsafe {
                libc::write(
                    session.master_fd,
                    bytes.as_ptr() as *const libc::c_void,
                    bytes.len(),
                )
            };
            if n < 0 {
                Err(ForgeError::Io(format!(
                    "failed to write to PTY: errno {}",
                    std::io::Error::last_os_error()
                )))
            } else {
                Ok(n as usize)
            }
        }

        #[cfg(not(unix))]
        {
            Err(ForgeError::Rejected(
                "PTY is not supported on non-unix".into(),
            ))
        }
    }

    /// Read output buffer from the PTY session.
    pub async fn read(
        &self,
        id: u64,
        strip_ansi: bool,
        tail_bytes: Option<usize>,
        cursor: Option<usize>,
    ) -> Result<PtyReadResult, ForgeError> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(&id).ok_or(ForgeError::NotFound)?;

        let running = *session.running.read().unwrap_or_else(|e| e.into_inner());
        let exit_code = *session.exit_code.read().unwrap_or_else(|e| e.into_inner());
        let buf = session.buffer.read().unwrap_or_else(|e| e.into_inner());
        let total_bytes = buf.len();

        let start_idx = if let Some(c) = cursor {
            c.min(total_bytes)
        } else if let Some(t) = tail_bytes {
            total_bytes.saturating_sub(t)
        } else {
            0
        };

        let raw_slice = &buf[start_idx..];
        let utf8_text = String::from_utf8_lossy(raw_slice);

        let output = if strip_ansi {
            strip_ansi_codes(&utf8_text)
        } else {
            utf8_text.into_owned()
        };

        Ok(PtyReadResult {
            id,
            output,
            total_bytes,
            running,
            exit_code,
        })
    }

    /// Resize terminal rows and columns.
    pub async fn resize(&self, id: u64, cols: u16, rows: u16) -> Result<(), ForgeError> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(&id).ok_or(ForgeError::NotFound)?;

        #[cfg(unix)]
        {
            let ws = libc::winsize {
                ws_row: rows,
                ws_col: cols,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            let res = unsafe { libc::ioctl(session.master_fd, libc::TIOCSWINSZ, &ws) };
            if res != 0 {
                return Err(ForgeError::Io(format!(
                    "failed to resize PTY: errno {}",
                    std::io::Error::last_os_error()
                )));
            }
            if let Ok(mut c) = session.cols.write() {
                *c = cols;
            }
            if let Ok(mut r) = session.rows.write() {
                *r = rows;
            }
            Ok(())
        }

        #[cfg(not(unix))]
        {
            Err(ForgeError::Rejected(
                "PTY is not supported on non-unix".into(),
            ))
        }
    }

    /// Kill the PTY child and its entire process group.
    pub async fn kill(&self, id: u64) -> Result<bool, ForgeError> {
        let sessions = self.sessions.read().await;
        let session = sessions.get(&id).ok_or(ForgeError::NotFound)?;

        let is_running = *session.running.read().unwrap_or_else(|e| e.into_inner());
        if !is_running {
            return Ok(false);
        }

        kill_process_group(session.pid);
        if let Ok(mut r) = session.running.write() {
            *r = false;
        }

        Ok(true)
    }

    /// List all active PTY sessions.
    pub async fn list(&self) -> Vec<PtySummary> {
        let sessions = self.sessions.read().await;
        let mut list = Vec::new();
        for session in sessions.values() {
            let running = *session.running.read().unwrap_or_else(|e| e.into_inner());
            let exit_code = *session.exit_code.read().unwrap_or_else(|e| e.into_inner());
            let cols = *session.cols.read().unwrap_or_else(|e| e.into_inner());
            let rows = *session.rows.read().unwrap_or_else(|e| e.into_inner());
            list.push(PtySummary {
                id: session.id,
                program: session.program.clone(),
                args: session.args.clone(),
                cols,
                rows,
                running,
                exit_code,
                pid: session.pid,
                uptime_secs: session.started_at.elapsed().as_secs(),
            });
        }
        list.sort_by_key(|s| s.id);
        list
    }
}

/// Helper function to strip standard ANSI escape sequences from terminal output.
pub fn strip_ansi_codes(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut in_escape = false;
    for ch in input.chars() {
        if in_escape {
            if ch.is_alphabetic() || ch == 'm' || ch == 'H' || ch == 'J' || ch == 'K' {
                in_escape = false;
            }
        } else if ch == '\x1b' {
            in_escape = true;
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn pty_session_spawns_writes_reads_and_kills() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path().to_path_buf());
        let manager = PtyManager::new(root);

        let id = manager
            .spawn("cargo", &["--version"], Some(80), Some(24), None)
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(300)).await;

        let read_res = manager.read(id, true, None, None).await.unwrap();
        assert!(
            read_res.output.contains("cargo"),
            "PTY output should contain cargo: {}",
            read_res.output
        );

        let list = manager.list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, id);

        manager.resize(id, 100, 30).await.unwrap();

        let _ = manager.kill(id).await;
    }

    #[test]
    fn test_ansi_stripping() {
        let colored = "\x1b[31mRed Text\x1b[0m Normal";
        let stripped = strip_ansi_codes(colored);
        assert!(stripped.contains("Red Text") && stripped.contains("Normal"));
    }
}
