//! The **dap** family: headless runtime debugger for breakpoint placement, stack inspection, and variable evaluations.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::dap::BreakpointLocation;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BreakpointSpec {
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DapArgs {
    pub action: String,
    pub session_id: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub breakpoints: Option<Vec<BreakpointSpec>>,
    pub frame_id: Option<usize>,
}

#[tool_router(router = dap_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_dap_debug",
        description = "Headless DAP/runtime debugger for breakpoint placement, execution stepping, call stack inspection, and variable evaluations"
    )]
    pub async fn dap(&self, Parameters(args): Parameters<DapArgs>) -> Json<ToolResponse> {
        let action = args.action.as_str();
        match action {
            "start_session" => {
                let cmd = match args.command {
                    Some(c) => c,
                    None => {
                        return Json(ToolResponse::error(
                            "command is required for start_session action",
                        ))
                    }
                };
                let cmd_args = args.args.unwrap_or_default();
                let bps = args
                    .breakpoints
                    .unwrap_or_default()
                    .into_iter()
                    .map(|b| BreakpointLocation {
                        file: b.file,
                        line: b.line,
                        verified: false,
                    })
                    .collect();

                match self
                    .dap_manager
                    .start_session(&self.root, &cmd, &cmd_args, bps)
                    .await
                {
                    Ok(state) => {
                        let stopped_str = match state.stopped_at_breakpoint {
                            Some(ref b) => format!("{}:{}", b.file, b.line),
                            None => "None (Running)".to_string(),
                        };
                        let text = format!(
                            "### Debug Session Started\n\n- **Session ID:** `{}`\n- **Command:** `{}`\n- **Status:** {}\n- **Stopped At:** `{}`\n- **Active Breakpoints:** {}\n",
                            state.session_id,
                            state.command,
                            if state.is_running { "Running / Paused" } else { "Terminated" },
                            stopped_str,
                            state.breakpoints.len()
                        );
                        Json(ToolResponse::success(Some(text)))
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "set_breakpoints" => {
                let sid = match args.session_id {
                    Some(s) => s,
                    None => return Json(ToolResponse::error("session_id is required")),
                };
                let bps = args
                    .breakpoints
                    .unwrap_or_default()
                    .into_iter()
                    .map(|b| BreakpointLocation {
                        file: b.file,
                        line: b.line,
                        verified: false,
                    })
                    .collect();
                match self.dap_manager.set_breakpoints(&sid, bps).await {
                    Ok(verified) => {
                        let mut text = format!(
                            "### Breakpoints Updated (Session: `{}`)\n\n",
                            sid
                        );
                        for b in verified {
                            text.push_str(&format!("- `{}:{}` (Verified)\n", b.file, b.line));
                        }
                        Json(ToolResponse::success(Some(text)))
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "continue_execution" | "continue" => {
                let sid = match args.session_id {
                    Some(s) => s,
                    None => return Json(ToolResponse::error("session_id is required")),
                };
                match self.dap_manager.continue_execution(&sid).await {
                    Ok(state) => {
                        let stopped_str = match state.stopped_at_breakpoint {
                            Some(ref b) => format!("{}:{}", b.file, b.line),
                            None => "None (Exited/Running)".to_string(),
                        };
                        let text = format!(
                            "### Execution Resumed\n\n- **Session ID:** `{}`\n- **Stopped At:** `{}`\n- **Running:** {}\n",
                            state.session_id, stopped_str, state.is_running
                        );
                        Json(ToolResponse::success(Some(text)))
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "step_next" | "step_in" | "step_out" | "step" => {
                let sid = match args.session_id {
                    Some(s) => s,
                    None => return Json(ToolResponse::error("session_id is required")),
                };
                match self.dap_manager.step_next(&sid).await {
                    Ok(state) => {
                        let stopped_str = match state.stopped_at_breakpoint {
                            Some(ref b) => format!("{}:{}", b.file, b.line),
                            None => "None".to_string(),
                        };
                        let text = format!(
                            "### Stepped (Next Frame)\n\n- **Session ID:** `{}`\n- **Current Line:** `{}`\n",
                            state.session_id, stopped_str
                        );
                        Json(ToolResponse::success(Some(text)))
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "inspect_stack" => {
                let sid = match args.session_id {
                    Some(s) => s,
                    None => return Json(ToolResponse::error("session_id is required")),
                };
                match self.dap_manager.inspect_stack(&sid).await {
                    Ok(frames) => {
                        let mut text = format!("### Call Stack Frames (Session: `{}`)\n\n", sid);
                        for f in frames {
                            let loc = match (&f.file, f.line) {
                                (Some(file), Some(line)) => format!("{}:{}", file, line),
                                _ => "unknown".to_string(),
                            };
                            text.push_str(&format!("- `#{}` **{}** at `{}`\n", f.id, f.name, loc));
                        }
                        Json(ToolResponse::success(Some(text)))
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "inspect_variables" => {
                let sid = match args.session_id {
                    Some(s) => s,
                    None => return Json(ToolResponse::error("session_id is required")),
                };
                match self.dap_manager.inspect_variables(&sid, args.frame_id).await {
                    Ok(vars) => {
                        let mut text = format!("### Local Variables (Session: `{}`)\n\n", sid);
                        for v in vars {
                            let t = v.type_name.as_deref().unwrap_or("unknown");
                            text.push_str(&format!("- `{}` (`{}`): `{}`\n", v.name, t, v.value));
                        }
                        Json(ToolResponse::success(Some(text)))
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            "terminate_session" | "terminate" => {
                let sid = match args.session_id {
                    Some(s) => s,
                    None => return Json(ToolResponse::error("session_id is required")),
                };
                match self.dap_manager.terminate_session(&sid).await {
                    Ok(term) => {
                        if term {
                            Json(ToolResponse::success(Some(format!(
                                "Terminated debug session `{sid}`."
                            ))))
                        } else {
                            Json(ToolResponse::error(format!(
                                "Debug session `{sid}` was not found"
                            )))
                        }
                    }
                    Err(e) => Json(ToolResponse::error(e.to_string())),
                }
            }
            other => Json(ToolResponse::error(format!(
                "Unknown DAP action '{}'. Expected: start_session, set_breakpoints, continue_execution, step_next, inspect_stack, inspect_variables, terminate_session",
                other
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dap_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        let res = server
            .dap(Parameters(DapArgs {
                action: "start_session".to_string(),
                session_id: None,
                command: Some("git".to_string()),
                args: Some(vec!["status".to_string()]),
                breakpoints: Some(vec![BreakpointSpec {
                    file: "src/main.rs".to_string(),
                    line: 10,
                }]),
                frame_id: None,
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Debug Session Started"));
    }
}
