
use hadron_lattice::{
    Actor, Event, Flavor, Kind, QuarkId, QuarkState,
    TurnOutcome,
};

use crate::field::read_events;
use crate::router::{parse_addressee, parse_all_addressees};

use super::*;

impl super::Engine {
    /// Everything that happens *after* a turn returns: energy, the reply (routed by
    /// its line-leading `@mention`), the permission ask, and the terminal status.
    ///
    /// Grounding is skipped on both permission paths, exactly as before: an
    /// auto-approved quark is re-dispatched by the grant (`to == quark`, so
    /// `next_pending` re-selects it) and grounds at the end of its *next* turn, and
    /// an ask-the-human quark ends `Waiting` until a human grant resumes it.
    /// Refuse to excite `target`, loudly and without stranding it: a Gluon message
    /// explaining why, then `Status{Blocked}` for the quark. The dispatch loop then
    /// `continue`s, so the quark's *siblings* still run — the reroute property.
    ///
    /// One shape for every refusal (energy depletion, an unusable worktree, a turn
    /// with no assignment) rather than a new mechanism per reason.
    pub(super) async fn reroute_blocked(&self, target: &QuarkId, why: &str) -> anyhow::Result<()> {
        self.reroute_blocked_with_severity(target, why, hadron_lattice::Severity::Warning).await
    }

    pub(super) async fn reroute_blocked_with_severity(
        &self,
        target: &QuarkId,
        why: &str,
        severity: hadron_lattice::Severity,
    ) -> anyhow::Result<()> {
        let msg = if self.roster.iter().any(|c| c.flavor == Flavor::Orchestrator) && !why.starts_with('@') {
            format!("@{} {why}", crate::router::ORCHESTRATOR_ALIAS)
        } else {
            why.to_string()
        };
        self.append(Event::new(Actor::Gluon, None, Kind::Message { body: msg }).with_severity(severity))
            .await?;
        self.park_blocked(target).await
    }

    /// Mark `target` `Blocked` with **no** chat message — the reroute property
    /// (siblings keep running, the quark is not left forever-Excited) without the
    /// noise, for a refusal the human has already chosen and does not need told
    /// about again. The SSOT for the status half of a reroute:
    /// [`reroute_blocked_with_severity`] appends the message and then calls this,
    /// so the two can never disagree about what "blocked" writes to the field.
    pub(super) async fn park_blocked(&self, target: &QuarkId) -> anyhow::Result<()> {
        self.append(Event::new(
            Actor::Quark(target.clone()),
            None,
            Kind::Status { state: QuarkState::Blocked },
        ))
        .await?;
        Ok(())
    }

