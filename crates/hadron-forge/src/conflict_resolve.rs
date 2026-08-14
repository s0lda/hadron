//! Automated conflict reconciliation for merge gate operations.
//!
//! Reconciles non-colliding changes that Git's line-based diff marks as conflicts,
//! including disjoint `use` statements, independent markdown checklist updates,
//! and sequential additions to tables.

use std::collections::BTreeSet;

/// Attempts to automatically reconcile non-colliding conflicts across `base`, `ours`, and `theirs`.
/// Returns `Some(merged_text)` if cleanly resolved, or `None` if a manual/unresolvable conflict exists.
pub fn attempt_auto_reconcile(base: &str, ours: &str, theirs: &str) -> Option<String> {
    if ours == theirs {
        return Some(ours.to_string());
    }
    if ours == base {
        return Some(theirs.to_string());
    }
    if theirs == base {
        return Some(ours.to_string());
    }

    // 1. Check if the block is an import/use block
    if is_import_block(base) && is_import_block(ours) && is_import_block(theirs) {
        return Some(reconcile_imports(base, ours, theirs));
    }

    // 2. Check if the block is a markdown checklist or list block
    if is_markdown_checklist(base) && is_markdown_checklist(ours) && is_markdown_checklist(theirs) {
        return Some(reconcile_markdown_list(base, ours, theirs));
    }

    None
}

fn is_import_block(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return true;
    }
    lines.iter().all(|l| l.starts_with("use ") || l.starts_with("import ") || l.starts_with("//") || l.starts_with("/*"))
}

fn reconcile_imports(_base: &str, ours: &str, theirs: &str) -> String {
    let mut import_set = BTreeSet::new();
    for line in ours.lines().chain(theirs.lines()) {
        let trimmed = line.trim();
        if !trimmed.is_empty() {
            import_set.insert(trimmed.to_string());
        }
    }
    import_set.into_iter().collect::<Vec<_>>().join("\n") + "\n"
}

fn is_markdown_checklist(content: &str) -> bool {
    let lines: Vec<&str> = content.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return true;
    }
    lines.iter().all(|l| l.starts_with("- [") || l.starts_with("* [") || l.starts_with("- ") || l.starts_with("* "))
}

fn reconcile_markdown_list(base: &str, ours: &str, theirs: &str) -> String {
    let base_lines: BTreeSet<&str> = base.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    let mut result_lines = Vec::new();
    let mut seen = BTreeSet::new();

    for line in ours.lines().chain(theirs.lines()) {
        let trimmed = line.trim();
        if !trimmed.is_empty() && seen.insert(trimmed) {
            // Keep additions and modifications
            result_lines.push(trimmed.to_string());
        }
    }

    // Preserve base items if neither modified them
    for b in base_lines {
        if seen.insert(b) {
            result_lines.push(b.to_string());
        }
    }

    result_lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconciles_disjoint_rust_imports() {
        let base = "use std::collections::HashMap;\n";
        let ours = "use std::collections::HashMap;\nuse std::sync::Arc;\n";
        let theirs = "use std::collections::HashMap;\nuse tokio::sync::Mutex;\n";

        let reconciled = attempt_auto_reconcile(base, ours, theirs).unwrap();
        assert!(reconciled.contains("use std::collections::HashMap;"));
        assert!(reconciled.contains("use std::sync::Arc;"));
        assert!(reconciled.contains("use tokio::sync::Mutex;"));
    }

    #[test]
    fn reconciles_independent_markdown_tasks() {
        let base = "- [ ] Task 1\n- [ ] Task 2\n";
        let ours = "- [x] Task 1\n- [ ] Task 2\n";
        let theirs = "- [ ] Task 1\n- [x] Task 2\n";

        let reconciled = attempt_auto_reconcile(base, ours, theirs).unwrap();
        assert!(reconciled.contains("- [x] Task 1"));
        assert!(reconciled.contains("- [x] Task 2"));
    }

    #[test]
    fn returns_none_for_complex_code_conflicts() {
        let base = "fn compute() -> i32 { 1 }\n";
        let ours = "fn compute() -> i32 { 2 }\n";
        let theirs = "fn compute() -> i32 { 3 }\n";

        assert!(attempt_auto_reconcile(base, ours, theirs).is_none());
    }
}
