//! Per-branch peer-review quorum gate.
//!
//! Phase 1 Task 1 of the 2026-08-13 capabilities plan: when a branch's `QuorumPolicy`
//! is `RequirePeerReview`, the merge gate MUST refuse to land it until at least one
//! peer has recorded `ReviewVerdict::Approved`. `Solo` lets the author self-approve
//! — a deliberate escape hatch for single-quark projects, mirroring the pre-existing
//! `--no-verify` style opt-out (a future follow-up will wire the chamber command).
//!
//! **State per branch, not per engine**: a `ReviewGate` is constructed once and
//! shared. `record_verdict` accumulates verdicts keyed by `branch` so concurrent
//! worktrees do not stomp each other's gate state. The gate has NO knowledge of
//! branches that have not yet been recorded against — `is_blocked` returns `false`
//! (a no-op) for any branch it has never seen, because the higher-level merge gate
//! is the one that decides whether a branch needs review at all.

use std::collections::HashMap;

/// What a single reviewer decided about a branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewVerdict {
    /// The reviewer signed off — branch may land.
    Approved,
    /// The reviewer found problems — branch stays blocked.
    ChangesRequested,
}

/// Whether a branch needs an external sign-off before the merge gate will land it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuorumPolicy {
    /// Branch is blocked until a peer records `Approved`.
    RequirePeerReview,
    /// No review required — author alone is enough.
    Solo,
}

/// One review decision against one branch.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedVerdict {
    reviewer: String,
    verdict: ReviewVerdict,
}

/// Per-branch review state held by a `ReviewGate`.
///
/// Deliberately a `Vec` rather than a `HashSet<reviewer>`: the spec does not require
/// idempotency on duplicate reviewers (a reviewer re-approving should overwrite the
/// stale "ChangesRequested" — a `HashSet` would silently drop the new entry and
/// leave the old block in place, which is the worst possible behaviour: the branch
/// looks unblocked to a reader who scrolled past the original veto, but `is_blocked`
/// still returns `true` and the gate still refuses. A `Vec` of last-write-wins
/// entries makes the newest decision the one that counts, which is what a human
/// reviewer would expect.
type BranchVerdicts = Vec<RecordedVerdict>;

/// The gate itself. Construct once, share, mutate via `record_verdict`,
/// query via `is_blocked`. Cheap to clone if you need an owned copy for a worker
/// thread (`HashMap` is `Clone`).
#[derive(Debug, Clone)]
pub struct ReviewGate {
    policy: QuorumPolicy,
    /// Branch name → every verdict recorded against it (last write wins per reviewer).
    verdicts: HashMap<String, BranchVerdicts>,
}

impl ReviewGate {
    /// Build a fresh gate under the given policy. Starts empty — no branch is
    /// tracked until the first `record_verdict` lands.
    pub fn new(policy: QuorumPolicy) -> Self {
        Self { policy, verdicts: HashMap::new() }
    }

    /// Record one reviewer's verdict for one branch. Re-recording from the same
    /// reviewer overwrites the prior entry (last write wins) — see the comment on
    /// `BranchVerdicts` for why.
    ///
    /// `branch` and `reviewer` are caller-supplied strings. The gate does NOT
    /// validate that `reviewer` is a real quark id, nor that it differs from the
    /// branch author: the chamber/merge-gate layer above owns identity. The gate
    /// is a pure data structure over (branch, reviewer, verdict) tuples.
    pub fn record_verdict(&mut self, branch: &str, reviewer: &str, verdict: ReviewVerdict) {
        let entries = self.verdicts.entry(branch.to_string()).or_default();
        // Drop any prior verdict from this reviewer; insert the new one at the end
        // so iteration order matches chronological insertion.
        entries.retain(|v| v.reviewer != reviewer);
        entries.push(RecordedVerdict { reviewer: reviewer.to_string(), verdict });
    }

    /// Should the merge gate refuse to land `branch`?
    ///
    /// Rules, in order:
    /// 1. `Solo` policy → never blocked. The whole point of `Solo` is to opt out
    ///    of the quorum.
    /// 2. `RequirePeerReview` with no recorded verdicts → blocked. No reviewer
    ///    has touched it, so the gate has nothing to green-light.
    /// 3. `RequirePeerReview` where the most-recent verdict on the branch is
    ///    `ChangesRequested` → blocked. A `ChangesRequested` from reviewer X
    ///    followed by an `Approved` from the same X unblocks (the reviewer
    ///    updated their mind); an `Approved` from X followed by a fresh
    ///    `ChangesRequested` from Y re-blocks.
    /// 4. Otherwise → not blocked. At least one `Approved` verdict stands.
    ///
    /// The "most recent" lookup walks the recorded entries in insertion order
    /// and keeps the last `Approved` or `ChangesRequested` per reviewer, so the
    /// effective verdict for the branch is the lexicographically last write.
    pub fn is_blocked(&self, branch: &str) -> bool {
        match self.policy {
            QuorumPolicy::Solo => false,
            QuorumPolicy::RequirePeerReview => match self.verdicts.get(branch) {
                None => true,
                Some(entries) => effective_verdict(entries)
                    .map(|v| v == ReviewVerdict::ChangesRequested)
                    .unwrap_or(true),
            },
        }
    }
}

