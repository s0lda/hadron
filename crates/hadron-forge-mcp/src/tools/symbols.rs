//! The **symbols** family: code intelligence, caller detection, and type hierarchy across 12+ languages.
//!
//! Operationalizes Standard Model Rule 1 ("Find its caller") using Tier 1 Tree-Sitter AST inspection
//! with zero external setup, plus Tier 2 LSP integration when available.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::ast_symbols::{
    extract_file_symbols, find_symbol_callers, query_type_hierarchy,
};
use hadron_forge::file::resolve_jailed_path;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolFindCallersArgs {
    /// The exact identifier / symbol name to search for across the workspace.
    pub symbol_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolDocumentOutlineArgs {
    /// Relative path to the source file within the worktree.
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolDefinitionArgs {
    /// Relative path to the source file.
    pub path: String,
    /// 1-indexed line number.
    pub line: usize,
    /// 1-indexed column number.
    pub col: usize,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SymbolHierarchyArgs {
    /// Name of the struct, enum, trait, or class to inspect across the workspace.
    pub type_name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct LspQueryArgs {
    /// Symbol name or keyword to query across the codebase
    pub query: String,
    /// Query kind: "callers", "hierarchy", or "outline" (defaults to "callers")
    pub kind: Option<String>,
    /// Optional file path (required for outline queries)
    pub path: Option<String>,
}

#[tool_router(router = symbols_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_symbol_find_callers",
        description = "Find all callers, references, and invocations of a symbol across the workspace (Tree-Sitter Tier 1 AST analysis)."
    )]
    pub async fn symbol_find_callers(
        &self,
        Parameters(args): Parameters<SymbolFindCallersArgs>,
    ) -> Json<ToolResponse> {
        match find_symbol_callers(&self.root, &args.symbol_name) {
            Ok(callers) => {
                if callers.is_empty() {
                    Json(ToolResponse::success(Some(format!(
                        "No callers found for `{}` (implemented, unwired)",
                        args.symbol_name
                    ))))
                } else {
                    let mut out = format!(
                        "Found {} call site(s) for `{}`:\n",
                        callers.len(),
                        args.symbol_name
                    );
                    for c in &callers {
                        let caller_ctx = c
                            .caller_symbol
                            .as_deref()
                            .map(|s| format!(" [in `{s}`]"))
                            .unwrap_or_default();
                        out.push_str(&format!(
                            "- {}:{}:{}{}: {}\n",
                            c.file,
                            c.line,
                            c.col,
                            caller_ctx,
                            c.line_content.trim()
                        ));
                    }
                    Json(ToolResponse::success(Some(out)))
                }
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_symbol_hierarchy",
        description = "Query type hierarchy, struct fields, enum variants, and trait implementations across the workspace."
    )]
    pub async fn symbol_hierarchy(
        &self,
        Parameters(args): Parameters<SymbolHierarchyArgs>,
    ) -> Json<ToolResponse> {
        let root = self.root.clone();
        let type_name = args.type_name;
        let t_name = type_name.clone();

        let res = tokio::task::spawn_blocking(move || {
            query_type_hierarchy(&root, &t_name)
        })
        .await;

        match res {
            Ok(Ok(Some(hierarchy))) => {
                let json = serde_json::to_string_pretty(&hierarchy).unwrap_or_default();
                Json(ToolResponse::success(Some(json)))
            }
            Ok(Ok(None)) => Json(ToolResponse::success(Some(format!(
                "No type definition or implementation found for `{}`",
                type_name
            )))),
            Ok(Err(e)) => Json(ToolResponse::error(e.to_string())),
            Err(e) => Json(ToolResponse::error(format!("Symbol hierarchy query task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_lsp_query",
        description = "Unified code intelligence query (callers, hierarchy, or document outline)."
    )]
    pub async fn lsp_query(
        &self,
        Parameters(args): Parameters<LspQueryArgs>,
    ) -> Json<ToolResponse> {
        let kind = args.kind.unwrap_or_else(|| "callers".to_string());
        match kind.as_str() {
            "hierarchy" => {
                self.symbol_hierarchy(Parameters(SymbolHierarchyArgs {
                    type_name: args.query,
                }))
                .await
            }
            "outline" => {
                let file_path = args.path.unwrap_or(args.query);
                self.symbol_document_outline(Parameters(SymbolDocumentOutlineArgs {
                    path: file_path,
                }))
                .await
            }
            _ => {
                self.symbol_find_callers(Parameters(SymbolFindCallersArgs {
                    symbol_name: args.query,
                }))
                .await
            }
        }
    }

    #[tool(
        name = "hadron_forge_symbol_document_outline",
        description = "Extract all symbol definitions (functions, structs, classes, traits, methods) in a source file."
    )]
    pub async fn symbol_document_outline(
        &self,
        Parameters(args): Parameters<SymbolDocumentOutlineArgs>,
    ) -> Json<ToolResponse> {
        match extract_file_symbols(&self.root, &args.path) {
            Ok(symbols) => {
                if symbols.is_empty() {
                    Json(ToolResponse::success(Some(format!(
                        "No symbols found in `{}`",
                        args.path
                    ))))
                } else {
                    let mut out = format!("Outline for `{}` ({} symbols):\n", args.path, symbols.len());
                    for s in &symbols {
                        let sig = s.signature.as_deref().unwrap_or("");
                        out.push_str(&format!(
                            "- L{}:{} [{}] {}: {}\n",
                            s.line, s.col, s.kind, s.name, sig
                        ));
                    }
                    Json(ToolResponse::success(Some(out)))
                }
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_symbol_definition",
        description = "Find the definition of a symbol at the specified file, line, and column position."
    )]
    pub async fn symbol_definition(
        &self,
        Parameters(args): Parameters<SymbolDefinitionArgs>,
    ) -> Json<ToolResponse> {
        // Validate path jail
        if let Err(e) = resolve_jailed_path(&self.root, &args.path) {
            return Json(ToolResponse::error(e.to_string()));
        }

        // Tier 1 fallback: scan document outline to find matching symbol span
        match extract_file_symbols(&self.root, &args.path) {
            Ok(symbols) => {
                let matching = symbols.iter().find(|s| s.line == args.line);
                if let Some(sym) = matching {
                    Json(ToolResponse::success(Some(format!(
                        "Definition: [{}] `{}` at {}:{}:{}",
                        sym.kind, sym.name, sym.file, sym.line, sym.col
                    ))))
                } else {
                    Json(ToolResponse::success(Some(format!(
                        "No local definition found at {}:{}:{}",
                        args.path, args.line, args.col
                    ))))
                }
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[tokio::test]
    async fn mcp_symbol_tools_find_callers_and_outline() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            src_dir.join("lib.rs"),
            "pub fn merge_gate() -> bool { true }\n\npub fn orchestrate() {\n    let _ = merge_gate();\n}\n",
        )
        .unwrap();

        let server = ForgeMcpServer::new(dir.path());

        let callers_res = server
            .symbol_find_callers(Parameters(SymbolFindCallersArgs {
                symbol_name: "merge_gate".into(),
            }))
            .await;
        assert!(callers_res.0.ok);
        assert!(callers_res.0.blocks.as_ref().unwrap().contains("Found 1 call site"));
        assert!(callers_res.0.blocks.as_ref().unwrap().contains("orchestrate"));

        let outline_res = server
            .symbol_document_outline(Parameters(SymbolDocumentOutlineArgs {
                path: "src/lib.rs".into(),
            }))
            .await;
        assert!(outline_res.0.ok);
        assert!(outline_res.0.blocks.as_ref().unwrap().contains("merge_gate"));
        assert!(outline_res.0.blocks.as_ref().unwrap().contains("orchestrate"));
    }

    #[tokio::test]
    async fn mcp_symbol_hierarchy_and_lsp_query_tools() {
        let dir = tempfile::tempdir().unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            src_dir.join("models.rs"),
            r#"
pub struct User {
    pub id: u64,
    pub name: String,
}

pub trait Auth {
    fn check(&self) -> bool;
}

impl Auth for User {
    fn check(&self) -> bool { true }
}
"#,
        )
        .unwrap();

        let server = ForgeMcpServer::new(dir.path());

        let hier_res = server
            .symbol_hierarchy(Parameters(SymbolHierarchyArgs {
                type_name: "User".into(),
            }))
            .await;
        assert!(hier_res.0.ok);
        let blocks = hier_res.0.blocks.unwrap();
        assert!(blocks.contains("User"));
        assert!(blocks.contains("Auth"));
        assert!(blocks.contains("name"));

        let lsp_res = server
            .lsp_query(Parameters(LspQueryArgs {
                query: "User".into(),
                kind: Some("hierarchy".into()),
                path: None,
            }))
            .await;
        assert!(lsp_res.0.ok);
    }
}
