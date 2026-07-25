use std::collections::HashMap;

use chrono::DateTime;

use super::{message_fresh, MessageRow, QuarkStats, SessionStats, SpendPoint, SpendTimeline, StatsWindow, TurnSpend};

impl super::ChamberView {
    /// Aggregate the session's telemetry, per quark and in total.
    ///
    /// A quark is one that holds a **roster seat** — `actor_str` renders a quark
    /// as its bare id, so testing `from` for an `@` sigil matches nothing and
    /// silently zeroes every statistic.
    #[allow(dead_code)] // exercised by tests; no live GUI caller yet
    pub fn session_stats(&self) -> SessionStats {
        self.fold_stats(self.messages.iter())
    }

    /// Aggregate telemetry over a [`StatsWindow`]. `archived` is the projected
    /// messages of the archived sessions (`load_archived_messages`); it is ignored for
    /// [`StatsWindow::Session`] (live field only) and, for the wider windows, merged
    /// with the live field and filtered to the window's `cutoff(now)` by `ts`.
    ///
    /// Attribution is always against the **live** roster: an archived turn from a quark
    /// that no longer holds a seat is dropped, the same as a human/gluon message.
    pub fn stats_for(
        &self,
        archived: &[MessageRow],
        window: StatsWindow,
        now: DateTime<chrono::Utc>,
    ) -> SessionStats {
        // Merged (archives ahead of the live field), windowed, chronological — the fold
        // needs `ts` order (first_seen / spend_history depend on it).
        self.fold_stats(self.windowed_messages(archived, window, now).into_iter())
    }

    /// The window's messages (live field, plus archives for the wider windows), filtered
    /// to the cutoff and sorted chronologically. Shared by [`Self::stats_for`] and
    /// [`Self::spend_timeline`] so both read the exact same stream.
    fn windowed_messages<'a>(
        &'a self,
        archived: &'a [MessageRow],
        window: StatsWindow,
        now: DateTime<chrono::Utc>,
    ) -> Vec<&'a MessageRow> {
        let cutoff = window.cutoff(now);
        let in_window = |m: &&MessageRow| cutoff.map_or(true, |c| m.ts >= c);
        let mut msgs: Vec<&MessageRow> = if window.includes_archives() {
            archived
                .iter()
                .chain(self.messages.iter())
                .filter(in_window)
                .collect()
        } else {
            self.messages.iter().filter(in_window).collect()
        };
        msgs.sort_by_key(|m| m.ts);
        if window == StatsWindow::Current {
            if let Some(pos) = msgs.iter().rposition(|m| m.from == "human") {
                msgs = msgs[pos..].to_vec();
            }
        }
        msgs
    }

    /// Cumulative fresh spend over turns, per quark and team, for the combined spend area
    /// chart (see [`SpendTimeline`]). Walks the same windowed, chronological, roster-
    /// attributed stream as [`Self::stats_for`], emitting one point per spend event with a
    /// snapshot of every quark's running total and the team sum. Quarks that never spend
    /// in the window are dropped, so the series count matches what actually has data.
    pub fn spend_timeline(
        &self,
        archived: &[MessageRow],
        window: StatsWindow,
        now: DateTime<chrono::Utc>,
    ) -> SpendTimeline {
        // Series order = roster order, so the chart, the cards, and the roster agree.
        let order: Vec<&str> = self.roster.iter().map(|r| r.id.as_str()).collect();
        let index: HashMap<&str, usize> =
            order.iter().enumerate().map(|(i, s)| (*s, i)).collect();

        let mut cum = vec![0f64; order.len()];
        let mut team = 0f64;
        let mut points: Vec<SpendPoint> = Vec::new();

        for m in self.windowed_messages(archived, window, now) {
            let Some(&qi) = index.get(m.from.as_str()) else {
                continue; // human, gluon, or an actor with no seat
            };
            let Some(f) = message_fresh(m, self.roster[qi].transport).filter(|f| *f > 0) else {
                continue;
            };
            cum[qi] += f as f64;
            team += f as f64;
            points.push(SpendPoint {
                step: points.len() as u32 + 1,
                per_quark: cum.clone(),
                team,
            });
        }

        // Drop columns for quarks that never spent, so the chart draws only live series.
        let active: Vec<usize> = (0..order.len()).filter(|&i| cum[i] > 0.0).collect();
        SpendTimeline {
            quarks: active.iter().map(|&i| order[i].to_string()).collect(),
            points: points
                .into_iter()
                .map(|p| SpendPoint {
                    step: p.step,
                    per_quark: active.iter().map(|&i| p.per_quark[i]).collect(),
                    team: p.team,
                })
                .collect(),
        }
    }

    /// The shared fold behind [`Self::session_stats`] and [`Self::stats_for`]: sum the
    /// given messages per quark (attributed via the live roster) and in total.
    fn fold_stats<'a>(&self, messages: impl Iterator<Item = &'a MessageRow>) -> SessionStats {
        let mut stats: HashMap<&str, QuarkStats> = HashMap::new();
        let mut out = SessionStats::default();

        for m in messages {
            let Some(row) = self.roster.iter().find(|r| r.id == m.from) else {
                continue; // human, gluon, or an actor with no seat
            };
            let s = stats.entry(row.id.as_str()).or_default();

            if s.first_seen.is_none() {
                s.first_seen = Some(m.ts);
            }
            s.last_active = Some(m.ts);

            if m.is_chat() {
                s.turns += 1;
                out.total_turns += 1;
            }

            let fresh = message_fresh(m, row.transport);

            if let Some(f) = fresh {
                s.fresh += f;
                out.total_fresh += f;

                let cost_usd = m.usage.as_ref().and_then(|u| u.cost_usd());
                if let Some(c) = cost_usd {
                    s.cost_usd = Some(s.cost_usd.unwrap_or(0.0) + c);
                    out.total_cost_usd = Some(out.total_cost_usd.unwrap_or(0.0) + c);
                }

                s.spend_history.push(TurnSpend {
                    turn: s.turns,
                    fresh: f,
                    cost_usd,
                });
                out.spend_history.push(TurnSpend {
                    turn: out.total_turns,
                    fresh: f,
                    cost_usd,
                });
            }

            let Some(u) = &m.usage else { continue };

            if let Some(c) = u.spend.cached() {
                s.cached += c;
                out.total_cached += c;
            }
            if let Some(ctx) = &u.context {
                s.context = Some(ctx.clone());
            }
            if !u.quota.is_empty() {
                s.quota = u.quota.clone();
            }
        }

        out.per_quark = self
            .roster
            .iter()
            .map(|r| {
                (
                    r.id.clone(),
                    stats.remove(r.id.as_str()).unwrap_or_default(),
                )
            })
            .collect();
        out
    }
}
