//! Rule 1, mechanised: does a newly added `pub` item have a production caller?
//!
//! Text search, not name resolution — so it is an **advisory**: a hit with zero
//! callers is a strong "implemented, unwired" signal, never proof. Items carrying a
//! non-inert attribute (`#[tool]`, `#[no_mangle]`, …) are skipped because macro or
//! registry dispatch hides their callers from text search.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnwiredSymbol {
    pub symbol: String,
    /// `fn`, `struct`, `enum` or `trait`.
    pub kind: String,
    pub file_path: String,
    pub line_number: usize,
    /// Named only by `use` / `pub use` lines: re-exported, never called.
    pub reexport_only: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LivenessReport {
    pub checked_symbols: usize,
    pub unwired: Vec<UnwiredSymbol>,
}

struct Candidate {
    symbol: String,
    kind: String,
    file_path: String,
    line_number: usize,
}

fn def_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r#"^\s*pub\s+(?:(?:async|const|unsafe|extern\s+"[^"]*")\s+)*(fn|struct|enum|trait)\s+([A-Za-z_][A-Za-z0-9_]*)"#)
            .expect("static regex")
    })
}

/// Attributes that never dispatch a call on their own.
const INERT_ATTRS: [&str; 10] =
    ["derive", "inline", "must_use", "allow", "warn", "deny", "doc", "serde", "cfg_attr", "deprecated"];

fn is_test_path(path: &str) -> bool {
    let p = format!("/{}", path.replace('\\', "/"));
    ["/tests/", "/benches/", "/examples/"].iter().any(|d| p.contains(d))
}

/// Index of the first `#[cfg(test)]` line: everything from there on is test code.
fn test_tail_start(lines: &[&str]) -> usize {
    lines.iter().position(|l| l.trim_start().starts_with("#[cfg(test)]")).unwrap_or(lines.len())
}

/// New `pub` items in the `+` lines of production `.rs` files of a unified diff.
fn added_candidates(workspace_root: &Path, diff: &str) -> Vec<Candidate> {
    let mut out = Vec::new();
    let mut file: Option<String> = None;
    let mut file_lines: Vec<String> = Vec::new();
    let mut tail = usize::MAX;
    let mut new_line = 0usize;
    let mut in_test = false;
    for raw in diff.lines() {
        if let Some(path) = raw.strip_prefix("+++ ") {
            let path = path.trim().trim_start_matches("b/");
            file = (path.ends_with(".rs") && path != "/dev/null" && !is_test_path(path)).then(|| path.to_string());
            file_lines = file
                .as_ref()
                .and_then(|f| std::fs::read_to_string(workspace_root.join(f)).ok())
                .map(|s| s.lines().map(str::to_string).collect())
                .unwrap_or_default();
            let refs: Vec<&str> = file_lines.iter().map(String::as_str).collect();
            tail = if refs.is_empty() { usize::MAX } else { test_tail_start(&refs) };
            in_test = false;
            continue;
        }
        if raw.starts_with("--- ") || raw.starts_with("diff ") || raw.starts_with("index ") {
            continue;
        }
        if let Some(hunk) = raw.strip_prefix("@@ ") {
            // `@@ -a,b +c,d @@` → next new-file line is c.
            new_line = hunk
                .split_whitespace()
                .find_map(|t| t.strip_prefix('+'))
                .and_then(|t| t.split(',').next()?.parse().ok())
                .unwrap_or(0);
            continue;
        }
        let Some(f) = file.as_ref() else { continue };
        let (is_added, text) = match raw.chars().next() {
            Some('+') => (true, &raw[1..]),
            Some(' ') => (false, &raw[1..]),
            Some('-') => continue,
            _ => continue,
        };
        if text.trim_start().starts_with("#[cfg(test)]") {
            in_test = true;
        }
        let line_number = new_line;
        new_line += 1;
        if !is_added || in_test || line_number == 0 || line_number > tail {
            continue;
        }
        let Some(cap) = def_re().captures(text) else { continue };
        if has_dispatching_attribute(&file_lines, line_number) {
            continue;
        }
        out.push(Candidate { symbol: cap[2].to_string(), kind: cap[1].to_string(), file_path: f.clone(), line_number });
    }
    out
}

/// Does the item defined at 1-based `line_number` carry an attribute that may
/// register it for macro/registry dispatch?
fn has_dispatching_attribute(file_lines: &[String], line_number: usize) -> bool {
    file_lines
        .iter()
        .take(line_number.saturating_sub(1))
        .rev()
        .map(|l| l.trim())
        .take_while(|l| l.starts_with("#[") || l.starts_with("///") || l.starts_with("//!") || l.ends_with(")]"))
        .filter_map(|l| l.strip_prefix("#["))
        .any(|attr| !INERT_ATTRS.iter().any(|inert| attr.starts_with(inert)))
}

fn production_rs_files(root: &Path) -> Vec<PathBuf> {
    const SKIP: [&str; 6] = ["target", "tests", "benches", "examples", "node_modules", "vendor"];
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_dir() {
                if !name.starts_with('.') && !SKIP.contains(&name.as_str()) {
                    stack.push(path);
                }
            } else if ft.is_file() && name.ends_with(".rs") {
                out.push(path);
            }
        }
    }
    out
}

