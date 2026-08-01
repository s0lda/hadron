
use hadron_lattice::term::{self, Source};
use hadron_lattice::{
    Actor, Event, Kind, QuarkId, QuarkState,
};

use crate::field::read_events;

use super::*;

/// Does this text read like a turn that had to debug something?
///
/// The three markers are the vocabulary the nucleus lesson
/// `grepping-a-test-run-throws-away-the-only-diagnostic` already names: a panic line, a
/// failed test, or a rustc error code. Deliberately a dumb substring check — deciding
/// whether a lesson was *learned* is a model judgment, not something the engine can
/// observe, so this only spots the evidence and leaves the judgment to whoever reads it.
pub(super) fn looks_like_a_debugging_turn(text: &str) -> bool {
    text.contains("panicked") || text.contains("FAILED") || text.contains("error[E")
}

/// How far back the nudge looks for that evidence. The field is one long log, so this is
/// an approximation of "this turn" — generous enough to catch a debugging pass, short
/// enough that yesterday's red suite does not nag forever.
const NUDGE_LOOKBACK: usize = 20;

/// The sentence every merge-gate hand-back opens with. Two jobs: it tells the quark what
/// happened, and it is what `is_gate_handback` matches to bound the retries. A
/// visible sentence rather than a hidden token on purpose — the quark reads this body as
/// its task, so a marker it can see is one it can quote back.
pub(super) const GATE_HANDBACK_MARKER: &str = "The merge gate stopped this branch from landing.";

/// How many repair turns ONE assignment gets before the gate stops handing the branch
/// back and asks a human instead. Each one costs a full test run plus a model turn, so
/// this is deliberately small: a branch that cannot heal itself in two passes is not
/// going to heal itself in five, and the loop is the expensive failure mode here.
const MAX_GATE_HANDBACKS: usize = 2;

impl super::Engine {
    /// Whether `e` is a merge-gate hand-back already issued to `target` for `assignment`.
    fn is_gate_handback(e: &Event, target: &QuarkId, assignment: ulid::Ulid) -> bool {
        e.from == Actor::Gluon
            && e.to.as_ref() == Some(target)
            && e.answers == Some(assignment)
            && matches!(&e.kind, Kind::Message { body } if body.contains(GATE_HANDBACK_MARKER))
    }

