use std::collections::{HashMap, HashSet};

use hadron_lattice::{
    Actor, Event, Kind, QuarkId, QuarkState,
};
use tokio::task::AbortHandle;
use ulid::Ulid;


impl super::Engine {
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
        in_flight: &mut HashSet<QuarkId>,
        abort_handles: &mut HashMap<QuarkId, AbortHandle>,
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
            let Some(quark) = self.quarks.get(&target).cloned() else {
                eprintln!(
                    "gluon: reboot for unseated quark {} — ignored",
                    target.as_str()
                );
                continue;
            };

            if in_flight.remove(&target) {
                if let Some(handle) = abort_handles.remove(&target) {
                    handle.abort();
                }
                self.append(Event::new(
                    Actor::Quark(target.clone()),
                    None,
                    Kind::Status { state: QuarkState::Ground },
                ))
                .await?;
                grounded.push(target.clone());
            }

            // Reaps a resident session (idempotent). The `lock().await` waits for a
            // just-aborted turn to drop its guard before we take it.
            quark.lock().await.reset_session();
            eprintln!("gluon: force-restarted quark {}", target.as_str());
        }

        Ok(grounded)
    }
}
