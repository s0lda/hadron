---
author: acp-claude
status: draft
---

# Swarm-Wide Full Mediation & MCP Editing Suite — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Execute phases in order — later phases consume types from earlier ones.

**Goal:** Make Hadron's own file-editing tools the *provided, preferred, and (where the provider allows) enforced* edit path for every quark, backed by a single `rmcp` stdio MCP server, so edit-by-hash actually runs in production.

**Architecture:** A new `hadron-forge-mcp` binary crate exposes Hadron's editing suite as MCP stdio tools built on `rmcp 2.2.0`. `hadron-forge` grows from an in-memory `apply_edit(&str, &HashedEdit)` into a file-level, multi-language editing engine (`edit`/`write`/`create`, AST-block hashing for code, whole-file compare-and-swap for everything else). Each seat is wired to launch/attach that MCP server and, where the provider supports it, to disallow native edit tools; ACP seats additionally reject native-edit permission requests at the handler.

**Tech Stack:** Rust, `rmcp 2.2.0` (official MCP SDK), `tree-sitter-{rust,python,typescript,go}`, `blake3`, `serde`/`serde_json`, `tempfile` (atomic writes).

## Global Constraints

- **Rust edition / workspace:** new crate joins the root workspace `members` list; use workspace dependency versions where they exist.
- **Baseline gate (must stay green):** `cargo test --workspace` = 125 passed; `cargo test -p hadron-gluon` = 332 passed; `cargo test -p hadron-chamber --features gui` = 120 passed; `cargo test -p hadron-forge` = 9 passed. Run the FULL gate at the end of each phase. You own only your delta.
- **`.hadron/` is gitignored** — plan/spec docs must be committed with `git add -f`.
- **No fabricated external APIs (Standard Model rule 1/6).** The exact `rmcp 2.2.0` attribute/trait API is NOT reproduced verbatim in this plan because it was not run while writing it. Phase 0 pins it from `docs.rs/rmcp/2.2.0` before any tool code is written. Do not copy an invented macro name from memory.
- **Worktree jail (security, rule 7):** every tool that writes MUST reject a `path` that escapes the server's configured root (`..` traversal, absolute paths outside root, symlinks out). This is untrusted (LLM-supplied) input at a new file-write boundary — see each write task's Security note.

## Honest scope bounds (read before estimating)

These are verified facts that shape the plan; do not let a task quietly over-promise past them.

1. **`agy --print` cannot be mediated at the CLI.** `agy --help` exposes ONLY `--dangerously-skip-permissions` — no `--disallowedTools`, no MCP flag. The agy seat is mediated **only** via its ACP form (`acp-agy`, our own `agy_acp.py` bridge). Phase 3 covers agy over ACP; the `cli-agy` seat is explicitly out of scope for enforcement.
2. **codex/copilot native-edit *deny* is unconfirmed.** `mcp` attach is confirmed for both; the exact flag that removes their native edit tool is not. Phase 3 Task for each starts by confirming the deny flag from `--help`, and falls back to "provide + prefer" (soft) if none exists.
3. **The already-merged merge-gate "block guard" (`9b260f8`) is a no-op stub.** `merge.rs:114` calls `check_forge_block_conflicts` as `let _ = …` (result discarded); the fn walks only the worktree *top level* (non-recursive → misses `src/**`), parses each `.rs` into `let _blocks` and **throws it away**, always returning `Ok(())`. It detects nothing and blocks nothing. Phase 4 replaces it with a real base-vs-branch block-hash conflict check. Until then, do not describe it as working.
4. **"Preferred" is soft; "enforced" needs the deny flag.** Providing our tools + prompting yields high but not 100% adherence (models fall back to native on edge cases). Only native-tool *removal* guarantees ours-only, and only on providers that support removal (claude ✓, copilot likely ✓, codex ~, agy-CLI ✗).

---

## File Structure

