//! The **task_scheduler** tool: parse markdown plan dependencies and compute execution waves.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::task_scheduler::PlanTaskScheduler;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TaskSchedulerArgs {
    pub plan_markdown: String,
}

#[tool_router(router = task_scheduler_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_task_scheduler",
        description = "Parse implementation plan markdown tasks and compute topological execution waves for multi-quark dispatch"
    )]
    pub async fn task_scheduler(
        &self,
        Parameters(args): Parameters<TaskSchedulerArgs>,
    ) -> Json<ToolResponse> {
        let summary = PlanTaskScheduler::summarize_plan(&args.plan_markdown);
        match serde_json::to_string_pretty(&summary) {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn task_scheduler_tool_handler_runs() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .task_scheduler(Parameters(TaskSchedulerArgs {
                plan_markdown: "- [ ] Task 1\n- [x] Task 2".into(),
            }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("total_tasks"));
    }
}
