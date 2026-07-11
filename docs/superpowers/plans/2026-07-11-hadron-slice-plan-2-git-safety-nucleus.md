# Hadron Slice — Plan 2: Git Safety + Nucleus SSOT

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the gluon two of the pillars it needs before real quarks arrive: (1) a **git safety net** — snapshot the target project's worktree before a quark acts, expose the working diff for the projection, and restore to any snapshot (undo); and (2) the **nucleus** — the per-project SSOT knowledge layer (repo-level over global-level, with strict override), digested into the projection so a quark starts a turn already oriented. Still **zero API spend, zero GPUI** — driven by `MockQuark` and real temp git repos.

**Architecture:** Extend `hadron-lattice` with two pure-data types (`SnapshotRef`, `NucleusIndex`). Extend `hadron-gluon` with a `snapshot` module (git operations behind a seam) and a `nucleus` module (layered load + digest + staleness). Wire both into `Engine` additively — via builder methods (`with_git`, `with_nucleus`) so **Plan 1's `Engine::new` and its tests are untouched**.

**Tech Stack:** Rust (edition 2021), plus `std::process::Command` driving the `git` CLI. No new crate dependencies for the snapshot path. Dev: `tempfile` (already present).

**This is Plan 2 of 4** for the Hadron vertical slice (spec: `docs/superpowers/specs/2026-07-10-hadron-vertical-slice-design.md`). Plan 1 built the schema + engine. Later plans add real Claude/Antigravity adapters (3) and the GPUI chamber (4).

## Deviation from spec (flagged for review)

The spec (§11) names **`gix` (gitoxide, pure-Rust git)** for git safety. This plan instead drives the **`git` CLI** via `std::process::Command`, kept entirely behind the `snapshot` module. Rationale:

- `gix`'s *write* surface (writing trees, creating commits, updating refs) is its least-mature, most version-sensitive area; the *read* surface is solid but we need writes.
- The `snapshot` module is a clean seam: engine/router/schema never see git internals. Swapping the module's body to `gix` later is a localized change — the "buy the land" motorway swap, exactly the pattern used for the `field` (remote-control) and `Quark` (adapter) seams.
- Trade-off accepted for v1: this reintroduces a runtime dependency on a `git` binary (near-universal on dev machines). Pure-Rust `gix` remains bought land.

**If you'd rather hold the `gix` line now, this is the one thing to veto.**

## Global Constraints

- **Rust edition:** `2021`. Latest stable.
- **The snapshot module is the only place that shells out to `git`.** Everything else stays pure Rust. This is the gix-swap seam.
- **Snapshots never touch the user's index or HEAD.** Snapshotting uses a throwaway `GIT_INDEX_FILE` and stores commits under **shadow refs** `refs/hadron/snapshots/<ulid>` — invisible to `git log`, `git branch`, and normal history. The project's real staging area and branch are never mutated by a snapshot.
- **Git identity is passed explicitly** (`GIT_AUTHOR_*` / `GIT_COMMITTER_*` env) so snapshotting works even in a repo with no configured user.
- **Nucleus layering:** repo-level (`<repo>/.hadron/nucleus/`) is the base; global-level (`~/.hadron/nucleus/`, or an injected path) **strictly overrides** it — a doc present in both resolves to the global copy. (Per the design decision: global > repo, strict override.)
- **The engine wires both in additively.** `Engine::new` keeps its Plan 1 signature; git and nucleus are opt-in via `with_git(repo_root)` / `with_nucleus(nucleus)`. Plan 1 tests must still pass unchanged.
- **Local-only, no network, no spend.** Same as Plan 1.
- **Vocabulary (exact names):** quark, field, event, gluon, lattice, chamber, nucleus, flavor, energy, excite, snapshot.

---

### Task 1: SnapshotRef type + snapshot create/list

**Files:**
- Create: `crates/hadron-lattice/src/snapshot.rs`
- Modify: `crates/hadron-lattice/src/lib.rs` (add `mod snapshot; pub use snapshot::*;`)
- Create: `crates/hadron-gluon/src/snapshot.rs`
- Modify: `crates/hadron-gluon/src/lib.rs` (add `pub mod snapshot;`)

