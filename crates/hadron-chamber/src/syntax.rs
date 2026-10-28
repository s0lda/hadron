//! Universal lexical code tokenizer and GPUI syntax highlighting helpers ("Code Synaptics").
//!
//! Provides fast, zero-dependency, UTF-8 char-boundary-safe syntax tokenization mapped
//! to `crate::theme::syntax_*()` semantic tokens for diff hunks (`Git > Changes`, `Graph`,
//! `Branches`, `3-Way Diff`) and automatic language inference for untagged Markdown code fences.

use std::ops::Range;

/// Semantic syntax token categories mapped to Chamber's 14 syntax theme tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxTokenKind {
    Keyword,
    Function,
    Type,
    String,
    Number,
    Comment,
    Operator,
    Attribute,
    Variable,
    VariableSpecial,
    Constant,
    Punctuation,
}

const KEYWORDS: &[&str] = &[
    // Rust
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum",
    "extern", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut",
    "pub", "ref", "return", "static", "struct", "trait", "type", "unsafe", "use", "where",
    "while", "yield",
    // Python / JS / TS / Go / Shell / C / SQL
    "class", "def", "del", "elif", "except", "export", "extends", "finally", "from", "func",
    "function", "global", "go", "import", "interface", "is", "lambda", "namespace", "new",
    "nonlocal", "not", "or", "package", "pass", "raise", "readonly", "select", "switch", "case",
    "default", "defer", "do", "done", "fi", "then", "throw", "try", "typeof", "var", "void",
    "with", "and",
];

const SPECIAL_VARS: &[&str] = &["self", "Self", "super", "this"];

const CONSTANTS: &[&str] = &[
    "true", "false", "True", "False", "None", "Some", "Ok", "Err", "null", "undefined", "nil",
    "NaN", "Infinity",
];

fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

fn is_special_var(word: &str) -> bool {
    SPECIAL_VARS.contains(&word)
}

fn is_builtin_constant(word: &str) -> bool {
    CONSTANTS.contains(&word)
}

fn is_screaming_constant(word: &str) -> bool {
    word.len() >= 2
        && word.bytes().any(|b| b.is_ascii_uppercase())
        && word
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
}

