//! Tier 1 Tree-Sitter multi-language symbol extraction, caller detection, and hierarchy engine.
//!
//! Provides zero-config AST-level symbol outline, type hierarchy, trait implementation discovery,
//! and caller discovery across 12+ programming languages.
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

/// Information about a struct/class/interface member (field, variant, method).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeMemberInfo {
    pub name: String,
    pub kind: String, // "field", "variant", "method"
    pub type_annotation: Option<String>,
    pub line: usize,
}

/// Information about an implementation block or trait implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraitImplInfo {
    pub trait_name: Option<String>,
    pub target_type: String,
    pub file: String,
    pub line: usize,
    pub methods: Vec<String>,
}

/// Comprehensive type hierarchy report for a struct, enum, class, or trait.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypeHierarchyInfo {
    pub type_name: String,
    pub kind: String, // "struct", "enum", "class", "trait", "interface"
    pub file: String,
    pub line: usize,
    pub members: Vec<TypeMemberInfo>,
    pub implementations: Vec<TraitImplInfo>,
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

/// Extract detailed type definition (members, fields, enum variants) from source string.
pub fn extract_type_members(lang: Lang, source: &str, target_type: &str) -> Option<(String, usize, Vec<TypeMemberInfo>)> {
    let language = tree_sitter_language_for(lang)?;
    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(source, None)?;

    let mut result = None;
    find_type_node(tree.root_node(), source, target_type, lang, &mut result);
    result
}

