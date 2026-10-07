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
pub mod screenshot;
pub mod pty;
pub mod mock;
pub mod sqlite;
pub mod gate;
pub mod peers;
pub mod nucleus_lint;
pub mod spec;
pub mod e2e;
pub mod preview;
pub mod scaffold;
pub mod security_audit;
pub mod watchdog;
pub mod blast_radius;
pub mod git_bisect;
pub mod wiretap;
pub mod ast_rewrite;
pub mod secret_vault;
pub mod flamegraph;
pub mod fuzz_harness;
pub mod nucleus_graph;
pub mod binary_bloat;
pub mod release_sync;
pub mod time_travel;
pub mod mutation;
pub mod benchmark_guard;
pub mod topology;
pub mod task_scheduler;
pub mod preon_evolution;
pub mod prompt_distiller;
pub mod mesh;
pub mod pty_pairing;
pub mod breakpoints;
pub mod research;
pub mod trace_slicer;
pub mod tree_checkpoint;
pub mod vcr;
pub mod profile_runner;
pub mod dap;
pub mod pruning;
pub mod scratch;

use hadron_forge::file::Root;
use hadron_forge::mock::MockServerManager;
use hadron_forge::process::ProcessManager;
use hadron_forge::pty::PtyManager;
use hadron_forge::vcr::VcrProxyManager;
use hadron_forge::dap::DapSessionManager;
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
    pub pty_manager: PtyManager,
    pub mock_manager: MockServerManager,
    pub vcr_manager: VcrProxyManager,
    pub dap_manager: DapSessionManager,
    pub lsp_daemon: std::sync::Arc<tokio::sync::RwLock<Option<hadron_forge::lsp_daemon::LspDaemon>>>,
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
        Self::with_filter(root_path, nucleus_root, pruning::ToolFilter::all())
    }

    pub fn with_skill(
        root_path: impl Into<std::path::PathBuf>,
        nucleus_root: impl Into<std::path::PathBuf>,
        skill: &str,
    ) -> Self {
        Self::with_filter(root_path, nucleus_root, pruning::ToolFilter::from_skill(skill))
    }

    pub fn with_filter(
        root_path: impl Into<std::path::PathBuf>,
        nucleus_root: impl Into<std::path::PathBuf>,
        filter: pruning::ToolFilter,
    ) -> Self {
        let root = Root::new(root_path);
        let nucleus_root = Root::new(nucleus_root);
        let process_manager = ProcessManager::new(root.clone());
        let pty_manager = PtyManager::new(root.clone());
        let mock_manager = MockServerManager::new();
        let vcr_manager = VcrProxyManager::new();
        let dap_manager = DapSessionManager::new();
        let lsp_daemon = std::sync::Arc::new(tokio::sync::RwLock::new(None));

        let mut tool_router = Self::edit_router()
            + Self::exec_router()
            + Self::inspect_router()
            + Self::git_router()
            + Self::scratch_router();

        if filter.categories.contains(&pruning::ToolCategory::Nucleus) {
            tool_router = tool_router
                + Self::nucleus_router()
                + Self::nucleus_lint_router()
                + Self::nucleus_graph_router();
        }
        if filter.categories.contains(&pruning::ToolCategory::Diagnostics) {
            tool_router = tool_router
                + Self::diagnostics_router()
                + Self::cargo_tree_router()
                + Self::semantic_router()
                + Self::symbols_router()
                + Self::trace_slicer_router();
        }
        if filter.categories.contains(&pruning::ToolCategory::Debugger) {
            tool_router = tool_router
                + Self::dap_router()
                + Self::breakpoints_router()
                + Self::vcr_router();
        }
        if filter.categories.contains(&pruning::ToolCategory::Profiling) {
            tool_router = tool_router
                + Self::flamegraph_router()
                + Self::profile_runner_router()
                + Self::binary_bloat_router()
                + Self::benchmark_guard_router();
        }
        if filter.categories.contains(&pruning::ToolCategory::Swarm) {
            tool_router = tool_router
                + Self::peers_router()
                + Self::topology_router()
                + Self::mesh_router()
                + Self::gate_router()
                + Self::task_scheduler_router();
        }
        if filter.categories.contains(&pruning::ToolCategory::Web) {
            tool_router = tool_router
                + Self::browser_router()
                + Self::screenshot_router()
                + Self::preview_router()
                + Self::e2e_router();
        }

        tool_router = tool_router
            + Self::process_router()
            + Self::pty_router()
            + Self::mock_router()
            + Self::sqlite_router()
            + Self::spec_router()
            + Self::scaffold_router()
            + Self::security_audit_router()
            + Self::watchdog_router()
            + Self::blast_radius_router()
            + Self::git_bisect_router()
            + Self::wiretap_router()
            + Self::ast_rewrite_router()
            + Self::secret_vault_router()
            + Self::fuzz_harness_router()
            + Self::release_sync_router()
            + Self::time_travel_router()
            + Self::mutation_router()
            + Self::preon_evolution_router()
            + Self::prompt_distiller_router()
            + Self::pty_pairing_router()
            + Self::research_router()
            + Self::tree_checkpoint_router();

        Self {
            tool_router,
            root,
            nucleus_root,
            process_manager,
            pty_manager,
            mock_manager,
            vcr_manager,
            dap_manager,
            lsp_daemon,
        }
    }

    /// Retrieve or lazily initialize resident LspDaemon.
    pub async fn get_or_init_lsp_daemon(&self) -> hadron_forge::lsp_daemon::LspDaemon {
        {
            let guard = self.lsp_daemon.read().await;
            if let Some(d) = guard.as_ref() {
                return d.clone();
            }
        }
        let mut guard = self.lsp_daemon.write().await;
        if let Some(d) = guard.as_ref() {
            return d.clone();
        }
        let daemon = hadron_forge::lsp_daemon::LspDaemon::new_mock(self.root.path());
        *guard = Some(daemon.clone());
        daemon
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
        self.pty_manager = PtyManager::new(self.root.clone());
        self.mock_manager = MockServerManager::new();
        self.vcr_manager = VcrProxyManager::new();
        self.dap_manager = DapSessionManager::new();
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