    /// Stop the merge, but let the quark keep working on it.
    ///
    /// The gate refuses for two very different kinds of reason, and they need two very
    /// different routes:
    ///
    /// - Something wrong INSIDE the quark's own worktree — a rebase that conflicts, a
    ///   block conflict, red tests, uncommitted work. The quark is the only party that
    ///   can fix it, it is sitting in the tree that needs fixing, and asking a human is
    ///   just latency. That is this function.
    /// - Something wrong in the TARGET checkout — `git merge --ff-only` refusing because
    ///   the human's tree has uncommitted changes to a file the branch rewrites. The
    ///   quark cannot fix that, and must not try: it works in the human's own checkout
    ///   (see the `live-swarm-shares-the-checkout` hazard), so a quark told "fix the
    ///   merge" would `stash`/`add` work the human never committed. Those stay on
    ///   [`reroute_blocked_with_severity`], addressed to the human/orchestrator.
    ///
    /// The shape here is the merge refusal's, not a new mechanism: the finished turn
    /// still ends on a terminal `Blocked` — appended FIRST, so nothing dangles and so
    /// `next_pending` reads the hand-back below as unanswered rather than as already
    /// completed — and the hand-back is then an ordinary addressed `Message`, which the
    /// existing dispatch loop picks up with no new wiring.
    ///
    /// It is stamped `answers = assignment`, which is what keeps the quark on its OWN
    /// branch (see [`Engine::continued_assignment`]). Without that the re-excited quark
    /// resolves a fresh assignment ULID, `worktree::ensure` cuts a new branch off `base`,
    /// and the failed branch is left behind for `run.rs`'s superseded-branch check to
    /// re-gate and re-fail on every pass — the quark frozen forever, which is exactly
    /// the bug this exists to fix.
    ///
    /// Bounded by [`MAX_GATE_HANDBACKS`] per assignment: a branch that will not heal
    /// escalates to a human rather than burning a test run and a turn per pass.
    pub(super) async fn hand_back_to_quark(
        &self,
        target: &QuarkId,
        assignment: ulid::Ulid,
        why: &str,
    ) -> anyhow::Result<()> {
        // Re-read rather than take the caller's snapshot: the gate's test run takes
        // minutes, and a concurrent sibling may have moved the field since.
        let events = read_events(&self.field_path)?;
        let prior = events
            .iter()
            .filter(|e| Self::is_gate_handback(e, target, assignment))
            .count();

        if prior >= MAX_GATE_HANDBACKS {
            return self
                .reroute_blocked_with_severity(
                    target,
                    &format!(
                        "{why}\n\n`@{}` has already had {prior} repair turn(s) on this branch and it \
                         still cannot land, so the gate is escalating rather than spending another. \
                         The branch is preserved — it needs a human.",
                        target.as_str()
                    ),
                    hadron_lattice::Severity::Error,
                )
                .await;
        }

        self.park_blocked(target).await?;
        self.append(
            Event::new(
                Actor::Gluon,
                Some(target.clone()),
                Kind::Message {
                    body: format!(
                        "@{id} {GATE_HANDBACK_MARKER} Nothing was merged and nothing was lost.\n\n\
                         {why}\n\n\
                         You are still on your own branch in your own worktree — fix it THERE and \
                         end your turn as you normally would; the gate retries the merge on its own. \
                         Do not touch the main checkout, and do not start new work until this lands. \
                         If you cannot fix it, say so and hand back to a human rather than forcing it.",
                        id = target.as_str(),
                    ),
                },
            )
            .with_severity(hadron_lattice::Severity::Error)
            .with_answers(assignment),
        )
        .await
    }

