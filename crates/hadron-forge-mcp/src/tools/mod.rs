//! The MCP surface hadron-forge exposes to a seated agent.
//!
//! One module per **family** of tools, each contributing its own
//! [`ToolRouter`] which [`ForgeMcpServer::tool_router`] adds together. The split
//! is not cosmetic: it is what lets separate families be written, reviewed and
//! landed independently without three branches all editing one file.

pub mod edit;
pub mod exec;
pub mod inspect;
pub mod nucleus;

use hadron_forge::file::Root;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::schemars::JsonSchema;
use rmcp::{tool_handler, ServerHandler};
use serde::Serialize;

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ForgeMcpServer {}

#[derive(Clone)]
pub struct ForgeMcpServer {
    tool_router: ToolRouter<Self>,
    pub root: Root,
    pub nucleus_root: Root,
}

impl ForgeMcpServer {
    pub fn new(root_path: impl Into<std::path::PathBuf>) -> Self {
        let root_pb = root_path.into();
        let nucleus = hadron_forge::nucleus::derive_nucleus_root()
            .unwrap_or_else(|_| Root::new(root_pb.join(".hadron").join("nucleus")));
        Self::with_nucleus(root_pb, nucleus.path())
    }

    pub fn with_nucleus(
        root_path: impl Into<std::path::PathBuf>,
        nucleus_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        Self {
            tool_router: Self::edit_router()
                + Self::exec_router()
                + Self::inspect_router()
                + Self::nucleus_router(),
            root: Root::new(root_path),
            nucleus_root: Root::new(nucleus_root),
        }
    }
}

/// What every forge tool answers with: a flag, the payload, and — when it
/// refused — the reason, in the agent's own words rather than a status code.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ToolResponse {
    pub fn success(blocks: Option<String>) -> Self {
        Self {
            ok: true,
            blocks,
            reason: None,
        }
    }
    pub fn error(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            blocks: None,
            reason: Some(reason.into()),
        }
    }
}
