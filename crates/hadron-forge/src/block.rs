use tree_sitter::{Node, Parser};
use crate::lang::Lang;

/// How many hex chars of the blake3 digest identify a block.
/// 8 hex = 32 bits — enough to disambiguate blocks within files for production.
pub const HASH_LEN: usize = 8;

/// The kind of top-level item a [`Block`] represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockKind {
    Function,
    Struct,
    Enum,
    Impl,
    Trait,
    Class,
    Element,
    Rule,
    Statement,
    Chunk,
}

impl BlockKind {
    fn from_node_kind_lang(kind: &str, lang: Lang) -> Option<BlockKind> {
        match lang {
            Lang::Rust => match kind {
                "function_item" => Some(BlockKind::Function),
                "struct_item" => Some(BlockKind::Struct),
                "enum_item" => Some(BlockKind::Enum),
                "impl_item" => Some(BlockKind::Impl),
                "trait_item" => Some(BlockKind::Trait),
                _ => None,
            },
            Lang::Python => match kind {
                "function_definition" => Some(BlockKind::Function),
                "class_definition" => Some(BlockKind::Struct),
                _ => None,
            },
            Lang::TypeScript => match kind {
                "function_declaration" | "method_definition" => Some(BlockKind::Function),
                "class_declaration" => Some(BlockKind::Struct),
                "interface_declaration" => Some(BlockKind::Trait),
                "enum_declaration" => Some(BlockKind::Enum),
                _ => None,
            },
            Lang::Go => match kind {
                "function_declaration" | "method_declaration" => Some(BlockKind::Function),
                "type_declaration" => Some(BlockKind::Struct),
                _ => None,
            },
            Lang::C => match kind {
                "function_definition" => Some(BlockKind::Function),
                "struct_specifier" => Some(BlockKind::Struct),
                _ => None,
            },
            Lang::Cpp => match kind {
                "function_definition" => Some(BlockKind::Function),
                "class_specifier" | "struct_specifier" => Some(BlockKind::Class),
                _ => None,
            },
            Lang::Java => match kind {
                "method_declaration" | "constructor_declaration" => Some(BlockKind::Function),
                "class_declaration" => Some(BlockKind::Class),
                "interface_declaration" => Some(BlockKind::Trait),
                "enum_declaration" => Some(BlockKind::Enum),
                _ => None,
            },
            Lang::CSharp => match kind {
                "method_declaration" | "constructor_declaration" => Some(BlockKind::Function),
                "class_declaration" => Some(BlockKind::Class),
                "interface_declaration" => Some(BlockKind::Trait),
                "enum_declaration" => Some(BlockKind::Enum),
                "struct_declaration" => Some(BlockKind::Struct),
                _ => None,
            },
            Lang::JavaScript => match kind {
                "function_declaration" | "method_definition" => Some(BlockKind::Function),
                "class_declaration" => Some(BlockKind::Class),
                _ => None,
            },
            Lang::Ruby => match kind {
                "method" | "singleton_method" => Some(BlockKind::Function),
                "class" | "module" => Some(BlockKind::Class),
                _ => None,
            },
            Lang::Php => match kind {
                "function_definition" | "method_declaration" => Some(BlockKind::Function),
                "class_declaration" => Some(BlockKind::Class),
                "interface_declaration" => Some(BlockKind::Trait),
                _ => None,
            },
            Lang::Html => match kind {
                "element" => Some(BlockKind::Element),
                _ => None,
            },
            Lang::Css => match kind {
                "rule_set" | "media_statement" => Some(BlockKind::Rule),
                _ => None,
            },
            Lang::Sql | Lang::Opaque => None,
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
            BlockKind::Class => "class",
            BlockKind::Element => "element",
            BlockKind::Rule => "rule",
            BlockKind::Statement => "statement",
            BlockKind::Chunk => "chunk",
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
    if let Some(n) = node.child_by_field_name("declarator") {
        let mut cursor = n.walk();
        for child in n.named_children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "field_identifier" {
                return src[child.byte_range()].to_string();
            }
        }
        return src[n.byte_range()].to_string();
    }
    if let Some(ty) = node.child_by_field_name("type") {
        let ty_s = src[ty.byte_range()].to_string();
        if let Some(tr) = node.child_by_field_name("trait") {
            return format!("{} for {}", &src[tr.byte_range()], ty_s);
        }
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            if child.kind() == "identifier" || child.kind() == "type_identifier" {
                return src[child.byte_range()].to_string();
            }
        }
        return ty_s;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if let Some(n) = child.child_by_field_name("name") {
            return src[n.byte_range()].to_string();
        }
        if child.kind() == "identifier" || child.kind() == "type_identifier" {
            return src[child.byte_range()].to_string();
        }
    }
    "<anon>".to_string()
}