/// Tokenizes a single line of source code into non-overlapping `(Range<usize>, SyntaxTokenKind)`
/// spans guaranteed to lie on valid UTF-8 character boundaries.
pub fn tokenize_code_line(
    line: &str,
    file_hint: Option<&str>,
) -> Vec<(Range<usize>, SyntaxTokenKind)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let len = bytes.len();
    if len == 0 {
        return out;
    }

    let ext = file_hint
        .and_then(|p| std::path::Path::new(p).extension())
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_hash_comment_lang = matches!(
        ext.as_str(),
        "py" | "sh" | "bash" | "zsh" | "toml" | "yaml" | "yml" | "rb" | "pl" | "conf" | "dockerfile"
    );

    let mut i = 0;
    while i < len {
        // Ensure `i` is always at a UTF-8 char boundary.
        if !line.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let b = bytes[i];

        // Skip whitespace.
        if b.is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Line comments: `//` or `--` (SQL/Lua)
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'/' {
            out.push((i..len, SyntaxTokenKind::Comment));
            break;
        }

        // Block comments: `/* ... */`
        if i + 1 < len && bytes[i] == b'/' && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = if i + 1 < len { i + 2 } else { len };
            out.push((start..i, SyntaxTokenKind::Comment));
            continue;
        }

        // Rust attributes `#[...]` / `#![...]` or hash comments `# ...`
        if b == b'#' {
            if i + 1 < len && (bytes[i + 1] == b'[' || (bytes[i + 1] == b'!' && i + 2 < len && bytes[i + 2] == b'[')) {
                let start = i;
                let mut depth = 0usize;
                while i < len {
                    if bytes[i] == b'[' {
                        depth += 1;
                    } else if bytes[i] == b']' {
                        depth = depth.saturating_sub(1);
                        if depth == 0 {
                            i += 1;
                            break;
                        }
                    }
                    i += line[i..].chars().next().map_or(1, char::len_utf8);
                }
                out.push((start..i, SyntaxTokenKind::Attribute));
                continue;
            } else if is_hash_comment_lang || i == 0 || bytes[i - 1].is_ascii_whitespace() {
                // Treat `#` as a comment if followed by space or in hash-comment languages
                if is_hash_comment_lang || (i + 1 < len && bytes[i + 1] == b' ') {
                    out.push((i..len, SyntaxTokenKind::Comment));
                    break;
                }
            }
        }

        // Decorators / Mentions `@ident`
        if b == b'@' && i + 1 < len && (bytes[i + 1].is_ascii_alphabetic() || bytes[i + 1] == b'_') {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'-' | b'.')) {
                i += 1;
            }
            out.push((start..i, SyntaxTokenKind::Attribute));
            continue;
        }

        // String literals: `"..."`, `'...'` (excluding Rust lifetimes like `'a`), or `` `...` ``
        if b == b'"' || b == b'`' || b == b'\'' {
            if b == b'\'' {
                // Check if this is a Rust lifetime like `'static` or `'a` (not closed by `'` after 1 char / escape)
                let rest = &line[i + 1..];
                let ident_len: usize = rest
                    .bytes()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == b'_')
                    .count();
                if ident_len > 0 && (i + 1 + ident_len >= len || bytes[i + 1 + ident_len] != b'\'') {
                    let end = i + 1 + ident_len;
                    out.push((i..end, SyntaxTokenKind::Attribute));
                    i = end;
                    continue;
                }
            }
            let quote = b;
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\\' {
                    let step = line[i..].chars().take(2).map(char::len_utf8).sum::<usize>().max(1);
                    i = (i + step).min(len);
                    continue;
                }
                if bytes[i] == quote {
                    i += 1;
                    break;
                }
                i += line[i..].chars().next().map_or(1, char::len_utf8);
            }
            out.push((start..i, SyntaxTokenKind::String));
            continue;
        }

        // Numbers: decimal, hex `0x...`, binary `0b...`, octal `0o...`, floats
        if b.is_ascii_digit() {
            let start = i;
            while i < len
                && (bytes[i].is_ascii_alphanumeric()
                    || bytes[i] == b'_'
                    || (bytes[i] == b'.' && i + 1 < len && bytes[i + 1].is_ascii_digit()))
            {
                i += 1;
            }
            out.push((start..i, SyntaxTokenKind::Number));
            continue;
        }

        // Identifiers, keywords, types, functions, macros
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            let word = &line[start..i];

            // Macro invocation `ident!`
            if i < len && bytes[i] == b'!' && (i + 1 >= len || bytes[i + 1] != b'=') {
                i += 1;
                out.push((start..i, SyntaxTokenKind::Function));
                continue;
            }

            // Look ahead past spaces for `(` to detect function calls/definitions
            let mut look = i;
            while look < len && bytes[look] == b' ' {
                look += 1;
            }
            let followed_by_paren = look < len && bytes[look] == b'(';

            let kind = if is_keyword(word) {
                SyntaxTokenKind::Keyword
            } else if is_special_var(word) {
                SyntaxTokenKind::VariableSpecial
            } else if is_builtin_constant(word) || is_screaming_constant(word) {
                SyntaxTokenKind::Constant
            } else if followed_by_paren {
                SyntaxTokenKind::Function
            } else if word.as_bytes()[0].is_ascii_uppercase() {
                SyntaxTokenKind::Type
            } else {
                SyntaxTokenKind::Variable
            };

            out.push((start..i, kind));
            continue;
        }

        // Multi-char or single-char operators
        if matches!(
            b,
            b'+' | b'-' | b'*' | b'/' | b'=' | b'<' | b'>' | b'!' | b'&' | b'|' | b'^' | b'%' | b'~' | b'?'
        ) {
            let start = i;
            while i < len
                && matches!(
                    bytes[i],
                    b'+' | b'-' | b'*' | b'/' | b'=' | b'<' | b'>' | b'!' | b'&' | b'|' | b'^' | b'%' | b'~' | b'?'
                )
            {
                i += 1;
            }
            out.push((start..i, SyntaxTokenKind::Operator));
            continue;
        }

        // Punctuation / delimiters
        if matches!(b, b'(' | b')' | b'[' | b']' | b'{' | b'}' | b',' | b';' | b':' | b'.') {
            let start = i;
            if b == b':' && i + 1 < len && bytes[i + 1] == b':' {
                i += 2;
                out.push((start..i, SyntaxTokenKind::Operator));
            } else {
                i += 1;
                out.push((start..i, SyntaxTokenKind::Punctuation));
            }
            continue;
        }

        // Advance by one full UTF-8 character for anything else (emojis, box drawing, etc.)
        let step = line[i..].chars().next().map_or(1, char::len_utf8);
        i += step;
    }

    out
}

