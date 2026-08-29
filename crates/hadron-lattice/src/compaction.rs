use std::collections::HashSet;

pub struct NucleusCompactor;

impl NucleusCompactor {
    pub fn compact_index(raw: &str) -> String {
        let mut seen = HashSet::new();
        let mut output = Vec::new();

        for line in raw.lines() {
            if line.starts_with("- [") {
                if let Some(slug_end) = line.find(']') {
                    let slug = &line[3..slug_end];
                    if !seen.insert(slug.to_string()) {
                        continue;
                    }
                }
            }
            output.push(line);
        }
        output.join("\n")
    }

    pub fn check_budget(index_content: &str, max_bytes: usize) -> bool {
        index_content.len() <= max_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nucleus_compaction_removes_duplicate_slugs() {
        let raw_index = "- [bug-a](notes/bug-a.md) — Bug A hook\n- [bug-a](notes/bug-a.md) — Duplicate hook\n- [bug-b](notes/bug-b.md) — Bug B hook";
        let compacted = NucleusCompactor::compact_index(raw_index);
        assert_eq!(compacted.lines().count(), 2);
        assert!(compacted.contains("bug-a"));
        assert!(compacted.contains("bug-b"));
    }
}
