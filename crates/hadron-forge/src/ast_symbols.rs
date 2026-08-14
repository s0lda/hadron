//! Tier 1 Tree-Sitter multi-language symbol extraction and caller detection engine.
//!
//! Provides zero-config AST-level symbol outline and caller discovery across 12+ programming languages.
//! Directly operationalizes Standard Model Rule 1 ("Find its caller") without requiring external LSP daemons.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::file::{resolve_jailed_path, ForgeError, Root};
use crate::lang::{lang_for_path, Lang};

/// A defined symbol extracted from source code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolDefinition {
    pub name: String,
    pub kind: String,
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub signature: Option<String>,
}

/// A site where a symbol is referenced or invoked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolCallSite {
    pub caller_symbol: Option<String>,
    pub file: String,
    pub line: usize,
    pub col: usize,
    pub line_content: String,
}

fn tree_sitter_language_for(lang: Lang) -> Option<tree_sitter::Language> {
    match lang {
        Lang::Rust => Some(tree_sitter_rust::LANGUAGE.into()),
        Lang::Python => Some(tree_sitter_python::LANGUAGE.into()),
        Lang::TypeScript => Some(tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()),
        Lang::Go => Some(tree_sitter_go::LANGUAGE.into()),
        Lang::C => Some(tree_sitter_c::LANGUAGE.into()),
        Lang::Cpp => Some(tree_sitter_cpp::LANGUAGE.into()),
        Lang::Java => Some(tree_sitter_java::LANGUAGE.into()),
        Lang::CSharp => Some(tree_sitter_c_sharp::LANGUAGE.into()),
        Lang::JavaScript => Some(tree_sitter_javascript::LANGUAGE.into()),
        Lang::Ruby => Some(tree_sitter_ruby::LANGUAGE.into()),
        Lang::Php => Some(tree_sitter_php::LANGUAGE_PHP.into()),
        Lang::Html => Some(tree_sitter_html::LANGUAGE.into()),
        Lang::Css => Some(tree_sitter_css::LANGUAGE.into()),
        Lang::Sql | Lang::Opaque => None,
    }
}

/// Extract all symbol definitions from a source string for the given language.
pub fn extract_source_symbols(lang: Lang, rel_path: &str, source: &str) -> Vec<SymbolDefinition> {
    let Some(language) = tree_sitter_language_for(lang) else {
        return Vec::new();
    };

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }

    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };

    let mut symbols = Vec::new();
    collect_definitions(tree.root_node(), source, rel_path, lang, &mut symbols);
    symbols
}