    pub(super) async fn finish_turn(
        &self,
        target: &QuarkId,
        outcome: TurnOutcome,
        tree: Option<&TurnTree>,
        // The message this turn was dispatched to answer. Stamped onto everything the
        // turn emits, so a reader can ask "is the human still waiting?" and get a fact
        // rather than an inference from what happens to come after what.
        assignment: Option<ulid::Ulid>,
    ) -> anyhow::Result<()> {
        // ONE id for everything this turn emits. The reply and the energy report are
        // separate events, and turns run in parallel — so without this a reader trying
        // to say "this reply cost X" has only ADJACENCY to join on, and adjacency is
        // wrong the first time two quarks answer at once. Stamped here, at the single
        // place a turn's events are written, so they cannot disagree.
        let turn = ulid::Ulid::new();
        // Loaded once for the whole turn-completion, not once per `parse_addressee`
        // call below — both calls resolve mentions in the SAME reply/turn, so one
        // fresh read of the preons corpus covers both.
        let preons = self.loaded_preons();
        // Kept before the reply is moved into the field: the commit message names the
        // quark, its first line, and the assignment — greppable back to the event.
        let headline = outcome
            .message
            .as_deref()
            .and_then(|m| m.lines().find(|l| !l.trim().is_empty()))
            .unwrap_or("work")
            .trim()
            .chars()
            .take(72)
            .collect::<String>();
        // Who this reply addresses (its single line-leading `@mention`), if anyone —
        // `None` means it hands back to the human. Drives the merge gate below.
        let addressee = outcome
            .message
            .as_deref()
            .and_then(|body| parse_addressee(body, &self.roster, Some(target), &preons));

        // THE ONE TOTALLER. No adapter computes this; they report components and
        // `TokenSpend::fresh` sums the comparable ones (input + output, cache
        // excluded). `None` means the provider said nothing about tokens — unknown,
        // which is not the same as zero, so we report nothing rather than a hollow 0.
        let fresh = outcome.usage.spend.fresh();

        // The event fires when there is *anything* to say — spend, context, or quota.
        // It used to fire only on `used_tokens > 0`, which meant a provider that
        // reported context but no tokens had its telemetry silently dropped.
        if fresh.unwrap_or(0) > 0 || !outcome.usage.is_empty() {
            if let Some(ledger) = &self.ledger {
                // The depletion gate reads this ledger (`is_depleted`). It is fed the
                // cache-excluded figure on purpose: the old cross-provider sum would
                // have tripped an ACP quark ~200x early on cache reads it never paid
                // for. See `the-depletion-gate-is-a-loaded-gun` in the shared nucleus index.
                if let Some(t) = fresh.filter(|t| *t > 0) {
                    ledger.record_usage(target, t)?;
                }
            }
            // `used_tokens` stays a `u32` on the Kind because the chamber matches
            // `Kind` exhaustively with no wildcard arm (see the note on `Event.usage`
            // in `event.rs`): widening the variant would break a crate this one does
            // not own. The components ride on the envelope's `usage` instead, which is
            // additive and which every existing reader already ignores safely.
            self.append(
                Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::EnergyReport { used_tokens: fresh.unwrap_or(0) },
                )
                .with_usage(outcome.usage.clone())
                .with_turn(turn)
                .answering(assignment),
            )
            .await?;
        }

        if let Some(body) = outcome.message.as_ref() {
            if self.is_orchestrator(target) {
                let body_trimmed = body.trim();
                if body_trimmed.starts_with("/approve ") || body_trimmed.starts_with("/deny ") {
                    let parts: Vec<&str> = body_trimmed.split_whitespace().collect();
                    if parts.len() >= 2 {
                        let cmd = parts[0];
                        let worker_name = parts[1].trim_start_matches('@');
                        let remember = parts.get(2).map(|s| *s == "remember").unwrap_or(false);
                        let approved = cmd == "/approve";
                        
                        if let Some(worker_id) = self.roster.iter().find(|c| c.id.as_str() == worker_name || c.display_name.as_deref() == Some(worker_name)).map(|c| c.id.clone()) {
                            let grant_ev = Event::new(
                                Actor::Quark(target.clone()),
                                Some(worker_id),
                                Kind::PermissionGrant { approved, remember },
                            );
                            self.append(grant_ev).await?;
                        }
                    }
                }
            }
        }

        if let Some(body) = outcome.message {
            // The reply enters the field whole — the engine does NOT trim it. Brevity is
            // asked for in the prompt and explained, never enforced by cutting bytes: a
            // truncated report can hide the one line the human needed (a reported-but-
            // unread critical issue is worse than a long one). The addressee is parsed
            // from the reply as written, so what routes is exactly what the field carries.
            let addressees = parse_all_addressees(&body, &self.roster, Some(target), &preons);
            let to = if addressees.len() == 1 {
                Some(addressees[0].clone())
            } else {
                None
            };
            let event = Event::new(Actor::Quark(target.clone()), to, Kind::Message { body })
                .with_turn(turn)
                .answering(assignment);
            self.append(event).await?;
        }