fn parse_opaque_chunks(source: &str) -> Vec<Block> {
    if source.trim().is_empty() {
        return Vec::new();
    }
    let mut blocks = Vec::new();
    let mut chunk_start_byte = None;
    let mut chunk_start_line = 1;
    let mut chunk_end_byte = 0;
    let mut chunk_end_line = 1;

    let base_ptr = source.as_ptr() as usize;

    for (idx, line) in source.lines().enumerate() {
        let current_line = idx + 1;
        let line_start = line.as_ptr() as usize - base_ptr;
        let line_end = line_start + line.len();

        if line.trim().is_empty() {
            if let Some(start_byte) = chunk_start_byte {
                let text = &source[start_byte..chunk_end_byte];
                blocks.push(Block {
                    kind: BlockKind::Chunk,
                    name: format!("lines {}-{}", chunk_start_line, chunk_end_line),
                    hash: short_hash(text),
                    start_line: chunk_start_line,
                    end_line: chunk_end_line,
                    byte_start: start_byte,
                    byte_end: chunk_end_byte,
                });
                chunk_start_byte = None;
            }
        } else {
            if chunk_start_byte.is_none() {
                chunk_start_byte = Some(line_start);
                chunk_start_line = current_line;
            }
            chunk_end_byte = line_end;
            chunk_end_line = current_line;
        }
    }

    if let Some(start_byte) = chunk_start_byte {
        let text = &source[start_byte..chunk_end_byte];
        blocks.push(Block {
            kind: BlockKind::Chunk,
            name: format!("lines {}-{}", chunk_start_line, chunk_end_line),
            hash: short_hash(text),
            start_line: chunk_start_line,
            end_line: chunk_end_line,
            byte_start: start_byte,
            byte_end: chunk_end_byte,
        });
    }

    blocks
}

/// Parse Rust `source` into its top-level hashed blocks, in source order.
pub fn parse_blocks(source: &str) -> Vec<Block> {
    parse_blocks_lang(source, Lang::Rust)
}

/// Parse `source` into its top-level hashed blocks for `lang`, in source order.
/// Only top-level items are addressed in v1; unparseable or empty input yields an empty list.
pub fn parse_blocks_lang(source: &str, lang: Lang) -> Vec<Block> {
    if lang == Lang::Opaque || lang == Lang::Sql {
        return parse_opaque_chunks(source);
    }
    let mut parser = Parser::new();
    let language: tree_sitter::Language = match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Go => tree_sitter_go::LANGUAGE.into(),
        Lang::C => tree_sitter_c::LANGUAGE.into(),
        Lang::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Lang::Java => tree_sitter_java::LANGUAGE.into(),
        Lang::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Lang::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        Lang::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Lang::Html => tree_sitter_html::LANGUAGE.into(),
        Lang::Css => tree_sitter_css::LANGUAGE.into(),
        Lang::Sql | Lang::Opaque => return parse_opaque_chunks(source),
    };
    if parser.set_language(&language).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let root = tree.root_node();
    let mut blocks = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        if let Some(kind) = BlockKind::from_node_kind_lang(child.kind(), lang) {
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
    annotate_lang(source, Lang::Rust)
}

