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

/// Language servers tried by [`LspDaemon::discover_and_spawn`], in order: the project
/// marker file that selects it, the binary, and its stdio arguments.
const LSP_CANDIDATES: &[(&[&str], &str, &[&str])] = &[
    (&["Cargo.toml"], "rust-analyzer", &[]),
    (&["tsconfig.json", "package.json"], "typescript-language-server", &["--stdio"]),
    (&["pyproject.toml"], "pyright-langserver", &["--stdio"]),
    (&["go.mod"], "gopls", &[]),
    (&["compile_commands.json"], "clangd", &[]),
];

/// The first executable named `bin` on `PATH` (trying `.exe`/`.cmd` on Windows).
fn find_on_path(bin: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) { &[".exe", ".cmd", ""] } else { &[""] };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{bin}{ext}")))
            .find(|p| p.is_file())
    })
}

impl LspDaemon {
    /// Start the language server matching the workspace's project markers, if one is
    /// installed on `PATH`. `None` when no marker matches, no server is installed, or
    /// every matching server failed to start — callers fall back to Tier-1 AST lookups.
    pub async fn discover_and_spawn(root: &Root) -> Option<Self> {
        for (markers, bin, args) in LSP_CANDIDATES {
            if !markers.iter().any(|m| root.path().join(m).is_file()) {
                continue;
            }
            let Some(exe) = find_on_path(bin) else { continue };
            if let Ok(daemon) = Self::spawn(&exe.to_string_lossy(), args, root).await {
                return Some(daemon);
            }
        }
        None
    }

    /// Creates a mock resident LSP daemon for unit tests.
    #[cfg(test)]
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
        assert_eq!(daemon.workspace_root(), Path::new("/mock/workspace"));
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

    #[tokio::test]
    async fn discover_and_spawn_is_none_without_a_project_marker() {
        let dir = tempfile::tempdir().unwrap();
        assert!(LspDaemon::discover_and_spawn(&Root::new(dir.path())).await.is_none());
    }

    /// End-to-end against a real `rust-analyzer` on `PATH`:
    /// `cargo test -p hadron-forge -- --ignored real_rust_analyzer`.
    #[tokio::test]
    #[ignore = "needs rust-analyzer on PATH and indexes a crate (slow)"]
    async fn real_rust_analyzer_resolves_a_definition() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn target() -> u8 { 1 }\n\npub fn caller() -> u8 {\n    target()\n}\n",
        )
        .unwrap();

        let daemon = LspDaemon::discover_and_spawn(&Root::new(dir.path()))
            .await
            .expect("rust-analyzer should start for a Cargo.toml workspace");
        // Indexing is asynchronous: poll until the server answers with a location.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(90);
        loop {
            if let Ok(locs) = daemon.goto_definition("src/lib.rs", 4, 6).await {
                if let Some(loc) = locs.first() {
                    assert_eq!(loc.file, "src/lib.rs");
                    assert_eq!(loc.start_line, 1);
                    return;
                }
            }
            assert!(std::time::Instant::now() < deadline, "no definition within 90s");
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    }
}