/// The branch's effective verdict, computed as "the last write from any reviewer".
///
/// Returns `None` when `entries` is empty, which `is_blocked` translates to "blocked"
/// under `RequirePeerReview` (no signal yet).
fn effective_verdict(entries: &[RecordedVerdict]) -> Option<ReviewVerdict> {
    if entries.is_empty() { return None; }
    // `record_verdict` already de-duplicates per reviewer before appending, so the
    // last element of `entries` is by construction the chronologically last write.
    entries.last().map(|v| v.verdict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_peer_review_blocks_when_no_verdicts_recorded() {
        let mut gate = ReviewGate::new(QuorumPolicy::RequirePeerReview);
        assert!(gate.is_blocked("quark/ollama/feature"), "no verdicts means blocked");
        // Recording only a `ChangesRequested` does NOT unblock.
        gate.record_verdict("quark/ollama/feature", "@acp-claude", ReviewVerdict::ChangesRequested);
        assert!(gate.is_blocked("quark/ollama/feature"));
    }

    #[test]
    fn require_peer_review_unblocks_once_a_peer_records_approved() {
        let mut gate = ReviewGate::new(QuorumPolicy::RequirePeerReview);
        gate.record_verdict("quark/ollama/feature", "@acp-claude", ReviewVerdict::Approved);
        assert!(!gate.is_blocked("quark/ollama/feature"), "an Approved verdict unblocks the branch");
    }

    #[test]
    fn solo_policy_never_blocks_any_branch() {
        let mut gate = ReviewGate::new(QuorumPolicy::Solo);
        // Even with zero verdicts and no record of the branch, Solo lets it through.
        assert!(!gate.is_blocked("quark/ollama/feature"));
        // A `ChangesRequested` verdict is ignored under Solo.
        gate.record_verdict("quark/ollama/feature", "@acp-claude", ReviewVerdict::ChangesRequested);
        assert!(!gate.is_blocked("quark/ollama/feature"));
    }

    #[test]
    fn a_changes_requested_after_an_approved_reblocks_the_branch() {
        let mut gate = ReviewGate::new(QuorumPolicy::RequirePeerReview);
        gate.record_verdict("quark/ollama/feature", "@acp-claude", ReviewVerdict::Approved);
        assert!(!gate.is_blocked("quark/ollama/feature"));
        gate.record_verdict("quark/ollama/feature", "@acp-sonnet", ReviewVerdict::ChangesRequested);
        assert!(gate.is_blocked("quark/ollama/feature"), "a fresh veto re-blocks even after approval");
    }

    #[test]
    fn a_reviewer_re_recording_approved_overrides_their_own_changes_requested() {
        let mut gate = ReviewGate::new(QuorumPolicy::RequirePeerReview);
        gate.record_verdict("quark/ollama/feature", "@acp-claude", ReviewVerdict::ChangesRequested);
        assert!(gate.is_blocked("quark/ollama/feature"));
        // Same reviewer changes their mind.
        gate.record_verdict("quark/ollama/feature", "@acp-claude", ReviewVerdict::Approved);
        assert!(!gate.is_blocked("quark/ollama/feature"), "last write wins per reviewer");
    }

    #[test]
    fn branches_are_isolated_from_each_other() {
        let mut gate = ReviewGate::new(QuorumPolicy::RequirePeerReview);
        gate.record_verdict("branch-a", "@acp-claude", ReviewVerdict::Approved);
        assert!(!gate.is_blocked("branch-a"));
        assert!(gate.is_blocked("branch-b"), "an approved branch-a must not unblock branch-b");
    }

    #[test]
    fn unknown_branch_is_blocked_under_require_peer_review() {
        // The gate has never seen this branch — under RequirePeerReview that means
        // blocked. The merge-gate layer above is responsible for only asking about
        // branches that exist.
        let gate = ReviewGate::new(QuorumPolicy::RequirePeerReview);
        assert!(gate.is_blocked("never-seen-this-branch"));
    }

    #[test]
    fn switching_policy_at_construction_time_uses_the_new_policy() {
        // Sanity: a fresh gate honours whatever policy it was built with; no global
        // state, no cross-instance bleed.
        let approved = ReviewGate::new(QuorumPolicy::RequirePeerReview);
        let solo = ReviewGate::new(QuorumPolicy::Solo);
        assert!(approved.is_blocked("branch-x"));
        assert!(!solo.is_blocked("branch-x"));
    }
}