/// Infers a syntax-highlighting language tag for a bare code block from its content.
pub fn infer_code_language(code: &str) -> &'static str {
    let trimmed = code.trim();
    if trimmed.is_empty() {
        return "rust";
    }
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        if trimmed.contains("\":") {
            return "json";
        }
    }
    for line in trimmed.lines().take(12) {
        let l = line.trim();
        if l.starts_with("diff --git ") || l.starts_with("@@ ") || l.starts_with("--- a/") || l.starts_with("+++ b/") {
            return "diff";
        }
        if l.starts_with("fn ")
            || l.starts_with("pub fn ")
            || l.starts_with("pub(crate) ")
            || l.starts_with("pub(super) ")
            || l.starts_with("let mut ")
            || l.starts_with("impl ")
            || l.starts_with("use crate::")
            || l.starts_with("use std::")
            || l.starts_with("#[derive")
            || l.starts_with("#[test]")
        {
            return "rust";
        }
        if l.starts_with("def ") || l.starts_with("from ") && l.contains(" import ") || l.starts_with("elif ") {
            return "python";
        }
        if l.starts_with("const ") || l.starts_with("export ") || l.starts_with("interface ") || l.starts_with("function ") {
            return "typescript";
        }
        if l.starts_with("#!/bin/") || l.starts_with("$ ") || l.starts_with("cargo ") || l.starts_with("git ") {
            return "sh";
        }
        if l.starts_with("[package]") || l.starts_with("[dependencies]") {
            return "toml";
        }
    }
    "rust"
}

/// Annotates bare Markdown fenced code blocks (```` ``` ```` with no language tag) with an
/// inferred language identifier so `TextView::markdown` applies tree-sitter syntax highlighting.
pub fn annotate_bare_code_fences(markdown: &str) -> String {
    if !markdown.contains("```") {
        return markdown.to_string();
    }
    let lines: Vec<&str> = markdown.lines().collect();
    let mut out = String::with_capacity(markdown.len() + 32);
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed_start = line.trim_start();
        let tick_count = trimmed_start.bytes().take_while(|&b| b == b'`').count();

        if tick_count >= 3 {
            let after_ticks = trimmed_start[tick_count..].trim();
            let prefix_ws = &line[..line.len() - trimmed_start.len()];
            let fence_marker = &trimmed_start[..tick_count];

            // Find matching closing fence
            let mut close_ix = None;
            let mut j = i + 1;
            while j < lines.len() {
                let cand = lines[j].trim_start();
                let cand_ticks = cand.bytes().take_while(|&b| b == b'`').count();
                if cand_ticks >= tick_count && cand[cand_ticks..].trim().is_empty() {
                    close_ix = Some(j);
                    break;
                }
                j += 1;
            }

            if let Some(end_ix) = close_ix {
                if after_ticks.is_empty() {
                    let block_body = lines[i + 1..end_ix].join("\n");
                    let inferred = infer_code_language(&block_body);
                    out.push_str(prefix_ws);
                    out.push_str(fence_marker);
                    out.push_str(inferred);
                    out.push('\n');
                } else {
                    out.push_str(line);
                    out.push('\n');
                }
                for body_line in &lines[i + 1..=end_ix] {
                    out.push_str(body_line);
                    out.push('\n');
                }
                i = end_ix + 1;
                continue;
            }
        }

        out.push_str(line);
        if i + 1 < lines.len() || markdown.ends_with('\n') {
            out.push('\n');
        }
        i += 1;
    }

    if !markdown.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}

