//! The **exec** family: run a build, a test suite or a git query in the worktree.
//!
//! The one tool here that spawns a process. It takes a program **name** and an
//! argument **list** — never a command line — so there is no shell to interpret a
//! `;`, a pipe or a redirect, and both the name and every argument are checked
//! before anything runs.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::exec::{exec, Program, EXEC_DEADLINE};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExecArgs {
    /// The program to run. Only `cargo` and `git` are allowed.
    pub program: String,
    /// Arguments, already split — this is not a shell command line.
    #[serde(default)]
    pub args: Vec<String>,
}

#[tool_router(router = exec_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_exec",
        description = "Run `cargo` or `git` inside the worktree. Arguments are a list, not a \
                       shell command line: no pipes, redirects or globbing, and no argument \
                       may name a path outside the worktree. Bounded by a deadline; the \
                       output is the tail, capped, and a cut is always announced."
    )]
    pub async fn exec(&self, Parameters(args): Parameters<ExecArgs>) -> Json<ToolResponse> {
        let Some(program) = Program::parse(&args.program) else {
            return Json(ToolResponse::error(format!(
                "{:?} is not a program a quark may run — only `cargo` and `git`",
                args.program
            )));
        };
        match exec(&self.root, program, &args.args, EXEC_DEADLINE) {
            Ok(out) => {
                let mut body = String::new();
                if out.timed_out {
                    body.push_str("[timed out — the process group was killed]\n");
                }
                body.push_str(&format!("exit: {:?}\n", out.code));
                if !out.stdout.is_empty() {
                    body.push_str(&format!("--- stdout ---\n{}\n", out.stdout));
                }
                if !out.stderr.is_empty() {
                    body.push_str(&format!("--- stderr ---\n{}\n", out.stderr));
                }
                // A non-zero exit is a RESULT, not a tool failure: the agent asked
                // what the command does, and "it failed, here is why" is the answer.
                Json(ToolResponse::success(Some(body)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_program_off_the_allowlist_never_spawns() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .exec(Parameters(ExecArgs {
                program: "sh".into(),
                args: vec!["-c".into(), "touch pwned".into()],
            }))
            .await;
        assert!(!res.0.ok);
        assert!(res.0.reason.as_ref().unwrap().contains("only `cargo` and `git`"));
        assert!(!dir.path().join("pwned").exists(), "nothing may have run");
    }

    #[tokio::test]
    async fn a_failing_command_is_a_result_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .exec(Parameters(ExecArgs {
                program: "git".into(),
                args: vec!["status".into()],
            }))
            .await;
        assert!(res.0.ok, "the tool call succeeded even though git did not");
        assert!(res.0.blocks.as_ref().unwrap().contains("exit:"));
    }
}
