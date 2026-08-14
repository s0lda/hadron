//! The MCP surface hadron-forge exposes to a seated agent.
//!
//! One module per **family** of tools, each contributing its own
//! [`ToolRouter`] which [`ForgeMcpServer::tool_router`] adds together. The split
//! is not cosmetic: it is what lets separate families be written, reviewed and
//! landed independently without three branches all editing one file.

pub mod browser;
pub mod cargo_tree;
pub mod diagnostics;
pub mod edit;
pub mod exec;
pub mod git;
pub mod inspect;
pub mod nucleus;
pub mod process;
pub mod semantic;
pub mod symbols;

use hadron_forge::file::Root;
use hadron_forge::process::ProcessManager;
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
    pub process_manager: ProcessManager,
}

impl ForgeMcpServer {
    pub fn new(root_path: impl Into<std::path::PathBuf>) -> Self {
        let root_pb = root_path.into();
        // The nucleus follows the PROJECT, not this binary. A non-git project keeps the
        // plain fallback — that is the correct root there, not a degraded one.
        let nucleus = hadron_forge::nucleus::derive_nucleus_root(&root_pb)
            .unwrap_or_else(|_| Root::new(root_pb.join(".hadron").join("nucleus")));
        Self::with_nucleus(root_pb, nucleus.path())
    }

    pub fn with_nucleus(
        root_path: impl Into<std::path::PathBuf>,
        nucleus_root: impl Into<std::path::PathBuf>,
    ) -> Self {
        let root = Root::new(root_path);
        let nucleus_root = Root::new(nucleus_root);
        let process_manager = ProcessManager::new(root.clone());
        Self {
            tool_router: Self::edit_router()
                + Self::exec_router()
                + Self::inspect_router()
                + Self::nucleus_router()
                + Self::git_router()
                + Self::diagnostics_router()
                + Self::cargo_tree_router()
                + Self::process_router()
                + Self::semantic_router()
                + Self::symbols_router()
                + Self::browser_router(),
            root,
            nucleus_root,
            process_manager,
        }
    }

    /// Grant external roots to the **project** root only.
    ///
    /// `nucleus_root` is deliberately left alone: the nucleus jail answers a different
    /// question ("which knowledge directory") and widening it would be a second, silent
    /// escape hatch nobody asked for.
    pub fn allowing_external(
        mut self,
        roots: impl IntoIterator<Item = hadron_forge::file::ExternalRoot>,
    ) -> Self {
        for root in roots {
            self.root = self.root.allowing(root);
        }
        self.process_manager = ProcessManager::new(self.root.clone());
        self
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
