//! Run a command for a jailed agent: bounded, group-killed, and output-capped.
//!
//! **What this is a boundary against.** Accidents, not adversaries. `cargo`
//! compiles and runs code from the tree it is pointed at, so a quark that can run
//! `cargo test` can already run anything that repo's own `build.rs` says — no
//! allowlist changes that. What the allowlist, the argument guard and the pinned
//! working directory do buy is that a *mistaken* command cannot wander out of the
//! worktree: there is no shell, so no `;`, no `|`, no `>`, no globbing, and an
//! argument naming a path outside the jail is refused before anything spawns.
//!
//! The deadline and the process-**group** kill are the parts that have already
//! paid for themselves elsewhere in this project: `cargo` is a launcher, and
//! killing the launcher alone once orphaned a test binary for four CPU-hours.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use crate::file::{ForgeError, Root};

/// How long a jailed command may run before its whole process group is killed.
///
/// Matches `hadron-gluon`'s `GATE_TEST_DEADLINE`: the longest thing a quark runs
/// through here is the same test suite the merge gate runs.
pub const EXEC_DEADLINE: Duration = Duration::from_secs(15 * 60);

/// How much of a command's output travels back to the agent.
///
/// Output is the expensive half of a tool call — every byte lands in the model's
/// context and is re-read on every later turn. The tail is what carries the
/// failure, so the tail is what survives; the cut is always announced.
pub const OUTPUT_BUDGET_BYTES: usize = 16 * 1024;

/// A program a jailed agent is allowed to run.
///
/// An enum, not a string: "is this program allowed" is then a question the
/// compiler answers at every call site rather than a check someone can forget.
/// Deliberately the minimum that lets a quark work in this repo — build, test and
/// inspect. Widen it when a real seat needs more, not before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Program {
    Cargo,
    Git,
}

impl Program {
    pub fn as_str(self) -> &'static str {
        match self {
            Program::Cargo => "cargo",
            Program::Git => "git",
        }
    }

    /// Parse the program an agent asked for. `None` means "not on the allowlist",
    /// which is the only way this type can be constructed from untrusted input.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "cargo" => Some(Program::Cargo),
            "git" => Some(Program::Git),
            _ => None,
        }
    }
}

/// True when an argument cannot point the command out of the jail.
///
/// There is no shell, so the only way out is an argument that names somewhere
/// else — `git -C /etc`, `cargo --manifest-path ../../other/Cargo.toml`. The rule
/// is deliberately blunt and covers both the bare and the `--flag=value` forms:
/// **no absolute paths, no `..` component, and no working-directory flag.**
///
/// A relative path is fine; `resolve_jailed_path` is what the file tools use, and
/// a command's own relative paths resolve against the pinned `current_dir`.
pub fn arg_is_jailed(arg: &str) -> bool {
    // `-C <dir>` and `--work-tree`/`--git-dir` re-root git wholesale, and the
    // directory may arrive as a *separate* argument that looks relative on its
    // own. Refuse the flag itself rather than trying to pair it with its value.
    const REROOTING: [&str; 4] = ["-C", "--git-dir", "--work-tree", "--manifest-path"];
    let (flag, value) = match arg.split_once('=') {
        Some((f, v)) => (f, v),
        None => (arg, arg),
    };
    if REROOTING.contains(&flag) {
        return false;
    }
    let path = Path::new(value);
    if path.is_absolute() {
        return false;
    }
    !path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
}

/// What a bounded command did.
///
/// Carries an exit **code** rather than an `ExitStatus` because a run cut short
/// by the deadline has no status to report — and a type that can represent
/// "finished with status X" for a process we killed is a type that lies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundedOutput {
    /// `None` when the process was killed by a signal — including our own
    /// deadline kill — rather than exiting on its own.
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

impl BoundedOutput {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }
    pub fn stderr_lossy(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(&self.stderr)
    }
}

/// What a jailed command did, as the agent sees it: text, capped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutput {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Keep the **last** `budget` bytes, cut on a character boundary, and say so.
///
/// Never a silent truncation: a model that cannot tell it is missing something
/// costs far more than the bytes ever did.
pub fn tail_within_budget(s: &str, budget: usize) -> String {
    if s.len() <= budget {
        return s.to_string();
    }
    let cut = s.len() - budget;
    // Land on a char boundary — a byte slice through the middle of a multi-byte
    // character panics (this project has paid for that one already).
    let start = s
        .char_indices()
        .map(|(i, _)| i)
        .find(|&i| i >= cut)
        .unwrap_or(s.len());
    format!(
        "[… {} earlier bytes dropped — this is the tail]\n{}",
        start,
        &s[start..]
    )
}

/// Signal the child's whole process **group**.
///
/// The single home for this: `cargo` and `git` are both launchers, so killing the
/// leader alone leaves its children running with nobody waiting on them.
#[cfg(unix)]
pub fn kill_process_group(pid: u32) {
    // Safety: `kill(2)` with a negative pid signals the process group. `pid` came
    // from a child spawned with `process_group(0)`, so it leads its own group and
    // the signal cannot reach the caller or anything else.
    unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
}

#[cfg(not(unix))]
pub fn kill_process_group(_pid: u32) {}