pub fn annotate_lang(source: &str, lang: Lang) -> String {
    let mut out = String::new();
    for b in parse_blocks_lang(source, lang) {
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

    #[test]
    fn python_top_level_def_is_one_block() {
        let src = "def alpha(x):\n    return x + 1";
        let blocks = parse_blocks_lang(src, Lang::Python);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind, BlockKind::Function);
        assert_eq!(blocks[0].name, "alpha");
        assert_eq!(&src[blocks[0].byte_start..blocks[0].byte_end], "def alpha(x):\n    return x + 1");
    }

    #[test]
    fn typescript_function_and_class_are_blocks() {
        let src = "function beta() {}\nclass Foo {}\ninterface Bar {}\n";
        let blocks = parse_blocks_lang(src, Lang::TypeScript);
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].name, "beta");
        assert_eq!(blocks[1].name, "Foo");
        assert_eq!(blocks[2].name, "Bar");
    }

    #[test]
    fn go_function_and_type_are_blocks() {
        let src = "func main() {}\ntype User struct{}\n";
        let blocks = parse_blocks_lang(src, Lang::Go);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].name, "main");
        assert_eq!(blocks[1].name, "User");
    }

    #[test]
    fn ast_block_parsing_for_new_languages() {
        let c_src = "int add(int a, int b) { return a + b; }\n";
        let c_blocks = parse_blocks_lang(c_src, Lang::C);
        assert_eq!(c_blocks.len(), 1);
        assert_eq!(c_blocks[0].kind, BlockKind::Function);
        assert_eq!(c_blocks[0].name, "add");

        let cpp_src = "class Vector { int x; };\n";
        let cpp_blocks = parse_blocks_lang(cpp_src, Lang::Cpp);
        assert_eq!(cpp_blocks.len(), 1);
        assert_eq!(cpp_blocks[0].kind, BlockKind::Class);
        assert_eq!(cpp_blocks[0].name, "Vector");

        let java_src = "public class App {\n}\n";
        let java_blocks = parse_blocks_lang(java_src, Lang::Java);
        assert_eq!(java_blocks.len(), 1);
        assert_eq!(java_blocks[0].kind, BlockKind::Class);
        assert_eq!(java_blocks[0].name, "App");

        let cs_src = "class Server {\n}\n";
        let cs_blocks = parse_blocks_lang(cs_src, Lang::CSharp);
        assert_eq!(cs_blocks.len(), 1);
        assert_eq!(cs_blocks[0].kind, BlockKind::Class);
        assert_eq!(cs_blocks[0].name, "Server");

        let js_src = "function greet(name) { return 'Hello ' + name; }\n";
        let js_blocks = parse_blocks_lang(js_src, Lang::JavaScript);
        assert_eq!(js_blocks.len(), 1);
        assert_eq!(js_blocks[0].kind, BlockKind::Function);
        assert_eq!(js_blocks[0].name, "greet");

        let rb_src = "def calculate\n  1 + 1\nend\n";
        let rb_blocks = parse_blocks_lang(rb_src, Lang::Ruby);
        assert_eq!(rb_blocks.len(), 1);
        assert_eq!(rb_blocks[0].kind, BlockKind::Function);
        assert_eq!(rb_blocks[0].name, "calculate");

        let php_src = "<?php\nfunction work() {}\n";
        let php_blocks = parse_blocks_lang(php_src, Lang::Php);
        assert_eq!(php_blocks.len(), 1);
        assert_eq!(php_blocks[0].kind, BlockKind::Function);
        assert_eq!(php_blocks[0].name, "work");

        let html_src = "<div class=\"container\">\n  <p>Hello</p>\n</div>\n";
        let html_blocks = parse_blocks_lang(html_src, Lang::Html);
        assert_eq!(html_blocks.len(), 1);
        assert_eq!(html_blocks[0].kind, BlockKind::Element);

        let css_src = ".header { color: red; }\n";
        let css_blocks = parse_blocks_lang(css_src, Lang::Css);
        assert_eq!(css_blocks.len(), 1);
        assert_eq!(css_blocks[0].kind, BlockKind::Rule);
    }

    #[test]
    fn opaque_fallback_chunker_parses_any_text_file() {
        let json_src = "{\n  \"name\": \"hadron\",\n  \"version\": \"1.0.0\"\n}\n\n{\n  \"key\": \"value\"\n}\n";
        let blocks = parse_blocks_lang(json_src, Lang::Opaque);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].kind, BlockKind::Chunk);
        assert_eq!(blocks[0].name, "lines 1-4");
        assert_eq!(&json_src[blocks[0].byte_start..blocks[0].byte_end], "{\n  \"name\": \"hadron\",\n  \"version\": \"1.0.0\"\n}");

        let yaml_src = "server:\n  port: 8080\n\ndatabase:\n  host: localhost\n";
        let yblocks = parse_blocks_lang(yaml_src, Lang::Opaque);
        assert_eq!(yblocks.len(), 2);
        assert_eq!(yblocks[0].name, "lines 1-2");
        assert_eq!(yblocks[1].name, "lines 4-5");
    }
}