- `crates/hadron-forge/src/lib.rs` — re-export the new `file` module.
- `crates/hadron-forge/src/lang.rs` *(new)* — `Lang` enum + grammar selection; maps a path extension to a tree-sitter grammar or to `Lang::Opaque` (non-AST).
- `crates/hadron-forge/src/block.rs` — generalise `parse_blocks` to take a `Lang` (keep the Rust-only fn as a thin wrapper for existing callers/tests).
- `crates/hadron-forge/src/file.rs` *(new)* — file-level operations: `apply_block_edit`, `write_file_cas`, `create_file`, each returning a typed result carrying the new block digest list. All path-jailed.
- `crates/hadron-forge-mcp/` *(new crate)* — `Cargo.toml`, `src/main.rs` (stdio server bootstrap), `src/tools.rs` (the three MCP tools → `hadron_forge::file`).
- `crates/hadron-gluon/src/adapter/registry/…` — seat construction: attach the MCP server config + provider-specific native-tool suppression.
- `crates/hadron-gluon/src/adapter/cli.rs` — posture args already flow through `posture.for_mode` (`cli.rs:231`); add MCP + disallow args there.
- `crates/hadron-gluon/src/adapter/acp/session.rs` — populate `NewSessionRequest.mcp_servers` (schema field confirmed in `agent-client-protocol-schema-1.4.0`); tighten the permission handler (`session.rs:371`) to reject native-edit tool calls.
- `crates/hadron-gluon/src/engine/merge.rs` — replace the `check_forge_block_conflicts` stub.

---

## Phase 0 — De-risk the externals (do this first, no production code)

### Task 0.1: Pin the `rmcp 2.2.0` server API

**Files:** none (spike only; delete the spike after).

- [ ] **Step 1:** Add a throwaway `examples/rmcp_spike.rs` under a scratch crate OR read `docs.rs/rmcp/2.2.0`. Record, in the plan's Phase 2 comments, the EXACT names for: the tool-registration attribute/macro, the server handler trait, and the stdio transport constructor.
- [ ] **Step 2:** Write the smallest server that exposes one `ping()` tool over stdio; run it and hand it one `tools/list` + one `tools/call` JSON-RPC line via a piped stdin.
- [ ] **Step 3:** Confirm the handshake returns the tool and a result. Record the working skeleton. **Expected:** a `tools/list` response naming `ping`.
- [ ] **Step 4:** Delete the spike. Commit nothing but the recorded API notes into Phase 2 task comments.

**Gate:** if `rmcp 2.2.0`'s API differs materially from the shape Phase 2 assumes, STOP and report to `@orchestrator` before writing Phase 2 — do not force a fabricated API.

### Task 0.2: Confirm ACP `mcp_servers` wiring compiles

**Files:** `crates/hadron-gluon/src/adapter/acp/session.rs` (temporary).

- [ ] **Step 1:** In a scratch test, construct a `NewSessionRequest` and set `mcp_servers` to a one-element `vec![McpServer::Stdio(…)]` using the real `agent-client-protocol-schema-1.4.0` types (field confirmed present in that crate).
- [ ] **Step 2:** `cargo build -p hadron-gluon`. **Expected:** compiles. Record the exact `McpServer::Stdio` constructor shape for Phase 3.
- [ ] **Step 3:** Revert the scratch change.

---

## Phase 1 — `hadron-forge` file-level, multi-language editing engine

### Task 1.1: `Lang` detection

**Files:**
- Create: `crates/hadron-forge/src/lang.rs`
- Modify: `crates/hadron-forge/src/lib.rs` (add `pub mod lang;`)
- Test: inline `#[cfg(test)]` in `lang.rs`

**Interfaces:**
- Produces: `pub enum Lang { Rust, Python, TypeScript, Go, Opaque }` and `pub fn lang_for_path(path: &str) -> Lang`.

- [ ] **Step 1: Write the failing test**
```rust
#[test]
fn extensions_map_to_langs() {
    assert_eq!(lang_for_path("src/x.rs"), Lang::Rust);
    assert_eq!(lang_for_path("a/b.py"), Lang::Python);
    assert_eq!(lang_for_path("c.ts"), Lang::TypeScript);
    assert_eq!(lang_for_path("m.go"), Lang::Go);
    assert_eq!(lang_for_path("Cargo.toml"), Lang::Opaque);
    assert_eq!(lang_for_path("README.md"), Lang::Opaque);
}
```
- [ ] **Step 2:** Run `cargo test -p hadron-forge lang::` → FAIL (unresolved).
- [ ] **Step 3: Implement**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang { Rust, Python, TypeScript, Go, Opaque }