        // A self-declared permission ask: record it, then let the effective mode
        // decide. The mode + allow-list are folded from the field as it stands
        // *before* the req is appended (the req itself must not become its own
        // remembered rule), but re-read here rather than reused from dispatch time —
        // a concurrent turn may have moved the field on since.
        if let Some(ask) = outcome.permission {
            let events = read_events(&self.field_path)?;
            let risk = ask.risk;
            let op = ask.description.clone();
            self.append(Event::new(
                Actor::Quark(target.clone()),
                None,
                Kind::PermissionReq { risk, description: ask.description },
            ))
            .await?;
            let mode =
                hadron_gatekeeper::effective_mode(&events, target, self.no_human, self.is_orchestrator(target));
            let global = hadron_gatekeeper::global_mode(&events);
            let mut rules = hadron_gatekeeper::allow_rules(&events);
            // No deny-rule SOURCE exists in the field yet — no event teaches a
            // deny the way a remembered `PermissionGrant` teaches an allow — so
            // this starts empty (same placeholder Task 1/2 shipped with) and is
            // then folded with the seat's own `commands` config below.
            // `decide`'s deny-absolute branch is exercised at the matrix level
            // (`deny_listed_op_never_reaches_orchestrator`).
            let mut deny = hadron_gatekeeper::DenyRules::new();
            // Config command rules bite ONLY under No-Human-Mode (the batch
            // invariant). The WHOLE fold is gated on `no_human`: `decide`'s
            // toggle-OFF table still consults `allow` for field-taught remembered
            // grants, so folding config `allowed` in unconditionally would
            // auto-approve a config-allowed op with the toggle OFF — and
            // asymmetrically, since `deny` is inert off. Gating both halves keeps
            // config rules genuinely inert until §2 is activated. (Review C1.)
            //
            // SECURITY (review I2/M1): these rules are roster-sourced — a quark
            // unseated mid-adjudication yields `commands_for → None`, so its deny
            // is not folded (field-taught rules have no such dependency). And the
            // patterns match the quark's self-declared `op` string, so a
            // mis-declared op silently never fires its deny. Both are inherent to
            // a config allow/deny list layered on a self-declared-op gate.
            if self.no_human {
                if let Some(cmds) = self.commands_for(target) {
                    for p in &cmds.allowed {
                        rules.insert((target.clone(), p.clone()));
                    }
                    for p in &cmds.not_allowed {
                        deny.insert((target.clone(), p.clone()));
                    }
                }
            }
            let asker_is_orchestrator = self.orchestrator_id().as_ref() == Some(target);
            let decision = hadron_gatekeeper::decide(mode, global, self.no_human, risk, &op, target, &rules, &deny);
            let decision = if asker_is_orchestrator && decision == hadron_gatekeeper::Decision::AskOrchestrator {
                hadron_gatekeeper::Decision::AskHuman
            } else {
                decision
            };
            match decision {
                hadron_gatekeeper::Decision::AutoApprove => {
                    // Pre-authorized by the mode: the gluon grants on the
                    // orchestrator's / human's standing authority.
                    self.append(Event::new(
                        Actor::Gluon,
                        Some(target.clone()),
                        Kind::PermissionGrant { approved: true, remember: false },
                    ))
                    .await?;
                    return Ok(());
                }
                hadron_gatekeeper::Decision::AskHuman => {
                    // Pause: mark the quark waiting. The dispatch loop no longer has
                    // any pending work for it, so once every *other* in-flight turn
                    // finishes the engine quiesces and the human is asked.
                    //
                    // NOTE: deliberately NO commit here. A quark pausing mid-assignment
                    // for permission is not done — its uncommitted work must still be
                    // sitting in its worktree when the grant resumes it. That is exactly
                    // why `worktree::ensure` is idempotent for the same assignment.
                    self.append(
                        Event::new(
                            Actor::Quark(target.clone()),
                            None,
                            Kind::Status { state: QuarkState::Waiting },
                        )
                        .with_turn(turn)
                        .answering(assignment),
                    )
                    .await?;
                    return Ok(());
                }
                hadron_gatekeeper::Decision::AskOrchestrator => {
                    // No-Human-Mode (toggle on, global Bypass): the SAME park as
                    // AskHuman — reused verbatim, on purpose. What differs is who
                    // gets asked: `run_until_quiesce`'s auto-scheduler recomputes
                    // `decide` for the pending `PermissionReq` once the swarm
                    // quiesces, and when it still resolves to `AskOrchestrator`
                    // it dispatches an adjudication turn to the orchestrator seat
                    // instead of leaving the ask for a human. The RESUME path is
                    // identical either way: any `PermissionGrant` addressed to
                    // this quark (human- or orchestrator-authored) re-excites it
                    // via `next_pending`.
                    self.append(
                        Event::new(
                            Actor::Quark(target.clone()),
                            None,
                            Kind::Status { state: QuarkState::Waiting },
                        )
                        .with_turn(turn)
                        .answering(assignment),
                    )
                    .await?;
                    return Ok(());
                }
            }
        }

