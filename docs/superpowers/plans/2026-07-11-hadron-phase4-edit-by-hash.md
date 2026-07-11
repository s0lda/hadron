# Hadron Phase 4 (core slice) — Edit-by-Hash: AST Block Hashing & Optimistic Concurrency

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Give Hadron the pure, testable heart of Phase 4's "manage concurrent, multi-agent file editing without corrupting line numbers": parse a Rust source file into logical blocks (functions, structs, enums, impls, traits), hash each block with blake3, and reconcile an agent's edit against a block's hash so a stale edit is *rejected* instead of clobbering another agent's change.

**Architecture:** A **new, dependency-isolated crate `hadron-forge`** holding two pure modules. `block` parses source with `tree-sitter` + `tree-sitter-rust` and hashes each top-level item with `blake3`, producing addressable `Block`s (kind, name, short hash, line span, byte span). `edit` implements optimistic concurrency: `apply_edit(source, HashedEdit{target_hash, new_text})` splices the replacement in only when exactly one current block still hashes to `target_hash`, otherwise it returns `Rejected` (stale or ambiguous). Everything is a pure function of `&str` — no filesystem, no process, no network, no GPUI — so it is exhaustively unit-testable with inline source strings and spends nothing.

**Why a separate crate (not `hadron-gluon`):** the roadmap files this under `hadron-gluon`, but `tree-sitter`/`blake3` are heavy C-compiling deps that must not leak into the lightweight `hadron-lattice` schema crate (which `hadron-chamber` depends on) nor bloat `hadron-gluon` unnecessarily. A standalone `hadron-forge` keeps the heavy deps confined, is reusable by both the gluon engine (edit application) and the chamber (block-overlay visualization, Phase 5), and — importantly — **merges cleanly alongside Phase 3**, which is concurrently editing `hadron-lattice` and `hadron-gluon`. When Phase 3 and the swarm-loop land, the engine takes a dependency on `hadron-forge`; nothing here touches Phase 3's files.

**Tech Stack:** Rust (edition 2021), `tree-sitter = "0.22"`, `tree-sitter-rust = "0.21"`, `blake3 = "1"`. No dev-deps beyond the std test harness.

