//! The **cargo_tree** family: workspace dependencies and feature flags.
//!
//! Empty until task 6 of `docs/plans/2026-07-26-forge-tool-suite.md` fills it.
//! It exists now so that family lands on its own file and never rebases against
//! another worker's.

use super::ForgeMcpServer;
use rmcp::tool_router;

#[tool_router(router = cargo_tree_router, vis = "pub(super)")]
impl ForgeMcpServer {}