fn collect_definitions(
    node: Node,
    source: &str,
    file: &str,
    lang: Lang,
    symbols: &mut Vec<SymbolDefinition>,
) {
    let kind_str = node.kind();
    let is_def = match lang {
        Lang::Rust => matches!(
            kind_str,
            "function_item" | "struct_item" | "enum_item" | "trait_item" | "impl_item" | "type_item"
        ),
        Lang::Python => matches!(kind_str, "function_definition" | "class_definition"),
        Lang::TypeScript | Lang::JavaScript => matches!(
            kind_str,
            "function_declaration"
                | "method_definition"
                | "class_declaration"
                | "interface_declaration"
                | "type_alias_declaration"
                | "enum_declaration"
        ),
        Lang::Go => matches!(
            kind_str,
            "function_declaration" | "method_declaration" | "type_declaration"
        ),
        Lang::C | Lang::Cpp => matches!(
            kind_str,
            "function_definition" | "class_specifier" | "struct_specifier" | "enum_specifier"
        ),
        Lang::Java | Lang::CSharp => matches!(
            kind_str,
            "method_declaration"
                | "constructor_declaration"
                | "class_declaration"
                | "interface_declaration"
                | "enum_declaration"
                | "struct_declaration"
        ),
        Lang::Ruby => matches!(kind_str, "method" | "singleton_method" | "class" | "module"),
        Lang::Php => matches!(kind_str, "function_definition" | "method_declaration" | "class_declaration"),
        _ => false,
    };

    if is_def {
        let name = node
            .child_by_field_name("name")
            .map(|n| source[n.byte_range()].to_string())
            .or_else(|| {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "identifier" || child.kind() == "type_identifier" {
                        return Some(source[child.byte_range()].to_string());
                    }
                }
                None
            });

        if let Some(sym_name) = name {
            let start = node.start_position();
            let sig_end = node.byte_range().end.min(node.byte_range().start + 120);
            let first_line = source[node.byte_range().start..sig_end]
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_string();

            symbols.push(SymbolDefinition {
                name: sym_name,
                kind: simplify_kind(kind_str),
                file: file.to_string(),
                line: start.row + 1,
                col: start.column + 1,
                signature: if first_line.is_empty() { None } else { Some(first_line) },
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_definitions(child, source, file, lang, symbols);
    }
}

fn simplify_kind(kind: &str) -> String {
    match kind {
        "function_item" | "function_definition" | "function_declaration" | "method" => "function".into(),
        "method_definition" | "method_declaration" | "singleton_method" => "method".into(),
        "struct_item" | "struct_specifier" | "struct_declaration" => "struct".into(),
        "class_definition" | "class_declaration" | "class_specifier" | "class" => "class".into(),
        "enum_item" | "enum_declaration" | "enum_specifier" => "enum".into(),
        "trait_item" | "interface_declaration" => "trait".into(),
        "impl_item" => "impl".into(),
        "type_item" | "type_alias_declaration" | "type_declaration" => "type".into(),
        other => other.to_string(),
    }
}

/// Find all caller invocations or references to `target_symbol` in the given source text.
pub fn find_callers_in_source(
    lang: Lang,
    rel_path: &str,
    source: &str,
    target_symbol: &str,
) -> Vec<SymbolCallSite> {
    let Some(language) = tree_sitter_language_for(lang) else {
        return Vec::new();
    };

    let mut parser = Parser::new();
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }

    let Some(tree) = parser.parse(source, None) else {
        return Vec::new();
    };

    let lines: Vec<&str> = source.lines().collect();
    let mut call_sites = Vec::new();

    walk_callers(
        tree.root_node(),
        source,
        rel_path,
        target_symbol,
        None,
        &lines,
        &mut call_sites,
    );

    call_sites
}

fn walk_callers(
    node: Node,
    source: &str,
    file: &str,
    target: &str,
    current_enclosing: Option<&str>,
    lines: &[&str],
    call_sites: &mut Vec<SymbolCallSite>,
) {
    let kind = node.kind();

    // Check if this node establishes an enclosing scope (function/method)
    let is_scope = matches!(
        kind,
        "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_definition"
            | "method_declaration"
            | "method"
    );

    let scope_name = if is_scope {
        node.child_by_field_name("name").map(|n| &source[n.byte_range()])
    } else {
        None
    };

    let active_enclosing = scope_name.or(current_enclosing);

    // Check if this node is a reference/call to the target symbol
    let is_ident = matches!(
        kind,
        "identifier" | "field_identifier" | "property_identifier" | "type_identifier"
    );

    if is_ident {
        let node_text = &source[node.byte_range()];
        if node_text == target {
            let parent = node.parent();
            let is_declaration_name = parent.map_or(false, |p| {
                p.child_by_field_name("name").map_or(false, |n| n.id() == node.id())
            });

            if !is_declaration_name {
                let start = node.start_position();
                let line_idx = start.row;
                let line_content = lines.get(line_idx).unwrap_or(&"").to_string();

                call_sites.push(SymbolCallSite {
                    caller_symbol: active_enclosing.map(|s| s.to_string()),
                    file: file.to_string(),
                    line: line_idx + 1,
                    col: start.column + 1,
                    line_content,
                });
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_callers(
            child,
            source,
            file,
            target,
            active_enclosing,
            lines,
            call_sites,
        );
    }
}

/// Extract symbols from a file within the workspace root.
pub fn extract_file_symbols(root: &Root, rel_path: &str) -> Result<Vec<SymbolDefinition>, ForgeError> {
    let abs_path = resolve_jailed_path(root, rel_path)?;
    let content = fs::read_to_string(&abs_path).map_err(|_| ForgeError::NotFound)?;
    let lang = lang_for_path(rel_path);
    Ok(extract_source_symbols(lang, rel_path, &content))
}

/// Search the entire worktree for callers and references to `symbol`.
pub fn find_symbol_callers(root: &Root, symbol: &str) -> Result<Vec<SymbolCallSite>, ForgeError> {
    let mut callers = Vec::new();
    let base_path = root.path();
    walk_dir_callers(base_path, base_path, symbol, &mut callers)?;
    Ok(callers)
}

fn walk_dir_callers(
    base_root: &Path,
    dir: &Path,
    symbol: &str,
    out: &mut Vec<SymbolCallSite>,
) -> Result<(), ForgeError> {
    if !dir.exists() || !dir.is_dir() {
        return Ok(());
    }

    let entries = fs::read_dir(dir).map_err(|e| ForgeError::Io(e.to_string()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip ignored directories
        if path.is_dir() {
            if matches!(file_name, ".git" | "target" | "node_modules" | "dist" | ".hadron" | "vendor" | ".cache") {
                continue;
            }
            walk_dir_callers(base_root, &path, symbol, out)?;
        } else if path.is_file() {
            let rel = path
                .strip_prefix(base_root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let lang = lang_for_path(&rel);
            if lang != Lang::Opaque {
                if let Ok(content) = fs::read_to_string(&path) {
                    let mut sites = find_callers_in_source(lang, &rel, &content, symbol);
                    out.append(&mut sites);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_symbols_extracts_definitions_across_rust_ts_and_python() {
        let rust_src = "pub fn calculate_metrics(x: i32) -> i32 { x * 2 }\npub struct MetricCollector;\n";
        let rs_syms = extract_source_symbols(Lang::Rust, "src/metrics.rs", rust_src);
        assert_eq!(rs_syms.len(), 2);
        assert_eq!(rs_syms[0].name, "calculate_metrics");
        assert_eq!(rs_syms[0].kind, "function");
        assert_eq!(rs_syms[1].name, "MetricCollector");
        assert_eq!(rs_syms[1].kind, "struct");

        let ts_src = "export function processOrder(id: string): boolean { return true; }\nexport class OrderService {}\n";
        let ts_syms = extract_source_symbols(Lang::TypeScript, "src/orders.ts", ts_src);
        assert_eq!(ts_syms.len(), 2);
        assert_eq!(ts_syms[0].name, "processOrder");
        assert_eq!(ts_syms[1].name, "OrderService");

        let py_src = "def format_response(data):\n    return data\n\nclass ResponseFormatter:\n    pass\n";
        let py_syms = extract_source_symbols(Lang::Python, "lib/format.py", py_src);
        assert_eq!(py_syms.len(), 2);
        assert_eq!(py_syms[0].name, "format_response");
        assert_eq!(py_syms[1].name, "ResponseFormatter");
    }

    #[test]
    fn ast_symbols_finds_callers_with_enclosing_scope() {
        let rust_src = r#"
fn worker() {
    let result = calculate_metrics(42);
    println!("{}", result);
}

fn other() {
    let _ = calculate_metrics(10);
}
"#;
        let callers = find_callers_in_source(Lang::Rust, "src/main.rs", rust_src, "calculate_metrics");
        assert_eq!(callers.len(), 2);
        assert_eq!(callers[0].caller_symbol.as_deref(), Some("worker"));
        assert_eq!(callers[0].line, 3);
        assert_eq!(callers[1].caller_symbol.as_deref(), Some("other"));
        assert_eq!(callers[1].line, 8);
    }

    #[test]
    fn ast_symbols_finds_callers_across_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let src_dir = temp.path().join("src");
        fs::create_dir_all(&src_dir).unwrap();

        fs::write(
            src_dir.join("calc.rs"),
            "pub fn calculate_metrics() -> i32 { 100 }\n",
        )
        .unwrap();

        fs::write(
            src_dir.join("main.rs"),
            "fn run() {\n    let _ = calculate_metrics();\n}\n",
        )
        .unwrap();

        let root = Root::new(temp.path().to_path_buf());
        let callers = find_symbol_callers(&root, "calculate_metrics").unwrap();
        assert_eq!(callers.len(), 1);
        assert_eq!(callers[0].caller_symbol.as_deref(), Some("run"));
        assert_eq!(callers[0].line, 2);

        let syms = extract_file_symbols(&root, "src/calc.rs").unwrap();
        assert_eq!(syms.len(), 1);
        assert_eq!(syms[0].name, "calculate_metrics");
    }
}