    /// The merge gate, fired when an assignment completes. Returns `true` if it parked
    /// the quark (Waiting on a human, or Blocked on red tests), in which case the
    /// caller must NOT append `Ground`.
    ///
    /// The DECISION is pure and lives in `hadron-gatekeeper` (that crate is
    /// side-effect-free by contract). Only the EFFECTS — `cargo test`, `git merge` —
    /// live here, behind the [`MergeRunner`](crate::merge::MergeRunner) seam.
    ///
    /// Human approval reuses the EXISTING permission channel: a `PermissionReq` from
    /// the quark, surfaced by `gatekeeper::pending_permission`, rendered by the chamber
    /// the human already has, answered by the same `PermissionGrant`. No second
    /// approval mechanism.
    pub(super) async fn merge_gate(&self, target: &QuarkId, t: &TurnTree) -> anyhow::Result<bool> {
        use hadron_gatekeeper::{BlockReason, BranchState, MergeVerdict};
        let Some(runner) = &self.merge else { return Ok(false) };
        let Some(root) = &self.repo_root else { return Ok(false) };

        let state = BranchState {
            commits: crate::worktree::commits_ahead(&t.wt, &t.base)?,
            dirty: crate::worktree::is_dirty(&t.wt.path)?,
            is_default_branch: t.wt.branch == t.base,
        };
        // Nothing to land (a pure-conversation turn): quiesce normally, silently.
        if state.commits == 0 {
            return Ok(false);
        }

        // **Rebase BEFORE testing, not after.** See `merge::sync` for the two live
        // failures this order fixes; the short version is that a branch cut before a
        // fix landed on `base` was tested without that fix and reported red forever.
        // A conflict is reported here rather than after a full test run: a branch that
        // cannot replay onto `base` cannot land whatever the tests say.
        //
        // A DIRTY tree is skipped, not rebased: `git rebase` would refuse and the human
        // would be told "conflict" when the real answer is `BlockReason::DirtyTree`,
        // which the verdict below already words correctly.
        let state = if state.dirty {
            state
        } else {
            match runner.sync(&t.wt, &t.base) {
                crate::merge::Synced::Conflicted(err) => {
                    // A conflict a machine must not resolve *unattended* — but the quark
                    // whose branch it is may absolutely resolve it, in its own tree, with
                    // its own judgment. Hand it back rather than freezing it.
                    self.hand_back_to_quark(
                        target,
                        t.assignment,
                        &crate::merge::Landed::Conflicted(err).describe(&t.wt.branch, &t.base),
                    )
                    .await?;
                    return Ok(true);
                }
                crate::merge::Synced::AlreadyCurrent => state,
                crate::merge::Synced::Rebased => BranchState {
                    commits: crate::worktree::commits_ahead(&t.wt, &t.base)?,
                    ..state
                },
            }
        };
        // The rebase emptied the branch: every commit was already on `base` by another
        // route (a cherry-pick, a sibling branch). There is nothing to test and nothing
        // to land — falling through here is what made an already-landed branch fail the
        // gate on every single retry. It is NOT silent, unlike the pure-conversation
        // early-out above: this branch has been visibly failing, and going quiet is
        // indistinguishable from the gate breaking. `Actor::Gluon` + `to: None` + no
        // line starting with `@` prints without waking a seat.
        if state.commits == 0 {
            self.append(
                Event::new(
                    Actor::Gluon,
                    None,
                    Kind::Message {
                        body: crate::merge::Landed::AlreadyLanded
                            .describe(&t.wt.branch, &t.base),
                    },
                )
                .with_severity(hadron_lattice::Severity::Info),
            )
            .await?;
            return Ok(false);
        }

        let events = read_events(&self.field_path)?;
        let op = hadron_gatekeeper::merge_op(&t.wt.branch, &t.base);

        // Approval: an explicit human grant for THIS branch, or the mode ladder saying
        // the human already delegated it. A merge is BashExec-class, so `decide` gives
        // Bypass ⇒ auto-merge for free, and Ask/Write/Auto ⇒ ask. (Auto never remembers
        // a merge: the op string contains the assignment ULID, so it is never the same
        // op twice — which is the right answer. You should not blanket-trust merges.)
        let mode =
            hadron_gatekeeper::effective_mode(&events, target, self.no_human, self.is_orchestrator(target));
        let global = hadron_gatekeeper::global_mode(&events);
        let rules = hadron_gatekeeper::allow_rules(&events);
        // Per-seat `commands` allow/deny is deliberately NOT folded here (review I1).
        // The merge-gate `op` is a synthetic merge sentence carrying the assignment
        // ULID (see the comment above), never a shell command — so a command-shaped
        // `not_allowed` is dead against a merge, and a broad command `allowed`
        // (e.g. `*`) must not silently auto-land one. Command authority governs
        // BashExec, not merges; the merge stays on the base ladder + field grants.
        let deny = hadron_gatekeeper::DenyRules::new();
        // Under No-Human-Mode a non-delegated merge falls to `MergeVerdict::Block
        // (NotApproved)` below, which parks the SAME PermissionReq/Waiting the
        // permission-ask gate does — so a merge that escalates to AskOrchestrator
        // is picked up by the very same auto-scheduler, with no separate wiring.
        let delegated = matches!(
            hadron_gatekeeper::decide(
                mode,
                global,
                self.no_human,
                hadron_gatekeeper::Risk::BashExec,
                &op,
                target,
                &rules,
                &deny
            ),
            hadron_gatekeeper::Decision::AutoApprove
        );
        let approved = delegated || hadron_gatekeeper::merge_approved(&events, target, &op);

        // Tests run IN the quark's worktree, on the branch as it now stands — so we
        // never land untested commits, even on the re-asked second pass.
        let (tests_passed, tail) = runner.tests(&t.wt).await?;

        match hadron_gatekeeper::merge_decision(tests_passed, approved, &state) {
            MergeVerdict::Merge => {
                if delegated {
                    // Bypass: record req + grant for audit, exactly as the existing
                    // permission path does, then land without asking.
                    self.append(Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::PermissionReq {
                            risk: hadron_gatekeeper::Risk::BashExec,
                            description: op.clone(),
                        },
                    ))
                    .await?;
                    self.append(Event::new(
                        Actor::Gluon,
                        Some(target.clone()),
                        Kind::PermissionGrant { approved: true, remember: false },
                    ))
                    .await?;
                }
                // A `land` FAILURE is a git error, not a rebase conflict (that comes
                // back as `Landed::Conflicted`, an `Ok`). The realistic cause is the
                // target checkout being unmergeable — e.g. an uncommitted local change
                // to a file this branch rewrites, so `git merge --ff-only` refuses. It
                // MUST NOT propagate: the audit `PermissionReq`/`PermissionGrant` above
                // are already in the field, and an `Err` here returns out of
                // `run_until_quiesce` with no terminal status for the quark — so the
                // daemon's re-invoke loop re-reads that dangling `PermissionGrant{→quark}`
                // as an unanswered turn-request and re-dispatches the quark forever (a
                // live hot loop: many `Excited`, never a `Ground`). Reroute it to
                // `Blocked` instead — a turn-completion, which answers the grant and
                // closes the loop — exactly as every other merge refusal already does.
                let conflicts = forge_block_conflicts(&root, &t.wt.path, root);
                if !conflicts.is_empty() {
                    let details = conflicts
                        .iter()
                        .map(|c| format!("- `{}` block `{}` [hash: {}]", c.file, c.block_name, c.base_hash))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.hand_back_to_quark(
                        target,
                        t.assignment,
                        &format!(
                            "AST block conflict detected landing `{}` onto `{}`:\n{}\n\nResolve block conflicts before merging.",
                            t.wt.branch, t.base, details
                        ),
                    )
                    .await?;
                    return Ok(true);
                }

                let landed = match runner.land(root, &t.wt, &t.base) {
                    Ok(landed) => landed,
                    Err(e) => {
                        // **Deliberately NOT a hand-back.** This failure is in the TARGET
                        // checkout, not the quark's worktree — the realistic cause is the
                        // human's own tree carrying an uncommitted change to a file this
                        // branch rewrites. The quark cannot fix that from its worktree, and
                        // must not be invited to try: quarks run git in the human's actual
                        // checkout, so one told "fix the merge" would `stash` or `add` work
                        // the human never committed. A human resolves this one.
                        self.reroute_blocked_with_severity(
                            target,
                            &format!(
                                "`{}` could not be merged → `{}`: {e:#}. This is in the TARGET checkout, not `@{}`'s worktree, so the quark is not being asked to fix it — commit or stash the conflicting local changes there and the branch lands on this quark's next turn. The branch is preserved at `{}`.",
                                t.wt.branch,
                                t.base,
                                target.as_str(),
                                t.wt.path.display()
                            ),
                            hadron_lattice::Severity::Error,
                        )
                        .await?;
                        return Ok(true);
                    }
                };
                let body = landed.describe(&t.wt.branch, &t.base);
                self.append(
                    Event::new(Actor::Gluon, None, Kind::Message { body })
                        .with_severity(hadron_lattice::Severity::Info),
                )
                .await?;

                // A turn that visibly debugged something is the turn most likely to have
                // learned something, and the moment it lands is the last moment anyone is
                // still holding the context. So: a reminder, never a gate — blocking here
                // is the shape that produced `a-failed-merge-land-hot-loops-via-the-audit-grant`.
                // `Actor::Gluon` + `to: None` + no line starting with `@` is the one way to
                // print without waking a seat (see the "Printing Without Waking the Swarm"
                // invariant); an `@orchestrator` here would silently bill a turn per land.
                if events
                    .iter()
                    .rev()
                    .take(NUDGE_LOOKBACK)
                    .any(|e| matches!(&e.kind, Kind::Message { body } if looks_like_a_debugging_turn(body)))
                {
                    self.append(Event::new(
                        Actor::Gluon,
                        None,
                        Kind::Message {
                            body: "This turn's transcript shows a debugging pass. If it taught \
                                   something the swarm should not pay for twice, `/learn <lesson>` \
                                   writes it to the nucleus without spending a turn."
                                .to_string(),
                        },
                    ))
                    .await?;
                }

                // Ride cleanup on every successful land, not just daemon startup — a
                // long-running daemon otherwise accumulates one `quark/*` branch per
                // turn between restarts. Non-fatal, same as the startup sweep in
                // `bin/hadron-gluon.rs`: a land that succeeded must not be undone by a
                // prune failure. The branch just landed is still checked out in `t.wt`
                // (a new assignment only moves it off at the next `ensure`), so `-d`
                // will safely refuse to delete it and this pass sweeps everyone ELSE's
                // already-landed, now-idle branches instead.
                if let Err(e) = crate::worktree::prune_merged_branches(root, &t.base) {
                    term::warn(Source::Gluon, &format!("branch prune after land failed (non-fatal): {e:#}"));
                }

                Ok(false) // landed → the quark grounds normally
            }
            MergeVerdict::Block(BlockReason::NotApproved) => {
                // Idempotent: if the ask is already outstanding for this branch, stay
                // Waiting rather than appending a second request.
                let already_asked = hadron_gatekeeper::pending_permission(&events, target)
                    .is_some_and(|p| p.description == op);
                if !already_asked {
                    self.append(Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::PermissionReq {
                            risk: hadron_gatekeeper::Risk::BashExec,
                            description: op,
                        },
                    ))
                    .await?;
                }
                self.append(Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::Status { state: QuarkState::Waiting },
                ))
                .await?;
                Ok(true)
            }
            MergeVerdict::Block(reason) => {
                // Red tests / a dirty tree / a branch that is somehow the default one.
                // The branch STAYS. Nothing is deleted — the work is evidence.
                let why = format!(
                    "merge of `{}` blocked: {}. The branch is preserved at `{}`.\n\n{tail}",
                    t.wt.branch,
                    reason.describe(),
                    t.wt.path.display()
                );
                match reason {
                    // Both live entirely inside the quark's own worktree: red tests are
                    // its code, and an uncommitted tree is its uncommitted work. It is
                    // already standing in the right place to fix them.
                    BlockReason::TestsFailed | BlockReason::DirtyWorktree => {
                        self.hand_back_to_quark(target, t.assignment, &why).await?;
                    }
                    // `BranchIsDefault` is a discipline violation the quark cannot undo
                    // from inside (it is standing ON the branch it must never be on), and
                    // `NoCommits` is unreachable here — the `commits == 0` early-outs above
                    // catch it. Neither is a repair a quark should be handed.
                    BlockReason::BranchIsDefault
                    | BlockReason::NoCommits
                    | BlockReason::NotApproved => {
                        self.reroute_blocked_with_severity(
                            target,
                            &why,
                            hadron_lattice::Severity::Error,
                        )
                        .await?;
                    }
                }
                Ok(true)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockConflict {
    pub file: String,
    pub block_name: String,
    pub base_hash: String,
}

fn collect_code_files(dir: &std::path::Path, rel: &std::path::Path, acc: &mut Vec<std::path::PathBuf>) {
    let current = if rel.as_os_str().is_empty() {
        dir.to_path_buf()
    } else {
        dir.join(rel)
    };
    let Ok(entries) = std::fs::read_dir(&current) else { return; };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "target" || name_str == "node_modules" {
            continue;
        }
        let child_rel = if rel.as_os_str().is_empty() {
            std::path::PathBuf::from(&name)
        } else {
            rel.join(&name)
        };
        if let Ok(ft) = entry.file_type() {
            if ft.is_dir() {
                collect_code_files(dir, &child_rel, acc);
            } else if ft.is_file() {
                let rel_str = child_rel.to_str().unwrap_or("");
                if hadron_forge::lang::lang_for_path(rel_str) != hadron_forge::lang::Lang::Opaque {
                    acc.push(child_rel);
                }
            }
        }
    }
}

