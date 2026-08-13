//! Semantic AST-aware merge resolution for Rust sources.
//!
//! Reconciles concurrent edits at the AST item level (functions, structs, impls, traits)
//! rather than relying purely on text line diffs. If both branches add non-colliding
//! top-level items or edit disjoint items, the merge resolves cleanly without conflict.

use std::collections::{HashMap, HashSet};
use crate::block::{parse_blocks_lang, Block, BlockKind};
use crate::lang::Lang;

/// One AST-level conflict block between two concurrent branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AstConflictBlock {
    pub name: String,
    pub base: String,
    pub ours: String,
    pub theirs: String,
}

/// The outcome of an AST-aware 3-way merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AstMergeResult {
    /// Merged cleanly with resulting complete source code.
    Clean(String),
    /// Conflicting item(s) that require resolution.
    Conflict(Vec<AstConflictBlock>),
}

/// Merges Rust source code from `base`, `ours`, and `theirs` using AST block decomposition.
pub fn merge_rust_ast(base: &str, ours: &str, theirs: &str) -> AstMergeResult {
    // If ours and theirs are identical, no merge needed
    if ours == theirs {
        return AstMergeResult::Clean(ours.to_string());
    }
    // If ours is unchanged from base, take theirs
    if ours == base {
        return AstMergeResult::Clean(theirs.to_string());
    }
    // If theirs is unchanged from base, take ours
    if theirs == base {
        return AstMergeResult::Clean(ours.to_string());
    }

    let base_blocks = parse_blocks_lang(base, Lang::Rust);
    let ours_blocks = parse_blocks_lang(ours, Lang::Rust);
    let theirs_blocks = parse_blocks_lang(theirs, Lang::Rust);

    // If parsing failed to produce structured blocks, fallback to direct text comparison
    if base_blocks.is_empty() && ours_blocks.is_empty() && theirs_blocks.is_empty() {
        return AstMergeResult::Conflict(vec![AstConflictBlock {
            name: "root".to_string(),
            base: base.to_string(),
            ours: ours.to_string(),
            theirs: theirs.to_string(),
        }]);
    }

    // Map item key (kind, name) -> block text
    let get_block_text = |src: &str, b: &Block| -> String {
        src[b.byte_start..b.byte_end].to_string()
    };

    let base_map: HashMap<(BlockKind, String), String> = base_blocks
        .iter()
        .map(|b| ((b.kind, b.name.clone()), get_block_text(base, b)))
        .collect();

    let ours_map: HashMap<(BlockKind, String), String> = ours_blocks
        .iter()
        .map(|b| ((b.kind, b.name.clone()), get_block_text(ours, b)))
        .collect();

    let theirs_map: HashMap<(BlockKind, String), String> = theirs_blocks
        .iter()
        .map(|b| ((b.kind, b.name.clone()), get_block_text(theirs, b)))
        .collect();

    let mut all_keys: Vec<(BlockKind, String)> = Vec::new();
    let mut seen = HashSet::new();

    // Preserve order of appearance: ours first, then theirs additions, then base
    for b in &ours_blocks {
        let k = (b.kind, b.name.clone());
        if seen.insert(k.clone()) {
            all_keys.push(k);
        }
    }
    for b in &theirs_blocks {
        let k = (b.kind, b.name.clone());
        if seen.insert(k.clone()) {
            all_keys.push(k);
        }
    }
    for b in &base_blocks {
        let k = (b.kind, b.name.clone());
        if seen.insert(k.clone()) {
            all_keys.push(k);
        }
    }

    let mut merged_items: Vec<String> = Vec::new();
    let mut conflicts: Vec<AstConflictBlock> = Vec::new();

    for key in all_keys {
        let in_base = base_map.get(&key);
        let in_ours = ours_map.get(&key);
        let in_theirs = theirs_map.get(&key);

        match (in_base, in_ours, in_theirs) {
            (Some(b), Some(o), Some(t)) => {
                if o == t {
                    merged_items.push(o.clone());
                } else if o == b {
                    // only theirs changed
                    merged_items.push(t.clone());
                } else if t == b {
                    // only ours changed
                    merged_items.push(o.clone());
                } else {
                    // both changed differently
                    conflicts.push(AstConflictBlock {
                        name: key.1.clone(),
                        base: b.clone(),
                        ours: o.clone(),
                        theirs: t.clone(),
                    });
                }
            }
            (Some(b), Some(o), None) => {
                if o == b {
                    // theirs deleted item
                } else {
                    // ours modified, theirs deleted -> conflict
                    conflicts.push(AstConflictBlock {
                        name: key.1.clone(),
                        base: b.clone(),
                        ours: o.clone(),
                        theirs: String::new(),
                    });
                }
            }
            (Some(b), None, Some(t)) => {
                if t == b {
                    // ours deleted item
                } else {
                    // theirs modified, ours deleted -> conflict
                    conflicts.push(AstConflictBlock {
                        name: key.1.clone(),
                        base: b.clone(),
                        ours: String::new(),
                        theirs: t.clone(),
                    });
                }
            }
            (Some(_b), None, None) => {
                // both deleted item
            }
            (None, Some(o), Some(t)) => {
                if o == t {
                    merged_items.push(o.clone());
                } else {
                    // both added same name differently -> conflict
                    conflicts.push(AstConflictBlock {
                        name: key.1.clone(),
                        base: String::new(),
                        ours: o.clone(),
                        theirs: t.clone(),
                    });
                }
            }
            (None, Some(o), None) => {
                // only ours added
                merged_items.push(o.clone());
            }
            (None, None, Some(t)) => {
                // only theirs added
                merged_items.push(t.clone());
            }
            (None, None, None) => {}
        }
    }

    if !conflicts.is_empty() {
        AstMergeResult::Conflict(conflicts)
    } else {
        let mut res = merged_items.join("\n\n");
        if !res.is_empty() && !res.ends_with('\n') {
            res.push('\n');
        }
        AstMergeResult::Clean(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ast_merge_resolves_independent_function_additions_cleanly() {
        let base = "fn foo() {}\n";
        let ours = "fn foo() {}\n\nfn bar() {}\n";
        let theirs = "fn foo() {}\n\nfn baz() {}\n";
        let result = merge_rust_ast(base, ours, theirs);
        match result {
            AstMergeResult::Clean(merged) => {
                assert!(merged.contains("fn foo() {}"));
                assert!(merged.contains("fn bar() {}"));
                assert!(merged.contains("fn baz() {}"));
            }
            AstMergeResult::Conflict(c) => panic!("Expected clean merge, got conflicts: {:?}", c),
        }
    }

    #[test]
    fn ast_merge_detects_conflicting_edits_on_same_function() {
        let base = "fn foo() { 1 }\n";
        let ours = "fn foo() { 2 }\n";
        let theirs = "fn foo() { 3 }\n";
        let result = merge_rust_ast(base, ours, theirs);
        match result {
            AstMergeResult::Conflict(conflicts) => {
                assert_eq!(conflicts.len(), 1);
                assert_eq!(conflicts[0].name, "foo");
                assert_eq!(conflicts[0].ours, "fn foo() { 2 }");
                assert_eq!(conflicts[0].theirs, "fn foo() { 3 }");
            }
            AstMergeResult::Clean(m) => panic!("Expected conflict, got clean: {}", m),
        }
    }
}
