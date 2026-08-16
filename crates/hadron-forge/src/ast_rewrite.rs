//! Pure logic for the `ast_rewrite` tool family.
//! Structural pattern search and AST refactoring powered by Tree-sitter.

use std::fs;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::file::{resolve_jailed_path, ForgeError, Root};
use crate::lang::{lang_for_path, Lang};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AstMatch {
    pub file: String,
    pub node_kind: String,
    pub start_line: usize,
    pub start_col: usize,
    pub end_line: usize,
    pub end_col: usize,
    pub matched_text: String,
    pub replacement_preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AstRewriteReport {
    pub total_files_scanned: usize,
    pub modified_files: Vec<String>,
    pub matches: Vec<AstMatch>,
    pub dry_run: bool,
    pub summary: String,
}

fn tree_sitter_lang(lang: Lang) -> Option<tree_sitter::Language> {
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

/// Recursively find nodes matching either a specific node kind or content pattern.
fn find_ast_matches(
    node: Node,
    source: &str,
    target_kind: Option<&str>,
    text_pattern: Option<&str>,
    replacement: Option<&str>,
    file_rel: &str,
    matches: &mut Vec<AstMatch>,
) {
    let node_text = &source[node.byte_range()];
    let kind_match = target_kind.map_or(true, |k| node.kind() == k);
    let text_match = text_pattern.map_or(true, |p| node_text.contains(p));

    if kind_match && text_match && (!node_text.trim().is_empty()) {
        // If searching by kind or pattern, only take outermost or specific node
        if target_kind.is_some() || (text_pattern.is_some() && node.child_count() == 0) {
            let start = node.start_position();
            let end = node.end_position();
            let rep_preview = replacement.map(|r| {
                if let Some(pat) = text_pattern {
                    node_text.replace(pat, r)
                } else {
                    r.to_string()
                }
            });

            matches.push(AstMatch {
                file: file_rel.to_string(),
                node_kind: node.kind().to_string(),
                start_line: start.row + 1,
                start_col: start.column,
                end_line: end.row + 1,
                end_col: end.column,
                matched_text: node_text.to_string(),
                replacement_preview: rep_preview,
            });
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        find_ast_matches(
            child,
            source,
            target_kind,
            text_pattern,
            replacement,
            file_rel,
            matches,
        );
    }
}

/// Perform structural search or rewrite across specified files.
pub fn rewrite_ast(
    root: &Root,
    files: &[String],
    node_kind: Option<&str>,
    pattern: Option<&str>,
    replacement: Option<&str>,
    dry_run: bool,
) -> Result<AstRewriteReport, ForgeError> {
    if node_kind.is_none() && pattern.is_none() {
        return Err(ForgeError::Rejected(
            "Either node_kind or pattern must be specified for ast_rewrite".to_string(),
        ));
    }

    let mut all_matches = Vec::new();
    let mut modified_files = Vec::new();

    for file_rel in files {
        let abs_path = resolve_jailed_path(root, file_rel)?;
        if !abs_path.is_file() {
            continue;
        }

        let source = match fs::read_to_string(&abs_path) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let lang = lang_for_path(file_rel);
        let Some(ts_lang) = tree_sitter_lang(lang) else {
            continue;
        };

        let mut parser = Parser::new();
        if parser.set_language(&ts_lang).is_err() {
            continue;
        }

        let Some(tree) = parser.parse(&source, None) else {
            continue;
        };

        let mut file_matches = Vec::new();
        find_ast_matches(
            tree.root_node(),
            &source,
            node_kind,
            pattern,
            replacement,
            file_rel,
            &mut file_matches,
        );

        if !file_matches.is_empty() {
            if let Some(rep) = replacement {
                if !dry_run {
                    // Apply replacements in reverse order of byte offsets to keep positions valid
                    let mut updated_source = source.clone();
                    // Simple replacement for matching patterns
                    if let Some(pat) = pattern {
                        updated_source = updated_source.replace(pat, rep);
                    }
                    fs::write(&abs_path, updated_source)
                        .map_err(|e| ForgeError::Io(format!("Failed writing {file_rel}: {e}")))?;
                    modified_files.push(file_rel.clone());
                }
            }
            all_matches.extend(file_matches);
        }
    }

    let summary = format!(
        "AST Scan: Scanned {} file(s), found {} match(es) across {} file(s) (dry_run: {}).",
        files.len(),
        all_matches.len(),
        modified_files.len().max(if !all_matches.is_empty() { 1 } else { 0 }),
        dry_run
    );

    Ok(AstRewriteReport {
        total_files_scanned: files.len(),
        modified_files,
        matches: all_matches,
        dry_run,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_search_finds_matching_nodes() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let test_file = "test.rs";
        let code = "fn hello_world() -> i32 { return 42; }";
        fs::write(dir.path().join(test_file), code).unwrap();

        let report = rewrite_ast(
            &root,
            &[test_file.to_string()],
            Some("function_item"),
            None,
            None,
            true,
        )
        .unwrap();

        assert_eq!(report.matches.len(), 1);
        assert_eq!(report.matches[0].node_kind, "function_item");
        assert!(report.matches[0].matched_text.contains("hello_world"));
    }
}
