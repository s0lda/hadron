//! The **inspect** family: read a slice of a file, list a directory, search the
//! tree — everything a quark needs to look around without a shell.
//!
//! Reserved by the forge tool-suite plan; the tools land here.

use super::ForgeMcpServer;
use rmcp::tool_router;

#[tool_router(router = inspect_router, vis = "pub(super)")]
impl ForgeMcpServer {}
