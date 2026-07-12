//! The merge gate's **decision**, and nothing else.
//!
//! This crate is offline and side-effect-free by contract (see the crate doc), so
//! the gate is split down that line: the truth table and the field-fold live here;
//! `cargo test` and `git merge` — the effects — live in `hadron_gluon::merge`.
//! Keeping the decision pure is what makes it a truth table you can read, rather
//! than a shell script you have to run to understand.

use hadron_lattice::{Actor, Event, Kind, QuarkId};

/// What the gate knows about a quark's branch, as observed by the caller. Plain
/// data: gathering it is the effectful side's job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchState {
    /// Commits this branch is ahead of the default branch. 0 ⇒ nothing to land.
    pub commits: usize,
    /// Uncommitted work still in the worktree. A turn is supposed to end on a
    /// commit, so a dirty tree at the gate means something went wrong.
    pub is_default_branch: bool,
    pub dirty: bool,
}

/// Why a merge did not happen. Every arm is a *preserved* branch: the gate blocks,
/// it never deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    TestsFailed,
    NotApproved,
    BranchIsDefault,
    DirtyWorktree,
    NoCommits,
}

impl BlockReason {
    /// Human-facing text for the Gluon message the engine appends.
    pub fn describe(&self) -> &'static str {
        match self {
            BlockReason::TestsFailed => "the workspace tests are red on this branch",
            BlockReason::NotApproved => "a human has not approved it",
            BlockReason::BranchIsDefault => {
                "the branch IS the default branch — a quark must never work there"
            }
            BlockReason::DirtyWorktree => {
                "the worktree still has uncommitted work (a turn must end on a commit)"
            }
            BlockReason::NoCommits => "there is nothing to land",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeVerdict {
    Merge,
    Block(BlockReason),
}

/// The truth table. No I/O, no clock, no git.
///
/// Order matters: the *structural* refusals (a branch that isn't one, nothing to
/// land, work not committed) are reported ahead of the policy ones, because they
/// tell the human something different — "this cannot be merged" rather than "this
/// may not be merged yet".
pub fn merge_decision(
    tests_passed: bool,
    human_approved: bool,
    state: &BranchState,
) -> MergeVerdict {
    if state.is_default_branch {
        return MergeVerdict::Block(BlockReason::BranchIsDefault);
    }
    if state.commits == 0 {
        return MergeVerdict::Block(BlockReason::NoCommits);
    }
    if state.dirty {
        return MergeVerdict::Block(BlockReason::DirtyWorktree);
    }
    if !tests_passed {
        return MergeVerdict::Block(BlockReason::TestsFailed);
    }
    if !human_approved {
        return MergeVerdict::Block(BlockReason::NotApproved);
    }
    MergeVerdict::Merge
}

/// The canonical operation string for merging `branch` into `base`.
///
/// It is derivable from `(branch, base)` alone — deliberately carrying no commit
/// count, no test result, nothing that varies between the moment the request is
/// written and the moment the field is folded to see whether it was granted. A
/// description that drifted would never match, and every merge would look
/// unapproved forever.
pub fn merge_op(branch: &str, base: &str) -> String {
    format!("merge `{branch}` into `{base}`")
}

/// Fold the field: has the human approved *this* branch's merge?
///
/// Reuses the existing permission channel exactly — a `PermissionReq` authored by
/// the quark with the canonical op, answered by an approving `PermissionGrant`
/// addressed back to it. There is no second approval mechanism, and the chamber
/// renders this ask with the UI it already has.
///
/// A grant for one branch does not approve another: the op carries the branch, and
/// the branch carries the assignment's ULID.
pub fn merge_approved(events: &[Event], quark: &QuarkId, op: &str) -> bool {
    let Some(idx) = events.iter().rposition(|e| {
        matches!(&e.from, Actor::Quark(q) if q == quark)
            && matches!(&e.kind, Kind::PermissionReq { description, .. } if description == op)
    }) else {
        return false;
    };
    events[idx + 1..].iter().any(|e| {
        e.to.as_ref() == Some(quark)
            && matches!(e.kind, Kind::PermissionGrant { approved: true, .. })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hadron_lattice::Risk;

    fn q(id: &str) -> QuarkId {
        QuarkId::new(id)
    }

    fn clean(commits: usize) -> BranchState {
        BranchState { commits, dirty: false, is_default_branch: false }
    }

    fn req(from: &str, op: &str) -> Event {
        Event::new(
            Actor::Quark(q(from)),
            None,
            Kind::PermissionReq { risk: Risk::BashExec, description: op.into() },
        )
    }

    fn grant(to: &str, approved: bool) -> Event {
        Event::new(
            Actor::Human,
            Some(q(to)),
            Kind::PermissionGrant { approved, remember: false },
        )
    }

    #[test]
    fn merge_decision_truth_table() {
        // Green + approved ⇒ the only way anything reaches the default branch.
        assert_eq!(merge_decision(true, true, &clean(1)), MergeVerdict::Merge);
        // Red tests ⇒ blocked, however loudly the human approves.
        assert_eq!(
            merge_decision(false, true, &clean(1)),
            MergeVerdict::Block(BlockReason::TestsFailed)
        );
        // Green but unapproved ⇒ blocked. Tests are necessary, not sufficient.
        assert_eq!(
            merge_decision(true, false, &clean(1)),
            MergeVerdict::Block(BlockReason::NotApproved)
        );
        // Nothing to land.
        assert_eq!(
            merge_decision(true, true, &clean(0)),
            MergeVerdict::Block(BlockReason::NoCommits)
        );
        // A turn must end on a commit; uncommitted work is never landed.
        assert_eq!(
            merge_decision(true, true, &BranchState { commits: 1, dirty: true, is_default_branch: false }),
            MergeVerdict::Block(BlockReason::DirtyWorktree)
        );
        // The branch IS main — the case the whole plan exists to make impossible.
        assert_eq!(
            merge_decision(true, true, &BranchState { commits: 1, dirty: false, is_default_branch: true }),
            MergeVerdict::Block(BlockReason::BranchIsDefault)
        );
    }

    #[test]
    fn merge_op_is_stable_and_carries_the_branch() {
        assert_eq!(merge_op("quark/opus/01A", "main"), "merge `quark/opus/01A` into `main`");
        // Same inputs, same string — the fold depends on it.
        assert_eq!(merge_op("quark/opus/01A", "main"), merge_op("quark/opus/01A", "main"));
        assert_ne!(merge_op("quark/opus/01A", "main"), merge_op("quark/opus/01B", "main"));
    }

    #[test]
    fn merge_approved_folds_the_grant_for_this_branch_only() {
        let a = merge_op("quark/opus/01A", "main");
        let b = merge_op("quark/opus/01B", "main");
        let events = vec![req("opus", &a), grant("opus", true)];

        assert!(merge_approved(&events, &q("opus"), &a), "the granted branch is approved");
        // THE property: a grant for A does not approve B.
        assert!(!merge_approved(&events, &q("opus"), &b), "a grant for A leaked onto B");
        // …nor does it approve another quark's identically-named ask.
        assert!(!merge_approved(&events, &q("agy"), &a));
    }

    #[test]
    fn a_denial_is_not_an_approval() {
        let op = merge_op("quark/opus/01A", "main");
        let events = vec![req("opus", &op), grant("opus", false)];
        assert!(!merge_approved(&events, &q("opus"), &op));
    }

    #[test]
    fn an_unanswered_request_is_not_approved() {
        let op = merge_op("quark/opus/01A", "main");
        assert!(!merge_approved(&[req("opus", &op)], &q("opus"), &op));
    }

    /// A grant that lands *before* the request cannot answer it — otherwise a stale
    /// approval from an earlier assignment would silently authorize a new merge.
    #[test]
    fn a_grant_before_the_request_does_not_approve_it() {
        let op = merge_op("quark/opus/01A", "main");
        let events = vec![grant("opus", true), req("opus", &op)];
        assert!(!merge_approved(&events, &q("opus"), &op));
    }
}
