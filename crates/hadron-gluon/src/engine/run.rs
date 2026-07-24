use std::collections::{HashMap, HashSet};

use hadron_lattice::{
    Actor, Event, Flavor, Kind, Mode, QuarkId, QuarkState,
    TurnOutcome,
};
use tokio::task::{AbortHandle, JoinSet};

use crate::field::read_events;

use super::*;

impl super::Engine {
    fn format_error_message(&self, quark_id: &QuarkId, err: &anyhow::Error) -> String {
        let orchestrator = self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator);
        if let Some(orch) = orchestrator {
            if &orch.id != quark_id {
                return format!(
                    "@{} Quark `{}` turn errored: {err:#}",
                    crate::router::ORCHESTRATOR_ALIAS,
                    quark_id.as_str()
                );
            }
        }
        format!("Quark `{}` turn errored: {err:#}", quark_id.as_str())
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
        let mut in_flight: HashSet<QuarkId> = HashSet::new();
        // The abort handle for each in-flight turn, so the human's force-restart
        // ([`Kind::Reboot`]) can kill a wedged quark's turn *now* instead of waiting
        // out the 30-minute deadline. Kept in lockstep with `in_flight`: inserted at
        // spawn, removed the instant a turn joins or is rebooted.
        let mut abort_handles: HashMap<QuarkId, AbortHandle> = HashMap::new();
        // The assignment rides along with the turn so that `finish_turn` can stamp
        // `answers` on what the turn emits. Without it, "has this quark answered the
        // human?" degenerates into "has it said anything since?", which silently eats
        // any message the human sends while the quark is already working.
        let mut turns: JoinSet<(
            QuarkId,
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

                for (target, fallback_task) in self.pending_targets(&events) {
                    // One turn per quark at a time. A quark that becomes pending again
                    // while it is running is picked up on a later pass (its reply, or
                    // the event that re-addressed it, is still in the field).
                    //
                    // `rebooted` guards the just-force-restarted quarks: their `Ground`
                    // was appended after this `events` snapshot, so on THIS snapshot the
                    // answered message still reads as pending — dispatching now would
                    // re-excite the very turn we just aborted. Next read sees the Ground.
                    if in_flight.contains(&target) || rebooted.contains(&target) {
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
                        eprintln!(
                            "  {} is disabled — skipping the turn addressed to it",
                            target.as_str()
                        );
                        self.park_blocked(&target).await?;
                        continue;
                    }

                    if let Some(card) = self.roster.iter().find(|c| c.id == target) {
                        if card.exclusive
                            && !self.exclusive_task_names_target(&events, &target, fallback_task.as_deref())
                        {
                            let msg = format!(
                                "@{} is exclusive to role(s) [{}] and this task did not address it by role or @id; skipping.",
                                target.as_str(),
                                card.roles.join(", ")
                            );
                            self.reroute_blocked(&target, &msg).await?;
                            continue;
                        }

                        let task_text = self.driver_for(&events, &target, fallback_task.as_deref())
                            .map(|d| d.task)
                            .unwrap_or_else(|| fallback_task.clone().unwrap_or_default());
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

                    if let Some(ledger) = &self.ledger {
                        let limit = self.roster.iter()
                            .find(|c| c.id == target)
                            .and_then(|c| c.energy_limit)
                            .unwrap_or(self.energy_limit);
                        if ledger.is_depleted(&target, limit)? {
                            let msg = format!("Quark {} is depleted (exceeded {} tokens).", target.as_str(), limit);
                            self.reroute_blocked(&target, &msg).await?;
                            continue;
                        }
                    }

                    let Some(quark) = self.quarks.get(&target).cloned() else {
                        first_err =
                            Some(anyhow::anyhow!("no such quark on roster: {}", target.as_str()));
                        break;
                    };

                    // The assignment that drives this turn. Its ULID names the branch,
                    // and its body is the task — resolved ONCE, so both agree.
                    let driver = self.driver_for(&events, &target, fallback_task.as_deref());

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
                                        if self.merge.is_some() && self.merge_gate(&target, &old_turn_tree).await? {
                                            continue;
                                        }
                                    }
                                }
                            }
                        }

                        let wt = match crate::worktree::ensure(
                            &root,
                            &target,
                            &driver.assignment.to_string(),
                        ) {
                            Ok(wt) => wt,
                            Err(e) => {
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
                        // The lock is acquired OUTSIDE the timeout on purpose: the
                        // deadline measures the turn, not the wait for a turn slot.
                        let outcome = match tokio::time::timeout(
                            deadline,
                            quark.excite(projection),
                        )
                        .await
                        {
                            Ok(outcome) => outcome,
                            Err(_) => Err(anyhow::anyhow!(
                                "turn exceeded deadline with no terminal status: {} was excited \
                                 for {}s and its turn never returned (process gone, or hung with \
                                 no outcome); the engine is ending the turn on its behalf",
                                turn_id.as_str(),
                                deadline.as_secs(),
                            )),
                        };
                        (turn_id, turn_tree, assignment, outcome)
                    });
                    abort_handles.insert(target.clone(), abort);
                    in_flight.insert(target);
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
                Ok((target, tree, assignment, Ok(outcome))) => {
                    in_flight.remove(&target);
                    abort_handles.remove(&target);
                    if let Err(err) =
                        self.finish_turn(&target, outcome, tree.as_ref(), assignment).await
                    {
                        if first_err.is_none() {
                            first_err = Some(err);
                        }
                    }
                }
                Ok((target, _, _, Err(err))) => {
                    // A failed turn must still leave a terminal status behind, or the
                    // quark reads as forever-working. Its siblings keep running.
                    in_flight.remove(&target);
                    abort_handles.remove(&target);
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
                    for target in std::mem::take(&mut in_flight) {
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
