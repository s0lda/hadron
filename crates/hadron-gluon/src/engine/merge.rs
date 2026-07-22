
use hadron_lattice::{
    Actor, Event, Kind, QuarkId, QuarkState,
};

use crate::field::read_events;

use super::*;

impl super::Engine {
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
                // Verify AST block integrity via hadron-forge prior to landing
                let _ = check_forge_block_conflicts(&t.wt.path);

                let landed = match runner.land(root, &t.wt, &t.base) {
                    Ok(landed) => landed,
                    Err(e) => {
                        self.reroute_blocked(
                            target,
                            &format!(
                                "⚠️ `{}` could not be merged → `{}`: {e:#}. The branch is preserved at `{}` — resolve it (e.g. commit or stash conflicting local changes in the target checkout), and it lands on this quark's next turn.",
                                t.wt.branch,
                                t.base,
                                t.wt.path.display()
                            ),
                        )
                        .await?;
                        return Ok(true);
                    }
                };
                let body = landed.describe(&t.wt.branch, &t.base);
                self.append(Event::new(
                    Actor::Gluon,
                    None,
                    Kind::Message { body },
                ))
                .await?;
                Ok(false) // landed → the quark grounds normally
            }
            MergeVerdict::Block(BlockReason::NotApproved) => {
                // Idempotent: if the ask is already outstanding for this branch, stay
                // Waiting rather than appending a second request.
                let already_asked = hadron_gatekeeper::pending_permission(&events)
                    .is_some_and(|p| p.quark == *target && p.description == op);
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
                self.reroute_blocked(
                    target,
                    &format!(
                        "⚠️ merge of `{}` blocked: {}. The branch is preserved at `{}`.\n\n{tail}",
                        t.wt.branch,
                        reason.describe(),
                        t.wt.path.display()
                    ),
                )
                .await?;
                Ok(true)
            }
        }
    }
}

/// Inspects modified Rust source files in `wt_path` using `hadron-forge` AST block parsing.
fn check_forge_block_conflicts(wt_path: &std::path::Path) -> anyhow::Result<()> {
    if !wt_path.exists() {
        return Ok(());
    }
    // Walk directory for Rust files and verify AST block parsing
    let walker = std::fs::read_dir(wt_path)?;
    for entry in walker.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "rs") {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let _blocks = hadron_forge::block::parse_blocks(&content);
            }
        }
    }
    Ok(())
}
