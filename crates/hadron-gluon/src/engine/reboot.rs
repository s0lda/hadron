use std::collections::{HashMap, HashSet};

use hadron_lattice::term::{self, Source};
use hadron_lattice::{
    Actor, Event, Kind, QuarkId, QuarkState,
};
use tokio::task::AbortHandle;
use ulid::Ulid;


impl super::Engine {
    /// Abort `key`'s in-flight turn, if it has one: removed from `in_flight`,
    /// its handle taken out and aborted. Pure bookkeeping — no field append, no
    /// lock — so a caller that aborts several keys for one seat (a whole-seat
    /// reboot, below) can decide for itself whether to ground once or per key.
    /// Returns whether it was in flight.
    pub(super) fn abort_in_flight(
        key: &(QuarkId, super::Lane),
        in_flight: &mut HashSet<(QuarkId, super::Lane)>,
        abort_handles: &mut HashMap<(QuarkId, super::Lane), AbortHandle>,
    ) -> bool {
        let was_in_flight = in_flight.remove(key);
        if let Some(handle) = abort_handles.remove(key) {
            handle.abort();
        }
        was_in_flight
    }

    /// Append the terminal `Status{Ground}` an abandoned turn needs so the
    /// message it was answering reads as *answered* and `next_pending` does
    /// not re-select the quark onto the abandoned turn.
    ///
    /// Paired with [`Self::abort_in_flight`] at both places a turn gets cut
    /// short outside the normal `join_next` path (Task 9 of `.hadron/docs/
    /// plans/2026-07-31-responsive-orchestrator.md`) — a human's force-restart
    /// ([`Self::service_reboots`], below) and a graceful cancel that never
    /// resolved in time (`CANCEL_DEADLINE`, `engine/run.rs`). The deadline
    /// fallback used to abort without grounding, so the abandoned assignment's
    /// dispatch record stayed unanswered and the next pass re-excited the
    /// quark onto the stale task instead of the message that actually
    /// interrupted it. Kept separate from `abort_in_flight` rather than one
    /// combined call: a reboot aborts up to two lanes but grounds the SEAT
    /// once (`Status` carries no lane), which only the caller can decide.
    ///
    /// `assignment` is REQUIRED, not defaulted, so a caller has to say which
    /// assignment this Ground answers — leaving it unstamped falls into
    /// `has_answered`'s "legacy event: fall back to it spoke after the
    /// message" arm, which reads an unstamped Ground as answering EVERY
    /// unaddressed message before it in the field, including a genuinely
    /// newer one that has nothing to do with the turn just aborted (caught by
    /// this task's own test: the interrupting message was silently swallowed
    /// before this parameter existed). `service_reboots` passes `None` — a
    /// reboot has no single assignment to name (up to two lanes, two
    /// different ones) and keeps its existing legacy-fallback behaviour.
    pub(super) async fn ground(&self, target: &QuarkId, assignment: Option<ulid::Ulid>) -> anyhow::Result<()> {
        self.append(
            Event::new(Actor::Quark(target.clone()), None, Kind::Status { state: QuarkState::Ground })
                .answering(assignment),
        )
        .await
    }

