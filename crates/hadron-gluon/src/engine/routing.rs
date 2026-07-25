use std::collections::HashSet;
use std::path::PathBuf;

use hadron_lattice::{
    Actor, EnergyState, Event, Flavor, Kind, Projection, QuarkId,
};

use crate::router::{human_mentions, next_pending, parse_all_addressees};
use crate::skills;
use std::fs;

use super::*;
use super::nucleus::{build_invariants, bounded_window, nucleus_index_path, nucleus_notes_dir, read_nucleus_index_with_fallback, FIELD_WINDOW_BUDGET_BYTES};

impl super::Engine {
    /// Whether `id` holds the orchestrator seat — the SSOT this module uses
    /// everywhere a permission-gate call site needs to know (worker clamping,
    /// deny-absolute is unaffected by this, and the No-Human-Mode auto-scheduler
    /// below all key off the same lookup rather than re-deriving it).
    pub(super) fn is_orchestrator(&self, id: &QuarkId) -> bool {
        self.roster.iter().any(|c| &c.id == id && c.flavor == Flavor::Orchestrator)
    }

    /// The seat holding the orchestrator role, if any — reuses the exact
    /// `Flavor::Orchestrator` lookup `human_addressees` already relies on.
    pub(super) fn orchestrator_id(&self) -> Option<QuarkId> {
        self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator).map(|c| c.id.clone())
    }

    /// Whether a finished turn's reply ENDS `target`'s assignment — the signal the merge
    /// gate keys on (`engine/turn.rs`). Complete = handed back to the human (`addressee`
    /// `None`) OR a worker reporting up to the orchestrator. A worker handing to a peer,
    /// or the orchestrator dispatching down to a worker, is a mid-chain hand-off: NOT
    /// complete, so the branch stays open. Before this, the gate fired only on the
    /// no-`@mention` hand-back, so a worker's `@orchestrator`-addressed completion never
    /// landed and its branch stranded every turn
    /// (`merge-gate-fires-only-on-no-mention-handback`).
    pub(super) fn assignment_complete(&self, target: &QuarkId, addressee: Option<&QuarkId>) -> bool {
        match addressee {
            None => true,
            Some(a) => !self.is_orchestrator(target) && self.orchestrator_id().as_ref() == Some(a),
        }
    }

    /// The command allow/deny lists carried on `id`'s roster card, if seated —
    /// the SSOT `decide()`'s three call sites fold into their `AllowRules`/
    /// `DenyRules` under No-Human-Mode. Mirrors `is_orchestrator`/`orchestrator_id`'s
    /// pattern of reading straight off the roster rather than re-deriving from
    /// `team.json`.
    pub(super) fn commands_for(&self, id: &QuarkId) -> Option<&hadron_lattice::SeatCommands> {
        self.roster.iter().find(|c| &c.id == id).map(|c| &c.commands)
    }

    /// Who a human message addresses: every quark it `@mentions` (anywhere, in
    /// order — the multi-dispatch case, "@opus X and @agy Y"), or, if it mentions
    /// no one, the orchestrator (default-routing, so the human can "just type").
    /// An empty result means no one can field it (e.g. no orchestrator on the
    /// roster and no valid mention).
    pub(super) fn human_addressees(&self, body: &str) -> Vec<QuarkId> {
        let preons = self.loaded_preons();
        let mut addressees = human_mentions(body, &self.roster, &preons);
        if addressees.is_empty() {
            if let Some(orch) = self.roster.iter().find(|c| c.flavor == Flavor::Orchestrator) {
                addressees.push(orch.id.clone());
            }
        }

        // Soft preference (spec 2026-07-20 §3.2): bubble preferred role-holder to the front
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_root = workspace_root_of(&self.field_path, &base);
        let repo_skills_dir = workspace_root.join(".hadron").join("skills");
        let skill_corpus = skills::load_skills(self.global_skills_dir.as_deref(), Some(&repo_skills_dir));
        if let Some(m) = skills::select(body, &skill_corpus) {
            if let Some(role) = skills::preferred_role(&m.id) {
                if let Some(preferred) = crate::router::card_for_role(&self.roster, role) {
                    addressees.sort_by_key(|id| if id == &preferred.id { 0 } else { 1 });
                }
            }
        }

        addressees
    }

    /// Route the human's UNADDRESSED (`to == None`) messages. The chamber writes human
    /// messages with the mentions left in the body (not stripped into `to`), so one
    /// message can name several quarks, and it fans out *in parallel*: every addressee
    /// that hasn't answered yet is returned, each handed its message, and the dispatch
    /// loop excites them all at once. Addressed messages and quark hand-offs are
    /// `next_pending`'s job. An empty result means every pending message is served.
    ///
    /// **Every unserved message, not just the newest.** The old version looked only at
    /// the single latest human message (`rposition`). So a human who fired "@Claude do X"
    /// and then, before Claude was even dispatched, typed anything else — a "@Claude ?"
    /// follow-up, or a "@orchestrator ..." aside — saw the "@Claude" request silently
    /// abandoned: the newer message became "the latest" and the older one was never
    /// looked at again. Claude answered nothing while the orchestrator churned. Now each
    /// quark is dispatched for its MOST RECENT unserved mention.
    ///
    /// Walking newest-first with a per-quark `seen` set gives each quark exactly one
    /// target (its latest mention) and lets `has_answered` suppress anything already
    /// handled or in flight: "answered" means the quark authored *any* event since the
    /// message — including the `Status{Excited}` appended before a turn, and the
    /// `Status{Blocked}` a `reroute_blocked` leaves behind — so a dispatched, in-flight,
    /// or un-dispatchable quark is not re-selected. The walk stops once every seat is
    /// accounted for, because an older message can add no addressee a newer one hasn't.
    pub(super) fn unaddressed_message_targets(&self, events: &[Event]) -> Vec<(QuarkId, String)> {
        let mut out: Vec<(QuarkId, String)> = Vec::new();
        let mut seen: HashSet<QuarkId> = HashSet::new();
        let preons = self.loaded_preons();
        for idx in (0..events.len()).rev() {
            if seen.len() >= self.roster.len() {
                break; // every seat already has its most-recent status; older msgs add none
            }
            let e = &events[idx];
            let Kind::Message { body } = &e.kind else { continue };
            if e.to.is_some() {
                continue; // only unaddressed messages
            }
            let msg_id = e.id;
            let addressees = match &e.from {
                Actor::Human => self.human_addressees(body),
                Actor::Quark(sender) => {
                    parse_all_addressees(body, &self.roster, Some(sender), &preons)
                }
                Actor::Gluon => {
                    parse_all_addressees(body, &self.roster, None, &preons)
                }
            };
            for addressee in addressees {
                // A newer message already claimed this quark → this older mention is stale.
                if !seen.insert(addressee.clone()) {
                    continue;
                }
                if !Self::has_answered(&events[idx + 1..], &addressee, msg_id) {
                    out.push((addressee, body.clone()));
                }
            }
        }
        out
    }

    /// Has `addressee` answered the human message `msg_id`?
    ///
    /// The obvious reading — *"has it authored anything since?"* — is **wrong the moment
    /// the human speaks while the quark is already working.** The quark finishes the turn
    /// it was on, its reply lands after the newer message, and that reply gets counted as
    /// an answer to a message it could not possibly have seen. The newer message is then
    /// dropped, silently and permanently. Jake hit exactly this by typing twice.
    ///
    /// So an event answers a message only if it **says** it does: the engine stamps
    /// `answers` with the assignment the turn was dispatched for.
    ///
    /// The `answers.is_none()` arm is not a loophole, it is the legacy reading, and it
    /// has to stay: every event written before this field existed carries `None`, and
    /// treating those as "has not answered" would re-excite a quark for every historical
    /// message in the field the next time the daemon starts. Absent is unknown, and for
    /// unknown we keep the old, order-based answer. New events are precise.
    ///
    /// **What counts as answering is [`is_turn_completion`]** — a reply or a terminal
    /// status, NOT the `Excited` "I started" the engine writes at dispatch. That status
    /// carries no `answers` stamp, so before the shared predicate it hit the legacy arm
    /// and stranded a quark whose turn was interrupted after it went Excited. `next_pending`
    /// already used this reading; sharing it is what keeps the two from disagreeing.
    fn has_answered(after: &[Event], addressee: &QuarkId, msg_id: ulid::Ulid) -> bool {
        after.iter().any(|e| {
            crate::router::is_turn_completion(e, addressee)
                && match e.answers {
                    Some(a) => a == msg_id,
                    None => true, // legacy event: fall back to "it spoke after the message"
                }
        })
    }

    /// Everyone the field is currently waiting on, in dispatch order: the explicit
    /// addressee / hand-off first (`next_pending`), then every unserved addressee of
    /// the latest unaddressed human message. The `String` is the `fallback_task` —
    /// the human message body, carried along because it is `to: None` and so cannot
    /// be found by the `to == target` trigger-finder.
    pub(super) fn pending_targets(&self, events: &[Event]) -> Vec<(QuarkId, Option<String>)> {
        let mut targets: Vec<(QuarkId, Option<String>)> = Vec::new();
        if let Some(q) = next_pending(events) {
            targets.push((q, None));
        }
        for (q, task) in self.unaddressed_message_targets(events) {
            if !targets.iter().any(|(id, _)| id == &q) {
                targets.push((q, Some(task)));
            }
        }
        targets
    }

    /// Whether the task about to drive `target`'s turn actually names it — by one of
    /// its `roles` (a `@role` mention naming it) or its own `@id` — the eligibility
    /// test an `exclusive` card must pass before it is ever admitted.
    ///
    /// Deliberately reads the task's TEXT rather than trusting `to == target`: the
    /// event that produced `target` may have addressed it directly (a hand-off, or a
    /// `Kind::Assign` written straight to its id) with no relation between `to` and
    /// the task body at all. `fallback_task` covers the unaddressed-human-message
    /// case (the body IS the task, already in hand); anything else re-resolves the
    /// driving event the same way dispatch itself will a moment later.
    ///
    /// Uses [`crate::router::task_names_card_specifically`], NOT `human_mentions`:
    /// `human_mentions` expands `@team` to the whole roster, which let a plain
    /// `@team status` broadcast admit an exclusive card that was never named by role
    /// or id — the review-flagged gap this now closes.
    pub(super) fn exclusive_task_names_target(
        &self,
        events: &[Event],
        target: &QuarkId,
        fallback_task: Option<&str>,
    ) -> bool {
        let Some(card) = self.roster.iter().find(|c| &c.id == target) else {
            return false;
        };
        let task_text = fallback_task
            .map(|s| s.to_string())
            .or_else(|| self.driver_for(events, target, None).map(|d| d.task));
        let preons = self.loaded_preons();
        task_text.as_deref().is_some_and(|t| crate::router::task_names_card_specifically(t, card, &preons))
    }

    /// The event that drives this turn — the *assignment*. Its `Ulid` names the
    /// quark's branch, so every turn of one assignment (including a turn resumed
    /// after a permission pause) resolves the **same** ULID and lands back in the
    /// same worktree on the same branch. A resumed quark that cut a fresh branch
    /// would orphan the uncommitted work it paused with; that is the exact inverse
    /// of the intent, and this function is what prevents it.
    ///
    /// The resolution order is the one `projection_for` has always used, now with
    /// the driving event's identity kept instead of thrown away. `None` = no
    /// task-bearing driver at all.
    pub(super) fn driver_for(
        &self,
        events: &[Event],
        target: &QuarkId,
        fallback_task: Option<&str>,
    ) -> Option<Driver> {
        // 1. An unaddressed human message (single- or multi-mention, or default
        //    routing): the task is that message itself — there is no `to == target`
        //    event to recover it from.
        if let Some(task) = fallback_task {
            let preons = self.loaded_preons();
            let ev = events.iter().rev().find(|e| {
                let Kind::Message { body } = &e.kind else { return false; };
                if e.to.is_some() {
                    return false;
                }
                match &e.from {
                    Actor::Human => {
                        self.human_addressees(body).contains(target)
                    }
                    Actor::Quark(sender) => {
                        parse_all_addressees(body, &self.roster, Some(sender), &preons).contains(target)
                    }
                    Actor::Gluon => {
                        parse_all_addressees(body, &self.roster, None, &preons).contains(target)
                    }
                }
            })?;
            return Some(Driver {
                assignment: ev.id,
                task: task.to_string(),
                invariants: vec![],
            });
        }

        // 2. The most recent *task-bearing* event addressed to this quark. Skip
        //    non-task events like a PermissionGrant (also addressed to the quark, to
        //    re-trigger it) — otherwise a resumed quark would get an empty task, and
        //    (now) a branch named for the grant rather than the assignment.
        if let Some(trigger) = events.iter().rev().find(|e| {
            e.to.as_ref() == Some(target)
                && matches!(e.kind, Kind::Assign { .. } | Kind::Message { .. })
        }) {
            return Some(match &trigger.kind {
                Kind::Assign { task, invariants } => Driver {
                    assignment: trigger.id,
                    task: task.clone(),
                    invariants: invariants.clone(),
                },
                Kind::Message { body } => {
                    // A follow-up message inherits the invariants of the most recent
                    // Assign to this quark.
                    let invariants = events
                        .iter()
                        .rev()
                        .find_map(|e| match (&e.to, &e.kind) {
                            (Some(to), Kind::Assign { invariants, .. }) if to == target => {
                                Some(invariants.clone())
                            }
                            _ => None,
                        })
                        .unwrap_or_default();
                    Driver { assignment: trigger.id, task: body.clone(), invariants }
                }
                _ => unreachable!("the find matched Assign | Message"),
            });
        }

        // 3. No event is addressed `to == target` — this is a quark resuming after a
        //    permission grant whose DRIVING message is an unaddressed (`to: None`)
        //    human message that named this quark in its body (a mention, or an
        //    unmentioned message the quark orchestrates). Recover the task from that
        //    message so the resumed turn isn't handed "". Resolution matches
        //    `human_message_targets` exactly, so both agree which message drives it.
        let driving = events.iter().rev().find(|e| {
            matches!(&e.kind, Kind::Message { body } if e.from == Actor::Human && self.human_addressees(body).contains(target))
        })?;
        let Kind::Message { body } = &driving.kind else {
            return None;
        };
        Some(Driver { assignment: driving.id, task: body.clone(), invariants: vec![] })
    }

    /// Build the projection handed to `target` for this turn, from the field as read
    /// at dispatch time and the already-resolved [`Driver`] (so the projection's task
    /// and the quark's branch name cannot disagree — they come from one event).
    ///
    /// `cwd` is where the quark works: its own worktree when worktree discipline is
    /// on, else the workspace root.
    pub(super) fn projection_for(
        &self,
        events: &[Event],
        target: &QuarkId,
        driver: Option<&Driver>,
        git_diff: String,
        cwd: Option<PathBuf>,
    ) -> Projection {
        let task_desc = driver.map(|d| d.task.clone()).unwrap_or_default();
        let requested_invariants = driver.map(|d| d.invariants.clone()).unwrap_or_default();

        // Whether the task names THIS quark specifically (by `@id`, display name, or
        // role) rather than reaching it only via a `@team` broadcast — deliberately
        // reusing `task_names_card_specifically`, the exact predicate the exclusive-seat
        // filter already relies on to exclude `@team`, rather than writing a second
        // matcher that could drift from it.
        let preons = self.loaded_preons();
        let named_specifically = self
            .roster
            .iter()
            .find(|c| &c.id == target)
            .is_some_and(|card| crate::router::task_names_card_specifically(&task_desc, card, &preons));

        // Resolved against the daemon's cwd: a *relative* field path (`.hadron/field.jsonl`,
        // exactly how the daemon is launched) used to bottom out on the empty ancestor —
        // `"".join(".hadron")` exists, so the search "succeeded" with a root of "". That
        // empty root became the CLI's `cwd`, and `current_dir("")` is ENOENT, which the
        // spawn error then blamed on the program: `failed to spawn claude`.
        let base = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let workspace_root = workspace_root_of(&self.field_path, &base);

        let (mut invariants_text, available_invariants) =
            build_invariants(&workspace_root, &requested_invariants);

        // The skill for THIS turn, appended after the static tiers so the long,
        // byte-stable prefix (Standard Model → global → repo) keeps its prompt cache
        // and only the tail varies per task.
        //
        // The engine picks it, and the engine computes who could take the next step —
        // both on purpose. A model asked to decide whether to follow a process skips it
        // under pressure, and a model asked who is available guesses; a disabled seat
        // keeps its roster card (`disable-is-not-unseat`), so naming it as a reviewer
        // routes the work into a void. Here, "available" is checked at dispatch.
        // The always-on skill index — the "you have these procedures, invoke the right
        // one as the work crosses phases" discipline, injected every turn. This is
        // hadron's analog of the Superpowers using-superpowers bootstrap, which rides a
        // SessionStart hook that does not exist over ACP.
        //
        // The corpus is built-ins merged with whatever custom `.md` skills a human has
        // dropped in `~/.hadron/skills` (global) and `<workspace>/.hadron/skills`
        // (repo) — `load_skills` degrades to exactly `builtins()` when neither
        // directory exists, so a machine with no custom skills installed sees
        // byte-for-byte the same corpus as before this wiring landed.
        //
        // Loaded fresh per projection rather than cached on the engine: this mirrors
        // how `team.json` is already re-read every turn (see the roster live-reload),
        // keeps a skill edit picked up on the very next turn with no reload/restart
        // path to wire, and the cost is a handful of small file reads — not a hot loop.
        //
        // The global directory is `self.global_skills_dir` — injected (see
        // `with_global_skills_dir`), NOT resolved inline via `user_hadron_dir()` here.
        // Resolving it inline would make every engine test read the real `$HOME`,
        // whatever custom skills happen to be sitting in the machine's actual
        // `~/.hadron/skills` — `Engine::new` defaults this to `None` for exactly that
        // reason. The repo directory stays derived from `workspace_root`, which every
        // test already controls via its own tempdir field.
        let repo_skills_dir = workspace_root.join(".hadron").join("skills");
        let skill_corpus = skills::load_skills(self.global_skills_dir.as_deref(), Some(&repo_skills_dir));
        invariants_text.push_str(&skills::index(&skill_corpus));

        let mut role_body = None;
        if let Some(m) = skills::select(&task_desc, &skill_corpus) {
            if let Some(role) = skills::preferred_role(&m.id) {
                if let Some(card) = self.roster.iter().find(|c| &c.id == target) {
                    if card.roles.iter().any(|r| r.eq_ignore_ascii_case(role)) {
                        let roles_corpus = self.loaded_roles();
                        if let Some(matched_role) = roles_corpus.iter().find(|p| p.name.eq_ignore_ascii_case(role)) {
                            role_body = Some(matched_role.body.clone());
                        }
                    }
                }
            }

            let peers = self
                .roster
                .iter()
                .filter(|c| &c.id != target)
                .filter(|c| c.energy != EnergyState::Depleted)
                .filter(|c| self.is_enabled(&c.id))
                .map(|c| c.id.clone())
                .collect();

            // Provenance read off DISK, not taken from the turn's word for it: this is
            // the one part of separation-of-duties the engine can actually prove.
            let plan_author = skills::plan_ref(&task_desc)
                .map(|rel| workspace_root.join(rel))
                .and_then(|path| fs::read_to_string(path).ok())
                .and_then(|md| skills::plan_author(&md));

            // Every quark — resident (ACP) or one-shot (CLI) — gets the SAME shape now:
            // the always-on index above (the full menu, so it can still invoke a skill
            // this task didn't start in) plus the active skill's full body here. A
            // resident quark used to also get the whole library dumped into its
            // cache-stable prefix every turn (`skills::corpus()`, ~70-80k tokens); that
            // is gone — composition still works off the index, and the one skill this
            // turn actually needs arrives in full, same as it always has for CLI.
            invariants_text.push_str(&skills::render(&m, target, &skills::Handoff { peers, plan_author }, true));
        }

        // Resolve the quark's effective mode from the field before the turn:
        // real adapters translate it into the CLI's permission posture, so the
        // mode must ride along on the projection (not just gate a post-turn ask).
        // `effective_mode` degrades to `resolve_mode` byte-for-byte when
        // `no_human` is off (the default) — see `env_no_human_mode`.
        let turn_mode =
            hadron_gatekeeper::effective_mode(events, target, self.no_human, self.is_orchestrator(target));

        // Truncation must be *observable*, not just performed: a quark that cannot
        // see an earlier instruction, and is not told so, acts on a partial field
        // as confidently as on a whole one.
        let window = bounded_window(events, FIELD_WINDOW_BUDGET_BYTES);
        let truncated = window.len() < events.len();

        let nucleus_index_path = nucleus_index_path(&workspace_root);
        let (nucleus_index, nucleus_index_truncated) = read_nucleus_index_with_fallback(&workspace_root);

        let live_dir = hadron_lattice::live::live_dir(&self.field_path);
        let mut live_activities = Vec::new();
        let now = chrono::Utc::now();
        for c in &self.roster {
            if c.id != *target {
                if let Some(act) = hadron_lattice::live::read(&live_dir, &c.id, now) {
                    live_activities.push(act);
                }
            }
        }

        Projection {
            nucleus_index,
            nucleus_index_truncated,
            nucleus_index_path,
            nucleus_notes_dir: nucleus_notes_dir(&workspace_root),
            task: task_desc,
            invariants: invariants_text,
            available_invariants,
            nucleus_digest: self.nucleus_digest.clone(),
            roster: self.roster.clone(),
            live_activities,
            // NOT `events.to_vec()`. The whole field is unbounded; it grew past the
            // kernel's 128 KiB single-argv limit and killed every agy turn with
            // E2BIG. Keep the most recent events that fit the byte budget.
            field_window: window,
            field_truncated: truncated,
            git_diff,
            // The quark's own worktree when worktree discipline is on. Without it,
            // the workspace root — the pre-worktree behaviour, kept for the mock
            // daemon and every test that doesn't opt into git.
            isolated: cwd.is_some(),
            cwd: cwd.unwrap_or(workspace_root),
            mode: turn_mode,
            role_body,
            named_specifically,
        }
    }
}