fn find_type_node(
    node: Node,
    source: &str,
    target_type: &str,
    lang: Lang,
    result: &mut Option<(String, usize, Vec<TypeMemberInfo>)>,
) {
    if result.is_some() {
        return;
    }

    let kind_str = node.kind();
    let is_type_def = match lang {
        Lang::Rust => matches!(kind_str, "struct_item" | "enum_item" | "trait_item"),
        Lang::TypeScript | Lang::JavaScript => matches!(kind_str, "class_declaration" | "interface_declaration" | "enum_declaration"),
        Lang::Python => matches!(kind_str, "class_definition"),
        _ => false,
    };

    if is_type_def {
        let name = node
            .child_by_field_name("name")
            .map(|n| &source[n.byte_range()])
            .unwrap_or("");

        if name == target_type {
            let start = node.start_position();
            let mut members = Vec::new();
            collect_type_members(node, source, lang, &mut members);
            *result = Some((simplify_kind(kind_str), start.row + 1, members));
            return;
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_type_node(child, source, target_type, lang, result);
    }
}

fn collect_type_members(
    node: Node,
    source: &str,
    lang: Lang,
    members: &mut Vec<TypeMemberInfo>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let kind = child.kind();
        match lang {
            Lang::Rust => {
                if kind == "field_declaration_list" || kind == "ordered_field_declaration_list" {
                    let mut field_cursor = child.walk();
                    for field in child.children(&mut field_cursor) {
                        if field.kind() == "field_declaration" {
                            let name = field
                                .child_by_field_name("name")
                                .map(|n| source[n.byte_range()].to_string())
                                .unwrap_or_else(|| "_".into());
                            let type_annot = field
                                .child_by_field_name("type")
                                .map(|n| source[n.byte_range()].to_string());
                            members.push(TypeMemberInfo {
                                name,
                                kind: "field".into(),
                                type_annotation: type_annot,
                                line: field.start_position().row + 1,
                            });
                        }
                    }
                } else if kind == "enum_variant_list" {
                    let mut var_cursor = child.walk();
                    for variant in child.children(&mut var_cursor) {
                        if variant.kind() == "enum_variant" {
                            let name = variant
                                .child_by_field_name("name")
                                .map(|n| source[n.byte_range()].to_string())
                                .unwrap_or_default();
                            members.push(TypeMemberInfo {
                                name,
                                kind: "variant".into(),
                                type_annotation: None,
                                line: variant.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            Lang::TypeScript | Lang::JavaScript => {
                if kind == "class_body" || kind == "interface_body" || kind == "object_type" {
                    let mut body_cursor = child.walk();
                    for m in child.children(&mut body_cursor) {
                        let m_kind = m.kind();
                        if matches!(m_kind, "method_definition" | "property_signature" | "public_field_definition") {
                            let name = m
                                .child_by_field_name("name")
                                .map(|n| source[n.byte_range()].to_string())
                                .unwrap_or_default();
                            let member_kind = if m_kind == "method_definition" { "method" } else { "field" };
                            members.push(TypeMemberInfo {
                                name,
                                kind: member_kind.into(),
                                type_annotation: m.child_by_field_name("type").map(|n| source[n.byte_range()].to_string()),
                                line: m.start_position().row + 1,
                            });
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Extract all `impl` blocks (both inherent and trait implementations) from source text.
pub fn extract_impl_blocks(lang: Lang, rel_path: &str, source: &str) -> Vec<TraitImplInfo> {
    if lang != Lang::Rust {
        return Vec::new();
    }
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

    let mut impls = Vec::new();
    walk_impl_blocks(tree.root_node(), source, rel_path, &mut impls);
    impls
}

fn walk_impl_blocks(node: Node, source: &str, file: &str, impls: &mut Vec<TraitImplInfo>) {
    if node.kind() == "impl_item" {
        let trait_name = node
            .child_by_field_name("trait")
            .map(|n| source[n.byte_range()].trim().to_string());
        let target_type = node
            .child_by_field_name("type")
            .map(|n| source[n.byte_range()].trim().to_string())
            .unwrap_or_default();

        let mut methods = Vec::new();
        if let Some(body) = node.child_by_field_name("body") {
            let mut cursor = body.walk();
            for item in body.children(&mut cursor) {
                if item.kind() == "function_item" {
                    if let Some(name_node) = item.child_by_field_name("name") {
                        methods.push(source[name_node.byte_range()].to_string());
                    }
                }
            }
        }

        if !target_type.is_empty() {
            impls.push(TraitImplInfo {
                trait_name,
                target_type,
                file: file.to_string(),
                line: node.start_position().row + 1,
                methods,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk_impl_blocks(child, source, file, impls);
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

/// Query complete type hierarchy (definition, fields/variants, and implementations) across workspace.
pub fn query_type_hierarchy(root: &Root, type_name: &str) -> Result<Option<TypeHierarchyInfo>, ForgeError> {
    let base_path = root.path();
    let mut files_to_scan = Vec::new();
    collect_code_files(base_path, base_path, &mut files_to_scan)?;

    let mut found_type = None;
    let mut all_impls = Vec::new();

    for rel in files_to_scan {
        let abs = base_path.join(&rel);
        let Ok(content) = fs::read_to_string(&abs) else { continue };
        let lang = lang_for_path(&rel);

        if found_type.is_none() {
            if let Some((kind, line, members)) = extract_type_members(lang, &content, type_name) {
                found_type = Some((kind, line, members, rel.clone()));
            }
        }

        let impls = extract_impl_blocks(lang, &rel, &content);
        for im in impls {
            if im.target_type == type_name || im.trait_name.as_deref() == Some(type_name) {
                all_impls.push(im);
            }
        }
    }

    if let Some((kind, line, members, file)) = found_type {
        Ok(Some(TypeHierarchyInfo {
            type_name: type_name.to_string(),
            kind,
            file,
            line,
            members,
            implementations: all_impls,
        }))
    } else if !all_impls.is_empty() {
        Ok(Some(TypeHierarchyInfo {
            type_name: type_name.to_string(),
            kind: "trait".into(),
            file: all_impls[0].file.clone(),
            line: all_impls[0].line,
            members: Vec::new(),
            implementations: all_impls,
        }))
    } else {
        Ok(None)
    }
}

/// Search the entire worktree for callers and references to `symbol`.
pub fn find_symbol_callers(root: &Root, symbol: &str) -> Result<Vec<SymbolCallSite>, ForgeError> {
    let mut callers = Vec::new();
    let base_path = root.path();
    walk_dir_callers(base_path, base_path, symbol, &mut callers)?;
    Ok(callers)
}

fn collect_code_files(base: &Path, current: &Path, acc: &mut Vec<String>) -> Result<(), ForgeError> {
    if !current.exists() || !current.is_dir() {
        return Ok(());
    }
    let entries = fs::read_dir(current).map_err(|e| ForgeError::Io(e.to_string()))?;
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if p.is_dir() {
            if matches!(name, ".git" | "target" | "node_modules" | "dist" | ".hadron" | "vendor" | ".cache") {
                continue;
            }
            collect_code_files(base, &p, acc)?;
        } else if p.is_file() {
            let rel = p.strip_prefix(base).unwrap_or(&p).to_string_lossy().to_string();
            if lang_for_path(&rel) != Lang::Opaque {
                acc.push(rel);
            }
        }
    }
    Ok(())
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
    fn ast_symbols_extracts_struct_fields_and_enum_variants() {
        let rust_src = r#"
pub struct UserConfig {
    pub username: String,
    pub retries: u32,
}

pub enum ConfigMode {
    Strict,
    Permissive,
}
"#;
        let (kind, line, members) = extract_type_members(Lang::Rust, rust_src, "UserConfig").unwrap();
        assert_eq!(kind, "struct");
        assert_eq!(line, 2);
        assert_eq!(members.len(), 2);
        assert_eq!(members[0].name, "username");
        assert_eq!(members[0].type_annotation.as_deref(), Some("String"));
        assert_eq!(members[1].name, "retries");

        let (enum_kind, _enum_line, enum_members) = extract_type_members(Lang::Rust, rust_src, "ConfigMode").unwrap();
        assert_eq!(enum_kind, "enum");
        assert_eq!(enum_members.len(), 2);
        assert_eq!(enum_members[0].name, "Strict");
        assert_eq!(enum_members[1].name, "Permissive");
    }

    #[test]
    fn ast_symbols_extracts_trait_implementations() {
        let rust_src = r#"
pub trait Describable {
    fn describe(&self) -> String;
}

pub struct Item;

impl Describable for Item {
    fn describe(&self) -> String {
        "item".into()
    }
}
"#;
        let impls = extract_impl_blocks(Lang::Rust, "src/item.rs", rust_src);
        assert_eq!(impls.len(), 1);
        assert_eq!(impls[0].trait_name.as_deref(), Some("Describable"));
        assert_eq!(impls[0].target_type, "Item");
        assert_eq!(impls[0].methods, vec!["describe"]);
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