**Interfaces:**
- Produces (lattice): `struct SnapshotRef { id: String, label: String, commit: String }` (serde-derived, `Eq`).
- Produces (gluon): `snapshot::create(repo_root: &Path, label: &str) -> anyhow::Result<SnapshotRef>`; `snapshot::list(repo_root: &Path) -> anyhow::Result<Vec<SnapshotRef>>`.

- [ ] **Step 1: Add the lattice type**

Create `crates/hadron-lattice/src/snapshot.rs`:
```rust
use serde::{Deserialize, Serialize};

/// A pointer to one worktree snapshot, stored in the project repo as the shadow
/// ref `refs/hadron/snapshots/<id>`. `commit` is the full snapshot commit SHA.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotRef {
    pub id: String,
    pub label: String,
    pub commit: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_ref_round_trips() {
        let s = SnapshotRef {
            id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
            label: "before edit".into(),
            commit: "9f86d081884c7d659a2feaa0c55ad015".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: SnapshotRef = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }
}
```

Update `crates/hadron-lattice/src/lib.rs`:
```rust
mod event;
mod projection;
mod quark;
mod snapshot;

pub use event::*;
pub use projection::*;
pub use quark::*;
pub use snapshot::*;
```

- [ ] **Step 2: Write the failing gluon snapshot test**

Create `crates/hadron-gluon/src/snapshot.rs`:
```rust
use std::path::Path;
use std::process::Command;

use anyhow::{anyhow, Context};
use hadron_lattice::SnapshotRef;
use ulid::Ulid;

const SNAPSHOT_REF_PREFIX: &str = "refs/hadron/snapshots/";

/// Run `git` inside `repo_root` with explicit identity so snapshotting works
/// even when the repo has no configured user. Returns stdout on success.
fn git(repo_root: &Path, args: &[&str]) -> anyhow::Result<String> {
    git_with_env(repo_root, args, &[])
}

fn git_with_env(repo_root: &Path, args: &[&str], envs: &[(&str, &str)]) -> anyhow::Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(repo_root).args(args);
    cmd.env("GIT_AUTHOR_NAME", "hadron")
        .env("GIT_AUTHOR_EMAIL", "hadron@localhost")
        .env("GIT_COMMITTER_NAME", "hadron")
        .env("GIT_COMMITTER_EMAIL", "hadron@localhost");
    for (k, v) in envs {
        cmd.env(k, v);
    }
    let out = cmd
        .output()
        .with_context(|| format!("failed to spawn git {args:?}"))?;
    if !out.status.success() {
        return Err(anyhow!(
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// True if HEAD resolves to a commit (i.e. the repo has at least one commit).
fn head_commit(repo_root: &Path) -> Option<String> {
    git(repo_root, &["rev-parse", "--verify", "HEAD"]).ok()
}

/// Snapshot the current worktree into a shadow ref without touching the user's
/// index or HEAD. Uses a throwaway index file, writes a tree, commit-trees it
/// (parented on HEAD when one exists), and points `refs/hadron/snapshots/<id>`
/// at the result.
pub fn create(repo_root: &Path, label: &str) -> anyhow::Result<SnapshotRef> {
    let id = Ulid::new().to_string();

    // Throwaway index so we never disturb the user's staging area.
    let tmp_index = repo_root.join(format!(".git/hadron-index-{id}"));
    let tmp_index_str = tmp_index.to_string_lossy().to_string();
    let env = [("GIT_INDEX_FILE", tmp_index_str.as_str())];

    // Stage everything into the throwaway index, then write its tree.
    git_with_env(repo_root, &["add", "-A"], &env)?;
    let tree = git_with_env(repo_root, &["write-tree"], &env)?;
    let _ = std::fs::remove_file(&tmp_index);

    // Commit the tree, parenting on HEAD if the project has history.
    let commit = match head_commit(repo_root) {
        Some(parent) => git(repo_root, &["commit-tree", &tree, "-p", &parent, "-m", label])?,
        None => git(repo_root, &["commit-tree", &tree, "-m", label])?,
    };

    // Park it under the shadow ref namespace.
    let refname = format!("{SNAPSHOT_REF_PREFIX}{id}");
    git(repo_root, &["update-ref", &refname, &commit])?;

    Ok(SnapshotRef { id, label: label.to_string(), commit })
}

/// List every hadron snapshot, oldest ref first. Labels come from each snapshot
/// commit's subject line.
pub fn list(repo_root: &Path) -> anyhow::Result<Vec<SnapshotRef>> {
    let out = git(
        repo_root,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)%09%(contents:subject)",
            "refs/hadron/snapshots/",
        ],
    )?;
    let mut refs = Vec::new();
    for line in out.lines() {
        let mut parts = line.splitn(3, '\t');
        let (Some(refname), Some(commit)) = (parts.next(), parts.next()) else {
            continue;
        };
        let label = parts.next().unwrap_or("").to_string();
        let id = refname.strip_prefix(SNAPSHOT_REF_PREFIX).unwrap_or(refname).to_string();
        refs.push(SnapshotRef { id, label, commit: commit.to_string() });
    }
    Ok(refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    /// Make a temp git repo with one committed file. Returns the repo root.
    fn repo_with_file(name: &str, contents: &str) -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]).unwrap();
        std::fs::write(root.join(name), contents).unwrap();
        git(root, &["add", name]).unwrap();
        git(root, &["commit", "-q", "-m", "initial"]).unwrap();
        dir
    }

    #[test]
    fn create_makes_a_shadow_ref_without_touching_head_or_index() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        let head_before = git(root, &["rev-parse", "HEAD"]).unwrap();

        let snap = create(root, "before edit").unwrap();
        assert!(!snap.commit.is_empty());
        assert_eq!(snap.label, "before edit");

        // HEAD unchanged; snapshot lives only under the shadow ref.
        assert_eq!(git(root, &["rev-parse", "HEAD"]).unwrap(), head_before);
        let refname = format!("{SNAPSHOT_REF_PREFIX}{}", snap.id);
        assert_eq!(git(root, &["rev-parse", &refname]).unwrap(), snap.commit);
        // The user's index/status is clean (nothing staged by snapshotting).
        assert_eq!(git(root, &["status", "--porcelain"]).unwrap(), "");
    }

    #[test]
    fn snapshot_captures_uncommitted_changes() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        // Dirty the worktree, then snapshot.
        std::fs::write(root.join("a.txt"), "two\n").unwrap();
        std::fs::write(root.join("new.txt"), "fresh\n").unwrap();
        let snap = create(root, "dirty state").unwrap();

        // The snapshot tree contains the modified + new file contents.
        let a = git(root, &["show", &format!("{}:a.txt", snap.commit)]).unwrap();
        let n = git(root, &["show", &format!("{}:new.txt", snap.commit)]).unwrap();
        assert_eq!(a, "two");
        assert_eq!(n, "fresh");
    }

    #[test]
    fn list_returns_created_snapshots() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        let s1 = create(root, "first").unwrap();
        let s2 = create(root, "second").unwrap();

        let listed = list(root).unwrap();
        let ids: Vec<&str> = listed.iter().map(|s| s.id.as_str()).collect();
        assert!(ids.contains(&s1.id.as_str()));
        assert!(ids.contains(&s2.id.as_str()));
        let labels: Vec<&str> = listed.iter().map(|s| s.label.as_str()).collect();
        assert!(labels.contains(&"first"));
        assert!(labels.contains(&"second"));
    }
}
```