#[cfg(feature = "gui")]
pub fn token_color(kind: SyntaxTokenKind) -> gpui::Rgba {
    use crate::theme;
    match kind {
        SyntaxTokenKind::Keyword => theme::syntax_keyword(),
        SyntaxTokenKind::Function => theme::syntax_function(),
        SyntaxTokenKind::Type => theme::syntax_type(),
        SyntaxTokenKind::String => theme::syntax_string(),
        SyntaxTokenKind::Number => theme::syntax_number(),
        SyntaxTokenKind::Comment => theme::syntax_comment(),
        SyntaxTokenKind::Operator => theme::syntax_operator(),
        SyntaxTokenKind::Attribute => theme::syntax_attribute(),
        SyntaxTokenKind::Variable => theme::syntax_variable(),
        SyntaxTokenKind::VariableSpecial => theme::syntax_variable_special(),
        SyntaxTokenKind::Constant => theme::syntax_constant(),
        SyntaxTokenKind::Punctuation => theme::syntax_punctuation(),
    }
}

#[cfg(feature = "gui")]
pub fn highlight_code_line_element(line: &str, file_hint: Option<&str>) -> gpui::AnyElement {
    use gpui::{HighlightStyle, Hsla, IntoElement, SharedString, StyledText};

    if line.is_empty() {
        return StyledText::new(SharedString::from(" ")).into_any_element();
    }

    let spans = tokenize_code_line(line, file_hint);
    if spans.is_empty() {
        return StyledText::new(SharedString::from(line.to_string())).into_any_element();
    }

    let highlights: Vec<(Range<usize>, HighlightStyle)> = spans
        .into_iter()
        .map(|(range, kind)| {
            let mut style = HighlightStyle::default();
            style.color = Some(Hsla::from(token_color(kind)));
            if kind == SyntaxTokenKind::Comment {
                style.font_style = Some(gpui::FontStyle::Italic);
            }
            (range, style)
        })
        .collect();

    StyledText::new(SharedString::from(line.to_string()))
        .with_highlights(highlights)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tokenize_rust_line_and_char_boundaries() {
        let line = "pub async fn launch_swarm(id: &str) -> Result<Quark> { // 🚀 launch";
        let tokens = tokenize_code_line(line, Some("src/main.rs"));
        assert!(!tokens.is_empty());
        for (range, _) in &tokens {
            assert!(line.is_char_boundary(range.start));
            assert!(line.is_char_boundary(range.end));
        }
        assert_eq!(tokens[0], (0..3, SyntaxTokenKind::Keyword)); // pub
        assert_eq!(tokens[1], (4..9, SyntaxTokenKind::Keyword)); // async
        assert_eq!(tokens[2], (10..12, SyntaxTokenKind::Keyword)); // fn
        assert_eq!(tokens[3], (13..25, SyntaxTokenKind::Function)); // launch_swarm
        assert!(tokens.iter().any(|(_, k)| *k == SyntaxTokenKind::Type));
        assert!(tokens.iter().any(|(_, k)| *k == SyntaxTokenKind::Comment));
    }

    #[test]
    fn test_tokenize_strings_numbers_and_attributes() {
        let line = "#[derive(Debug)] const MAX_RETRIES: usize = 42; let s = \"hello \\\"world\\\"\";";
        let tokens = tokenize_code_line(line, Some("lib.rs"));
        for (range, _) in &tokens {
            assert!(line.is_char_boundary(range.start));
            assert!(line.is_char_boundary(range.end));
        }
        assert_eq!(tokens[0].1, SyntaxTokenKind::Attribute);
        assert!(tokens.iter().any(|(_, k)| *k == SyntaxTokenKind::Constant));
        assert!(tokens.iter().any(|(_, k)| *k == SyntaxTokenKind::Number));
        assert!(tokens.iter().any(|(_, k)| *k == SyntaxTokenKind::String));
    }

    #[test]
    fn test_annotate_bare_code_fences_infers_language() {
        let md = "Here is code:\n```\npub fn hello() -> usize {\n    42\n}\n```\nAnd tagged:\n```python\ndef f(): pass\n```";
        let annotated = annotate_bare_code_fences(md);
        assert!(annotated.contains("```rust\npub fn hello()"));
        assert!(annotated.contains("```python\ndef f(): pass"));
    }
}
