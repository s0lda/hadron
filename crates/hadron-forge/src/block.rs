use tree_sitter::{Node, Parser};

/// How many hex chars of the blake3 digest identify a block.
/// 6 hex = 24 bits — enough to disambiguate blocks within one file for v1
/// dogfooding; a production multi-file bus should widen this (bought land).
pub const HASH_LEN: usize = 6;

/// The kind of top-level Rust item a [`Block`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Function,
    Struct,
    Enum,
    Impl,
    Trait,
}

impl BlockKind {
    fn from_node_kind(kind: &str) -> Option<BlockKind> {
        match kind {
            "function_item" => Some(BlockKind::Function),
            "struct_item" => Some(BlockKind::Struct),
            "enum_item" => Some(BlockKind::Enum),
            "impl_item" => Some(BlockKind::Impl),
            "trait_item" => Some(BlockKind::Trait),
            _ => None,
        }
    }

    /// A short human label used in the swarm digest.
    pub fn label(&self) -> &'static str {
        match self {
            BlockKind::Function => "fn",
            BlockKind::Struct => "struct",
            BlockKind::Enum => "enum",
            BlockKind::Impl => "impl",
            BlockKind::Trait => "trait",
        }
    }
}

/// One hashed, addressable logical block of source — the unit of edit-by-hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub kind: BlockKind,
    pub name: String,
    /// blake3 of the exact block text, truncated to [`HASH_LEN`] hex chars.
    pub hash: String,
    /// 1-based inclusive line span in the source.
    pub start_line: usize,
    pub end_line: usize,
    /// Byte range of the block within the source (for splicing an edit).
    pub byte_start: usize,
    pub byte_end: usize,
}

/// blake3 of `text`, hex, truncated to [`HASH_LEN`] chars.
pub fn short_hash(text: &str) -> String {
    blake3::hash(text.as_bytes()).to_hex()[..HASH_LEN].to_string()
}

/// Human name of a top-level item: `name` field for fn/struct/enum/trait;
/// for an `impl`, the `type` (optionally `Trait for Type`).
fn node_name(node: Node, src: &str) -> String {
    if let Some(n) = node.child_by_field_name("name") {
        return src[n.byte_range()].to_string();
    }
    if let Some(ty) = node.child_by_field_name("type") {
        let ty_s = src[ty.byte_range()].to_string();
        if let Some(tr) = node.child_by_field_name("trait") {
            return format!("{} for {}", &src[tr.byte_range()], ty_s);
        }
        return ty_s;
    }
    "<anon>".to_string()
}

/// Parse Rust `source` into its top-level hashed blocks, in source order.
/// Only top-level items are addressed in v1 (nested items ride inside their
/// parent's block); unparseable or empty input yields an empty list.
pub fn parse_blocks(source: &str) -> Vec<Block> {
    let mut parser = Parser::new();
    // The grammar is a compile-time constant; set_language only fails on an
    // ABI mismatch, a build-time impossibility here.
    parser
        .set_language(&tree_sitter_rust::language())
        .expect("load tree-sitter-rust grammar");
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let root = tree.root_node();
    let mut blocks = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if let Some(kind) = BlockKind::from_node_kind(child.kind()) {
            let range = child.byte_range();
            let text = &source[range.clone()];
            blocks.push(Block {
                kind,
                name: node_name(child, source),
                hash: short_hash(text),
                start_line: child.start_position().row + 1,
                end_line: child.end_position().row + 1,
                byte_start: range.start,
                byte_end: range.end,
            });
        }
    }
    blocks
}

/// Render the blocks of `source` as the Markdown context handed to the swarm:
/// one line per block, `[Hash: <h>] <kind> <name> (lines A–B)`.
pub fn annotate(source: &str) -> String {
    let mut out = String::new();
    for b in parse_blocks(source) {
        out.push_str(&format!(
            "[Hash: {}] {} {} (lines {}–{})\n",
            b.hash,
            b.kind.label(),
            b.name,
            b.start_line,
            b.end_line
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"
use std::fmt;

pub fn alpha(x: i32) -> i32 { x + 1 }

struct Point { x: f64, y: f64 }

impl Point {
    fn norm(&self) -> f64 { self.x }
}

impl fmt::Debug for Point {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { Ok(()) }
}

enum Color { Red, Green }

trait Shape { fn area(&self) -> f64; }
"#;

    #[test]
    fn parses_top_level_items_with_kinds_and_names() {
        let blocks = parse_blocks(SAMPLE);
        let got: Vec<(BlockKind, &str)> =
            blocks.iter().map(|b| (b.kind, b.name.as_str())).collect();
        assert_eq!(
            got,
            vec![
                (BlockKind::Function, "alpha"),
                (BlockKind::Struct, "Point"),
                (BlockKind::Impl, "Point"),
                (BlockKind::Impl, "fmt::Debug for Point"),
                (BlockKind::Enum, "Color"),
                (BlockKind::Trait, "Shape"),
            ]
        );
    }

    #[test]
    fn line_and_byte_spans_are_recoverable() {
        let blocks = parse_blocks(SAMPLE);
        let alpha = &blocks[0];
        // The byte span slices back to exactly the block text.
        assert_eq!(
            &SAMPLE[alpha.byte_start..alpha.byte_end],
            "pub fn alpha(x: i32) -> i32 { x + 1 }"
        );
        // 1-based lines.
        assert_eq!(alpha.start_line, 4);
        assert_eq!(alpha.end_line, 4);
    }

    #[test]
    fn short_hash_is_deterministic_and_content_sensitive() {
        assert_eq!(short_hash("fn a() {}"), short_hash("fn a() {}"));
        assert_ne!(short_hash("fn a() {}"), short_hash("fn a() { }"));
        assert_eq!(short_hash("anything").len(), HASH_LEN);
    }

    #[test]
    fn empty_or_non_item_source_yields_no_blocks() {
        assert!(parse_blocks("").is_empty());
        assert!(parse_blocks("$$$ not rust $$$").is_empty());
    }

    #[test]
    fn annotate_lists_a_hash_marker_per_block() {
        let text = annotate(SAMPLE);
        assert_eq!(text.lines().count(), 6);
        assert!(text.contains("[Hash: "));
        assert!(text.contains("fn alpha"));
        assert!(text.contains("trait Shape"));
        // The marker's hash equals the block's hash.
        let alpha = &parse_blocks(SAMPLE)[0];
        assert!(text.contains(&format!("[Hash: {}] fn alpha", alpha.hash)));
    }
}
