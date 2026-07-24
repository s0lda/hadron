
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
                let conflicts = forge_block_conflicts(&root, &t.wt.path, root);
                if !conflicts.is_empty() {
                    let details = conflicts
                        .iter()
                        .map(|c| format!("- `{}` block `{}` [hash: {}]", c.file, c.block_name, c.base_hash))
                        .collect::<Vec<_>>()
                        .join("\n");
                    self.reroute_blocked_with_severity(
                        target,
                        &format!(
                            "AST block conflict detected landing `{}` onto `{}`:\n{}\n\nResolve block conflicts before merging.",
                            t.wt.branch, t.base, details
                        ),
                        hadron_lattice::Severity::Error,
                    )
                    .await?;
                    return Ok(true);
                }

                let landed = match runner.land(root, &t.wt, &t.base) {
                    Ok(landed) => landed,
                    Err(e) => {
                        self.reroute_blocked_with_severity(
                            target,
                            &format!(
                                "`{}` could not be merged → `{}`: {e:#}. The branch is preserved at `{}` — resolve it (e.g. commit or stash conflicting local changes in the target checkout), and it lands on this quark's next turn.",
                                t.wt.branch,
                                t.base,
                                t.wt.path.display()
                            ),
                            hadron_lattice::Severity::Error,
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
                self.reroute_blocked_with_severity(
                    target,
                    &format!(
                        "merge of `{}` blocked: {}. The branch is preserved at `{}`.\n\n{tail}",
                        t.wt.branch,
                        reason.describe(),
                        t.wt.path.display()
                    ),
                    hadron_lattice::Severity::Error,
                )
                .await?;
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
