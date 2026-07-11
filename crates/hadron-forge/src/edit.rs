use crate::block::parse_blocks;

/// An agent's proposed replacement of the block currently hashing to
/// `target_hash` with `new_text`. The hash is the optimistic-concurrency token:
/// if the block changed since the agent read it, the hash no longer matches and
/// the edit is rejected instead of silently clobbering a concurrent change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedEdit {
    pub target_hash: String,
    pub new_text: String,
}

/// The result of attempting a [`HashedEdit`] against a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditOutcome {
    /// The target block was found uniquely and replaced; carries the new source.
    Applied { new_source: String },
    /// The edit was refused; `reason` explains why (stale or ambiguous hash).
    Rejected { reason: String },
}

/// Apply `edit` to `source` under optimistic concurrency.
///
/// - Exactly one current block hashes to `edit.target_hash` → splice `new_text`
///   in its place and return [`EditOutcome::Applied`].
/// - No block matches → the block was modified or removed since the agent read
///   it; return [`EditOutcome::Rejected`] so the agent pulls fresh state and retries.
/// - More than one block matches (identical text) → ambiguous; reject rather
///   than guess which one.
pub fn apply_edit(source: &str, edit: &HashedEdit) -> EditOutcome {
    let matches: Vec<_> = parse_blocks(source)
        .into_iter()
        .filter(|b| b.hash == edit.target_hash)
        .collect();
    match matches.as_slice() {
        [b] => {
            let mut new_source = String::with_capacity(
                source.len() - (b.byte_end - b.byte_start) + edit.new_text.len(),
            );
            new_source.push_str(&source[..b.byte_start]);
            new_source.push_str(&edit.new_text);
            new_source.push_str(&source[b.byte_end..]);
            EditOutcome::Applied { new_source }
        }
        [] => EditOutcome::Rejected {
            reason: format!(
                "stale hash {}: no current block matches (modified or removed) — pull and retry",
                edit.target_hash
            ),
        },
        _ => EditOutcome::Rejected {
            reason: format!(
                "ambiguous hash {}: {} blocks share it — widen the hash or disambiguate",
                edit.target_hash,
                matches.len()
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::parse_blocks;

    const SRC: &str = "pub fn alpha(x: i32) -> i32 { x + 1 }\n\nstruct Point { x: f64 }\n";

    #[test]
    fn applies_edit_when_hash_matches() {
        let alpha_hash = parse_blocks(SRC)[0].hash.clone();
        let edit = HashedEdit {
            target_hash: alpha_hash,
            new_text: "pub fn alpha(x: i32) -> i32 { x + 2 }".into(),
        };
        match apply_edit(SRC, &edit) {
            EditOutcome::Applied { new_source } => {
                assert!(new_source.contains("x + 2"));
                assert!(!new_source.contains("x + 1"));
                // Struct is untouched and still parses.
                assert!(new_source.contains("struct Point { x: f64 }"));
                assert_eq!(parse_blocks(&new_source).len(), 2);
            }
            other => panic!("expected Applied, got {other:?}"),
        }
    }

    #[test]
    fn rejects_stale_hash() {
        let edit = HashedEdit {
            target_hash: "000000".into(),
            new_text: "whatever".into(),
        };
        match apply_edit(SRC, &edit) {
            EditOutcome::Rejected { reason } => assert!(reason.contains("stale hash")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }

    #[test]
    fn concurrent_edit_to_same_block_is_rejected() {
        // Agent A reads alpha's hash.
        let a_hash = parse_blocks(SRC)[0].hash.clone();
        // Agent B modifies alpha first (its edit lands).
        let b_edit = HashedEdit {
            target_hash: a_hash.clone(),
            new_text: "pub fn alpha(x: i32) -> i32 { x + 99 }".into(),
        };
        let after_b = match apply_edit(SRC, &b_edit) {
            EditOutcome::Applied { new_source } => new_source,
            other => panic!("B should apply, got {other:?}"),
        };
        // Agent A now submits an edit against A's ORIGINAL (now stale) hash.
        let a_edit = HashedEdit {
            target_hash: a_hash,
            new_text: "pub fn alpha(x: i32) -> i32 { x + 7 }".into(),
        };
        match apply_edit(&after_b, &a_edit) {
            EditOutcome::Rejected { reason } => assert!(reason.contains("stale hash")),
            other => panic!("A's stale edit must be rejected, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_hash_is_rejected() {
        // Two byte-identical impl blocks hash the same.
        let src = "impl A {}\nimpl A {}\n";
        let blocks = parse_blocks(src);
        assert_eq!(blocks[0].hash, blocks[1].hash);
        let edit = HashedEdit {
            target_hash: blocks[0].hash.clone(),
            new_text: "impl A { fn x() {} }".into(),
        };
        match apply_edit(src, &edit) {
            EditOutcome::Rejected { reason } => assert!(reason.contains("ambiguous hash")),
            other => panic!("expected Rejected, got {other:?}"),
        }
    }
}