pub fn lang_for_path(path: &str) -> Lang {
    match path.rsplit('.').next() {
        Some("rs") => Lang::Rust,
        Some("py") => Lang::Python,
        Some("ts") | Some("tsx") => Lang::TypeScript,
        Some("go") => Lang::Go,
        _ => Lang::Opaque,
    }
}
```
- [ ] **Step 4:** Run test → PASS.
- [ ] **Step 5:** Commit `feat(forge): add Lang detection by path extension`.

### Task 1.2: Multi-language `parse_blocks`

**Files:**
- Modify: `crates/hadron-forge/src/block.rs`
- Modify: `crates/hadron-forge/Cargo.toml` (add `tree-sitter-python`, `tree-sitter-typescript`, `tree-sitter-go`)
- Test: inline in `block.rs`

**Interfaces:**
- Consumes: `Lang` (Task 1.1).
- Produces: `pub fn parse_blocks_lang(source: &str, lang: Lang) -> Vec<Block>`. Keep existing `pub fn parse_blocks(source: &str) -> Vec<Block>` as `parse_blocks_lang(source, Lang::Rust)` so all current callers/tests are unchanged (SSOT — do not fork the Rust path).

- [ ] **Step 1: Failing test** — a Python function parses to one block whose byte span round-trips:
```rust
#[test]
fn python_top_level_def_is_one_block() {
    let src = "def alpha(x):\n    return x + 1\n";
    let blocks = parse_blocks_lang(src, Lang::Python);
    assert_eq!(blocks.len(), 1);
    assert_eq!(&src[blocks[0].byte_start..blocks[0].byte_end], "def alpha(x):\n    return x + 1");
}
```
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3: Implement** — parameterise the parser: select the grammar from `Lang` (`tree_sitter_python::LANGUAGE`, etc.), and the node-kind→`BlockKind` map per language (Python `function_definition`/`class_definition`; TS `function_declaration`/`class_declaration`/`interface_declaration`; Go `function_declaration`/`type_declaration`). `Lang::Opaque` returns `Vec::new()`. Reuse `short_hash` and the existing `Block` fields unchanged.
- [ ] **Step 4:** Run the new test + the existing Rust `parse_blocks` tests → all PASS.
- [ ] **Step 5:** Commit `feat(forge): multi-language block parsing (py/ts/go)`.

### Task 1.3: `file.rs` — block edit against a file (AST languages)

**Files:**
- Create: `crates/hadron-forge/src/file.rs`
- Modify: `crates/hadron-forge/src/lib.rs` (`pub mod file;`)
- Test: inline in `file.rs` (use `tempfile::tempdir`)

**Interfaces:**
- Consumes: `apply_edit`, `HashedEdit`, `EditOutcome` (`edit.rs`), `lang_for_path`, `parse_blocks_lang`, `annotate`.
- Produces:
```rust
pub struct Root(std::path::PathBuf);            // the jail
impl Root { pub fn new(p: impl Into<PathBuf>) -> Self; }
pub enum ForgeError { OutsideRoot, NotFound, Io(String), Rejected(String), NotHashable }
pub struct EditReport { pub blocks: String }    // fresh annotate() digest after the op
pub fn apply_block_edit(root:&Root, rel_path:&str, target_hash:&str, new_text:&str) -> Result<EditReport, ForgeError>;
```
- Path jail: `apply_block_edit` canonicalises `root/rel_path` and returns `ForgeError::OutsideRoot` if the result is not under `root` (reject `..`, absolute, symlink-escape).

- [ ] **Step 1: Failing test** — edit a Rust fn in a temp file by its hash, and a path-jail test:
```rust
#[test]
fn edits_a_rust_fn_by_hash_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path());
    std::fs::write(dir.path().join("a.rs"), "pub fn a() -> i32 { 1 }\n").unwrap();
    let h = parse_blocks(&std::fs::read_to_string(dir.path().join("a.rs")).unwrap())[0].hash.clone();
    let rep = apply_block_edit(&root, "a.rs", &h, "pub fn a() -> i32 { 2 }").unwrap();
    assert!(std::fs::read_to_string(dir.path().join("a.rs")).unwrap().contains("2"));
    assert!(rep.blocks.contains("[Hash: "));
}
#[test]
fn rejects_path_escape() {
    let dir = tempfile::tempdir().unwrap();
    let root = Root::new(dir.path());
    assert!(matches!(apply_block_edit(&root, "../evil.rs", "abc", "x"), Err(ForgeError::OutsideRoot)));
}
```
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3: Implement** — jail-check; `NotHashable` if `lang_for_path == Opaque`; read file; build `HashedEdit{target_hash, new_text}`; call `apply_edit`; on `Applied` write atomically (write temp in same dir + `rename`); return `EditReport{ blocks: annotate(&new_source) }`; map `Rejected{reason}`→`ForgeError::Rejected`.
- [ ] **Step 4:** Run → PASS.
- [ ] **Step 5:** Commit `feat(forge): apply_block_edit against files, path-jailed`.

**Security (rule 7):** first write boundary taking LLM input. Jail-check is the control; the escape test is its guard. Atomic rename prevents torn writes.

### Task 1.4: `write_file` (compare-and-swap) + `create_file` — covers ALL files

**Files:** Modify `crates/hadron-forge/src/file.rs`; tests inline.

**Interfaces (Produces):**
```rust
pub fn create_file(root:&Root, rel_path:&str, content:&str) -> Result<EditReport, ForgeError>; // errors if exists
pub fn write_file_cas(root:&Root, rel_path:&str, content:&str, expected_hash: Option<&str>) -> Result<EditReport, ForgeError>;
// expected_hash = Some(blake3-of-current-file) → optimistic concurrency; None → unconditional write (new content).
```
This is the non-AST answer: for `Cargo.toml`/JSON/Markdown/new files, edit-by-hash at sub-file granularity is fragile (chunk boundaries shift every edit — reinventing patch). The robust primitive is **whole-file compare-and-swap on the file's content hash**. `EditReport.blocks` is `annotate()` when the path is an AST language, else the file's whole-content `short_hash`. (Divergence from the earlier chunk-hashing sketch is deliberate — rule 10, simplest correct thing. Sub-file text chunking can be a later enhancement, not v1.)

- [ ] **Step 1: Failing tests** — CAS rejects on stale hash, applies on match; `create_file` refuses to clobber:
```rust
#[test]
fn cas_rejects_stale_then_applies_fresh() {
    let dir = tempfile::tempdir().unwrap(); let root = Root::new(dir.path());
    std::fs::write(dir.path().join("c.toml"), "a = 1\n").unwrap();
    assert!(matches!(write_file_cas(&root,"c.toml","a = 2\n",Some("000000")), Err(ForgeError::Rejected(_))));
    let cur = hadron_forge::block::short_hash(&std::fs::read_to_string(dir.path().join("c.toml")).unwrap());
    assert!(write_file_cas(&root,"c.toml","a = 2\n",Some(&cur)).is_ok());
}
#[test]
fn create_refuses_existing() {
    let dir = tempfile::tempdir().unwrap(); let root = Root::new(dir.path());
    std::fs::write(dir.path().join("x.md"), "hi").unwrap();
    assert!(matches!(create_file(&root,"x.md","other"), Err(ForgeError::Rejected(_))));
}
```
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3: Implement** — jail-check both; `create_file` errors `Rejected` if the target exists, else atomic write + parent `create_dir_all`; `write_file_cas` compares `short_hash(current)` to `expected_hash` when `Some`, `Rejected` on mismatch, else atomic write.
- [ ] **Step 4:** Run → PASS. Run full `cargo test -p hadron-forge`.
- [ ] **Step 5:** Commit `feat(forge): whole-file CAS write + create_file (all file types)`.

**Security (rule 7):** same jail control; `create_file` non-clobber prevents an agent overwriting an unrelated file by asserting it is "new".

---

## Phase 2 — `hadron-forge-mcp` stdio server (rmcp 2.2.0)

> Uses the API pinned in Task 0.1. The tool **schemas and behaviour below are the contract**; the exact `rmcp` attribute/trait syntax comes from Task 0.1's notes, not from memory.

### Task 2.1: Crate skeleton + stdio bootstrap

**Files:** Create `crates/hadron-forge-mcp/Cargo.toml`, `src/main.rs`; Modify root `Cargo.toml` `members`.

- [ ] **Step 1:** Add crate to workspace `members`. Deps: `rmcp = "2.2.0"`, `hadron-forge = { path = "../hadron-forge" }`, `serde`, `serde_json`, `tokio` (if rmcp needs it — confirm from 0.1), `anyhow`.
- [ ] **Step 2:** `main.rs` reads the jail root from argv[1] (the worktree path the daemon passes), constructs the server (0.1 skeleton), serves over stdio.
- [ ] **Step 3:** `cargo build -p hadron-forge-mcp` → compiles.
- [ ] **Step 4:** Commit `feat(forge-mcp): crate skeleton + stdio bootstrap`.

### Task 2.2: The tool suite (Edit, Write, Create, Delete, Read Blocks)

**Files:** Create `crates/hadron-forge-mcp/src/tools.rs`; tests inline (call the tool fns directly, not over stdio).

**Tool contracts (JSON):**
- `hadron_forge_edit { path: string, target_hash: string, new_text: string }` → `hadron_forge::file::apply_block_edit`. Result: `{ ok, blocks }` or `{ ok:false, reason }`.
- `hadron_forge_write_file { path: string, content: string, expected_hash?: string }` → `write_file_cas`.
- `hadron_forge_create_file { path: string, content: string }` → `create_file`.
- `hadron_forge_delete_file { path: string, expected_hash?: string }` → `delete_file_cas` (deletes file after verifying content hash).
- `hadron_forge_read_blocks { path: string }` → `annotate` (returns AST block structure with `[Hash: 8hex]` annotations so the agent can read block hashes directly).

### Task 2.3: Context7 & Search Integration (Multi-MCP Bundling)

- In `hadron-gluon` seat registration (`session.rs`), inject `context7` alongside `hadron-forge-mcp` into `mcp_servers`.
- Exposes `query-docs` and `resolve-library-id` tools to all quarks alongside `hadron-forge` editing tools for accurate documentation lookup.

- [ ] **Step 1: Failing test** — the edit tool handler, given a temp root + a real hash, mutates the file and returns `ok:true`. (Reuse the Phase 1 pattern.)
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3: Implement** each tool as a thin adapter: deserialize args → call `hadron_forge::file::*` with the server's `Root` → serialize `EditReport`/`ForgeError` to the JSON above. Map every `ForgeError` to a structured, non-panicking tool result (rule 8 — never unwrap on LLM input).
- [ ] **Step 4:** Run tool tests → PASS.
- [ ] **Step 5: Smoke test (rule 1 — prove it RUNS):** pipe a `tools/list` then a `tools/call hadron_forge_edit` JSON-RPC line into the built binary over stdin against a temp file; assert the file changed. Record the transcript in the commit body.
- [ ] **Step 6:** Commit `feat(forge-mcp): edit/write/create tools + stdio smoke test`.

---

## Phase 3 — Seat wiring (provide + prefer + enforce-where-possible)

> Each task ends by launching the real seat and confirming, from a live turn or `--help`, that the tool is present. "Implemented, unwired" is a failure here.

### Task 3.1: Claude CLI seat — attach MCP + disallow native

**Files:** Modify `crates/hadron-gluon/src/adapter/cli.rs` (posture args) and the Claude seat spec in `registry/`.

- [ ] **Step 1:** Confirm from `claude --help`: `--mcp-config <file|json>` and `--disallowedTools <names…>` (both verified present).
- [ ] **Step 2:** Add to the Claude seat's posture: `--mcp-config` pointing at a generated config that launches `hadron-forge-mcp <worktree>`, and `--disallowedTools Edit Write MultiEdit NotebookEdit` (and prompt-directive naming `hadron_forge_*` as preferred).
- [ ] **Step 3: Test** — a `cli.rs` unit test asserting the built invocation contains both flags (extend the existing arg-assertion tests at `cli.rs:457-493`).
- [ ] **Step 4:** Live check: run one real edit turn on the Claude CLI seat; confirm the file changed via `hadron_forge_edit` (tool appears in the turn's tool calls), not native Edit.
- [ ] **Step 5:** Commit.

**Security (rule 7):** disallowing native edit tools narrows the agent's surface; the MCP server is the new surface — its jail (Phase 1) is the control.

### Task 3.2: ACP seats (claude-agent-acp, acp-agy) — inject MCP + reject native edits

**Files:** Modify `crates/hadron-gluon/src/adapter/acp/session.rs`.

- [ ] **Step 1:** Populate `NewSessionRequest.mcp_servers` with an `McpServer::Stdio` launching `hadron-forge-mcp <cwd>` (constructor shape from Task 0.2). This makes our tools available to the agent regardless of the agent's own toolset.
- [ ] **Step 2:** In the permission handler (`session.rs:371`), before the posture choice: if the requested tool is a native edit (`Edit`/`Write`/`MultiEdit`/`fs/write_text_file`), respond `RejectOnce` regardless of mode; else keep the existing posture logic. Keep the agent in an ask-mode internally so the handler always fires (Hadron's Bypass UX ≠ the agent's internal permission mode — this is the "few lines" path).
- [ ] **Step 3: Test** — a handler unit test: a native-edit `RequestPermissionRequest` → `RejectOnce`; a `hadron_forge_edit` (or non-edit) request → the posture choice. (Table-drive the tool-name classifier as a pure fn — rule 8.)
- [ ] **Step 4:** Live check on `acp-claude`: an edit turn uses `hadron_forge_edit`; a forced native Edit is rejected and the agent retries via the MCP tool.
- [ ] **Step 5:** Commit. **Note:** `agy_acp.py` (the `acp-agy` bridge) must also advertise the MCP tools / forward to the server — Python-only change, daemon RESTART required to take effect. If the agy SDK cannot attach an external MCP server, scope agy to "provide via prompt + reject native," and say so in the report.

### Task 3.3: copilot & codex seats — confirm deny flag, else provide-only

**Files:** Modify the copilot/codex seat specs in `registry/`.

- [ ] **Step 1:** From `--help`: attach MCP (copilot `--additional-mcp-config`; codex `mcp add` / config). Confirm whether a native-edit *deny* exists. If yes, add it (enforced); if no, attach MCP + prompt-prefer only (soft) and **`log`/record that this seat is provide-only, not enforced** (no silent cap).
- [ ] **Step 2: Test** — invocation/config unit test per seat.
- [ ] **Step 3:** Commit, stating per-seat whether it ended enforced or provide-only.

---

## Phase 4 — Replace the merge-gate stub with a real conflict detector

> `9b260f8`'s `check_forge_block_conflicts` is a no-op (see Honest Scope Bounds #3). This phase makes it real, or removes it. Either way the false "it guards merges" claim must not survive.

### Task 4.1: Real base-vs-branch block-hash conflict detection

**Files:** Modify `crates/hadron-gluon/src/engine/merge.rs`; tests inline.

**Interfaces (Produces):** `fn forge_block_conflicts(base_wt:&Path, branch_wt:&Path, target_wt:&Path) -> Vec<BlockConflict>` where a conflict = a file where both the landing branch and the target (since `base`) replaced the *same* block hash with different content. Returns `Vec` (empty = clean).

- [ ] **Step 1: Failing test** — construct three trees where branch A and target both rewrote the same `fn` differently; assert one `BlockConflict`. A control where they edited *different* blocks asserts zero.
- [ ] **Step 2:** Run → FAIL.
- [ ] **Step 3: Implement** — for each changed `.rs`/py/ts/go file (walk **recursively** — the stub's top-level-only walk was a bug), parse base/branch/target blocks by `Lang`, and flag blocks whose hash is present in base but replaced-differently in both branch and target.
- [ ] **Step 4:** Wire the result at `merge.rs:114` — on non-empty, `reroute_blocked` with the conflicting block list (do NOT `let _ =` it). Add a test that a detected conflict routes to `Blocked`, not `land()`.
- [ ] **Step 5:** Run `cargo test -p hadron-gluon` → PASS (baseline 332 + new).
- [ ] **Step 6:** Commit `fix(merge): real forge block-conflict gate (replaces no-op stub)`.

---

## Final gate (rule 5)

- [ ] `cargo test --workspace` — expect ≥125 passed (+ new forge/forge-mcp tests), 0 failed.
- [ ] `cargo test -p hadron-gluon` — expect ≥332 passed, 0 failed.
- [ ] `cargo test -p hadron-forge` — expect ≥9 passed (+ new), 0 failed.
- [ ] `cargo test -p hadron-forge-mcp` — new, all passed incl. the stdio smoke test.
- [ ] `cargo test -p hadron-chamber --features gui` — expect 120 passed (untouched).
- [ ] Update `features.md` (editing suite: status + entrypoints) and `docs/backlog.md`.

## Self-review notes (done while writing)

- **Coverage:** every spec item maps to a task — MCP crate (Ph2), multi-lang AST (1.2), non-AST files (1.4, via CAS not chunk-hash — deliberate), the three tools (2.2), CLI + ACP suppression (3.1–3.3). The merge-gate stub (out-of-spec but blocking) is added as Ph4.
- **Type consistency:** `Root`, `ForgeError`, `EditReport`, `apply_block_edit`/`write_file_cas`/`create_file` names are used identically across Phases 1→2.
- **Placeholder scan:** the only non-verbatim code is the `rmcp` server bootstrap, gated behind Task 0.1 by explicit design (not fabricated) — flagged in Global Constraints.
