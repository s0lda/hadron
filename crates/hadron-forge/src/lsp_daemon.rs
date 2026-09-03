//! Resident Rust-Analyzer / LSP Service Supervisor.
//!
//! Manages long-lived background language server processes (`rust-analyzer`, `pyright`, `tsserver`),
//! providing zero-token semantic symbol navigation, type definitions, and call hierarchies.

use std::path::{Path, PathBuf};
use crate::file::{ForgeError, Root};
use crate::lsp::{GenericLspClient, LspLocation, LspSymbol};

#[derive(Clone)]
pub struct LspDaemon {
    client: GenericLspClient,
    workspace_root: PathBuf,
}

impl LspDaemon {
    /// Creates a mock resident LSP daemon for unit tests and offline environments.
    pub fn new_mock(workspace_root: impl AsRef<Path>) -> Self {
        Self {
            client: GenericLspClient::new_mock(),
            workspace_root: workspace_root.as_ref().to_path_buf(),
        }
    }

    /// Spawns a real language server daemon subprocess with stdio JSON-RPC 2.0 pipes.
    pub async fn spawn(
        server_bin: &str,
        server_args: &[&str],
        root: &Root,
    ) -> Result<Self, ForgeError> {
        let client = GenericLspClient::spawn(server_bin, server_args, root).await?;
        Ok(Self {
            client,
            workspace_root: root.path().to_path_buf(),
        })
    }

    pub fn workspace_root(&self) -> &Path {
        &self.workspace_root
    }

    /// Queries the definition of a symbol at `rel_path:line:col`.
    pub async fn goto_definition(
        &self,
        rel_path: &str,
        line: usize,
        col: usize,
    ) -> Result<Vec<LspLocation>, ForgeError> {
        self.client.query_definition(rel_path, line, col).await
    }

    /// Queries all references to a symbol at `rel_path:line:col`.
    pub async fn find_references(
        &self,
        rel_path: &str,
        line: usize,
        col: usize,
    ) -> Result<Vec<LspLocation>, ForgeError> {
        self.client.query_references(rel_path, line, col, true).await
    }

    /// Retrieves document symbols / outlines for a given file.
    pub async fn document_symbols(&self, rel_path: &str) -> Result<Vec<LspSymbol>, ForgeError> {
        self.client.query_document_symbols(rel_path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lsp_daemon_mock_queries() {
        let daemon = LspDaemon::new_mock("/mock/workspace");
        let defs = daemon.goto_definition("src/lib.rs", 10, 4).await.unwrap();
        assert!(!defs.is_empty());
        assert!(defs[0].file.contains("src/lib.rs"));

        let refs = daemon.find_references("src/lib.rs", 10, 4).await.unwrap();
        assert!(!refs.is_empty());
        assert!(refs[0].file.contains("src/main.rs"));

        let symbols = daemon.document_symbols("src/lib.rs").await.unwrap();
        assert!(!symbols.is_empty());
        assert_eq!(symbols[0].name, "calculate_hash");
    }
}
