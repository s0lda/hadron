use std::collections::{HashMap, HashSet};

use hadron_lattice::term::{self, Source};
use hadron_lattice::{
    Actor, Event, Flavor, Kind, Mode, QuarkId, QuarkState,
    TurnOutcome,
};
use tokio::task::{AbortHandle, JoinSet};

use crate::field::read_events;

use super::*;

/// Resolve once `quark` has gone quiet for `deadline`, and never before — the
/// watchdog's clock, and the whole reason [`TURN_DEADLINE`] is not a wall-clock cap.
///
/// A "sign of life" is a fresh entry in `<field-dir>/live/<quark>.json`, the file the
/// adapter already overwrites on every thought chunk and tool call
/// (`hadron_lattice::live`) — this reuses the swarm's existing liveness signal rather
/// than inventing a second one, so anything the chamber's Live card can see, the
/// watchdog can see.
///
/// **The start of the turn counts as a sign of life.** A transport that publishes
/// nothing at all — a CLI seat whose `CliSpec.stream` is `None`, the only shape
/// `build_seat_watched` leaves unwatched — therefore expires at exactly `deadline`, bit-identical to the
/// flat timeout this replaced. The change can only ever *extend* a turn, never shorten
/// one, which is what makes it safe to land on the dispatch path.
///
/// Returns how long the silence lasted, so the error can say it.
async fn until_silent(
    live_dir: &std::path::Path,
    quark: &QuarkId,
    deadline: std::time::Duration,
) -> std::time::Duration {
    // Poll well inside `live::STALE_AFTER_SECS`, or a write could go stale between
    // two ticks and never be seen. Derived from the deadline so a test with a tiny
    // one exercises this same path rather than a code branch of its own.
    let tick = (deadline / 4).min(std::time::Duration::from_secs(30)).max(std::time::Duration::from_millis(10));
    let mut last_sign = std::time::Instant::now();
    let mut newest_seen: Option<chrono::DateTime<chrono::Utc>> = None;
    loop {
        tokio::time::sleep(tick).await;
        if let Some(activity) = hadron_lattice::live::read(live_dir, quark, chrono::Utc::now()) {
            if newest_seen.is_none_or(|seen| activity.at > seen) {
                newest_seen = Some(activity.at);
                last_sign = std::time::Instant::now();
            }
        }
        let silent_for = last_sign.elapsed();
        if silent_for >= deadline {
            return silent_for;
        }
    }
}

