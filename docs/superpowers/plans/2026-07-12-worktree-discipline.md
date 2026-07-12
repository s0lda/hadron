# Worktree discipline — engine-enforced collaboration rules

> **Execution status (2026-07-12): PLAN ONLY. Not started.** No code in this doc has been
> written. Every `file:line` reference below points at the code *as it stands on
> `wip/2026-07-12-session`* (`4dc9dcb`), not at code this plan introduces.

**Goal:** make the concurrency that already landed (`724df28 feat(engine): concurrent quark
turns + mid-turn dispatch`) *safe*. Today two quarks can be excited at the same time and both
write into **one shared working tree**. Give each quark its own `git worktree`, on its own
feature branch, per assignment; end a turn on a commit; and put a merge behind a gate
(workspace tests green + a human approval) before anything reaches `main`.

---

## 0. Ground truth — what is actually in the tree today

Read before planning; several premises in the brief needed correcting.

| Claim | Reality | Where |
|---|---|---|
| Two quarks write into one shared tree | **TRUE, and live.** `ProcessRunner::run` never calls `.current_dir()`, so every spawned `claude`/`agy` subprocess inherits the *daemon's* cwd. Two concurrent turns ⇒ two CLIs editing the same checkout. | `crates/hadron-gluon/src/adapter/runner.rs:49-55` |
| `CliInvocation` can carry a cwd | **FALSE.** The struct is `{ program, args, stdin }` — there is no cwd field to override. | `crates/hadron-gluon/src/adapter/runner.rs:5-10` |
| The engine snapshots the shared tree each turn | **TRUE in the engine, DORMANT in production.** The snapshot only runs when `repo_root.is_some()`, i.e. when `Engine::with_git` was called. The shipped daemon bin **never calls `with_git`** (nor `with_nucleus`, nor `with_ledger`). | snapshot site `engine.rs:472-483`; `with_git` `engine.rs:155-158`; bin `bin/hadron-gluon.rs:176` |
| There is a `TODO(worktrees)` marking it | **TRUE.** | `engine.rs:466-471` |
| `hadron-gatekeeper` can run the merge gate | **Needs splitting.** The crate's own doc comment: *"Intentionally offline and side-effect-free: it does NOT classify commands, emit events, pause the daemon, or render UI."* It cannot shell out to `cargo test` or `git merge`. | `crates/hadron-gatekeeper/src/lib.rs:1-7` |
| No git remote today | **TRUE.** `git remote -v` is empty. Local `--ff-only` merge is the live path; push + `gh pr create` is the dormant branch. | — |

### Surprises worth flagging

1. **The bin runs its own poll loop, not `Engine::serve`.** `bin/hadron-gluon.rs:184-195` is a
   naive `loop { run_until_quiesce().await; sleep(interval) }`. The proper notify-driven daemon
   — `Engine::serve` in `crates/hadron-gluon/src/daemon.rs:24-68` — exists, is tested, and is
   **not called by anything outside its own tests**. Worktree lifecycle needs a startup hook, so
   this plan folds the bin onto `serve()` (Task 0) rather than bolting reclamation onto a loop
   that shouldn't exist.
2. **The bin's own module doc admits the gap** (`bin/hadron-gluon.rs:12-18`: *"Real-adapter mode
   … `Engine::with_git(repo)` — is the glue described in Plan 3's notes … held for a
   human-present session"*). It was never done, but `seat_quarks` (`:121-151`) *does* build real
   adapters from `team.json`. So today: real CLIs, real spend, **no git safety at all**.
3. **`.hadron/` is gitignored** (`.gitignore:11`). `.hadron/trees/` therefore won't dirty the
   parent repo's `git status` — which is exactly why that's the right location. (Confirm during
   execution that `git worktree add` into a gitignored subdir of the repo's own working tree
   behaves; treat as risk-to-verify, not assumption.)
4. **The engine cannot name the quark that panicked.** `engine.rs:561-578` grounds *every*
   in-flight quark on a `JoinError` because the `JoinError` alone doesn't identify the task. So
   stale-worktree cleanup **cannot** be driven off the crash path — it must be a startup scan.
5. **`Projection` has no cwd**, and the worktree path is per-*assignment*, so it cannot be baked
   into `ClaudeQuark`/`AgyQuark` at seat-construction time (`adapter/registry.rs:59-68`). It has
   to ride on the `Projection`. This is the plan's spine.
6. **The brief's own wording conflicts:** a *stable* path `.hadron/trees/<quark>/` vs a *new
   branch per assignment*. Two worktrees cannot share a path. Resolution below (Task 1).

