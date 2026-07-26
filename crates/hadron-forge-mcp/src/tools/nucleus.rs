//! The **nucleus** family: search the swarm's shared lessons and invariants.
//!
//! Read-only, and rooted **outside** the worktree jail: the nucleus lives at
//! `<repo>/.hadron/nucleus`, which no quark's `Root` can reach. Reserved by the
//! forge tool-suite plan; the second root arrives with the tools.

use super::ForgeMcpServer;
use rmcp::tool_router;

#[tool_router(router = nucleus_router, vis = "pub(super)")]
impl ForgeMcpServer {}