pub fn forge_block_conflicts(
    base_wt: &std::path::Path,
    branch_wt: &std::path::Path,
    target_wt: &std::path::Path,
) -> Vec<BlockConflict> {
    let mut files = Vec::new();
    collect_code_files(branch_wt, std::path::Path::new(""), &mut files);
    collect_code_files(target_wt, std::path::Path::new(""), &mut files);
    files.sort();
    files.dedup();

    let mut conflicts = Vec::new();

    for rel in files {
        let base_p = base_wt.join(&rel);
        let branch_p = branch_wt.join(&rel);
        let target_p = target_wt.join(&rel);

        let rel_str = rel.to_str().unwrap_or("");
        let lang = hadron_forge::lang::lang_for_path(rel_str);

        let Ok(base_src) = std::fs::read_to_string(&base_p) else { continue; };
        let Ok(branch_src) = std::fs::read_to_string(&branch_p) else { continue; };
        let Ok(target_src) = std::fs::read_to_string(&target_p) else { continue; };

        let base_blocks = hadron_forge::block::parse_blocks_lang(&base_src, lang);
        let branch_blocks = hadron_forge::block::parse_blocks_lang(&branch_src, lang);
        let target_blocks = hadron_forge::block::parse_blocks_lang(&target_src, lang);

        for b_block in &base_blocks {
            let h = &b_block.hash;
            let in_branch = branch_blocks.iter().any(|b| &b.hash == h);
            let in_target = target_blocks.iter().any(|b| &b.hash == h);

            if !in_branch && !in_target {
                let br_match = branch_blocks.iter().find(|b| b.name == b_block.name);
                let tg_match = target_blocks.iter().find(|b| b.name == b_block.name);

                let br_hash = br_match.map(|b| &b.hash);
                let tg_hash = tg_match.map(|b| &b.hash);

                if br_hash != tg_hash {
                    conflicts.push(BlockConflict {
                        file: rel.to_string_lossy().to_string(),
                        block_name: b_block.name.clone(),
                        base_hash: h.clone(),
                    });
                }
            }
        }
    }

    conflicts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forge_block_conflicts_detects_conflicting_edits_to_same_block() {
        let base_dir = tempfile::tempdir().unwrap();
        let branch_dir = tempfile::tempdir().unwrap();
        let target_dir = tempfile::tempdir().unwrap();

        let base_code = "pub fn foo() -> i32 { 1 }\npub fn bar() -> i32 { 2 }\n";
        let branch_code = "pub fn foo() -> i32 { 10 }\npub fn bar() -> i32 { 2 }\n";
        let target_code = "pub fn foo() -> i32 { 20 }\npub fn bar() -> i32 { 2 }\n";

        std::fs::write(base_dir.path().join("main.rs"), base_code).unwrap();
        std::fs::write(branch_dir.path().join("main.rs"), branch_code).unwrap();
        std::fs::write(target_dir.path().join("main.rs"), target_code).unwrap();

        let conflicts = forge_block_conflicts(base_dir.path(), branch_dir.path(), target_dir.path());
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].file, "main.rs");
        assert_eq!(conflicts[0].block_name, "foo");
    }

    #[test]
    fn forge_block_conflicts_allows_disjoint_edits_to_different_blocks() {
        let base_dir = tempfile::tempdir().unwrap();
        let branch_dir = tempfile::tempdir().unwrap();
        let target_dir = tempfile::tempdir().unwrap();

        let base_code = "pub fn foo() -> i32 { 1 }\npub fn bar() -> i32 { 2 }\n";
        let branch_code = "pub fn foo() -> i32 { 10 }\npub fn bar() -> i32 { 2 }\n";
        let target_code = "pub fn foo() -> i32 { 1 }\npub fn bar() -> i32 { 20 }\n";

        std::fs::write(base_dir.path().join("main.rs"), base_code).unwrap();
        std::fs::write(branch_dir.path().join("main.rs"), branch_code).unwrap();
        std::fs::write(target_dir.path().join("main.rs"), target_code).unwrap();

        let conflicts = forge_block_conflicts(base_dir.path(), branch_dir.path(), target_dir.path());
        assert_eq!(conflicts.len(), 0);
    }
}
