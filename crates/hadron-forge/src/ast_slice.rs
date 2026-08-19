//! Adaptive AST Context Slicing (Capability #4).
//!
//! Slices relevant function, struct, and symbol subtrees from source code to reduce prompt token consumption.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AstSlice {
    pub symbol_name: String,
    pub kind: String, // "fn", "struct", "enum", "impl", "type"
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileAstSlices {
    pub file_path: String,
    pub slices: Vec<AstSlice>,
    pub token_savings_pct: f32,
}

/// Slices source text to extract only the AST blocks enclosing or directly referencing `target_symbols`.
pub fn slice_source_by_symbols(source: &str, target_symbols: &[&str]) -> Vec<AstSlice> {
    let mut slices = Vec::new();
    let lines: Vec<&str> = source.lines().collect();

    for target in target_symbols {
        let mut in_block = false;
        let mut block_start = 0;
        let mut brace_depth = 0;
        let mut kind = "unknown";

        for (ix, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            if !in_block && trimmed.contains(target) {
                if trimmed.starts_with("pub fn ")
                    || trimmed.starts_with("pub(crate) fn ")
                    || trimmed.starts_with("pub(super) fn ")
                    || trimmed.starts_with("fn ")
                    || trimmed.starts_with("async fn ")
                    || trimmed.starts_with("pub async fn ")
                    || trimmed.starts_with("def ")
                    || trimmed.starts_with("function ")
                {
                    in_block = true;
                    block_start = ix;
                    brace_depth = 0;
                    kind = "fn";
                } else if trimmed.starts_with("pub struct ")
                    || trimmed.starts_with("pub(crate) struct ")
                    || trimmed.starts_with("struct ")
                    || trimmed.starts_with("class ")
                {
                    in_block = true;
                    block_start = ix;
                    brace_depth = 0;
                    kind = "struct";
                } else if trimmed.starts_with("pub enum ")
                    || trimmed.starts_with("pub(crate) enum ")
                    || trimmed.starts_with("enum ")
                {
                    in_block = true;
                    block_start = ix;
                    brace_depth = 0;
                    kind = "enum";
                } else if trimmed.starts_with("impl ") {
                    in_block = true;
                    block_start = ix;
                    brace_depth = 0;
                    kind = "impl";
                }
            }

            if in_block {
                let open_braces = line.matches('{').count();
                let close_braces = line.matches('}').count();
                brace_depth += open_braces as i32;
                brace_depth -= close_braces as i32;

                if (open_braces > 0 || close_braces > 0) && brace_depth <= 0 {
                    in_block = false;
                    let slice_lines = &lines[block_start..=ix];
                    slices.push(AstSlice {
                        symbol_name: target.to_string(),
                        kind: kind.to_string(),
                        start_line: block_start + 1,
                        end_line: ix + 1,
                        content: slice_lines.join("\n"),
                    });
                }
            }
        }
    }

    slices
}

/// Slices a file and computes estimated token savings ratio compared to the full file.
pub fn slice_file_context(file_path: &str, source: &str, target_symbols: &[&str]) -> FileAstSlices {
    let slices = slice_source_by_symbols(source, target_symbols);
    let total_lines = source.lines().count().max(1);
    let sliced_lines: usize = slices.iter().map(|s| s.end_line - s.start_line + 1).sum();

    let savings = if sliced_lines < total_lines {
        ((total_lines - sliced_lines) as f32 / total_lines as f32) * 100.0
    } else {
        0.0
    };

    FileAstSlices {
        file_path: file_path.to_string(),
        slices,
        token_savings_pct: savings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ast_slicing_and_token_savings() {
        let source = r#"
// Top comments
use std::collections::HashMap;

pub struct Config {
    pub id: String,
    pub count: usize,
}

pub fn calculate_total(a: i32, b: i32) -> i32 {
    let sum = a + b;
    sum * 2
}

pub fn ignored_large_function() {
    println!("many lines here...");
}
"#;

        let res = slice_file_context("src/config.rs", source, &["calculate_total", "Config"]);
        assert_eq!(res.slices.len(), 2);
        assert_eq!(res.slices[0].symbol_name, "calculate_total");
        assert_eq!(res.slices[0].kind, "fn");
        assert!(res.slices[0].content.contains("let sum = a + b"));

        assert_eq!(res.slices[1].symbol_name, "Config");
        assert_eq!(res.slices[1].kind, "struct");
        assert!(res.token_savings_pct > 30.0);
    }
}
