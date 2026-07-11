use async_trait::async_trait;
use hadron_lattice::TurnOutcome;

/// A single CLI invocation: the program, its args, and the text piped to stdin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: String,
}

/// The result of one invocation. Kept CLI-agnostic: session ids and other
/// per-CLI structure are parsed by the adapter (e.g. `ClaudeQuark`) from
/// `stdout`, not here — a generic runner cannot know a CLI's output shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliResult {
    pub stdout: String,
    pub exit: i32,
}

/// The single seam where a subprocess is spawned. Faked in tests; real only in
/// production and `#[ignore]`d smoke tests.
#[async_trait]
pub trait CliRunner: Send + Sync {
    async fn run(&self, inv: CliInvocation) -> anyhow::Result<CliResult>;
}

/// Map a CLI result's stdout to a turn outcome: trimmed non-empty → a message,
/// empty/whitespace → no message (a silent turn).
pub fn reply_to_outcome(result: &CliResult) -> TurnOutcome {
    let trimmed = result.stdout.trim();
    if trimmed.is_empty() {
        TurnOutcome { message: None, used_tokens: 0, permission: None }
    } else {
        TurnOutcome { message: Some(trimmed.to_string()), used_tokens: 0, permission: None }
    }
}

/// Production runner over `tokio::process::Command`. Not unit-tested (it spawns
/// real processes); covered by Task 6's `#[ignore]`d live tests.
pub struct ProcessRunner;

#[async_trait]
impl CliRunner for ProcessRunner {
    async fn run(&self, inv: CliInvocation) -> anyhow::Result<CliResult> {
        use std::process::Stdio;
        use tokio::io::AsyncWriteExt;

        let mut child = tokio::process::Command::new(&inv.program)
            .args(&inv.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to spawn {}: {e}", inv.program))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(inv.stdin.as_bytes()).await?;
            stdin.shutdown().await?; // close stdin so the CLI proceeds
        }

        let output = child.wait_with_output().await?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        // A nonzero exit is a real failure (expired auth, rate-limit, bad flag).
        // Surface stderr as the error rather than silently returning empty stdout
        // — otherwise the loop just stalls with no diagnostic signal.
        if !output.status.success() {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            return Err(anyhow::anyhow!(
                "{} exited with {code}: {}",
                inv.program,
                stderr.trim()
            ));
        }

        Ok(CliResult { stdout, exit: output.status.code().unwrap_or(0) })
    }
}

/// A deterministic runner for tests: returns queued replies in order and records
/// every invocation it received.
#[cfg(test)]
pub struct FakeRunner {
    replies: std::sync::Mutex<std::collections::VecDeque<CliResult>>,
    pub recorded: std::sync::Mutex<Vec<CliInvocation>>,
}

#[cfg(test)]
impl FakeRunner {
    pub fn new(replies: Vec<CliResult>) -> Self {
        FakeRunner {
            replies: std::sync::Mutex::new(replies.into_iter().collect()),
            recorded: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Convenience: a runner that returns one plain-stdout reply per turn.
    pub fn with_stdout(stdouts: Vec<&str>) -> Self {
        Self::new(
            stdouts
                .into_iter()
                .map(|s| CliResult { stdout: s.to_string(), exit: 0 })
                .collect(),
        )
    }
}

#[cfg(test)]
#[async_trait]
impl CliRunner for FakeRunner {
    async fn run(&self, inv: CliInvocation) -> anyhow::Result<CliResult> {
        self.recorded.lock().unwrap().push(inv);
        let reply = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow::anyhow!("FakeRunner ran out of queued replies"))?;
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_maps_stdout_to_message() {
        let some = reply_to_outcome(&CliResult { stdout: "  hello  ".into(), exit: 0 });
        assert_eq!(some.message.as_deref(), Some("hello"));
        let none = reply_to_outcome(&CliResult { stdout: "   \n ".into(), exit: 0 });
        assert_eq!(none.message, None);
    }

    // ProcessRunner tests use standard Unix tools (cat/sh), never a real CLI —
    // local, free, deterministic. They de-risk the actual subprocess plumbing
    // (stdin piping, stdout capture, nonzero-exit handling) before any live run.
    #[cfg(unix)]
    #[tokio::test]
    async fn process_runner_pipes_stdin_to_stdout() {
        let out = ProcessRunner
            .run(CliInvocation {
                program: "cat".into(),
                args: vec![],
                stdin: "hello world".into(),
            })
            .await
            .unwrap();
        assert_eq!(out.stdout, "hello world");
        assert_eq!(out.exit, 0);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn process_runner_errors_on_nonzero_with_stderr() {
        let err = ProcessRunner
            .run(CliInvocation {
                program: "sh".into(),
                args: vec!["-c".into(), "echo boom >&2; exit 3".into()],
                stdin: String::new(),
            })
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("boom"), "stderr should surface in the error: {msg}");
        assert!(msg.contains('3'), "exit code should surface: {msg}");
    }

    #[tokio::test]
    async fn fake_runner_returns_in_order_and_records() {
        let runner = FakeRunner::with_stdout(vec!["first", "second"]);
        let r1 = runner
            .run(CliInvocation { program: "claude".into(), args: vec!["-p".into()], stdin: "a".into() })
            .await
            .unwrap();
        let r2 = runner
            .run(CliInvocation { program: "agy".into(), args: vec![], stdin: "b".into() })
            .await
            .unwrap();
        assert_eq!(r1.stdout, "first");
        assert_eq!(r2.stdout, "second");

        let recorded = runner.recorded.lock().unwrap();
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].program, "claude");
        assert_eq!(recorded[0].stdin, "a");
        assert_eq!(recorded[1].program, "agy");
    }
}