    /// Service the human's **force-restart** ([`Kind::Reboot`]) for any reboot event
    /// appended since the last field read.
    ///
    /// The first call baselines — it records every reboot then in the field as
    /// already-seen and services nothing, because that history predates this daemon,
    /// when there was no live session to kill. Thereafter, each reboot whose id is not
    /// yet in the serviced set and that targets a seated quark is serviced exactly once:
    ///
    /// - **in-flight quark:** abort its running turn task. That drops the turn future,
    ///   releasing the `Mutex` guard the turn held on the quark — so the `lock().await`
    ///   below can then proceed — and appends a terminal `Ground` so the interrupted
    ///   message reads as *answered* (a bare terminal status counts as turn
    ///   completion), keeping the quark from being re-excited onto the abandoned turn.
    ///   It re-boots on its next `@mention`.
    /// - **both in-flight and idle:** lock the quark and [`Quark::reset_session`],
    ///   which drops a resident ACP session and reaps its subprocess. Aborting the turn
    ///   alone does NOT reap an ACP child — that child is owned by the session's pump
    ///   thread, not by the turn future — so the session reset is what actually kills
    ///   it. A no-op for a one-shot CLI quark or an already-idle ACP quark.
    ///
    /// The aborted task surfaces later in `join_next` as a *cancelled* `JoinError`; the
    /// dispatch loop absorbs that (cleanup already happened here) rather than treating
    /// it as a panic and grounding every sibling.
    /// Returns the quarks whose in-flight turn was aborted-and-grounded this pass. The
    /// caller must NOT re-dispatch them on the *same* field snapshot: that snapshot
    /// predates the `Ground` appended here, so the just-answered message still reads as
    /// pending and the quark would be re-excited onto the turn we just killed. On the
    /// next read the `Ground` is present and normal answered-logic takes over.
    pub(super) async fn service_reboots(
        &mut self,
        events: &[Event],
        in_flight: &mut HashSet<(QuarkId, super::Lane)>,
        abort_handles: &mut HashMap<(QuarkId, super::Lane), AbortHandle>,
    ) -> anyhow::Result<Vec<QuarkId>> {
        let mut grounded: Vec<QuarkId> = Vec::new();

        // First read: baseline. Stamp every reboot currently in the field as already-seen
        // (they predate the daemon's boot — no live session to kill) and service nothing.
        let Some(seen) = &self.serviced_reboots else {
            self.serviced_reboots = Some(
                events
                    .iter()
                    .filter(|e| matches!(e.kind, Kind::Reboot))
                    .map(|e| e.id)
                    .collect(),
            );
            return Ok(grounded);
        };

        // The reboots not yet serviced, in append order. A reboot is per-quark (envelope
        // `to`); a broadcast reboot is meaningless and dropped here. We snapshot (id,
        // target) now so the immutable borrow of `seen` is released before the mutations
        // below (`append`, `reset_session`). Matching by id — not position — is what makes
        // this survive `/clear`: a post-clear reboot's id is simply absent from the set, so
        // it fires no matter how the field length changed under us.
        let pending: Vec<(Ulid, QuarkId)> = events
            .iter()
            .filter(|e| matches!(e.kind, Kind::Reboot))
            .filter(|e| !seen.contains(&e.id))
            .filter_map(|e| e.to.clone().map(|target| (e.id, target)))
            .collect();

        for (id, target) in pending {
            // Mark serviced before acting, so a reboot for an unseated/unknown quark is
            // not re-evaluated every pass.
            if let Some(seen) = &mut self.serviced_reboots {
                seen.insert(id);
            }
            let Some(lanes) = self.quarks.get(&target).cloned() else {
                term::warn(Source::Gluon, &format!("reboot for unseated quark {} — ignored", target.as_str()));
                continue;
            };

            // A reboot targets the whole SEAT, not one lane — abort whichever of its
            // (up to two) lanes are in flight, and ground once if either was.
            let mut was_in_flight = false;
            for lane in [super::Lane::Work, super::Lane::Chat] {
                let key = (target.clone(), lane);
                if Self::abort_in_flight(&key, in_flight, abort_handles) {
                    was_in_flight = true;
                }
            }
            if was_in_flight {
                self.ground(&target, None).await?;
                grounded.push(target.clone());
            }

            // Reaps each lane's resident session (idempotent). The `lock().await`
            // waits for a just-aborted turn to drop its guard before we take it.
            lanes.work.lock().await.reset_session();
            if let Some(chat) = &lanes.chat {
                chat.lock().await.reset_session();
            }
            term::info(Source::Gluon, &format!("force-restarted quark {}", target.as_str()));
        }

        Ok(grounded)
    }
}
