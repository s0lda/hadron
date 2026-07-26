//! The **git** family: read-only history and diff queries against the worktree.
//!
//! Empty until task 4 of `docs/plans/2026-07-26-forge-tool-suite.md` fills it.
//! It exists now so that family lands on its own file and never rebases against
//! another worker's.

use super::ForgeMcpServer;
use rmcp::tool_router;

#[tool_router(router = git_router, vis = "pub(super)")]
impl ForgeMcpServer {}