---

## 1. The shape

```
<repo>/                       ← main. The human's tree. No quark ever writes here.
  .hadron/                    ← gitignored
    field.jsonl
    team.json
    trees/
      opus/                   ← git worktree, HEAD = quark/opus/<assign-ulid>
      agy/                    ← git worktree, HEAD = quark/agy/<assign-ulid>
```

**One worktree per quark (stable path), one branch per assignment.** This resolves the brief's
conflict and bounds disk cost to *N quarks*, not *N assignments*. A new assignment does not
create a new checkout; it does `git -C .hadron/trees/<id> checkout -b quark/<id>/<ulid>` from a
fresh `main`. The branch name's ULID comes from the driving `Assign`/`Message` event's `id`
(`Event.id: Ulid`, `crates/hadron-lattice/src/event.rs`), which is monotonic and unique — so
branch names cannot collide, even across daemon restarts.

**The assignment boundary** = the event that triggers the turn. `Engine::pending_targets`
(`engine.rs:228-239`) already computes exactly this: `next_pending(events)` (a hand-off / an
addressed event) plus each unserved addressee of the latest unaddressed human message. A turn
whose driving event is *the same assignment* as the previous turn (a permission-grant resume, a
hand-off within a chain) **stays on the existing branch**; a turn driven by a *new* human
`Assign`/`Message` cuts a new branch. The rule must be derivable purely from the field, in
keeping with the codebase's reconstruct-from-the-field discipline (cf. `router::next_pending`,
`gatekeeper::pending_permission`).

---

## Task 0 — Fold the bin onto `Engine::serve`, and wire git

**Prerequisite for everything else.** Worktree provisioning + stale reclamation need a
process-lifecycle hook, and the merge gate needs the daemon to know the repo root.

**Changes**
- `crates/hadron-gluon/src/bin/hadron-gluon.rs:176-195` — replace the hand-rolled poll loop with
  `Engine::serve(shutdown_rx)` (`daemon.rs:24`), plus a `tokio::signal::ctrl_c` → `watch::send(true)`
  shutdown. Delete the `--interval-ms` arg (`:85`) or make it a no-op alias — `serve` has its own
  `SAFETY_POLL` (`daemon.rs:13`).
- Same file, at engine construction: chain `.with_git(repo_root)`, where
  `repo_root = vcs-style resolution of the field path`. **Reuse the existing rule** — the chamber
  already has it: `hadron_chamber::vcs::repo_root_of` (`crates/hadron-chamber/src/vcs.rs:130-139`,
  `<root>/.hadron/field.jsonl` → `<root>`). The gluon must not depend on the chamber (the chamber
  deliberately does not depend on the gluon, `vcs.rs:1-7`), so **move that function into
  `hadron-lattice`** and have both call it. Do not fork a second copy.
- New `crates/hadron-gluon/src/worktree.rs` (module registered in `lib.rs`).

**Failure modes**
- Repo root resolves to a non-git directory (field outside any repo). Must degrade *loudly*:
  refuse to seat real quarks with a clear error, rather than silently running them cwd-inherited
  in a non-repo. Today's silence is the bug.
- `serve()` swallows the `run_until_quiesce` error (`daemon.rs:50` uses `?`, which kills the
  daemon), whereas the current bin logs-and-continues (`bin:187-189`). Decide explicitly: log,
  append a `Kind::Message` from `Actor::Gluon`, and continue. This is the open question the bin's
  own doc comment flags (`bin/hadron-gluon.rs:17-18`).

**Tests**
- `bin` test: `repo_root_of` moved to lattice, both crates' existing tests still green
  (`vcs.rs:146-156` moves with it).
- `daemon.rs`: a `run_until_quiesce` error does not terminate `serve()` (new).

---

## Task 1 — Worktree per quark, branch per assignment

### 1a. `crates/hadron-gluon/src/worktree.rs` (new)

Shell out to `git`, mirroring `snapshot.rs`'s style exactly — its private `git(repo_root, args)`
helper (`snapshot.rs:12-37`) already pins `GIT_AUTHOR_*`/`GIT_COMMITTER_*` so operations work in
a repo with no configured user. **Promote that helper to `pub(crate)`** and reuse it; do not
write a second `Command::new("git")` wrapper. (There are already three: `snapshot.rs:17`,
`chamber/vcs.rs:38`, and whatever this adds.)