Add `ulid` to `crates/hadron-gluon/Cargo.toml` dependencies:
```toml
ulid = { version = "1", features = ["serde"] }
```

Update `crates/hadron-gluon/src/lib.rs`:
```rust
pub mod engine;
pub mod field;
pub mod mock;
pub mod quark;
pub mod router;
pub mod snapshot;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p hadron-lattice snapshot` then `cargo test -p hadron-gluon snapshot::`
Expected: PASS (1 lattice + 3 gluon). These exercise real `git` in temp repos — confirm `git` is on PATH.

- [ ] **Step 4: Commit**

```bash
git add crates/hadron-lattice crates/hadron-gluon
git commit -m "feat(gluon): snapshot create/list via shadow refs (SnapshotRef)"
```

---

### Task 2: Working diff + restore (undo)

**Files:**
- Modify: `crates/hadron-gluon/src/snapshot.rs` (append `working_diff`, `restore`, tests)

**Interfaces:**
- Produces: `snapshot::working_diff(repo_root: &Path) -> anyhow::Result<String>` (diff vs HEAD, empty when no HEAD); `snapshot::restore(repo_root: &Path, snap: &SnapshotRef) -> anyhow::Result<()>` (revert worktree files to the snapshot's tree).

- [ ] **Step 1: Write the failing test**

Append to `crates/hadron-gluon/src/snapshot.rs` (before the `#[cfg(test)]` module, add the two functions; then add tests inside the existing test module):
```rust
/// The current working diff against HEAD — what a quark has changed so far.
/// Feeds `Projection.git_diff`. Empty string when the repo has no commit yet.
pub fn working_diff(repo_root: &Path) -> anyhow::Result<String> {
    if head_commit(repo_root).is_none() {
        return Ok(String::new());
    }
    git(repo_root, &["diff", "HEAD"])
}

/// Restore the worktree to a snapshot (undo). Reverts tracked paths to the
/// snapshot's tree without moving HEAD or the branch. v1 limitation: files
/// created after the snapshot are left in place (documented; a hard clean is a
/// later concern).
pub fn restore(repo_root: &Path, snap: &SnapshotRef) -> anyhow::Result<()> {
    git(repo_root, &["restore", "--source", &snap.commit, "--worktree", "--", "."])?;
    Ok(())
}
```

Add these tests inside the existing `mod tests`:
```rust
    #[test]
    fn working_diff_shows_uncommitted_edits() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        assert_eq!(working_diff(root).unwrap(), "");
        std::fs::write(root.join("a.txt"), "changed\n").unwrap();
        let diff = working_diff(root).unwrap();
        assert!(diff.contains("a.txt"));
        assert!(diff.contains("+changed"));
    }

    #[test]
    fn restore_reverts_worktree_to_snapshot() {
        let dir = repo_with_file("a.txt", "one\n");
        let root = dir.path();
        // Snapshot the clean state, then mutate the file.
        let snap = create(root, "clean").unwrap();
        std::fs::write(root.join("a.txt"), "corrupted\n").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "corrupted\n");

        restore(root, &snap).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("a.txt")).unwrap(), "one\n");
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hadron-gluon snapshot::`
Expected: PASS (5 tests). `restore_reverts_worktree_to_snapshot` proves the undo net; if `git restore` behaves unexpectedly on this git version, adjust the flags here (this is the intended TDD checkpoint for the recipe).

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-gluon/src/snapshot.rs
git commit -m "feat(gluon): working_diff + restore (undo to snapshot)"
```

---

### Task 3: Wire git safety into the engine (snapshot-before-excite + diff in projection)

**Files:**
- Modify: `crates/hadron-gluon/src/engine.rs` (add `repo_root` opt-in, `with_git`, snapshot + diff in the loop, tests)

**Interfaces:**
- Produces: `Engine::with_git(self, repo_root: PathBuf) -> Engine` (builder). When set, each turn: append a `Kind::Snapshot` event before exciting, and populate `Projection.git_diff` from `snapshot::working_diff`.

- [ ] **Step 1: Extend the engine**

In `crates/hadron-gluon/src/engine.rs`, add a `repo_root: Option<PathBuf>` field to `Engine`, default it to `None` in `new`, and add the builder + loop wiring.

Add the field to the struct and to the `Engine { .. }` construction in `new` (set `repo_root: None`). Then add:
```rust
    /// Opt in to git safety: snapshot the target repo before each excite and feed
    /// the working diff into the projection. Additive — off by default.
    pub fn with_git(mut self, repo_root: std::path::PathBuf) -> Self {
        self.repo_root = Some(repo_root);
        self
    }
```

Inside `run_until_quiesce`, after resolving `target` and passing the backstop check, but **before** building the projection, snapshot and compute the diff:
```rust
            let git_diff = if let Some(root) = &self.repo_root {
                let snap = crate::snapshot::create(root, &format!("before {}", target.as_str()))?;
                append_event(
                    &self.field_path,
                    &Event::new(
                        Actor::Gluon,
                        None,
                        Kind::Snapshot { git: snap.commit.clone(), label: snap.label.clone() },
                    ),
                )?;
                crate::snapshot::working_diff(root)?
            } else {
                String::new()
            };
```
Then use `git_diff` in the `Projection { .. git_diff, .. }` construction (replace the hard-coded `String::new()`).

- [ ] **Step 2: Write the failing test**

Add to `engine.rs` `mod tests`:
```rust
    fn git_init_repo() -> tempfile::TempDir {
        let dir = tempdir().unwrap();
        let root = dir.path();
        crate::snapshot::create(root, "noop").ok(); // ensure snapshot module linked
        // Real init + one commit so HEAD exists.
        std::process::Command::new("git").arg("-C").arg(root).args(["init", "-q"])
            .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").status().unwrap();
        std::fs::write(root.join("f.txt"), "x\n").unwrap();
        std::process::Command::new("git").arg("-C").arg(root).args(["add", "."]).status().unwrap();
        std::process::Command::new("git").arg("-C").arg(root).args(["commit", "-q", "-m", "init"])
            .env("GIT_AUTHOR_NAME", "t").env("GIT_AUTHOR_EMAIL", "t@t")
            .env("GIT_COMMITTER_NAME", "t").env("GIT_COMMITTER_EMAIL", "t@t").status().unwrap();
        dir
    }

    #[tokio::test]
    async fn engine_snapshots_before_excite_when_git_enabled() {
        let fdir = tempdir().unwrap();
        let path = fdir.path().join("field.jsonl");
        seed_human_message(&path, "orch", "do it");

        let repo = git_init_repo();
        let orch = MockQuark::scripted(
            QuarkId::new("orch"),
            Flavor::Orchestrator,
            vec![Some("done, back to human".into())],
        );
        let mut engine = Engine::new(path.clone(), vec![Box::new(orch)], "x".into(), 10)
            .with_git(repo.path().to_path_buf());
        engine.run_until_quiesce().await.unwrap();

        let events = read_events(&path).unwrap();
        let snapshots = events
            .iter()
            .filter(|e| matches!(e.kind, Kind::Snapshot { .. }))
            .count();
        assert_eq!(snapshots, 1, "one snapshot recorded before the single excite");
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p hadron-gluon engine::`
Expected: PASS (3 tests — the 2 from Plan 1 still pass, proving additivity, plus the new git one).

- [ ] **Step 4: Commit**

```bash
git add crates/hadron-gluon/src/engine.rs
git commit -m "feat(gluon): snapshot-before-excite + working diff in projection (opt-in with_git)"
```

---

### Task 4: NucleusIndex type (lattice)

**Files:**
- Create: `crates/hadron-lattice/src/nucleus.rs`
- Modify: `crates/hadron-lattice/src/lib.rs`

**Interfaces:**
- Produces: `struct NucleusIndex { version: u32, last_verified_commit: Option<String>, sources: Vec<String> }` (serde; `sources` are relative markdown filenames in scan order).

- [ ] **Step 1: Write the type + test**

Create `crates/hadron-lattice/src/nucleus.rs`:
```rust
use serde::{Deserialize, Serialize};

/// The manifest of a nucleus layer (`index.json`). `sources` lists the markdown
/// docs (relative names) that make up this layer, in the order they should be
/// digested. `last_verified_commit` is the project commit the knowledge was last
/// confirmed against — compared to HEAD to detect staleness.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NucleusIndex {
    pub version: u32,
    #[serde(default)]
    pub last_verified_commit: Option<String>,
    #[serde(default)]
    pub sources: Vec<String>,
}

impl Default for NucleusIndex {
    fn default() -> Self {
        NucleusIndex { version: 1, last_verified_commit: None, sources: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_round_trips_and_defaults_missing_fields() {
        let idx: NucleusIndex = serde_json::from_str(r#"{"version":1}"#).unwrap();
        assert_eq!(idx.sources.len(), 0);
        assert_eq!(idx.last_verified_commit, None);

        let full = NucleusIndex {
            version: 1,
            last_verified_commit: Some("abc123".into()),
            sources: vec!["map.md".into(), "conventions.md".into()],
        };
        let json = serde_json::to_string(&full).unwrap();
        let back: NucleusIndex = serde_json::from_str(&json).unwrap();
        assert_eq!(full, back);
    }
}
```

Update `crates/hadron-lattice/src/lib.rs`:
```rust
mod event;
mod nucleus;
mod projection;
mod quark;
mod snapshot;

pub use event::*;
pub use nucleus::*;
pub use projection::*;
pub use quark::*;
pub use snapshot::*;
```

- [ ] **Step 2: Run test**

Run: `cargo test -p hadron-lattice nucleus`
Expected: PASS (1 test).

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-lattice/src
git commit -m "feat(lattice): NucleusIndex manifest type"
```

---

### Task 5: Nucleus load with layering (repo base, global override)

**Files:**
- Create: `crates/hadron-gluon/src/nucleus.rs`
- Modify: `crates/hadron-gluon/src/lib.rs`

**Interfaces:**
- Produces: `struct Nucleus { docs: Vec<(String, String)>, last_verified_commit: Option<String> }`; `nucleus::load(repo_layer: Option<&Path>, global_layer: Option<&Path>) -> anyhow::Result<Nucleus>`.
- Layering rule: start from the repo layer's docs (in `index.json` `sources` order); for each global-layer doc, **override** a same-named repo doc in place, or append if new. `last_verified_commit` is taken from the repo layer (the project-specific truth); global has none.

- [ ] **Step 1: Write the loader + test**

Create `crates/hadron-gluon/src/nucleus.rs`:
```rust
use std::path::Path;

use anyhow::Context;
use hadron_lattice::NucleusIndex;

/// A resolved nucleus: ordered `(name, markdown)` docs after layering, plus the
/// project commit the knowledge was last verified against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Nucleus {
    pub docs: Vec<(String, String)>,
    pub last_verified_commit: Option<String>,
}

/// Read one layer directory: parse `index.json`, then read each listed source
/// doc. A missing directory or missing index yields an empty layer (not an
/// error) — layers are optional.
fn read_layer(dir: &Path) -> anyhow::Result<(NucleusIndex, Vec<(String, String)>)> {
    let index_path = dir.join("index.json");
    let index: NucleusIndex = match std::fs::read_to_string(&index_path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("parsing {}", index_path.display()))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((NucleusIndex::default(), Vec::new())),
        Err(e) => return Err(e).context("reading nucleus index"),
    };
    let mut docs = Vec::new();
    for name in &index.sources {
        let doc_path = dir.join(name);
        match std::fs::read_to_string(&doc_path) {
            Ok(body) => docs.push((name.clone(), body)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue, // listed but absent: skip
            Err(e) => return Err(e).with_context(|| format!("reading {}", doc_path.display())),
        }
    }
    Ok((index, docs))
}

/// Load and layer the nucleus. Repo layer is the base; global layer strictly
/// overrides same-named docs and appends new ones. Either layer may be `None`.
pub fn load(repo_layer: Option<&Path>, global_layer: Option<&Path>) -> anyhow::Result<Nucleus> {
    let (repo_index, mut docs) = match repo_layer {
        Some(dir) => read_layer(dir)?,
        None => (NucleusIndex::default(), Vec::new()),
    };

    if let Some(dir) = global_layer {
        let (_global_index, global_docs) = read_layer(dir)?;
        for (name, body) in global_docs {
            match docs.iter_mut().find(|(n, _)| *n == name) {
                Some(slot) => slot.1 = body,          // strict override
                None => docs.push((name, body)),      // new global-only doc
            }
        }
    }

    Ok(Nucleus { docs, last_verified_commit: repo_index.last_verified_commit })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn write_layer(dir: &Path, index: &str, files: &[(&str, &str)]) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("index.json"), index).unwrap();
        for (name, body) in files {
            std::fs::write(dir.join(name), body).unwrap();
        }
    }

    #[test]
    fn repo_only_loads_in_source_order() {
        let dir = tempdir().unwrap();
        write_layer(
            dir.path(),
            r#"{"version":1,"last_verified_commit":"abc","sources":["map.md","conventions.md"]}"#,
            &[("map.md", "# Map"), ("conventions.md", "# Conv")],
        );
        let n = load(Some(dir.path()), None).unwrap();
        assert_eq!(n.docs.len(), 2);
        assert_eq!(n.docs[0].0, "map.md");
        assert_eq!(n.docs[1].0, "conventions.md");
        assert_eq!(n.last_verified_commit, Some("abc".into()));
    }

    #[test]
    fn global_strictly_overrides_repo() {
        let repo = tempdir().unwrap();
        let global = tempdir().unwrap();
        write_layer(
            repo.path(),
            r#"{"version":1,"sources":["conventions.md"]}"#,
            &[("conventions.md", "REPO rules")],
        );
        write_layer(
            global.path(),
            r#"{"version":1,"sources":["conventions.md","user.md"]}"#,
            &[("conventions.md", "GLOBAL rules"), ("user.md", "my prefs")],
        );
        let n = load(Some(repo.path()), Some(global.path())).unwrap();
        // conventions.md is overridden by global; user.md appended.
        let conv = n.docs.iter().find(|(name, _)| name == "conventions.md").unwrap();
        assert_eq!(conv.1, "GLOBAL rules");
        assert!(n.docs.iter().any(|(name, _)| name == "user.md"));
    }

    #[test]
    fn missing_layers_are_empty_not_errors() {
        let dir = tempdir().unwrap();
        let missing = dir.path().join("does-not-exist");
        let n = load(Some(&missing), None).unwrap();
        assert_eq!(n.docs.len(), 0);
        assert_eq!(n.last_verified_commit, None);
    }
}
```

Update `crates/hadron-gluon/src/lib.rs`:
```rust
pub mod engine;
pub mod field;
pub mod mock;
pub mod nucleus;
pub mod quark;
pub mod router;
pub mod snapshot;
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p hadron-gluon nucleus::`
Expected: PASS (3 tests).

- [ ] **Step 3: Commit**

```bash
git add crates/hadron-gluon/src
git commit -m "feat(gluon): layered nucleus load (repo base, global strict override)"
```

---

### Task 6: Nucleus digest + staleness, wired into the projection

**Files:**
- Modify: `crates/hadron-gluon/src/nucleus.rs` (add `digest`, `Staleness`, `staleness`, tests)
- Modify: `crates/hadron-gluon/src/engine.rs` (add `nucleus` opt-in, `with_nucleus`, populate `nucleus_digest`, tests)

**Interfaces:**
- Produces: `nucleus::digest(n: &Nucleus, max_bytes: usize) -> String` (concatenate docs with `## <name>` headers, truncated to a byte budget); `enum Staleness { Fresh, Stale, Unknown }`; `nucleus::staleness(n: &Nucleus, head: Option<&str>) -> Staleness`; `Engine::with_nucleus(self, Nucleus) -> Engine` populating `Projection.nucleus_digest`.

- [ ] **Step 1: Add digest + staleness**

Append to `crates/hadron-gluon/src/nucleus.rs`:
```rust
/// Whether the nucleus knowledge is still trustworthy relative to the repo HEAD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Staleness {
    /// last_verified_commit matches HEAD.
    Fresh,
    /// HEAD has moved past last_verified_commit — knowledge may be outdated.
    Stale,
    /// No last_verified_commit or no HEAD to compare against.
    Unknown,
}

/// Render the nucleus into the projection's `nucleus_digest`: each doc under a
/// `## <name>` header, joined, then truncated to `max_bytes` on a char boundary.
pub fn digest(n: &Nucleus, max_bytes: usize) -> String {
    let mut out = String::new();
    for (name, body) in &n.docs {
        out.push_str("## ");
        out.push_str(name);
        out.push('\n');
        out.push_str(body.trim_end());
        out.push_str("\n\n");
    }
    let trimmed = out.trim_end();
    if trimmed.len() <= max_bytes {
        return trimmed.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !trimmed.is_char_boundary(end) {
        end -= 1;
    }
    trimmed[..end].to_string()
}

/// Compare the nucleus's verified commit to the current HEAD.
pub fn staleness(n: &Nucleus, head: Option<&str>) -> Staleness {
    match (n.last_verified_commit.as_deref(), head) {
        (Some(verified), Some(head)) if verified == head => Staleness::Fresh,
        (Some(_), Some(_)) => Staleness::Stale,
        _ => Staleness::Unknown,
    }
}
```

Add tests inside `nucleus.rs` `mod tests`:
```rust
    #[test]
    fn digest_headers_and_truncates() {
        let n = Nucleus {
            docs: vec![("map.md".into(), "alpha".into()), ("conventions.md".into(), "beta".into())],
            last_verified_commit: None,
        };
        let full = digest(&n, 10_000);
        assert!(full.contains("## map.md"));
        assert!(full.contains("alpha"));
        assert!(full.contains("## conventions.md"));
        // Truncation respects the byte budget.
        let short = digest(&n, 8);
        assert!(short.len() <= 8);
    }

    #[test]
    fn staleness_compares_commit_to_head() {
        let fresh = Nucleus { docs: vec![], last_verified_commit: Some("abc".into()) };
        assert_eq!(staleness(&fresh, Some("abc")), Staleness::Fresh);
        assert_eq!(staleness(&fresh, Some("def")), Staleness::Stale);
        assert_eq!(staleness(&fresh, None), Staleness::Unknown);
        let unknown = Nucleus { docs: vec![], last_verified_commit: None };
        assert_eq!(staleness(&unknown, Some("abc")), Staleness::Unknown);
    }
```

- [ ] **Step 2: Wire into the engine**

In `engine.rs`, add a `nucleus_digest: String` field to `Engine` (default `String::new()` in `new`), plus:
```rust
    /// Opt in to nucleus context: its digest is injected into every projection.
    pub fn with_nucleus(mut self, nucleus: hadron_lattice::... ) -> Self { ... }
```
Concretely, store the pre-rendered digest so the engine stays decoupled from the loader:
```rust
    pub fn with_nucleus(mut self, digest: String) -> Self {
        self.nucleus_digest = digest;
        self
    }
```
And in `run_until_quiesce`, set `nucleus_digest: self.nucleus_digest.clone()` in the `Projection { .. }` construction (replacing the hard-coded `String::new()`).

Add an engine test:
```rust
    #[tokio::test]
    async fn projection_carries_nucleus_digest() {
        let fdir = tempdir().unwrap();
        let path = fdir.path().join("field.jsonl");
        seed_human_message(&path, "orch", "go");

        // A probe quark asserts on the projection it receives.
        use hadron_lattice::{Projection, TurnOutcome};
        struct Probe;
        #[async_trait::async_trait]
        impl crate::quark::Quark for Probe {
            fn id(&self) -> QuarkId { QuarkId::new("orch") }
            fn flavor(&self) -> Flavor { Flavor::Orchestrator }
            fn energy(&self) -> hadron_lattice::EnergyState { hadron_lattice::EnergyState::Available }
            async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
                assert!(turn.nucleus_digest.contains("## map.md"));
                Ok(TurnOutcome { message: Some("done".into()) })
            }
        }

        let mut engine = Engine::new(path.clone(), vec![Box::new(Probe)], "x".into(), 10)
            .with_nucleus("## map.md\nthe project map".into());
        engine.run_until_quiesce().await.unwrap();
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p hadron-gluon nucleus::` then `cargo test -p hadron-gluon engine::`
Expected: PASS (nucleus: 5; engine: 4).

- [ ] **Step 4: Whole workspace green**

Run: `cargo test`
Expected: PASS across lattice + gluon (Plan 1 tests + all Plan 2 additions).

- [ ] **Step 5: Commit**

```bash
git add crates/hadron-gluon/src
git commit -m "feat(gluon): nucleus digest + staleness, injected into projection (opt-in with_nucleus)"
```

---

## Plan 2 Definition of Done

- `cargo test` passes across the workspace (Plan 1 tests unchanged + all Plan 2 additions).
- A snapshot captures the worktree into a shadow ref **without touching the user's HEAD or index**; `list` enumerates snapshots.
- `working_diff` reports uncommitted edits; `restore` reverts the worktree to a snapshot (undo proven).
- The engine, when opted in via `with_git`, records a `Kind::Snapshot` event before each excite and feeds the diff into the projection.
- The nucleus loads with **repo-base / global-strict-override** layering, digests into the projection, and reports staleness against HEAD.
- The engine's Plan 1 signature and tests are unchanged (additivity proven).
- No real adapters, no GPUI yet — Plans 3–4.

## Notes for later plans

- **gix swap:** the entire `snapshot` module is the seam. When pure-Rust git is wanted, reimplement these functions with `gix` and nothing else changes. Keep the shadow-ref layout (`refs/hadron/snapshots/<ulid>`) identical so existing snapshots remain readable.
- **Restore is v1-partial:** it reverts tracked files but does not delete files created after the snapshot. A full "clean to snapshot" (add `git clean` scoped to the diff) is bought land — wire it when destructive undo is needed.
- **Nucleus digest is pre-rendered into the engine** (`with_nucleus(String)`) to keep the engine decoupled from the loader/filesystem. The daemon binary (a later plan) composes `nucleus::load` → `nucleus::digest` → `Engine::with_nucleus`, and can re-render between human turns as the nucleus evolves.
- **Staleness is computed but not yet acted on.** Plan 3's Invariants preamble can surface a "⚠️ nucleus stale" banner to real quarks; the enum is ready.
- **`last_verified_commit` update flow** (bumping it after a quark verifies the map against new code) is deferred — the field already has the `Kind::Snapshot`/`Kind::Command` vocabulary to model a verify step later.