        // The turn ends on a COMMIT in the quark's branch. Only here, on the Ground
        // path: both permission branches above return early, uncommitted, on purpose.
        if let Some(t) = tree {
            let message = format!("{}: {headline} [{}]", target.as_str(), t.assignment);
            crate::worktree::commit_turn(&t.wt, &message)?;

            // Compare HEAD; don't assume WE committed. A Bypass-mode CLI may have run
            // `git commit` itself, leaving a clean tree behind an advanced branch —
            // `commit_turn` returns `Ok(None)` there, but the work still happened.
            let head_now = crate::worktree::head(&t.wt.path);
            if head_now.is_some() && head_now != t.head_before {
                let git = head_now.unwrap_or_default();
                let paths = crate::worktree::changed_paths(&t.wt, &t.base)?;
                // `Kind::Edit` has existed in the lattice since day one and was emitted
                // by nobody. This is what it was for: a quark's work, attributed.
                self.append(
                    Event::new(
                        Actor::Quark(target.clone()),
                        None,
                        Kind::Edit { paths, git, summary: headline.clone() },
                    )
                    .with_turn(turn)
                    .answering(assignment),
                )
                .await?;
            }

            // The assignment is COMPLETE — so its branch is gated for landing — when the
            // reply hands back to the human (no `@mention`) OR a worker reports up to the
            // orchestrator. A worker handing to a peer, or the orchestrator dispatching
            // down to a worker, is a mid-chain hand-off: the branch stays open, no gate.
            // Keying on this rather than mention-absence alone is Task 6: before it, a
            // worker's `@orchestrator`-addressed completion never gated, so its branch
            // stranded every turn while the swarm believed it had landed.
            if self.assignment_complete(target, addressee.as_ref())
                && self.merge.is_some()
                && self.merge_gate(target, t).await?
            {
                return Ok(()); // the gate parked the quark (Waiting / Blocked)
            }
        }

