//! The **diagnostics** family: compiler errors and warnings, parsed.
//!
//! Empty until task 5 of `docs/plans/2026-07-26-forge-tool-suite.md` fills it.
//! It exists now so that family lands on its own file and never rebases against
//! another worker's.

use super::ForgeMcpServer;
use rmcp::tool_router;

#[tool_router(router = diagnostics_router, vis = "pub(super)")]
impl ForgeMcpServer {}