/// Run `cmd` under a wall-clock `deadline`, killing its process group on expiry.
///
/// The wait happens on a worker thread via `wait_with_output`, which drains both
/// pipes — polling `try_wait` on this thread instead would deadlock on a child
/// that fills one.
pub fn run_bounded(
    mut cmd: Command,
    deadline: Duration,
    label: &str,
) -> std::io::Result<BoundedOutput> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let child = cmd.spawn()?;
    let pid = child.id();

    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    match rx.recv_timeout(deadline) {
        Ok(out) => out.map(|o| BoundedOutput {
            code: o.status.code(),
            stdout: o.stdout,
            stderr: o.stderr,
            timed_out: false,
        }),
        Err(_) => {
            kill_process_group(pid);
            // A timeout is a RESULT, not an error: the caller wants to know the run
            // was cut, not an exception to unwind through. An `Err` here once traded
            // a wedged gate for a hot loop.
            Ok(BoundedOutput {
                code: None,
                stdout: Vec::new(),
                stderr: format!(
                    "{label} timed out after {deadline:?} and its process group was killed"
                )
                .into_bytes(),
                timed_out: true,
            })
        }
    }
}

/// Run an allowlisted program inside `root`, bounded and output-capped.
pub fn exec(
    root: &Root,
    program: Program,
    args: &[String],
    deadline: Duration,
) -> Result<ExecOutput, ForgeError> {
    if let Some(bad) = args.iter().find(|a| !arg_is_jailed(a)) {
        return Err(ForgeError::Rejected(format!(
            "argument {bad:?} would point the command outside the worktree"
        )));
    }
    // Canonicalise: `current_dir` must be the real root, not a symlink to it.
    let cwd = root
        .path()
        .canonicalize()
        .map_err(|e| ForgeError::Io(e.to_string()))?;

    let mut cmd = Command::new(program.as_str());
    cmd.args(args).current_dir(&cwd);
    // No terminal here either — a credential prompt would wait forever.
    cmd.env("GIT_TERMINAL_PROMPT", "0");

    let label = format!("{} {:?}", program.as_str(), args);
    let out = run_bounded(cmd, deadline, &label).map_err(|e| ForgeError::Io(e.to_string()))?;

    Ok(ExecOutput {
        code: out.code,
        stdout: tail_within_budget(&String::from_utf8_lossy(&out.stdout), OUTPUT_BUDGET_BYTES),
        stderr: tail_within_budget(&String::from_utf8_lossy(&out.stderr), OUTPUT_BUDGET_BYTES),
        timed_out: out.timed_out,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_allowlist_parses() {
        assert_eq!(Program::parse("cargo"), Some(Program::Cargo));
        assert_eq!(Program::parse(" git "), Some(Program::Git));
        for denied in ["sh", "bash", "python3", "sed", "tee", "rm", "cargo;rm", ""] {
            assert_eq!(Program::parse(denied), None, "{denied:?} must not be runnable");
        }
    }

    #[test]
    fn an_argument_may_not_leave_the_worktree() {
        for ok in ["status", "--workspace", "src/lib.rs", "--message-format=json", "-p", "test"] {
            assert!(arg_is_jailed(ok), "{ok:?} is ordinary");
        }
        for bad in [
            "/etc/passwd",
            "../outside",
            "src/../../outside",
            "-C",
            "--git-dir",
            "--git-dir=/home/other/.git",
            "--work-tree=/",
            "--manifest-path=../../other/Cargo.toml",
        ] {
            assert!(!arg_is_jailed(bad), "{bad:?} must be refused");
        }
    }

    #[test]
    fn a_rerooting_argument_is_refused_before_anything_spawns() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let err = exec(
            &root,
            Program::Git,
            &["-C".into(), "/etc".into()],
            Duration::from_secs(5),
        );
        assert!(matches!(err, Err(ForgeError::Rejected(_))), "got {err:?}");
    }

    #[test]
    fn a_command_runs_in_the_root_and_reports_its_code() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        // `git status` outside a repo exits non-zero — enough to prove we spawned,
        // waited, and carried the exit code back without needing a repo fixture.
        let out = exec(&root, Program::Git, &["status".into()], Duration::from_secs(30)).unwrap();
        assert!(!out.timed_out);
        assert!(out.code.is_some(), "the process exited on its own");
    }

    #[test]
    fn a_tail_is_cut_on_a_char_boundary_and_announced() {
        let s = format!("{}é{}", "a".repeat(100), "b".repeat(100));
        let cut = tail_within_budget(&s, 50);
        assert!(cut.starts_with("[… "), "the cut is announced: {cut:?}");
        assert!(cut.ends_with("bbbb"), "the TAIL survives");
        assert!(s.len() > 50 && cut.len() < s.len() + 60);
        // Short input is returned untouched, with no notice.
        assert_eq!(tail_within_budget("short", 50), "short");
    }

    #[test]
    fn a_command_that_outlives_its_deadline_is_killed_and_reported() {
        // Driven through `run_bounded` directly: `sleep` is not on the allowlist
        // (correctly), and the deadline path is what is under test, not `exec`'s
        // argument guard.
        let mut cmd = Command::new("sleep");
        cmd.arg("30");
        let out = run_bounded(cmd, Duration::from_millis(200), "sleep").unwrap();
        assert!(out.timed_out, "the deadline must fire");
        assert_eq!(out.code, None, "a killed process reports no exit code");
        assert!(out.stderr_lossy().contains("process group was killed"));
    }
}