        self.append(
            Event::new(
                Actor::Quark(target.clone()),
                None,
                Kind::Status { state: QuarkState::Ground },
            )
            .with_turn(turn)
            .answering(assignment),
        )
        .await?;
        Ok(())
    }

    /// The marker prefix on the auto-scheduler's injected orchestrator turn.
    /// Load-bearing for idempotency (see [`Engine::orchestrator_adjudication_message`]):
    /// its presence after a request is what stops the request from being
    /// re-asked every quiesce pass.
    const NO_HUMAN_ADJUDICATION_MARKER: &'static str = "[no-human-mode]";

    /// No-Human-Mode's auto-scheduler (spec §2 D step 2). If the field's latest
    /// `PermissionReq` is still unanswered AND still resolves to
    /// `Decision::AskOrchestrator` under the field as it stands **right now**
    /// (mode/global/allow may have moved since the worker asked — re-decided,
    /// not cached), build the `Message` that puts it in front of the
    /// orchestrator, injecting the request's own description into the task.
    /// `None` when there is nothing pending, the pending request resolves to
    /// something other than `AskOrchestrator` (e.g. it now needs a human, or
    /// the toggle state changed), there is no orchestrator seat, the asker IS
    /// the orchestrator (nothing to escalate to), or the orchestrator has
    /// already been asked about this exact request.
    ///
    /// Pure — reads `events`, returns an `Event` to append; does not append it
    /// itself. `run_until_quiesce` is the one caller and the one that appends.
    pub(super) fn orchestrator_adjudication_message(&self, events: &[Event]) -> Option<Event> {
        let req_idx = events.iter().rposition(|e| matches!(e.kind, Kind::PermissionReq { .. }))?;
        // Already answered (by anyone, any way) → nothing to schedule.
        if events[req_idx + 1..].iter().any(|e| matches!(e.kind, Kind::PermissionGrant { .. })) {
            return None;
        }
        let Kind::PermissionReq { risk, description } = &events[req_idx].kind else {
            unreachable!("rposition matched PermissionReq")
        };
        // Mirrors `hadron_gatekeeper::pending_permission`'s addressee recovery:
        // a request not authored by a quark has no one to resume, so no one to
        // escalate on behalf of either.
        let Actor::Quark(quark) = &events[req_idx].from else { return None };
        let orch = self.orchestrator_id()?;
        if *quark == orch {
            return None; // the orchestrator cannot adjudicate its own request
        }
        // Idempotent: at most one adjudication ask per still-pending request.
        // Without this, a real orchestrator with no grant tool yet (or a fake
        // quark that just replies) would be re-asked every quiesce pass forever.
        let already_asked = events[req_idx + 1..].iter().any(|e| {
            e.to.as_ref() == Some(&orch)
                && matches!(&e.kind, Kind::Message { body } if body.starts_with(Self::NO_HUMAN_ADJUDICATION_MARKER))
        });
        if already_asked {
            return None;
        }

        // `is_orchestrator = false` here isn't a hardcoded assumption: we already
        // returned `None` above if `*quark == orch`, so the asker is provably
        // never the orchestrator seat by this point.
        let mode = hadron_gatekeeper::effective_mode(events, quark, self.no_human, self.is_orchestrator(quark));
        let global = hadron_gatekeeper::global_mode(events);
        let mut rules = hadron_gatekeeper::allow_rules(events);
        let mut deny = hadron_gatekeeper::DenyRules::new(); // see the permission-ask gate's comment
        // Config rules gated on No-Human-Mode — same invariant as the permission-ask
        // gate (review C1). This path only runs under No-Human-Mode anyway, so the
        // guard is belt-and-suspenders, but it keeps the fold identical across sites.
        if self.no_human {
            if let Some(cmds) = self.commands_for(quark) {
                for p in &cmds.allowed {
                    rules.insert((quark.clone(), p.clone()));
                }
                for p in &cmds.not_allowed {
                    deny.insert((quark.clone(), p.clone()));
                }
            }
        }
        let decision =
            hadron_gatekeeper::decide(mode, global, self.no_human, *risk, description, quark, &rules, &deny);
        if decision != hadron_gatekeeper::Decision::AskOrchestrator {
            return None; // needs a human (or was resolved another way) — not ours to schedule
        }

        Some(Event::new(
            Actor::Gluon,
            Some(orch),
            Kind::Message {
                body: format!(
                    "{marker} @{quark} is asking for permission to run: {description} (risk: {risk:?}). \
                     No-Human-Mode is on — a human is not being asked. Review it against the allow/deny \
                     lists and reply with your decision, addressed to @{quark}.",
                    marker = Self::NO_HUMAN_ADJUDICATION_MARKER,
                    quark = quark.as_str(),
                ),
            },
        )
        // A worker is parked until this is answered — it needs attention, not a
        // neutral notice, so it carries the same amber the other "you must act"
        // reroutes do.
        .with_severity(hadron_lattice::Severity::Warning))
    }
}