> **Execution status (2026-07-11):** All 3 tasks **executed and committed** on branch `worktree-phase4-edit-by-hash` (branched from `main`, isolated from Gemini's concurrent Phase 3 work). 9 new `hadron-forge` tests green (5 block + 4 edit, incl. the concurrent-edit-rejection thesis test); full `cargo test --workspace` = 73 passed / 0 failed; `clippy -p hadron-forge` clean. Zero API spend, zero external process, no GPUI in the path. Deferred items (notify watcher, tokio/reqwest swarm loop, new `Kind` event variants) intentionally held — they extend Phase 3's `engine.rs`/`event.rs` and must merge after Phase 3 lands.

**This is the isolable core of Phase 4** (roadmap: `docs/plans/001_Initial_Plan.md` §"Phase 4: The 0-CPU File Bus & Edit-by-Hash"). Deliberately **out of scope** for this slice (see "Deferred / bought land"): the `notify` filesystem watcher, the tokio 0-CPU swarm loop, `reqwest` API calls, and any new `hadron-lattice` `Kind` variants — those extend Phase 3's `engine.rs`/`event.rs` and must merge *after* Phase 3.

## Global Constraints

- **Rust edition:** `2021`. Use latest stable Rust.
- **Pure and offline.** Every function in this plan is a pure function of its inputs — no filesystem, subprocess, network, or clock. Tests spend zero budget and spawn nothing.
- **Append-only field / unknown-tolerant readers** remain the project contract, but this slice adds nothing to the field yet (no new event kinds) — it is a library the engine will later call.
- **Do not touch Phase 3's files** (`hadron-lattice/src/event.rs`, `hadron-lattice/src/projection.rs`, `hadron-gluon/src/engine.rs`, `hadron-gluon/src/ledger.rs`, `hadron-gluon/src/lib.rs`, `hadron-gluon/src/mock.rs`, `hadron-gluon/src/adapter/claude.rs`, and their `Cargo.toml` dep lists). This plan only creates a new crate and adds one line to the root `Cargo.toml` members list.
- **Vocabulary (use these exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, ledger, block, hash, forge.
- **Hash format:** blake3 hex, truncated to `HASH_LEN = 6` chars (matches the roadmap's `[Hash: 9f86d0]`). Widening to a full-length hash for a multi-file production bus is bought land.

## Validated API facts (confirmed against the installed crates before writing this plan)

- `tree_sitter_rust::language()` returns a `tree_sitter::Language`; `parser.set_language(&lang)` on `tree-sitter 0.22`.
- Top-level Rust items are **depth-1 children of the root node**, with node kinds `function_item`, `struct_item`, `enum_item`, `impl_item`, `trait_item`.
- `node.child_by_field_name("name")` yields the item name for fn/struct/enum/trait. `impl_item` has **no `name`** — use `child_by_field_name("type")` (e.g. `Point`) and optional `child_by_field_name("trait")` (e.g. `Draw` in `impl Draw for Point`).
- `node.byte_range()` gives the block's byte span; `node.start_position().row` / `end_position().row` are 0-based line rows.
- `blake3::hash(bytes).to_hex()` derefs to `str`; `&…[..6]` yields the short hash.

---

### Task 1: Scaffold `hadron-forge` + parse-and-hash blocks

**Files:**
- Create: `crates/hadron-forge/Cargo.toml`
- Create: `crates/hadron-forge/src/lib.rs`
- Create: `crates/hadron-forge/src/block.rs`
- Modify: root `Cargo.toml` (add `crates/hadron-forge` to `members`)

**Interfaces:**
- Produces:
  - `pub const HASH_LEN: usize = 6;`
  - `pub enum BlockKind { Function, Struct, Enum, Impl, Trait }` with `pub fn label(&self) -> &'static str`.
  - `pub struct Block { pub kind: BlockKind, pub name: String, pub hash: String, pub start_line: usize, pub end_line: usize, pub byte_start: usize, pub byte_end: usize }`.
  - `pub fn short_hash(text: &str) -> String` — blake3 hex, first `HASH_LEN` chars.
  - `pub fn parse_blocks(source: &str) -> Vec<Block>` — top-level items in source order; unparseable/empty → empty vec.

- [x] **Step 1: Create the crate manifest and register it in the workspace**

Create `crates/hadron-forge/Cargo.toml`:
```toml
[package]
name = "hadron-forge"
version = "0.1.0"
edition = "2021"

[dependencies]
tree-sitter = "0.22"
tree-sitter-rust = "0.21"
blake3 = "1"
```

Edit the root `Cargo.toml` `members` list to add the new crate (append it — do not reorder existing entries, to stay merge-clean with concurrent work):
```toml
[workspace]
resolver = "2"
members = ["crates/hadron-lattice", "crates/hadron-gluon", "crates/hadron-chamber", "crates/hadron-forge"]
```

Create `crates/hadron-forge/src/lib.rs`:
```rust
//! `hadron-forge` — the edit-by-hash core: parse source into logical blocks,
//! hash them, and reconcile concurrent edits by hash. Pure and offline.
pub mod block;
pub mod edit;
```

> Note: `lib.rs` references `pub mod edit;`, which Task 2 creates. Until then the crate will not compile on its own; that is expected. Task 1's test is run with `--lib` after Task 2 exists, OR create an empty `crates/hadron-forge/src/edit.rs` containing only `// filled in Task 2` now so Task 1 compiles in isolation. **Do the latter:** create a placeholder `edit.rs` now:
```rust
// Implemented in Task 2.
```

- [x] **Step 2: Write the failing tests**

Create `crates/hadron-forge/src/block.rs`:
```rust
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
        assert_eq!(&SAMPLE[alpha.byte_start..alpha.byte_end], "pub fn alpha(x: i32) -> i32 { x + 1 }");
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
```

- [x] **Step 3: Run the tests to verify they fail (compile first)**

Run: `cargo test -p hadron-forge block::`
Expected: the crate compiles (deps fetch on first build — allow time), tests run. If you wrote `block.rs` exactly as above they should PASS immediately; the "failing" gate here is really "does the whole thing build and the assertions hold" — if any assertion is wrong, fix the *assertion* to match validated behavior, not the code.

- [x] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p hadron-forge block::`
Expected: PASS (5 tests).

- [x] **Step 5: Commit**
```bash
git add crates/hadron-forge/Cargo.toml crates/hadron-forge/src/lib.rs crates/hadron-forge/src/block.rs Cargo.toml
git commit -m "feat(forge): parse Rust into blake3-hashed top-level blocks"
```

---

### Task 2: Optimistic-concurrency `apply_edit`

**Files:**
- Modify: `crates/hadron-forge/src/edit.rs` (replace the Task-1 placeholder)

**Interfaces:**
- Consumes: `block::parse_blocks` (Task 1).
- Produces:
  - `pub struct HashedEdit { pub target_hash: String, pub new_text: String }`
  - `pub enum EditOutcome { Applied { new_source: String }, Rejected { reason: String } }`
  - `pub fn apply_edit(source: &str, edit: &HashedEdit) -> EditOutcome`

- [x] **Step 1: Write the failing tests + implementation**

Replace `crates/hadron-forge/src/edit.rs` with:
```rust
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
```

- [x] **Step 2: Run the tests**

Run: `cargo test -p hadron-forge edit::`
Expected: PASS (4 tests).

- [x] **Step 3: Commit**
```bash
git add crates/hadron-forge/src/edit.rs
git commit -m "feat(forge): optimistic-concurrency apply_edit (stale/ambiguous rejection)"
```

---

### Task 3: Workspace green + plan bookkeeping

**Files:**
- Modify: this plan file (check the boxes as you go)

**Interfaces:** none.

- [x] **Step 1: Full workspace build & test (default features)**

Run: `cargo test --workspace`
Expected: PASS — the pre-existing 64 tests plus 9 new `hadron-forge` tests (5 block + 4 edit), 0 failures, zero API spend, no GPUI in the path (the `gui` feature stays off).

- [x] **Step 2: Lint the new crate**

Run: `cargo clippy -p hadron-forge --all-targets`
Expected: no warnings from `hadron-forge` (fix any that appear; the crate is small and should be clean).

- [x] **Step 3: Commit any lint fixes (if the previous step changed files)**
```bash
git add crates/hadron-forge/src
git commit -m "chore(forge): clippy clean"
```
(If clippy made no changes, skip this commit.)

---

## Phase 4 (core slice) Definition of Done

- `hadron-forge` compiles and `cargo test --workspace` is green (9 new tests), with **zero API spend and no GPUI in the test path**.
- `parse_blocks` returns each top-level Rust item as a `Block` with its kind, human name, blake3 short hash, 1-based line span, and byte span.
- `annotate` renders the `[Hash: …] <kind> <name> (lines A–B)` swarm-context digest.
- `apply_edit` applies an edit iff exactly one current block matches the target hash, and **rejects** stale (no match) and ambiguous (multiple matches) edits — the optimistic-concurrency guarantee that two agents cannot silently clobber the same block.
- Nothing in Phase 3's files was touched; the only shared-file change is one appended entry in the root `Cargo.toml` `members` list.

## Deferred / bought land (explicitly NOT in this slice)

- **The `notify` filesystem watcher** on `field.jsonl` / `.hadron/ledger.jsonl` — the 0-CPU wake path. New code, but it belongs with the swarm loop below.
- **The tokio 0-CPU swarm loop + `reqwest`** — waking a quark on a field append and calling an API. This extends `hadron-gluon::engine` (Phase 3's file) and spends real budget; it must merge *after* Phase 3.
- **New `Kind` event variants** for edit-by-hash (e.g. an `EditRejected` / hash-context event on the field) — these touch `hadron-lattice::event.rs` (Phase 3's file). Wire them once Phase 3 lands so the two edits don't collide.
- **Multi-language support** — v1 hashes Rust only (the project dogfoods itself). Other tree-sitter grammars are a later add behind the same `parse_blocks` seam.
- **Full-length hashes / collision handling** — 6 hex chars suffice for one file; a multi-file production bus should widen `HASH_LEN` and/or namespace hashes by file path.
- **Chamber block-overlay visualization** (the spec's "pulsing bounding-box over a hashed block") — Phase 5, consuming `Block.byte_start/byte_end`.
