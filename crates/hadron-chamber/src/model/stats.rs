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
        if !msgs.windows(2).all(|w| w[0].ts <= w[1].ts) {
            msgs.sort_by_key(|m| m.ts);
        }
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
        let roster_map: HashMap<&str, &super::RosterRow> =
            self.roster.iter().map(|r| (r.id.as_str(), r)).collect();
        let mut stats: HashMap<&str, QuarkStats> = HashMap::new();
        let mut out = SessionStats::default();

        for m in messages {
            let Some(row) = roster_map.get(m.from.as_str()) else {
                continue; // human, gluon, or an actor with no seat
            };
            let s = stats.entry(row.id.as_str()).or_default();
            s.transport = Some(row.transport);

            if s.first_seen.is_none() {
                s.first_seen = Some(m.ts);
            }
            s.last_active = Some(m.ts);

            if m.is_chat() {
                s.turns = s.turns.saturating_add(1);
                out.total_turns = out.total_turns.saturating_add(1);
                let proto_key = row.transport.code().to_string();
                *out.protocol_turns.entry(proto_key).or_insert(0) += 1;
            }

            match m.kind_label {
                "edit" => {
                    s.total_edits = s.total_edits.saturating_add(1);
                    out.total_edits = out.total_edits.saturating_add(1);
                }
                "command" => {
                    s.total_commands = s.total_commands.saturating_add(1);
                    out.total_commands = out.total_commands.saturating_add(1);
                }
                "snapshot" => {
                    s.total_snapshots = s.total_snapshots.saturating_add(1);
                    out.total_snapshots = out.total_snapshots.saturating_add(1);
                }
                _ => {}
            }

            let fresh = message_fresh(m, row.transport);

            if let Some(f) = fresh {
                s.fresh = s.fresh.saturating_add(f as u64);
                out.total_fresh = out.total_fresh.saturating_add(f as u64);

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

            if let Some(i) = u.spend.input {
                s.input_tokens = s.input_tokens.saturating_add(i as u64);
                out.total_input = out.total_input.saturating_add(i as u64);
            }
            if let Some(o) = u.spend.output {
                s.output_tokens = s.output_tokens.saturating_add(o as u64);
                out.total_output = out.total_output.saturating_add(o as u64);
            }
            if let Some(cr) = u.spend.cache_read {
                s.cache_read = s.cache_read.saturating_add(cr as u64);
                out.total_cache_read = out.total_cache_read.saturating_add(cr as u64);
            }
            if let Some(cw) = u.spend.cache_write {
                s.cache_write = s.cache_write.saturating_add(cw as u64);
                out.total_cache_write = out.total_cache_write.saturating_add(cw as u64);
            }

            if let Some(c) = u.spend.cached() {
                s.cached = s.cached.saturating_add(c as u64);
                out.total_cached = out.total_cached.saturating_add(c as u64);
            }
            if let Some(ctx) = &u.context {
                s.context = Some(ctx.clone());
            }
            if !u.quota.is_empty() {
                s.quota = u.quota.clone();
            }
        }

        let has_unpriced = stats.values().any(|qs| qs.turns > 0 && qs.cost_usd.is_none());
        out.has_unpriced_quarks = has_unpriced;

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

/// Downsample a slice of [`SpendPoint`]s to at most `max_count` points for efficient
/// CPU software rasterized rendering in `AreaChart`. Preserves the first and last points
/// and uniformly samples intermediate steps.
pub fn downsample_spend_points(points: &[SpendPoint], max_count: usize) -> Vec<SpendPoint> {
    if points.len() <= max_count || max_count < 2 {
        return points.to_vec();
    }
    let mut res = Vec::with_capacity(max_count);
    res.push(points[0].clone());
    let step = (points.len() - 1) as f64 / (max_count - 1) as f64;
    for i in 1..(max_count - 1) {
        let idx = (i as f64 * step).round() as usize;
        if idx > 0 && idx < points.len() - 1 && res.last().map_or(true, |p: &SpendPoint| p.step != points[idx].step) {
            res.push(points[idx].clone());
        }
    }
    if let Some(last) = points.last() {
        if res.last().map_or(true, |p| p.step != last.step) {
            res.push(last.clone());
        }
    }
    res
}

/// Downsample a slice of [`TurnSpend`]s to at most `max_count` points.
pub fn downsample_turn_spend(points: &[TurnSpend], max_count: usize) -> Vec<TurnSpend> {
    if points.len() <= max_count || max_count < 2 {
        return points.to_vec();
    }
    let mut res = Vec::with_capacity(max_count);
    res.push(points[0].clone());
    let step = (points.len() - 1) as f64 / (max_count - 1) as f64;
    for i in 1..(max_count - 1) {
        let idx = (i as f64 * step).round() as usize;
        if idx > 0 && idx < points.len() - 1 && res.last().map_or(true, |p: &TurnSpend| p.turn != points[idx].turn) {
            res.push(points[idx].clone());
        }
    }
    if let Some(last) = points.last() {
        if res.last().map_or(true, |p| p.turn != last.turn) {
            res.push(last.clone());
        }
    }
    res
}

/// Downsample context points `(index, percentage)` to at most `max_count` points.
pub fn downsample_context_points(points: &[(usize, f64)], max_count: usize) -> Vec<(usize, f64)> {
    if points.len() <= max_count || max_count < 2 {
        return points.to_vec();
    }
    let mut res = Vec::with_capacity(max_count);
    res.push(points[0]);
    let step = (points.len() - 1) as f64 / (max_count - 1) as f64;
    for i in 1..(max_count - 1) {
        let idx = (i as f64 * step).round() as usize;
        if idx > 0 && idx < points.len() - 1 && res.last().map_or(true, |p| p.0 != points[idx].0) {
            res.push(points[idx]);
        }
    }
    if let Some(last) = points.last() {
        if res.last().map_or(true, |p| p.0 != last.0) {
            res.push(*last);
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use hadron_lattice::{Transport, Usage, TokenSpend, QuarkState, Mode};
    use crate::model::{ChamberView, MessageRow, RosterRow};

    fn test_roster_row(id: &str, transport: Transport) -> RosterRow {
        RosterRow {
            id: id.into(),
            display_name: Some(id.into()),
            state: QuarkState::Ground,
            mode: Mode::Bypass,
            mode_is_override: false,
            vendor: id.into(),
            model: "test-model".into(),
            flavor: None,
            transport,
            effort: None,
            enabled: true,
            adopted: true,
            tokens: 0,
            unknown_turns: 0,
        }
    }

    fn test_message_row(from: &str, ts: chrono::DateTime<Utc>, usage: Option<Usage>) -> MessageRow {
        MessageRow {
            from: from.into(),
            to: None,
            body: "test message".into(),
            kind_label: "message",
            usage,
            ts,
            legacy_used_tokens: None,
            turn: None,
            severity: None,
        }
    }

    #[test]
    fn day_window_filters_messages_within_24_hours() {
        let now = Utc::now();
        let view = ChamberView {
            roster: vec![test_roster_row("cli-agy", Transport::Cli)],
            messages: vec![
                test_message_row(
                    "cli-agy",
                    now - Duration::hours(10),
                    Some(Usage {
                        spend: TokenSpend { input: Some(100), output: Some(50), ..Default::default() },
                        ..Default::default()
                    }),
                ),
                test_message_row(
                    "cli-agy",
                    now - Duration::hours(30),
                    Some(Usage {
                        spend: TokenSpend { input: Some(500), output: Some(200), ..Default::default() },
                        ..Default::default()
                    }),
                ),
            ],
            ..Default::default()
        };

        let stats_day = view.stats_for(&[], StatsWindow::Day, now);
        assert_eq!(stats_day.total_fresh, 150);

        let stats_all = view.stats_for(&[], StatsWindow::AllTime, now);
        assert_eq!(stats_all.total_fresh, 850);
    }

    #[test]
    fn spend_timeline_computes_running_totals() {
        let now = Utc::now();
        let view = ChamberView {
            roster: vec![
                test_roster_row("agy", Transport::Cli),
                test_roster_row("claude", Transport::Cli),
            ],
            messages: vec![
                test_message_row(
                    "agy",
                    now - Duration::minutes(5),
                    Some(Usage {
                        spend: TokenSpend { input: Some(100), output: Some(100), ..Default::default() },
                        ..Default::default()
                    }),
                ),
                test_message_row(
                    "claude",
                    now - Duration::minutes(2),
                    Some(Usage {
                        spend: TokenSpend { input: Some(300), output: Some(200), ..Default::default() },
                        ..Default::default()
                    }),
                ),
            ],
            ..Default::default()
        };

        let timeline = view.spend_timeline(&[], StatsWindow::Session, now);
        assert_eq!(timeline.quarks, vec!["agy", "claude"]);
        assert_eq!(timeline.points.len(), 2);
        assert_eq!(timeline.points[0].team, 200.0);
        assert_eq!(timeline.points[1].team, 700.0);
    }
}