```rust
/// Where a quark works. Stable per quark; the branch inside it changes per assignment.
pub struct Worktree { pub quark: QuarkId, pub path: PathBuf, pub branch: String }

/// Ensure `.hadron/trees/<quark>/` exists as a worktree, checked out on
/// `quark/<id>/<assignment-ulid>` branched from `main`. Idempotent: if the worktree
/// already exists on that exact branch, it is a no-op (the resume / hand-off case).
pub fn ensure(repo_root: &Path, quark: &QuarkId, assignment: Ulid) -> anyhow::Result<Worktree>;

/// Every `.hadron/trees/*` entry git knows about, plus orphans on disk that it doesn't.
pub fn list(repo_root: &Path) -> anyhow::Result<Vec<Worktree>>;

/// Startup reclamation: `git worktree prune`, then for each surviving tree, reset it to a
/// clean state (or, if dirty, park the work on the branch and report it).
pub fn reclaim(repo_root: &Path) -> anyhow::Result<Vec<Reclaimed>>;

/// The commit that ends a turn (Task 3) and the diff that attributes it.
pub fn commit_turn(wt: &Worktree, message: &str) -> anyhow::Result<Option<String>>; // None = nothing to commit
pub fn branch_diff(wt: &Worktree, base: &str) -> anyhow::Result<String>; // git diff <base>...HEAD
```

Underlying git, in order, for `ensure` on a *new* assignment:
```
git -C <root> worktree add --detach .hadron/trees/<id> main   # first time only
git -C <root>/.hadron/trees/<id> checkout -b quark/<id>/<ulid> main
```

> **Do NOT `reset --hard main` first.** On the 2nd+ assignment the worktree is still on the
> *previous* branch `quark/<id>/A`. `git reset --hard main` would move **that branch's pointer**
> to main — and if `A` was never ff-merged (the gate denied it, or it is still pending), A's
> commits become unreachable. That would silently destroy the very evidence Task 4 promises to
> preserve ("Denied merge. The branch stays. Nothing is deleted."). `checkout -b <new> main`
> branches from main *without touching* A. The reset is both destructive and unnecessary.

### Assignment identity — where the ULID comes from (Tasks 1, 2 and 3 all depend on this)

The branch is named for the assignment's ULID, so `ensure` must compute the *same* ULID for every
turn of the same assignment — otherwise a resumed quark cuts a **new** branch and its paused,
uncommitted work is orphaned. That is the exact inverse of the intent.

**The assignment ULID is the `Event.id` of the task-bearing event that drives the turn** — and
`projection_for` (`engine.rs:245-297`) *already resolves exactly that event*: the `Assign`/`Message`
found by the trigger-finder at `:262-283`, or the unaddressed driving human message at `:284-297`.
It is stable across a permission pause, because the driving event for a resumed turn is **not** the
`PermissionGrant` — the grant is deliberately *skipped* by the trigger-finder (`engine.rs:255-257`:
*"Skip non-task events like a PermissionGrant … otherwise a resumed quark would get an empty task"*),
so the finder walks back to the same original task event. This existing behaviour is what makes
same-assignment idempotency possible; it is already load-bearing and already tested
(`human_grant_resumes_the_quark_with_its_task`, `engine.rs:846-868`).

**Required change:** `projection_for` currently discards the driving event's identity, returning only
`task_desc: String`. It must also return that event's `Ulid`, so the projection and the branch name
share one source of truth. Refactor it to hand back a small `Driver { task: String, assignment: Ulid,
invariants: Vec<String> }` (or have the dispatch loop resolve the driver *once* and pass it to both
`worktree::ensure` and `projection_for`). The latter is cleaner — the dispatch loop at
`engine.rs:432-511` already re-reads events and needs the ULID *before* it builds the projection.

A turn with **no** task-bearing driver (all three branches of `projection_for` fall through, leaving
`task_desc == ""`) has no assignment ⇒ must not cut a branch. Treat as an error and refuse to excite.

### 1b. Engine hook

`Engine` gains `worktrees: Option<...>` alongside `repo_root` (`engine.rs:93-96`), and
`run_until_quiesce` calls `worktree::ensure` **inside the dispatch `for` loop, immediately before
the snapshot** (`engine.rs:472`) — i.e. per target, per pass, before `projection_for`
(`engine.rs:485`) and before the `Status{Excited}` append (`engine.rs:495`). The resulting path
goes onto the `Projection` (Task 5).

Note `ensure` is blocking (`std::process::Command`) inside an async loop; it is short (ms) and the
existing `snapshot::create` at `engine.rs:474` already does exactly this, so it is consistent —
but if the dispatch loop's latency budget matters, wrap in `spawn_blocking`.

### Failure modes

| Mode | What happens today / would happen | Mitigation |
|---|---|---|
| **Worktree left dirty** | `checkout -b` from a dirty tree carries the previous assignment's uncommitted edits into the new branch — silent cross-assignment contamination. | `ensure` refuses to cut a new branch from a dirty tree. Either the previous turn committed (Task 3 guarantees it) or `reclaim` parked it. Hard-fail loudly + append a Gluon `Kind::Message`. |
| **Branch collision** | `checkout -b` fails if the branch exists. | ULID-named branches cannot collide. A collision means a re-run of the *same* assignment ⇒ `ensure` must detect "already on this branch" and no-op (the resume path — see `engine.rs:377-388`, a `Waiting` quark resumed by a `PermissionGrant` must land back in the *same* worktree on the *same* branch, or its half-finished work vanishes). **This is the single most important idempotency case.** |
| **Quark crashes mid-turn → stale worktree** | The panic path (`engine.rs:561-578`) cannot name the quark, so it cannot clean up. A killed daemon cleans up nothing. | **Startup scan, not crash-path cleanup.** `worktree::reclaim` runs once in the bin before `serve()`. It prunes worktrees git has lost track of, and reports (does not silently delete) any dirty tree — a crashed quark's half-work is *evidence*, not garbage. |
| **Disk cost** | N quarks × one full checkout. For hadron itself (~a few MB of source, `target/` is gitignored and not copied) this is trivial. It is not trivial for a monorepo. | Document. Optional later: `git worktree add` already shares the object store — only the working files are duplicated. Cap = one tree per *seated* quark (`team.json` has 2). |
| **`target/` rebuild per worktree** | Each worktree compiles from scratch — for a Rust workspace this is the real cost, far larger than the source checkout. | Flag as the main practical cost. Mitigation (out of scope, note it): a shared `CARGO_TARGET_DIR`, at the price of cargo lock contention between concurrent quarks. |
| **`.hadron/` is inside the repo it worktrees** | Recursion risk: the worktree at `.hadron/trees/opus/` contains its own `.hadron/`? No — `.hadron/` is gitignored, so it is not in any commit, so a fresh worktree checkout does **not** contain it. Verify. | Verify in execution. If wrong, move trees to a sibling (`../.hadron-trees/<repo>/`). |

### Tests (`worktree.rs`, using the `git_init_repo()` pattern at `engine.rs:1079-1099`)
- `ensure_creates_a_worktree_on_a_new_branch` — path exists; `git -C <wt> symbolic-ref --short HEAD` == `quark/opus/<ulid>`.
- `ensure_is_idempotent_for_the_same_assignment` — called twice ⇒ same path, same branch, no error (**the permission-resume case**).
- `ensure_refuses_a_new_branch_from_a_dirty_tree`.
- `two_quarks_get_distinct_worktrees` — paths differ, branches differ, an edit in one is invisible in the other.
- `reclaim_prunes_an_orphaned_worktree` — `rm -rf` the dir behind git's back, then `reclaim`, then `git worktree list` is clean.
- `reclaim_reports_but_does_not_destroy_a_dirty_tree`.
- `a_new_assignment_does_not_destroy_the_previous_branch` — the anti-`reset --hard` test. Assignment A commits, is *not* merged; assignment B then runs; `git rev-parse quark/<id>/A` still resolves and still contains A's commit.
- `the_assignment_ulid_is_stable_across_a_permission_pause` (engine) — a `PermissionQuark` (`engine.rs:616-649`) asks under `Ask`, is granted, resumes: the branch name is identical across both turns, and the work it left uncommitted before the pause is still in the tree.

---

## Task 2 — Branch-per-assignment enforced in code: HEAD can never be `main`

Not a convention — an assertion in the hot path.

**Changes**
- `worktree::ensure` returns `Err` if, after its work, `git -C <wt> symbolic-ref --short HEAD`
  resolves to the default branch (`main`/`master`) or is detached. This is a post-condition check,
  not a pre-condition — it catches every path into the function, including a human who manually
  `checkout main`'d inside a quark's tree.
- The engine treats that error as a **hard refusal to excite**: append `Actor::Gluon` →
  `Kind::Message` explaining, append `Kind::Status { state: QuarkState::Blocked }` for the target,
  and `continue` — reusing the *exact* reroute shape the ledger-depletion branch already uses
  (`engine.rs:445-458`). Do not invent a second refusal mechanism.
- The human's own tree (`<repo>/`) is never a quark cwd, so `main` stays quark-free by
  construction; this check is the belt to that brace.

**Failure modes**
- The default branch isn't named `main` — resolve it (`git symbolic-ref refs/remotes/origin/HEAD`,
  else `git config init.defaultBranch`, else literal `main`/`master`). Don't hardcode blindly.
- Detached HEAD (which `worktree add --detach` transiently produces) must also be refused as a
  *turn-running* state — a commit on a detached HEAD is unreachable garbage.

**Tests**
- `head_is_never_the_default_branch` — force `git -C <wt> checkout main` behind the engine's back, then `ensure` ⇒ `Err`.
- `engine_blocks_a_quark_whose_worktree_is_on_main` — the target gets `Status{Blocked}` + a Gluon message, its *sibling* still runs (the reroute property already proven for depletion at `engine.rs:1309-1349`).
- `detached_head_is_refused`.

---

## Task 3 — Turn ends on a commit; per-turn snapshot becomes real attribution

### The subtlety the brief glosses

`snapshot::working_diff` is `git diff HEAD` (`snapshot.rs:102-107`) — **uncommitted** changes.
Once a turn *ends on a commit*, `working_diff` is empty by construction. So per-quark attribution
does **not** come from repointing the existing snapshot at the worktree; it comes from the
**branch commit**. Two mechanisms, and the plan must say which wins:

- **Shadow-ref snapshot** (`snapshot::create` → `refs/hadron/snapshots/<ulid>`, `snapshot.rs:48-72`;
  `snapshot::restore`, `:113-116`) — keep it, repointed at the **worktree** path. Its job is
  **undo**: a pre-turn escape hatch that survives a quark trashing its tree. It touches neither
  HEAD nor the index (proven at `snapshot.rs:135-150`), so it composes fine with commits.
- **Branch commit** — this is **authoritative for attribution**. `git diff main...quark/<id>/<ulid>`
  is, by construction, exactly and only that quark's lines.

### Changes

- `engine.rs:472-483` — the snapshot block: pass `wt.path` instead of `root`. Delete the
  `TODO(worktrees)` comment (`engine.rs:466-471`) — this task is what discharges it.
- The `git_diff` fed to `Projection` (`engine.rs:480`, consumed at `engine.rs:317`) becomes
  `worktree::branch_diff(&wt, "main")` — *"here is everything you have done on this assignment so
  far"*, which is strictly more useful to the model than `git diff HEAD` and is now correct under
  concurrency (today it may show a **sibling's** edits — the live bug).
- `Engine::finish_turn` (`engine.rs:329-399`) — **new step, before the terminal `Status{Ground}`
  append at `engine.rs:392`**: `worktree::commit_turn(&wt, &msg)`. On a non-empty commit, append
  `Kind::Edit { paths, git: <sha>, summary }` — **the `Edit` event kind already exists and is
  serialized** (`crates/hadron-lattice/src/event.rs`, `Kind::Edit { paths, git, summary }`) and is
  **currently emitted by nobody**. This is what it was for.
- **Commit only on the `Ground` path.** The two early-returns in `finish_turn` — `AutoApprove`
  (`engine.rs:366-376`) and `AskHuman` (`engine.rs:377-388`) — must **not** commit: a quark
  pausing mid-assignment for permission is not done; its uncommitted work must still be sitting in
  its worktree when the grant resumes it. This is precisely why `ensure` must be idempotent for the
  same assignment (Task 1).
- Commit message: `"<quark>: <first line of the reply> [<assignment ulid>]"` — greppable, and ties
  the commit back to the field event.

### Failure modes
- **Nothing to commit** (a pure-conversation turn, or an `Ask`-mode turn which is read-only by
  posture, `claude.rs:22`). `commit_turn` returns `Ok(None)`; no `Edit` event. Must not error.
- **The CLI already committed** (a `Bypass`-mode quark that ran `git commit` itself). `commit_turn`
  finds a clean tree ⇒ `Ok(None)` ⇒ but the branch *has* advanced. Emit the `Edit` event from
  `git rev-parse HEAD` if HEAD moved since the pre-turn snapshot, regardless of who committed.
  Compare-HEAD, don't assume-we-committed.
- **The CLI committed to the wrong branch / checked out main mid-turn.** Task 2's post-condition
  re-checked at *end* of turn catches it; refuse to merge such a branch.
- Commit-under-concurrency is fine: each quark commits in its own worktree, its own branch, its
  own index. The shared object store handles the writes; there is no index lock contention because
  each worktree has its own `.git/worktrees/<id>/index`.

### Tests
- `a_turn_ends_on_a_commit_in_the_quarks_branch` — after `run_until_quiesce`, `git -C <wt> log --oneline main..HEAD` has one commit.
- `a_permission_pause_does_not_commit` — a `PermissionQuark` (the harness at `engine.rs:616-649`) under default `Ask` leaves the worktree *dirty*, on the same branch, with 0 commits — and the resumed turn (`engine.rs:846-868`) finds its work still there.
- `two_concurrent_quarks_produce_disjoint_attribution` — the discriminating test. Extend `two_quarks_named_in_one_message_run_concurrently` (`engine.rs:1441-1487`): quark `a` writes `a.txt`, quark `b` writes `b.txt`, concurrently; then `branch_diff(a, "main")` mentions `a.txt` and **not** `b.txt`, and vice-versa. **This test fails on today's code** (one tree, both files in both diffs) — it is the proof the hazard is fixed.
- `git_diff_on_the_projection_is_the_branch_diff_not_the_working_diff` — a probe quark (pattern at `engine.rs:1126-1154`) asserts on `turn.git_diff` after a prior committed turn on the same branch.
- `the_snapshot_ref_points_into_the_worktree` — `snapshot::create` labelled `before <quark>` contains the quark's tree, not the human's.

---

## Task 4 — The merge gate

### The split the gatekeeper's own doc demands

`hadron-gatekeeper/src/lib.rs:1-7` declares the crate pure. So:

**Pure, in `hadron-gatekeeper` (new `merge.rs`, exported from `lib.rs:12-16`):**
```rust
pub enum MergeVerdict { Merge, Block(BlockReason) }
pub enum BlockReason { TestsFailed, NotApproved, BranchIsDefault, DirtyWorktree, NoCommits }

/// Truth table. No I/O.
pub fn merge_decision(tests_passed: bool, human_approved: bool, state: &BranchState) -> MergeVerdict;

/// Fold the field: has the human approved *this* branch's merge?
pub fn merge_approved(events: &[Event], quark: &QuarkId, branch: &str) -> bool;
```

**Reuse the permission channel — do not invent a second approval mechanism.** A merge is
`Risk::BashExec`-class. The existing machinery already does everything needed:
- The engine appends `Kind::PermissionReq { risk: BashExec, description: "merge quark/opus/<ulid> → main (N commits, tests green)" }` on behalf of the quark.
- `gatekeeper::pending_permission` (`gate.rs:18-40`) surfaces it; the chamber already renders it.
- The human clicks Approve → `gate::grant` (`gate.rs:45-51`) appends the `PermissionGrant`.
- `gatekeeper::decide` (`matrix.rs:88-103`) already gives the right ladder for free:
  `Ask`/`Write` ⇒ `AskHuman`; `Auto` ⇒ ask once, then remembered per `(quark, op)` — but note the
  op string contains the ULID, so it is **never** the same op twice ⇒ Auto effectively always asks
  for merges. That is arguably correct (you should not blanket-trust merges); if not, normalize
  the description to `"merge to main"` for rule-matching. **Decide this explicitly at execution.**
  `Bypass` ⇒ auto-merge, which is the "the orchestrator owns it" contract (`event.rs`, `Mode` doc).

**Effectful, engine/bin-side (new `crates/hadron-gluon/src/merge.rs`):**
```rust
/// cargo test --workspace, run IN the quark's worktree. Returns (passed, tail_of_output).
pub async fn workspace_tests(wt: &Worktree) -> anyhow::Result<(bool, String)>;

/// Local: git -C <root> merge --ff-only <branch>   (no remote today)
/// Remote: git push -u origin <branch> && gh pr create --fill   (iff `git remote -v` non-empty)
pub async fn land(repo_root: &Path, wt: &Worktree) -> anyhow::Result<Landed>;
pub fn has_remote(repo_root: &Path) -> bool;  // `git remote -v` non-empty — FALSE for this repo today
```

**Trigger:** the merge gate fires when a quark's assignment *completes* — i.e. its turn ends
`Ground` **and** its reply carries no `@mention` hand-off (i.e. `parse_addressee` at
`engine.rs:343` returned `None` ⇒ control is back with the human). At that point the assignment is
finished, the branch is complete, and the gate runs.

### Failure modes
- **Tests are slow.** `cargo test --workspace` in a cold worktree is minutes. It must not block the
  dispatch loop — run it in the `JoinSet` alongside turns, or as its own spawned task; the daemon
  must stay responsive (the whole point of `FIELD_POLL`, `engine.rs:25`).
- **`--ff-only` refuses** because `main` moved (another quark landed first). Then: `git rebase main`
  in the worktree, re-run tests, re-ask. Do **not** auto-merge with a merge commit — the whole gate's
  value is that `main` only ever contains tested, approved, linear work. Concurrency makes this the
  *common* case, not the edge case.
- **Rebase conflicts.** Cannot be auto-resolved. Surface as a Gluon message + `Status{Blocked}`;
  the human (or the orchestrator, next turn) resolves it. The quark's branch is preserved, so
  nothing is lost.
- **Tests are green in the worktree but red on `main` after merge** (semantic conflict: two quarks'
  individually-passing changes are jointly broken). The rebase-then-retest loop above is the only
  defence; state it as accepted residual risk.
- **`gh` not installed / not authed** on the remote path. `has_remote()` is `false` here so this is
  dormant; guard it anyway and fall back to local + a Gluon message.
- **Denied merge.** The branch stays. Nothing is deleted. The human can inspect
  `.hadron/trees/<id>/` directly (the chamber's Files/Changes rail already reads a repo root —
  `app.rs:1224-1225` — and could be pointed at a quark's tree; noted, out of scope).

### Tests
- `merge_decision_truth_table` (pure, gatekeeper) — mirrors `matrix.rs:131-148`'s style: green+approved ⇒ `Merge`; red+approved ⇒ `Block(TestsFailed)`; green+unapproved ⇒ `Block(NotApproved)`; no commits ⇒ `Block(NoCommits)`.
- `merge_approved_folds_the_grant_for_this_branch_only` (pure) — a grant for `quark/opus/A` does not approve `quark/opus/B`.
- `has_remote_is_false_for_a_bare_local_repo` (git-temp-repo).
- `land_ff_merges_locally_when_there_is_no_remote` — branch commit reaches `main`; `main` is linear.
- `land_refuses_when_main_moved` ⇒ the rebase path is taken and the ff retried.
- `a_completed_assignment_raises_a_merge_permission_req` (engine) — the field contains a `PermissionReq` whose description names the branch, and the quark ends `Waiting`.
- `bypass_mode_auto_merges` (engine) — mirrors `bypass_mode_auto_approves_bash` (`engine.rs:1014-1021`).

---

## Task 5 — The cwd override: the plumbing chain

**This is the load-bearing mechanical change, and it is a four-layer chain.** Every layer needs its
own test, because a silent break at any one of them ⇒ the quarks are back in the shared tree with
no error.

| # | Layer | File:line today | Change |
|---|---|---|---|
| 1 | `Projection` gains `pub cwd: PathBuf` (or `Option<PathBuf>`; `None` = inherit, for mocks/tests) | `crates/hadron-lattice/src/` (the `Projection` struct — every construction site must add the field; there are ~8 in engine/adapter tests, e.g. `claude.rs:136-145`, `agy.rs:88-99`) | additive field |
| 2 | Engine sets it | `engine.rs:245-320` `projection_for(...)` builds the `Projection` at `:310-319` — add `cwd: wt.path` (threaded from the `ensure` call in the dispatch loop) | pass the worktree path in |
| 3 | `CliInvocation` gains `pub cwd: Option<PathBuf>` | `adapter/runner.rs:5-10` — the struct **has no cwd field today** | additive field |
| 3b | Both adapters copy `turn.cwd` into the invocation | `claude.rs:54-66` `fn invocation(&self, prompt, mode)` and `agy.rs:48-56` — both construct `CliInvocation { program, args, stdin }` with no cwd. Signature becomes `invocation(&self, prompt, mode, cwd)` | copy it through |
| 4 | `ProcessRunner` applies it | `adapter/runner.rs:49-55` — `tokio::process::Command::new(&inv.program).args(&inv.args)` … **never calls `.current_dir()`**. This is the actual bug. | `if let Some(cwd) = &inv.cwd { cmd.current_dir(cwd); }` |

**Do the CLIs inherit cwd today? Yes.** Neither `claude.rs` nor `agy.rs` nor `ProcessRunner` sets
a working directory, so `claude` and `agy` both run in whatever directory `hadron-gluon` was
launched from. In the live `.hadron/` setup (per `MEMORY.md`, Opus + Gemini Pro seated in
`dev/hadron/.hadron`) that is the repo root — **both quarks, concurrently, in the human's tree.**
That is the hazard, exactly as stated, and it is live.

### Neither CLI takes a `--cwd`-style flag in our invocations
`claude` is invoked as `claude -p --output-format json [--model M] <posture…> [--resume SID]`
(`claude.rs:55-65`); `agy` as `agy --print <prompt> [--model M] <posture…>` (`agy.rs:49-55`).
Neither carries a directory argument, so **process cwd is the only lever** — which is why the fix
lands in `ProcessRunner`, not in an argv change. (This also keeps the argv-security follow-up noted
in `MEMORY.md` for `agy` untouched: no new argv surface.)

### Failure modes
- **`Option<PathBuf>` defaulting to `None` is a silent regression channel.** If an adapter forgets to
  copy `turn.cwd`, the CLI silently inherits the daemon's cwd — i.e. exactly the current bug, with
  no error. **Mitigation: make it non-optional (`PathBuf`) on `CliInvocation` and let the type
  system force every construction site.** The mock/test path passes a tempdir. Cost: churn in ~8
  test constructors. Worth it — this is the whole point of the task.
- **The quark's session resumes with a different cwd.** `ClaudeQuark` threads `--resume <session>`
  (`claude.rs:61-64`) across turns. A resumed Claude session whose cwd changed between turns is
  untested territory (the CLI's session state may carry a project path). Since the worktree path is
  **stable per quark** (that's why Task 1 chose a stable path over per-assignment paths), the cwd
  never changes for a given quark — this is a design constraint, not an accident. **Do not** switch
  to per-assignment worktree paths without re-examining this.
- **Relative paths in prompts.** The prompt (`adapter/prompt.rs`) never mentions a directory, and
  the projection's `git_diff` is repo-relative — so nothing breaks. But the quark should be *told*
  where it is: add one line to `prompt::build` — *"You are working in `<cwd>`, on branch
  `<branch>`. Commit your work there; do not touch the parent checkout."* The precedent is
  `mode_guidance` (`prompt.rs:8-21`), which exists because a model that isn't told its constraints
  *confidently narrates work it never did* (observed live, per that comment). Same failure class here.

### Tests
- `process_runner_runs_in_the_given_cwd` — the direct proof, in the style of
  `process_runner_pipes_stdin_to_stdout` (`runner.rs:145-157`): run `pwd` with `cwd = tempdir` and
  assert stdout is that dir. Cheap, real, no CLI.
- `claude_invocation_carries_the_projection_cwd` / `agy_invocation_carries_the_projection_cwd` —
  `FakeRunner.recorded` (`runner.rs:91`) already captures the whole `CliInvocation`; assert
  `recorded[0].cwd == projection.cwd`. Mirrors `invocation_carries_the_turn_posture`
  (`claude.rs:157-171`, `agy.rs:131-141`).
- `concurrent_projections_carry_distinct_cwds` — extend
  `two_quarks_named_in_one_message_run_concurrently` (`engine.rs:1441-1487`) so `OverlapQuark`
  (`:1404-1436`) records `turn.cwd`; assert the two recorded cwds differ. **This is the test that
  pins the whole plan**: it fails today (both would be the same/empty).
- A `#[ignore]`d live smoke test (the convention noted at `runner.rs:40`): seat the real
  `team.json` pair, send `@opus write a.txt` + `@agy write b.txt` in one message, assert two
  branches, two commits, disjoint diffs, and `main` untouched.

---

## Order of execution

0. **Task 0** — bin onto `serve()`, wire `with_git`, move `repo_root_of` into lattice. *(Nothing else can hook a lifecycle without it.)*
1. **Task 5** — the cwd chain. *(Do this second, not last: it is the actual hazard fix, it is mechanical, and every later task's tests depend on a quark actually running somewhere.)*
2. **Task 1 + 2** — `worktree.rs`, `ensure`/`reclaim`, HEAD-never-main. *(Together: the invariant is meaningless without the provisioning.)*
3. **Task 3** — commit-per-turn, snapshot repoint, `Kind::Edit` finally emitted, `TODO(worktrees)` deleted.
4. **Task 4** — the merge gate. *(Last: it consumes everything above.)*

Tasks 0/5 are the safety fix and are independently shippable. Tasks 1–4 are the discipline.

## Open decisions for execution

1. Does a `Auto`-mode quark get a remembered merge rule? (ULID in the op string ⇒ never matches ⇒ always asks. Probably right. Decide.)
2. `serve()` on a turn error: abort or log-and-continue? (The bin's own doc, `bin/hadron-gluon.rs:17-18`, flags this as still-open. Recommend log-and-continue + a Gluon message.)
3. Shared `CARGO_TARGET_DIR` across worktrees (fast, lock contention) vs per-worktree (slow, isolated)? Recommend per-worktree first; measure.
4. Should the chamber's Changes rail (`app.rs:1224-1225`) point at the *selected quark's* worktree rather than the repo root? Natural follow-on, out of scope.