impl super::Engine {
    fn format_error_message(&self, quark_id: &QuarkId, err: &anyhow::Error) -> String {
        let err_text = super::quote_paths(&format!("{err:#}"));
        let orchestrator = self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator);
        if let Some(orch) = orchestrator {
            if &orch.id != quark_id {
                return format!(
                    "@{} Quark `{}` turn errored: {err_text}",
                    crate::router::ORCHESTRATOR_ALIAS,
                    quark_id.as_str()
                );
            }
        }
        format!("Quark `{}` turn errored: {err_text}", quark_id.as_str())
    }

    /// Dispatch every pending quark turn CONCURRENTLY until the field has no pending
    /// work **and** no turn is still in flight (quiesce), or the exchange budget is
    /// exhausted (backstop).
    ///
    /// Each pass re-reads the field, computes every pending target, and spawns a turn
    /// for each one that is not already running — so "@a do X and @b do Y" excites a
    /// and b at the same time instead of making b wait out a's whole turn. A quark
    /// only ever runs one turn at a time (`in_flight` + its own `Mutex`); a target
    /// that is already running is simply left for a later pass.
    ///
    /// Quiesce is the *conjunction*: an engine that has nothing pending but is still
    /// waiting on a running turn must not return, or the daemon would report the team
    /// idle while it is mid-thought.
    pub async fn run_until_quiesce(&mut self) -> anyhow::Result<()> {
        let mut exchanges = 0usize;
        // Keyed by (seat, lane), not bare `QuarkId`: a seat with a chat lane can have
        // its `Work` and `Chat` slots in flight at once — that concurrency is Task 6's
        // whole point. Every non-orchestrator seat only ever inserts `Work`, so this is
        // byte-for-byte the old single-lane behaviour for every seat without a chat lane.
        let mut in_flight: HashSet<(QuarkId, Lane)> = HashSet::new();
        // The abort handle for each in-flight turn, so the human's force-restart
        // ([`Kind::Reboot`]) can kill a wedged quark's turn *now* instead of waiting
        // out the 30-minute deadline. Kept in lockstep with `in_flight`: inserted at
        // spawn, removed the instant a turn joins or is rebooted.
        let mut abort_handles: HashMap<(QuarkId, Lane), AbortHandle> = HashMap::new();
        // When a graceful cancel was requested for an in-flight turn, and when — so a
        // second poll tick does not re-request it (the point of the slot is ONE
        // cancel per interruption, not a cancel every 150ms until it resolves), and
        // so a request that never resolves can be bounded against `CANCEL_DEADLINE`
        // rather than left to wait out `TURN_DEADLINE`. Cleared wherever the matching
        // `in_flight` entry is: both `join_next` arms below, and the deadline
        // fallback itself.
        let mut cancel_requested: HashMap<(QuarkId, Lane), std::time::Instant> = HashMap::new();
        // How many events existed when each in-flight turn was dispatched — the
        // boundary `message_arrived_since` measures "new" against. Without it, a
        // turn's own dispatch record (`pending_targets` legitimately still calls it
        // pending every pass it runs) would look identical to a fresh interrupting
        // message. Kept in lockstep with `in_flight`, same as `cancel_requested`.
        let mut dispatched_at_len: HashMap<(QuarkId, Lane), usize> = HashMap::new();
        // Which assignment each in-flight turn was dispatched for, and the task text
        // it was carrying — so the CANCEL_DEADLINE fallback's `ground` call (Task 9,
        // Defect 2) can stamp `answers` on the abandoned turn's OWN assignment, not
        // leave it unstamped. An unstamped Ground falls into `has_answered`'s
        // "legacy: it spoke after the message" arm, which reads as answering EVERY
        // earlier unaddressed message — including the very message that caused the
        // interrupt, silently swallowing it. The task text rides along so an
        // abandoned turn's work can be handed to `interrupted_task` below instead of
        // dropped (Task 3 of `.hadron/docs/plans/2026-08-02-fonts-tools-
        // orchestrator.md`). Kept in lockstep with `in_flight`, same as
        // `cancel_requested`.
        let mut dispatched_assignment: HashMap<(QuarkId, Lane), Option<(ulid::Ulid, String)>> =
            HashMap::new();
        // A task abandoned mid-turn by a graceful interrupt — keyed by quark only
        // (not lane: the resumed dispatch may land on a different lane than the one
        // that was interrupted, and the point is "this quark still owes this work",
        // not "this lane does"). Populated at BOTH places a turn can end on an
        // interrupt: the ordinary path (the transport honours `session/cancel`, the
        // turn resolves normally with `outcome.cancelled == true`, handled in the
        // `join_next` arm below) and the CANCEL_DEADLINE fallback (the transport
        // never resolves the request at all, so the turn is aborted destructively).
        // Either way `finish_turn`/the fallback's own `ground` call terminates the
        // assignment unconditionally, so without this map the abandoned task is gone
        // the instant it grounds. Consumed (removed) only once a real dispatch
        // actually merges it into that quark's next `Driver::task` via
        // `prompt::frame_interrupted_resumption` — peeked, not removed, on every pass
        // a merge is attempted, so a target blocked this pass by an unrelated gate
        // (exclusivity, energy, disabled) does not lose the interrupted task before
        // it ever reaches a real dispatch.
        let mut interrupted_task: HashMap<QuarkId, String> = HashMap::new();
        // The assignment rides along with the turn so that `finish_turn` can stamp
        // `answers` on what the turn emits. Without it, "has this quark answered the
        // human?" degenerates into "has it said anything since?", which silently eats
        // any message the human sends while the quark is already working.
        let mut turns: JoinSet<(
            QuarkId,
            Lane,
            Option<TurnTree>,
            Option<ulid::Ulid>,
            anyhow::Result<TurnOutcome>,
        )> = JoinSet::new();
        // The first turn error wins; siblings still run to completion (and still get
        // their terminal status) so a single failure can't strand the rest as
        // forever-Excited in the field.
        let mut first_err: Option<anyhow::Error> = None;
        let mut backstop = false;

        loop {
            let mut spawned_any = false;

            // Stop *starting* work once we're aborting or out of budget — but keep
            // looping, so already-running turns are drained rather than dropped.
            if first_err.is_none() && !backstop {
                let events = read_events(&self.field_path)?;

                // The human's force-restart, serviced on the same re-read the poll
                // tick loops back to — so a *solo* wedged quark (nothing else pending,
                // `join_next` blocked forever on its hung turn) is still rescued: the
                // FIELD_POLL arm fires, we loop, re-read, and abort it here.
                let rebooted = self
                    .service_reboots(&events, &mut in_flight, &mut abort_handles)
                    .await?;
                // A force-restarted seat's turn is gone whether or not a cancel was
                // pending for it — drop the bookkeeping so it cannot outlive the
                // `in_flight` entry it described.
                for target in &rebooted {
                    cancel_requested.remove(&(target.clone(), Lane::Work));
                    cancel_requested.remove(&(target.clone(), Lane::Chat));
                    dispatched_at_len.remove(&(target.clone(), Lane::Work));
                    dispatched_at_len.remove(&(target.clone(), Lane::Chat));
                    dispatched_assignment.remove(&(target.clone(), Lane::Work));
                    dispatched_assignment.remove(&(target.clone(), Lane::Chat));
                    // A force-restart is a deliberate human reset of this quark; an
                    // interrupted task queued up before it must not silently resurface
                    // on some unrelated later turn.
                    interrupted_task.remove(target);
                }

                for (target, fallback_task) in self.pending_targets(&events) {
                    // Resolved once, up front, so the in_flight check below and the
                    // worktree/branch logic further down (which used to call
                    // `driver_for` a second time) always agree on the same driver.
                    let mut driver =
                        self.driver_for(&events, &target, fallback_task.as_ref().map(|t| t.task.as_str()));
                    // A message arrived mid-turn ADDS work, it does not replace it
                    // (Task 3 of `.hadron/docs/plans/2026-08-02-fonts-tools-
                    // orchestrator.md`): if this quark still owes an interrupted task,
                    // fold it into whatever is driving this dispatch instead of
                    // letting the grounded assignment (and the message that
                    // interrupted it) read as the *only* task. Peeked, not removed —
                    // see `interrupted_task`'s own doc comment for why the actual
                    // consume happens only once a dispatch is committed to, below.
                    if let (Some(d), Some(old_task)) =
                        (driver.as_mut(), interrupted_task.get(&target))
                    {
                        d.task = crate::adapter::prompt::frame_interrupted_resumption(old_task, &d.task);
                    }
                    let lane = self.lane_for(&events, &target, driver.as_ref());

                    // One turn per (quark, lane) at a time. A quark that becomes
                    // pending again while its lane is running is picked up on a later
                    // pass (its reply, or the event that re-addressed it, is still in
                    // the field).
                    //
                    // `rebooted` guards the just-force-restarted quarks: their `Ground`
                    // was appended after this `events` snapshot, so on THIS snapshot the
                    // answered message still reads as pending — dispatching now would
                    // re-excite the very turn we just aborted. Next read sees the Ground.
                    let key = (target.clone(), lane);
                    if in_flight.contains(&key) || rebooted.contains(&target) {
                        // A newly-pending message for a seat that is already working:
                        // interrupt it instead of leaving the message queued behind a
                        // turn that may run for a long time (Task 4 of
                        // `.hadron/docs/plans/2026-07-31-responsive-orchestrator.md`).
                        // Only reachable when `in_flight` itself holds `key` — a
                        // seat caught by the `rebooted` half of the condition was
                        // already cleared from `in_flight` by `service_reboots` on
                        // THIS pass, so there is nothing here left to cancel.
                        if in_flight.contains(&key) {
                            if let Some(requested_at) = cancel_requested.get(&key).copied() {
                                // Already asked once — bound the wait. A transport
                                // that ignores `session/cancel` (or has none) must
                                // not wedge this seat for the message's sake; fall
                                // through to the same destructive abort-and-reset
                                // `service_reboots` already uses for a human force-
                                // restart, so the still-unanswered message gets a
                                // fresh turn on the next pass instead of waiting out
                                // `TURN_DEADLINE`.
                                if requested_at.elapsed() >= self.cancel_deadline {
                                    let abandoned = dispatched_assignment.get(&key).cloned().flatten();
                                    let abandoned_assignment = abandoned.as_ref().map(|(a, _)| *a);
                                    Self::abort_in_flight(&key, &mut in_flight, &mut abort_handles);
                                    cancel_requested.remove(&key);
                                    dispatched_at_len.remove(&key);
                                    dispatched_assignment.remove(&key);
                                    // The abandoned task is not gone — it is handed to
                                    // the next dispatch for this quark, which merges it
                                    // with whatever interrupted it (Task 3 of
                                    // `.hadron/docs/plans/2026-08-02-fonts-tools-
                                    // orchestrator.md`): "adds work, does not replace
                                    // it". A blank task (no task-bearing driver at
                                    // dispatch time) has nothing worth resuming.
                                    if let Some((_, task)) = abandoned {
                                        if !task.trim().is_empty() {
                                            interrupted_task.insert(target.clone(), task);
                                        }
                                    }
                                    // Grounds the abandoned assignment BEFORE
                                    // taking the quark's lock below — Defect 2 of
                                    // Task 9 (`.hadron/docs/plans/2026-07-31-
                                    // responsive-orchestrator.md`): without this,
                                    // `next_pending` kept finding the old
                                    // dispatch record unanswered and re-excited
                                    // the quark onto the STALE task instead of
                                    // the message that interrupted it. Stamped
                                    // with the abandoned assignment specifically
                                    // (not left unanswered/None) — an unstamped
                                    // Ground reads as answering EVERY earlier
                                    // message via `has_answered`'s legacy
                                    // fallback, which silently swallowed the very
                                    // message that caused the interrupt.
                                    self.ground(&target, abandoned_assignment).await?;
                                    // `quark.lock().await` right after an abort,
                                    // exactly like `service_reboots` already
                                    // does and documents: `abort()` only marks
                                    // the task, it does not synchronously drop
                                    // its `MutexGuard`, but this task's own
                                    // `.await` here cooperatively yields until
                                    // the aborted task is next polled and drops
                                    // it — bounded by the runtime's own
                                    // scheduling, not a deadlock (Task 9 Step 4;
                                    // confirmed, not just assumed, by this
                                    // task's own passing test).
                                    let quark =
                                        self.quarks.get(&target).and_then(|lanes| lanes.get(lane)).cloned();
                                    if let Some(quark) = quark {
                                        quark.lock().await.reset_session();
                                    }
                                }
                            } else {
                                // `pending_targets` resurfaces THIS turn's own still-
                                // open dispatch record every pass it runs — that is
                                // exactly what the plain `in_flight` skip has always
                                // absorbed as a no-op, and is not a reason to cancel
                                // anything. Only a genuinely new `Message` appended
                                // since dispatch (`message_arrived_since`) is.
                                let since = dispatched_at_len.get(&key).copied().unwrap_or(0);
                                let slot = self.cancel_slots.get(&key).cloned();
                                if let Some(slot) = slot {
                                    if self.message_arrived_since(&events, &target, since) {
                                        // Commit whatever this turn has written but not
                                        // yet committed BEFORE asking it to stop (Task 5
                                        // of `.hadron/docs/plans/2026-07-31-responsive-
                                        // orchestrator.md`): a cancel that never resolves
                                        // and falls through to the deadline abort above
                                        // never reaches `finish_turn`'s own commit, so an
                                        // interrupted turn with dirty, unversioned work
                                        // would otherwise strand its branch behind
                                        // `BlockReason::DirtyWorktree` forever — nothing
                                        // else in the dispatch loop commits on a quark's
                                        // behalf. Best-effort: a commit failure here must
                                        // not stop the cancel itself from being requested.
                                        if let Some(root) = self.repo_root.clone() {
                                            let wt_dir =
                                                crate::worktree::trees_dir(&root).join(target.as_str());
                                            if let Some(branch) =
                                                crate::worktree::current_branch(&wt_dir)
                                            {
                                                let wt = crate::worktree::Worktree {
                                                    quark: target.clone(),
                                                    path: wt_dir,
                                                    branch,
                                                };
                                                if crate::worktree::is_dirty(&wt.path).unwrap_or(false) {
                                                    let msg = format!(
                                                        "{}: WIP snapshot before graceful interrupt",
                                                        target.as_str()
                                                    );
                                                    if let Err(e) = crate::worktree::commit_turn(&wt, &msg) {
                                                        term::warn(
                                                            Source::Gluon,
                                                            &format!(
                                                                "{} failed to snapshot its dirty worktree \
                                                                 before interrupt: {e:#}",
                                                                target.as_str()
                                                            ),
                                                        );
                                                    }
                                                }
                                            }
                                        }
                                        if slot.request_cancel() {
                                            cancel_requested.insert(key, std::time::Instant::now());
                                        }
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // `effective_mode`, not `resolve_mode`: a worker clamped to
                    // `Auto` under No-Human-Mode must not get the Bypass seat's
                    // free pass on the exchange backstop either.
                    let turn_mode =
                        hadron_gatekeeper::effective_mode(&events, &target, self.no_human, self.is_orchestrator(&target));
                    if turn_mode != Mode::Bypass && exchanges >= self.max_exchanges {
                        backstop = true;
                        break;
                    }

                    // Switched off by the human — the ONE refusal that gets no chat
                    // message. Every other reroute tells the human something they did
                    // not already know; this one restates a switch they flipped
                    // themselves, and it fires once per disabled seat on every `@team`
                    // broadcast, which is pure noise (Jake's ask).
                    //
                    // It is NOT a silent drop, which is the failure mode this codebase
                    // keeps rediscovering: `park_blocked` still writes `Status{Blocked}`,
                    // so the field records the refusal and the roster shows the seat
                    // blocked rather than forever-Excited — and the daemon log still
                    // names it, exactly like the "seated but DISABLED" line at startup.
                    if !self.is_enabled(&target) {
                        term::warn(
                            Source::Gluon,
                            &format!("{} is disabled — skipping the turn addressed to it", target.as_str()),
                        );
                        self.park_blocked(&target).await?;
                        continue;
                    }

                    if let Some(card) = self.roster.iter().find(|c| c.id == target) {
                        // The eligibility text is `addressing`, NOT `task`: the two
                        // differ the moment the human names a seat in one message and
                        // types the ask in the next, and an exclusive seat must be
                        // judged on the words that named it.
                        if card.exclusive
                            && !self.exclusive_task_names_target(
                                &events,
                                &target,
                                fallback_task.as_ref().map(|t| t.addressing.as_str()),
                            )
                        {
                            let msg = format!(
                                "@{} is exclusive to role(s) [{}] and this task did not address it by role or @id; skipping.",
                                target.as_str(),
                                card.roles.join(", ")
                            );
                            self.reroute_blocked(&target, &msg).await?;
                            continue;
                        }

                        let human_task = fallback_task.as_ref().map(|t| t.task.as_str());
                        let task_text = self.driver_for(&events, &target, human_task)
                            .map(|d| d.task)
                            .unwrap_or_else(|| human_task.unwrap_or_default().to_string());
                        let base = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
                        let workspace_root = workspace_root_of(&self.field_path, &base);
                        let repo_skills_dir = workspace_root.join(".hadron").join("skills");
                        let skill_corpus = crate::skills::load_skills(self.global_skills_dir.as_deref(), Some(&repo_skills_dir));
                        if let Some(m) = crate::skills::select(&task_text, &skill_corpus) {
                            if card.deny_skills.iter().any(|d| d.eq_ignore_ascii_case(&m.id)) {
                                let msg = format!(
                                    "@{} locks out '{}' tasks (deny_skills); skipping.",
                                    target.as_str(),
                                    m.id
                                );
                                self.reroute_blocked(&target, &msg).await?;
                                continue;
                            }
                        }
                    }

                    // The depletion gate is opt-in. A seat is only cut off by a limit
                    // somebody set — its own `energy_limit` from Settings, or an
                    // explicit swarm-wide default. With neither, the ledger still
                    // records the spend; it just does not stop the turn.
                    if let Some(ledger) = &self.ledger {
                        let limit = self.roster.iter()
                            .find(|c| c.id == target)
                            .and_then(|c| c.energy_limit)
                            .or(self.default_energy_limit);
                        if let Some(limit) = limit {
                            if ledger.is_depleted(&target, limit)? {
                                let msg = format!("Quark {} is depleted (exceeded {} tokens).", target.as_str(), limit);
                                self.reroute_blocked(&target, &msg).await?;
                                continue;
                            }
                        }
                    }

                    // `driver` was already resolved above (for the lane check) — reused
                    // here so the branch/worktree logic below agrees with it.
                    let Some(quark) = self.quarks.get(&target).and_then(|lanes| lanes.get(lane)).cloned() else {
                        first_err = Some(anyhow::anyhow!(
                            "no such quark/lane on roster: {} ({lane:?})",
                            target.as_str()
                        ));
                        break;
                    };

                    // Worktree discipline (on iff `with_git`): the quark works in its
                    // own checkout, on its own branch, and never in the human's tree.
                    let mut tree: Option<TurnTree> = None;
                    let mut git_diff = String::new();
                    if let Some(root) = self.repo_root.clone() {
                        // No task-bearing driver ⇒ no assignment ⇒ no branch to cut.
                        // Refuse rather than commit a quark's work to an unnamed branch.
                        let Some(driver) = driver.as_ref() else {
                            self.reroute_blocked_with_severity(
                                &target,
                                &format!(
                                    "{} has no assignment to work on (no task-bearing event drives this turn); refusing to excite it.",
                                    target.as_str()
                                ),
                                hadron_lattice::Severity::Error,
                            )
                            .await?;
                            continue;
                        };

                        // Gate a superseded assignment branch: if this quark's worktree is sitting
                        // on a branch from a PREVIOUS assignment that has unlanded commits, gate and
                        // land that previous branch before cutting the new assignment branch.
                        let wt_dir = crate::worktree::trees_dir(&root).join(target.as_str());
                        if wt_dir.exists() {
                            if let Some(old_branch) = crate::worktree::current_branch(&wt_dir) {
                                let new_branch = crate::worktree::branch_name(&target, &driver.assignment.to_string());
                                if old_branch != new_branch {
                                    let base = crate::worktree::default_branch(&root);
                                    let old_wt = crate::worktree::Worktree {
                                        quark: target.clone(),
                                        path: wt_dir.clone(),
                                        branch: old_branch.clone(),
                                    };
                                    if crate::worktree::commits_ahead(&old_wt, &base).unwrap_or(0) > 0 {
                                        let old_assignment = crate::worktree::parse_assignment_from_branch(&old_branch)
                                            .unwrap_or(driver.assignment);
                                        let old_turn_tree = TurnTree {
                                            head_before: crate::worktree::head(&wt_dir),
                                            wt: old_wt,
                                            base,
                                            assignment: old_assignment,
                                        };
                                        if self.merge.is_some() {
                                            // Say so BEFORE the gate runs. This is the
                                            // first thing on the dispatch path that can
                                            // take minutes (it rebases and runs the target
                                            // project's whole test suite), and it sits
                                            // ahead of `ensure`, the snapshot and
                                            // `Status{Excited}` — so without this line a
                                            // slow or hung gate leaves the field completely
                                            // empty and the human has nothing to look at
                                            // while the whole daemon waits on it.
                                            // `Actor::Gluon`, `to: None`, no leading `@`:
                                            // it prints and wakes nobody.
                                            self.append(Event::new(
                                                Actor::Gluon,
                                                None,
                                                Kind::Message {
                                                    body: format!(
                                                        "gating `{old_branch}` (a previous assignment of `{}`) \
                                                         before its next turn — this can take minutes.",
                                                        target.as_str(),
                                                    ),
                                                },
                                            ))
                                            .await?;
                                            if self.merge_gate(&target, &old_turn_tree).await? {
                                                // The gate PARKED the quark, and parking may
                                                // have written new pending work to the field —
                                                // a merge-gate hand-back is a `Message`
                                                // addressed right back to this quark. Unlike
                                                // the gate's other call site (in `finish_turn`,
                                                // after a join, where the loop goes around
                                                // anyway), this one runs while nothing else is
                                                // in flight, so the quiesce check below would
                                                // fire on the SAME pass and return with that
                                                // hand-back unserved — the repair turn silently
                                                // deferred to whenever the daemon next happened
                                                // to wake. Force one more pass to re-read
                                                // instead. Costs a single extra read when the
                                                // park created nothing (the parked quark's own
                                                // terminal status suppresses re-selection, so
                                                // this cannot spin).
                                                spawned_any = true;
                                                continue;
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let live_dir = hadron_lattice::live::live_dir(&self.field_path);
                        let _ = hadron_lattice::live::publish(
                            &live_dir,
                            &hadron_lattice::Activity::new(
                                QuarkId::new("gluon"),
                                hadron_lattice::live::Doing::Working,
                                &format!("provisioning worktree for @{}", target.as_str()),
                            ),
                        );

                        let wt = match crate::worktree::ensure(
                            &root,
                            &target,
                            &driver.assignment.to_string(),
                        ) {
                            Ok(wt) => wt,
                            Err(e) => {
                                let _ = hadron_lattice::live::clear(&live_dir, &QuarkId::new("gluon"));
                                self.reroute_blocked_with_severity(
                                    &target,
                                    &format!(
                                        "refusing to excite {}: its worktree is not usable — {e:#}",
                                        target.as_str()
                                    ),
                                    hadron_lattice::Severity::Error,
                                )
                                .await?;
                                continue;
                            }
                        };

                        // The snapshot is the pre-turn escape hatch (undo). It now points
                        // at the QUARK'S tree, so "before <quark>" means what it says.
                        let snap = crate::snapshot::create(
                            &wt.path,
                            &format!("before {}", target.as_str()),
                        )?;
                        let _ = hadron_lattice::live::clear(&live_dir, &QuarkId::new("gluon"));
                        self.append(Event::new(
                            Actor::Gluon,
                            None,
                            Kind::Snapshot { git: snap.commit.clone(), label: snap.label.clone() },
                        ))
                        .await?;

                        // Attribution comes from the BRANCH, not the working diff: once a
                        // turn ends on a commit, `git diff HEAD` is empty by construction.
                        // `<base>...HEAD` is "everything you have done on this assignment",
                        // and under concurrency it cannot show a sibling's edits.
                        let base = crate::worktree::default_branch(&root);
                        git_diff = crate::worktree::branch_diff(&wt, &base)?;
                        tree = Some(TurnTree {
                            head_before: crate::worktree::head(&wt.path),
                            wt,
                            base,
                            assignment: driver.assignment,
                        });
                    }

                    let projection = self.projection_for(
                        &events,
                        &target,
                        driver.as_ref(),
                        git_diff,
                        tree.as_ref().map(|t| t.wt.path.clone()),
                    );

                    // The DISPATCH RECORD. Who was asked to do what was, until now, only
                    // ever re-derived from message text on each pass
                    // (`unaddressed_message_targets`) and never written down, so a brief
                    // and its result were two unrelated `Message`s with nothing linking
                    // them. `Kind::Assign` already existed and was constructed nowhere.
                    //
                    // Written by `Actor::Gluon` with `answers` = the assignment, so
                    // `continued_assignment` reads it as a CONTINUATION: `driver_for`
                    // finds it as the newest task-bearing event addressed to the quark,
                    // and without that stamp the next turn would take the record's own
                    // ULID as a new assignment and cut a fresh branch over the live one.
                    //
                    // One per assignment, never one per pass: an `Assign` is itself an
                    // `is_turn_request`, so a duplicate would be a dispatch record that
                    // re-dispatches. Appended after the projection is built, so the quark
                    // never reads its own record back as a second instruction.
                    if let Some(driver) = driver.as_ref() {
                        let already_recorded = events.iter().any(|e| {
                            e.to.as_ref() == Some(&target)
                                && matches!(e.kind, Kind::Assign { .. })
                                && e.answers == Some(driver.assignment)
                        });
                        if !already_recorded {
                            self.append(
                                Event::new(
                                    Actor::Gluon,
                                    Some(target.clone()),
                                    Kind::Assign {
                                        task: driver.task.clone(),
                                        invariants: driver.invariants.clone(),
                                    },
                                )
                                .with_answers(driver.assignment),
                            )
                            .await?;
                        }
                    }

                    // Announce the excitation *before* the turn runs, so the chamber can
                    // show the quark working while it works. The adapter only returns at
                    // the end of a turn, so without this the field is silent for the whole
                    // duration and the quark reads as ignoring the human. Appended after
                    // the projection is built, so the quark never sees its own status.
                    // It doubles as the in-flight marker in the field itself: `next_pending`
                    // and `human_message_targets` both count it as the quark having
                    // "authored since", so a running quark is never re-selected.
                    self.append(Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::Status { state: QuarkState::Excited },
                    ))
                    .await?;

                    let turn_id = target.clone();
                    let turn_tree = tree.clone();
                    let assignment = driver.as_ref().map(|d| d.assignment);
                    let deadline = self.turn_deadline;
                    let live_dir = hadron_lattice::live::live_dir(&self.field_path);
                    let abort = turns.spawn(async move {
                        let mut quark = quark.lock().await;
                        // THE WATCHDOG. A turn that never resolves — its CLI process
                        // died, or orphaned its stdout pipe to a grandchild so the
                        // adapter waits forever on an EOF that will never come — would
                        // otherwise keep this quark in `in_flight` for good: no terminal
                        // status, no quiesce, no re-dispatch, the quark simply gone.
                        // On expiry we DROP the turn future (which drops the adapter's
                        // `Child`, killing a still-live process — see `ProcessRunner`'s
                        // `kill_on_drop`) and return an error, which lands in the
                        // existing failed-turn arm below: `Status{Error}`, out of
                        // `in_flight`, excitable again by the next message.
                        //
                        // The lock is acquired OUTSIDE the deadline on purpose: it
                        // measures the turn, not the wait for a turn slot. And it
                        // measures SILENCE, not elapsed time — see `until_silent`.
                        let outcome = tokio::select! {
                            biased;
                            outcome = quark.excite(projection) => outcome,
                            silent_for = until_silent(&live_dir, &turn_id, deadline) => Err(
                                anyhow::anyhow!(
                                    "turn hit the silence deadline with no terminal status: {} \
                                     produced no sign of life for {}s (process gone, or hung with \
                                     no outcome); the engine is ending the turn on its behalf. A \
                                     turn that is still working publishes activity and is never \
                                     reaped — if this fired on healthy work, that quark's \
                                     transport publishes nothing (CLI seats do not).",
                                    turn_id.as_str(),
                                    silent_for.as_secs().max(deadline.as_secs()),
                                )
                            ),
                        };
                        (turn_id, lane, turn_tree, assignment, outcome)
                    });
                    dispatched_at_len.insert((target.clone(), lane), events.len());
                    dispatched_assignment.insert(
                        (target.clone(), lane),
                        driver.as_ref().map(|d| (d.assignment, d.task.clone())),
                    );
                    abort_handles.insert((target.clone(), lane), abort);
                    // Consumed here, not at the merge point above: this is the first
                    // moment a dispatch for `target` is actually committed to (the
                    // turn is spawned, `in_flight` is about to gain the key) rather
                    // than merely attempted — a target that fell through the merge
                    // but then hit an unrelated gate (exclusivity, energy, disabled)
                    // this same pass must keep its interrupted task for the next one.
                    interrupted_task.remove(&target);
                    in_flight.insert((target, lane));
                    exchanges += 1;
                    spawned_any = true;
                }

                // No-Human-Mode auto-scheduler (spec §2 D). Nothing else is pending
                // and nothing is in flight — the swarm is *about* to quiesce (the
                // exact condition the break below checks). Before conceding that,
                // check whether it is only quiesced because a worker is parked
                // waiting on the ORCHESTRATOR (not a human) to adjudicate: if so,
                // put the pending request in front of the orchestrator instead of
                // stopping. `orchestrator_adjudication_message` is idempotent (at
                // most one ask per still-pending request), so a real orchestrator
                // that has no grant tool wired up yet — or a fake/mock quark whose
                // reply is just a message — fails CLOSED: the ask goes out once,
                // nothing resumes the worker, and the *next* pass around this same
                // check sees the ask already made and lets the loop quiesce for a
                // human, exactly as if the toggle were off. It never spins.
                if turns.is_empty() && !spawned_any && self.no_human {
                    if let Some(msg) = self.orchestrator_adjudication_message(&events) {
                        self.append(msg).await?;
                        spawned_any = true; // re-loop; the next pass dispatches it
                    }
                }
            }

            // Quiesce is the conjunction: nothing new to start AND nothing running.
            if turns.is_empty() && !spawned_any {
                break;
            }

            // Something is running. Wait for the next turn to land — but do NOT wait
            // only on that: a message can arrive in the field *while* a turn grinds,
            // addressed to a quark that is free. Blocking solely on `join_next` would
            // queue it behind the running turn, so handing one quark a long task would
            // freeze the conversation with every other quark. So we race the join
            // against a poll tick, and on a tick we loop back to re-read the field and
            // dispatch anything newly pending.
            let joined = tokio::select! {
                joined = turns.join_next() => joined,
                _ = tokio::time::sleep(FIELD_POLL) => continue,
            };
            let Some(joined) = joined else {
                continue; // everything we spawned was already drained
            };

            match joined {
                Ok((target, lane, tree, assignment, Ok(outcome))) => {
                    in_flight.remove(&(target.clone(), lane));
                    abort_handles.remove(&(target.clone(), lane));
                    cancel_requested.remove(&(target.clone(), lane));
                    dispatched_at_len.remove(&(target.clone(), lane));
                    let dispatched = dispatched_assignment.remove(&(target.clone(), lane)).flatten();
                    // The graceful cancel's ordinary resolution: the transport honoured
                    // `session/cancel` and the turn returned normally with
                    // `outcome.cancelled == true` — this is the common path, NOT the
                    // CANCEL_DEADLINE fallback above, which only fires when a transport
                    // never resolves the request at all. `finish_turn` grounds this
                    // turn's assignment unconditionally (a cancelled turn is still a
                    // terminal one), so without this the interrupted task is gone the
                    // instant it grounds. Task 3 of `.hadron/docs/plans/2026-08-02-
                    // fonts-tools-orchestrator.md`: hand it to `interrupted_task`
                    // instead, the same as the fallback path does.
                    if outcome.cancelled {
                        if let Some((_, task)) = dispatched {
                            if !task.trim().is_empty() {
                                interrupted_task.insert(target.clone(), task);
                            }
                        }
                    }
                    if let Err(err) =
                        self.finish_turn(&target, outcome, tree.as_ref(), assignment).await
                    {
                        if first_err.is_none() {
                            first_err = Some(err);
                        }
                    }
                }
                Ok((target, lane, _, _, Err(err))) => {
                    // A failed turn must still leave a terminal status behind, or the
                    // quark reads as forever-working. Its siblings keep running.
                    in_flight.remove(&(target.clone(), lane));
                    abort_handles.remove(&(target.clone(), lane));
                    cancel_requested.remove(&(target.clone(), lane));
                    dispatched_at_len.remove(&(target.clone(), lane));
                    dispatched_assignment.remove(&(target.clone(), lane));
                    let err_msg = self.format_error_message(&target, &err);
                    let _ = self
                        .append(
                            Event::new(Actor::Gluon, None, Kind::Message { body: err_msg })
                                .with_severity(hadron_lattice::Severity::Error),
                        )
                        .await;
                    let grounded = self
                        .append(Event::new(
                            Actor::Quark(target.clone()),
                            None,
                            Kind::Status { state: QuarkState::Error },
                        ))
                        .await;
                    if first_err.is_none() {
                        first_err = Some(err);
                        if let Err(io_err) = grounded {
                            first_err = Some(io_err);
                        }
                    }
                }
                Err(join_err) if join_err.is_cancelled() => {
                    // A turn we aborted on purpose — the human's force-restart. Its
                    // cleanup (out of `in_flight`, terminal `Ground`, session reset)
                    // already happened in `service_reboots`; the corpse joining here is
                    // expected, not a panic. Discard it and keep the siblings running.
                    // (Distinguishing on `is_cancelled` is what keeps a targeted reboot
                    // from tripping the ground-everyone path below.)
                    continue;
                }
                Err(join_err) => {
                    // A panicking turn: we cannot tell which quark it was from the
                    // JoinError alone, so ground every quark still in flight rather than
                    // strand one Excited, and abort.
                    abort_handles.clear();
                    cancel_requested.clear();
                    dispatched_at_len.clear();
                    dispatched_assignment.clear();
                    for (target, _lane) in std::mem::take(&mut in_flight) {
                        let _ = self
                            .append(Event::new(
                                Actor::Quark(target),
                                None,
                                Kind::Status { state: QuarkState::Error },
                            ))
                            .await;
                    }
                    let orchestrator = self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator);
                    let panic_msg = match orchestrator {
                        Some(_orch) => format!(
                            "@{} A quark turn panicked: {join_err}",
                            crate::router::ORCHESTRATOR_ALIAS
                        ),
                        None => format!("A quark turn panicked: {join_err}"),
                    };
                    let _ = self
                        .append(Event::new(Actor::Gluon, None, Kind::Message { body: panic_msg }).with_severity(hadron_lattice::Severity::Error))
                        .await;
                    if first_err.is_none() {
                        first_err = Some(anyhow::anyhow!("a quark turn panicked: {join_err}"));
                    }
                }
            }
        }

        let _ = hadron_lattice::live::clear(
            &hadron_lattice::live::live_dir(&self.field_path),
            &QuarkId::new("gluon"),
        );

        if let Some(err) = first_err {
            return Err(err);
        }

        if backstop {
            self.append(Event::new(
                Actor::Gluon,
                None,
                Kind::Message {
                    body: format!(
                        "backstop reached ({} exchanges); returning control to the human.",
                        self.max_exchanges
                    ),
                },
            ).with_severity(hadron_lattice::Severity::Warning))
            .await?;
        }

        Ok(())
    }
}