/// For each candidate: `(real references, use-line mentions)` across production code.
fn count_references(root: &Path, candidates: &[Candidate]) -> Vec<(usize, usize)> {
    let word_res: Vec<Regex> = candidates
        .iter()
        .map(|c| Regex::new(&format!(r"\b{}\b", regex::escape(&c.symbol))).expect("escaped symbol"))
        .collect();
    let mut counts = vec![(0usize, 0usize); candidates.len()];
    for path in production_rs_files(root) {
        let Ok(content) = std::fs::read_to_string(&path) else { continue };
        let lines: Vec<&str> = content.lines().collect();
        for line in &lines[..test_tail_start(&lines)] {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            for (i, re) in word_res.iter().enumerate() {
                if !re.is_match(t) {
                    continue;
                }
                if def_re().captures(t).is_some_and(|c| c[2] == candidates[i].symbol)
                    || t.contains(&format!("{} {}", candidates[i].kind, candidates[i].symbol))
                {
                    continue; // the definition itself
                }
                let is_use = ["use ", "pub use ", "pub(crate) use ", "pub(super) use "].iter().any(|p| t.starts_with(p));
                if is_use {
                    counts[i].1 += 1;
                } else {
                    counts[i].0 += 1;
                }
            }
        }
    }
    counts
}

/// Newly added production `pub fn|struct|enum|trait` items in `diff` with no
/// production reference anywhere under `workspace_root` (tests, `#[cfg(test)]`
/// tails, comments and `use` lines do not count as callers).
pub fn audit_diff_liveness(workspace_root: &Path, diff: &str) -> LivenessReport {
    let candidates = added_candidates(workspace_root, diff);
    if candidates.is_empty() {
        return LivenessReport::default();
    }
    let counts = count_references(workspace_root, &candidates);
    let unwired = candidates
        .iter()
        .zip(counts)
        .filter(|(_, (real, _))| *real == 0)
        .map(|(c, (_, uses))| UnwiredSymbol {
            symbol: c.symbol.clone(),
            kind: c.kind.clone(),
            file_path: c.file_path.clone(),
            line_number: c.line_number,
            reexport_only: uses > 0,
        })
        .collect();
    LivenessReport { checked_symbols: candidates.len(), unwired }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    fn diff_adding(path: &str, body: &str) -> String {
        let lines: Vec<&str> = body.lines().collect();
        let mut d = format!("diff --git a/{path} b/{path}\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1,{} @@\n", lines.len());
        for l in lines {
            d.push('+');
            d.push_str(l);
            d.push('\n');
        }
        d
    }

    const LIB: &str = "\
pub mod feature;
pub use feature::Reexported;
";

    const FEATURE: &str = "\
pub fn wired() -> u8 { 1 }
pub fn unwired_fn() {}
pub struct Reexported;
pub(crate) fn crate_only() {}
#[tool(name = \"x\")]
pub async fn registry_dispatched() {}
/// docs
#[inline]
pub fn tested_only() {}
pub enum Mode { A }
#[cfg(test)]
mod tests {
    pub fn helper_in_tests() {}
    fn t() { super::tested_only(); super::unwired_fn(); }
}
";

    #[test]
    fn flags_unwired_reexport_only_and_test_only_items() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "crates/a/src/lib.rs", LIB);
        write(root, "crates/a/src/feature.rs", FEATURE);
        write(root, "crates/b/src/main.rs", "fn main() { let _ = a::feature::wired(); let _m: a::feature::Mode; }\n");
        write(root, "crates/a/tests/it.rs", "fn t() { a::feature::tested_only(); }\n");
        write(root, "target/debug/gen.rs", "fn x() { unwired_fn(); }\n");

        let report = audit_diff_liveness(root, &diff_adding("crates/a/src/feature.rs", FEATURE));
        let names: Vec<(&str, bool)> =
            report.unwired.iter().map(|u| (u.symbol.as_str(), u.reexport_only)).collect();
        assert_eq!(
            names,
            vec![("unwired_fn", false), ("Reexported", true), ("tested_only", false)],
            "{report:?}"
        );
        assert_eq!(report.checked_symbols, 5, "wired, unwired_fn, Reexported, tested_only, Mode");
        let u = &report.unwired[0];
        assert_eq!((u.kind.as_str(), u.file_path.as_str(), u.line_number), ("fn", "crates/a/src/feature.rs", 2));
    }

    #[test]
    fn ignores_test_files_non_rust_and_context_lines() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "crates/a/src/x.rs", "pub fn old_unwired() {}\npub fn new_one() {}\n");
        let diff = "\
--- a/crates/a/src/x.rs
+++ b/crates/a/src/x.rs
@@ -1,1 +1,2 @@
 pub fn old_unwired() {}
+pub fn new_one() {}
--- a/crates/a/tests/t.rs
+++ b/crates/a/tests/t.rs
@@ -0,0 +1,1 @@
+pub fn in_integration_test() {}
--- a/README.md
+++ b/README.md
@@ -0,0 +1,1 @@
+pub fn in_markdown() {}
";
        let report = audit_diff_liveness(root, diff);
        assert_eq!(report.checked_symbols, 1);
        assert_eq!(report.unwired.len(), 1);
        assert_eq!((report.unwired[0].symbol.as_str(), report.unwired[0].line_number), ("new_one", 2));
        assert_eq!(audit_diff_liveness(root, ""), LivenessReport::default());
    }
}
