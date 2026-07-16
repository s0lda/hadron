//! The chamber view-model: a pure projection of the field (`Vec<Event>`) into
//! what the UI renders — a chat row per event plus the derived per-quark roster
//! state. No GPUI here, so this is fully unit-tested.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, TimeZone};
use hadron_gatekeeper::{global_mode, has_override, resolve_mode, Mode};
use hadron_lattice::{Actor, Event, Kind, QuarkId, QuarkState, Team};

/// Wall-clock of an event, to the second.
///
/// Generic over the timezone so the *conversion* stays at the render site
/// (which uses `Local`) and this stays deterministic under test.
pub fn format_clock<Tz: TimeZone>(ts: DateTime<Tz>) -> String
where
    Tz::Offset: std::fmt::Display,
{
    ts.format("%H:%M:%S").to_string()
}

/// The label on a Discord-style date divider: relative for the two days a
/// reader thinks of by name, absolute before that.
pub fn date_divider_label(day: NaiveDate, today: NaiveDate) -> String {
    if day == today {
        "Today".to_string()
    } else if today.pred_opt() == Some(day) {
        "Yesterday".to_string()
    } else {
        day.format("%A, %B %-d, %Y").to_string()
    }
}

/// One rendered chat row. `kind_label` lets the UI style/filter by event type;
/// `body` is a display string synthesized for non-message events.
#[derive(Debug, Clone, PartialEq)]
pub struct MessageRow {
    pub from: String,
    pub to: Option<String>,
    pub body: String,
    pub kind_label: &'static str,
    pub usage: Option<hadron_lattice::Usage>,
    pub ts: chrono::DateTime<chrono::Utc>,
    pub legacy_used_tokens: Option<u32>,
}

fn resolve_fresh(
    usage: Option<&hadron_lattice::Usage>,
    legacy_used_tokens: u32,
    transport: hadron_lattice::Transport,
) -> Result<u32, u32> {
    match usage.and_then(|u| u.spend.fresh()) {
        Some(f) => Ok(f),
        None => {
            if matches!(transport, hadron_lattice::Transport::Cli) {
                Ok(legacy_used_tokens)
            } else {
                Err(1)
            }
        }
    }
}

/// One roster entry: a quark, its latest lifecycle state, its effective
/// permission mode (and whether that's an explicit per-quark override vs the
/// inherited global), and its legibility (`provider`/`model` from the team
/// config — empty strings when the seat isn't in `team.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRow {
    pub id: String,
    pub display_name: Option<String>,
    pub state: QuarkState,
    pub mode: Mode,
    pub mode_is_override: bool,
    pub provider: String,
    pub model: String,
    pub flavor: Option<hadron_lattice::Flavor>,
    pub transport: hadron_lattice::Transport,
    /// The reasoning effort the seat runs at (`low`/`medium`/`high`/…), resolved from
    /// the team config. `None` = inherit / not set, so no tag is shown.
    pub effort: Option<String>,
    pub enabled: bool,
    /// Whether this quark is *adopted* by the current repo (seated by the daemon) or
    /// only *available* in the global catalogue. A not-adopted row is shown greyed
    /// ("there to use when you want, but off"), the same visual as a disabled seat,
    /// and its lifecycle actions (enable/flavor/remove) are replaced by "Adopt".
    pub adopted: bool,
    /// Fresh tokens (input + output, cache excluded) — the one unit every transport
    /// reports the same way, so the only one comparable across a mixed roster.
    pub tokens: u32,
    /// Turns whose spend we cannot express in that unit: pre-components turns from an
    /// ACP seat, whose bare `used_tokens` was ACP's cache-inclusive total. Counted, so
    /// the UI can say "plus N turns of unknown spend" rather than silently omit them.
    pub unknown_turns: u32,
}

/// The reboots `/clear` must append after truncating the field: one per resident
/// (ACP) quark, so every live agent re-boots into the fresh session instead of
/// carrying pre-clear context. The field-is-session model means a cleared field
/// needs new sessions.
///
/// Only `Transport::Acp` quarks are resident and hold something to reap — a one-shot
/// CLI quark keeps nothing between turns, so it is skipped (a Reboot to it is a no-op
/// the daemon would service pointlessly). SSOT for the "restart residents on clear"
/// rule, kept here (pure, tested) so the GPUI command handler is a thin caller.
pub fn post_clear_reboots(roster: &[RosterRow]) -> Vec<Event> {
    roster
        .iter()
        .filter(|r| matches!(r.transport, hadron_lattice::Transport::Acp))
        .map(|r| Event::new(Actor::Human, Some(QuarkId::new(&r.id)), Kind::Reboot))
        .collect()
}

/// What one quark spent this session. `context` and `quota` are the *latest*
/// the provider reported, not a sum: occupancy and remaining allowance are
/// levels, and adding them up would mean nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnSpend {
    pub turn: u32,
    pub fresh: u32,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuarkStats {
    pub turns: u32,
    pub fresh: u32,
    pub cached: u32,
    pub cost_usd: Option<f64>,
    pub context: Option<hadron_lattice::ContextUsage>,
    pub quota: Vec<hadron_lattice::QuotaBucket>,
    pub first_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub last_active: Option<chrono::DateTime<chrono::Utc>>,
    pub spend_history: Vec<TurnSpend>,
}

/// Per-quark session statistics plus the totals that are meaningful to add.
/// Deliberately no total context or quota — see [`QuarkStats`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionStats {
    /// In roster order, so the tab and the roster agree.
    pub per_quark: Vec<(String, QuarkStats)>,
    pub total_turns: u32,
    pub total_fresh: u32,
    pub total_cached: u32,
    pub total_cost_usd: Option<f64>,
    pub spend_history: Vec<TurnSpend>,
}

/// Everything the chamber needs to render one frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChamberView {
    pub messages: Vec<MessageRow>,
    pub roster: Vec<RosterRow>,
    /// The global default permission mode (the status-bar mode badge), folded
    /// from the field's `ModeSet` events. `Mode::Ask` if none has been set.
    pub global_mode: Mode,
    /// An outstanding permission request awaiting the human's Approve/Deny, if any.
    /// The UI renders this as a toast; `None` means nothing to decide.
    pub pending_permission: Option<hadron_gatekeeper::PendingPermission>,
}

impl ChamberView {
    /// Aggregate the session's telemetry, per quark and in total.
    ///
    /// A quark is one that holds a **roster seat** — `actor_str` renders a quark
    /// as its bare id, so testing `from` for an `@` sigil matches nothing and
    /// silently zeroes every statistic.
    pub fn session_stats(&self) -> SessionStats {
        let mut stats: HashMap<&str, QuarkStats> = HashMap::new();
        let mut out = SessionStats::default();

        for m in &self.messages {
            let Some(row) = self.roster.iter().find(|r| r.id == m.from) else {
                continue; // human, gluon, or an actor with no seat
            };
            let s = stats.entry(row.id.as_str()).or_default();

            if s.first_seen.is_none() {
                s.first_seen = Some(m.ts);
            }
            s.last_active = Some(m.ts);

            if m.kind_label == "message" {
                s.turns += 1;
                out.total_turns += 1;
            }

            let fresh = if let Some(u) = &m.usage {
                u.spend.fresh()
            } else if let Some(used) = m.legacy_used_tokens {
                resolve_fresh(None, used, row.transport).ok()
            } else {
                None
            };

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

fn actor_str(a: &Actor) -> String {
    match a {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => q.as_str().to_string(),
    }
}

fn note(order: &mut Vec<String>, id: &str) {
    if !order.iter().any(|x| x == id) {
        order.push(id.to_string());
    }
}

fn render_row(e: &Event, turn_usages: &HashMap<String, hadron_lattice::Usage>) -> MessageRow {
    let from = actor_str(&e.from);
    let to = e.to.as_ref().map(|t| t.as_str().to_string());
    let mut legacy_used_tokens = None;
    let (body, kind_label): (String, &'static str) = match &e.kind {
        Kind::Message { body } => (body.clone(), "message"),
        Kind::Status { state } => (format!("{state:?}").to_lowercase(), "status"),
        Kind::Edit { paths, summary, .. } => {
            (format!("edited {} path(s): {summary}", paths.len()), "edit")
        }
        Kind::Command { cmd, exit, .. } => (format!("$ {cmd} (exit {exit})"), "command"),
        Kind::Snapshot { label, .. } => (format!("snapshot: {label}"), "snapshot"),
        Kind::EnergyReport { used_tokens } => {
            legacy_used_tokens = Some(*used_tokens);
            (format!("used {used_tokens} tokens"), "energy_report")
        }
        Kind::Assign { task, invariants } => (
            format!("assigned: {task} (invariants: {:?})", invariants),
            "assign",
        ),
        Kind::PermissionReq { risk, description } => (
            format!("⚠️ permission requested ({risk:?}): {description}"),
            "permission_req",
        ),
        Kind::PermissionGrant { approved, remember } => (
            format!(
                "permission {}{}",
                if *approved { "approved" } else { "denied" },
                if *remember { " (remembered)" } else { "" },
            ),
            "permission_grant",
        ),
        Kind::ModeSet { mode } => (format!("mode → {mode:?}").to_lowercase(), "mode_set"),
        Kind::ModeClear => ("mode → default (inherit global)".to_string(), "mode_clear"),
        Kind::Reboot => ("force-restart requested".to_string(), "reboot"),
        Kind::Unknown { kind, .. } => (format!("unrecognized event: {kind}"), "unrecognized"),
    };
    MessageRow {
        from,
        to,
        body,
        kind_label,
        usage: e
            .turn
            .and_then(|t| turn_usages.get(&t.to_string()).cloned())
            .or_else(|| e.usage.clone()),
        ts: e.ts,
        legacy_used_tokens,
    }
}

/// Project the field into a renderable view, with no team annotations
/// (`provider`/`model` blank). Convenience for tests and callers without a team.
#[cfg_attr(not(test), allow(dead_code))]
pub fn project(events: &[Event]) -> ChamberView {
    project_with_team(events, &Team::default(), &Team::default())
}

/// Project the field into a renderable view. Roster order is first-seen; a
/// quark's state is the latest `Kind::Status` it authored (default `Ground`);
/// its mode is folded from `ModeSet` events (per-quark override over global);
/// its `provider`/`model` come from `team` (blank when the seat is unknown).
/// `team` is the **resolved** team (adopted quarks, full definitions); `global` is
/// the whole catalogue, so quarks available but not adopted here still get a (greyed)
/// roster row.
pub fn project_with_team(events: &[Event], team: &Team, global: &Team) -> ChamberView {
    let mut messages = Vec::with_capacity(events.len());
    let mut order: Vec<String> = Vec::new();
    let mut tokens: HashMap<String, u32> = HashMap::new();
    let mut unknown_turns: HashMap<String, u32> = HashMap::new();
    let mut states: HashMap<String, QuarkState> = HashMap::new();

    let mut turn_usages: HashMap<String, hadron_lattice::Usage> = HashMap::new();
    for e in events {
        if let Some(turn) = e.turn {
            if let Some(usage) = &e.usage {
                turn_usages.insert(turn.to_string(), usage.clone());
            }
        }
    }

    for e in events {
        // Roster membership: any quark that authors or is addressed.
        if let Actor::Quark(q) = &e.from {
            note(&mut order, q.as_str());
        }
        if let Some(t) = &e.to {
            note(&mut order, t.as_str());
        }
        // Latest status per quark wins.
        if let (Actor::Quark(q), Kind::Status { state }) = (&e.from, &e.kind) {
            states.insert(q.as_str().to_string(), *state);
        }
        // Accumulate tokens — in ONE unit: fresh (input + output), cache excluded.
        if let (Actor::Quark(q), Kind::EnergyReport { used_tokens }) = (&e.from, &e.kind) {
            let cli = team
                .get(&QuarkId::new(q.as_str()))
                .map(|s| s.transport)
                .unwrap_or(hadron_lattice::Transport::Cli);

            match resolve_fresh(e.usage.as_ref(), *used_tokens, cli) {
                Ok(f) => *tokens.entry(q.as_str().to_string()).or_default() += f,
                Err(u) => *unknown_turns.entry(q.as_str().to_string()).or_default() += u,
            }
        }
        messages.push(render_row(e, &turn_usages));
    }

    // Adopted quarks that have not spoken yet still belong on the roster (seeded from
    // the resolved team), then catalogue quarks not adopted here — shown greyed,
    // "available but off". Both are appended after event-seen ids so the order stays
    // stable across restarts (active quarks first, then silent-adopted, then available).
    for seat in &team.quarks {
        note(&mut order, seat.id.as_str());
    }
    for seat in &global.quarks {
        note(&mut order, seat.id.as_str());
    }

    let roster = order
        .into_iter()
        .map(|id| {
            let state = states.get(&id).copied().unwrap_or(QuarkState::Ground);
            let qid = QuarkId::new(&id);
            // Legibility comes from the resolved team if the quark is adopted, else
            // from the catalogue (available-but-off), else it is an event-only id we
            // know nothing about (a live participant with no seat).
            let (display_name, provider, model, flavor, transport, effort, enabled, adopted) =
                match (team.get(&qid), global.get(&qid)) {
                    (Some(s), _) => (
                        s.display_name.clone(),
                        s.provider.clone(),
                        s.model.clone(),
                        Some(s.flavor.clone()),
                        s.transport,
                        s.effort.clone(),
                        s.enabled,
                        true,
                    ),
                    (None, Some(g)) => (
                        g.display_name.clone(),
                        g.provider.clone(),
                        g.model.clone(),
                        Some(g.flavor.clone()),
                        g.transport,
                        g.effort.clone(),
                        false, // not adopted here → inert, greyed like a disabled seat
                        false,
                    ),
                    (None, None) => (
                        None,
                        String::new(),
                        String::new(),
                        None,
                        hadron_lattice::Transport::Cli,
                        None,
                        true,
                        true, // event-only live participant, no seat
                    ),
                };
            let q_tokens = tokens.get(&id).copied().unwrap_or(0);
            let q_unknown = unknown_turns.get(&id).copied().unwrap_or(0);
            RosterRow {
                state,
                mode: resolve_mode(events, &qid),
                mode_is_override: has_override(events, &qid),
                display_name,
                provider,
                model,
                flavor,
                transport,
                effort,
                enabled,
                adopted,
                id,
                tokens: q_tokens,
                unknown_turns: q_unknown,
            }
        })
        .collect();

    ChamberView {
        messages,
        roster,
        global_mode: global_mode(events),
        pending_permission: hadron_gatekeeper::pending_permission(events),
    }
}

// NOTE: the human's `@mention` routing lives in the daemon now
// (`hadron_gluon::router::human_mentions`), not the chamber. The chamber writes
// the raw message with `to: None` and leaves mentions in the body, so ONE message
// can address several quarks; the daemon resolves and fans them out. (A former
// `parse_mention` here lifted a single leading mention into `to` — which silently
// dropped the second addressee — and has been removed.)

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(from: Actor, to: Option<&str>, kind: Kind) -> Event {
        Event::new(from, to.map(QuarkId::new), kind)
    }

    fn roster_entry(id: &str, transport: hadron_lattice::Transport) -> RosterRow {
        RosterRow {
            id: id.to_string(),
            display_name: None,
            state: QuarkState::Ground,
            mode: Mode::Ask,
            mode_is_override: false,
            provider: String::new(),
            model: String::new(),
            flavor: None,
            transport,
            effort: None,
            enabled: true,
            adopted: true,
            tokens: 0,
            unknown_turns: 0,
        }
    }

    /// `/clear` restarts every resident agent so the fresh field is a fresh session:
    /// one `Kind::Reboot` per ACP quark, addressed to it; CLI quarks (nothing resident)
    /// are skipped.
    #[test]
    fn post_clear_reboots_one_per_resident_quark_skipping_cli() {
        use hadron_lattice::Transport;
        let roster = vec![
            roster_entry("opus", Transport::Acp),
            roster_entry("agy", Transport::Cli),
            roster_entry("gemini", Transport::Acp),
        ];

        let reboots = post_clear_reboots(&roster);

        assert_eq!(reboots.len(), 2, "one reboot per ACP quark, none for the CLI quark");
        assert!(reboots.iter().all(|e| matches!(e.kind, Kind::Reboot)));
        assert!(reboots.iter().all(|e| e.from == Actor::Human));
        let targets: Vec<String> = reboots
            .iter()
            .filter_map(|e| e.to.as_ref().map(|q| q.as_str().to_string()))
            .collect();
        assert_eq!(targets, vec!["opus".to_string(), "gemini".to_string()]);
    }

    /// The Session tab reported zeros for every quark: it decided who was a quark by
    /// testing `from` for an `@` prefix, and `actor_str` renders a quark as its bare
    /// id. The filter matched nothing, so every statistic silently read as 0 — the
    /// tab looked like a swarm that had never spent a token.
    #[test]
    fn session_stats_attribute_to_the_quark_that_earned_them() {
        let spend = hadron_lattice::TokenSpend {
            input: Some(100),
            output: Some(20),
            cache_read: Some(9_000),
            cache_write: None,
        };
        let usage = hadron_lattice::Usage {
            model: Some("claude-3-opus-20240229".to_string()),
            spend: spend.clone(),
            context: Some(hadron_lattice::ContextUsage {
                used_tokens: 57_000,
                context_window_size: 200_000,
                used_percentage: 28.5,
            }),
            quota: vec![],
        };

        let mut reply = ev(
            Actor::Quark(QuarkId::new("opus")),
            None,
            Kind::Message {
                body: "done".into(),
            },
        );
        reply.usage = Some(usage);

        let evs = vec![
            ev(
                Actor::Human,
                Some("opus"),
                Kind::Message { body: "go".into() },
            ),
            reply,
        ];
        let stats = project(&evs).session_stats();

        let (id, s) = stats
            .per_quark
            .iter()
            .find(|(id, _)| id == "opus")
            .expect("opus holds a roster seat");
        assert_eq!(id, "opus");
        assert_eq!(s.turns, 1);
        assert_eq!(s.fresh, 120, "fresh is input+output, and it is NOT zero");
        assert_eq!(s.cached, 9_000, "cache is carried, separately");
        assert_eq!(s.context.as_ref().unwrap().used_tokens, 57_000);

        // The human's own message is not a turn, and never a quark's spend.
        assert_eq!(stats.total_turns, 1);
        assert_eq!(stats.total_fresh, 120);
    }

    /// The roster showed 14.4M against an ACP quark long after `fresh()` landed: the
    /// legacy fallback still folded a pre-components `used_tokens` into the same column,
    /// and for an ACP seat that number is ACP's cache-INCLUSIVE total. Different unit.
    /// A CLI seat's legacy number really is input+output, so that one still counts.
    #[test]
    fn a_legacy_acp_total_is_not_counted_as_fresh_tokens() {
        use hadron_lattice::{AcpCommand, Flavor, Seat, Transport};

        let team = Team {
            quarks: vec![
                Seat {
                    id: QuarkId::new("acp-claude"),
                    display_name: None,
                    provider: "acp-claude".into(),
                    model: "x".into(),
                    effort: None,
                    mode_config: None,
                    flavor: Flavor::Worker,
                    transport: Transport::Acp,
                    command: Some(AcpCommand {
                        program: "npx".into(),
                        args: vec![],
                    }),
                    enabled: true,
                },
                Seat {
                    id: QuarkId::new("opus"),
                    display_name: None,
                    provider: "claude".into(),
                    model: "opus".into(),
                    effort: None,
                    mode_config: None,
                    flavor: Flavor::Orchestrator,
                    transport: Transport::Cli,
                    command: None,
                    enabled: true,
                },
            ],
            roster: vec![],
            max_exchanges: None,
        };

        // Both legacy: no `usage` on the envelope, just the bare u32.
        let evs = vec![
            ev(
                Actor::Quark(QuarkId::new("acp-claude")),
                None,
                Kind::EnergyReport {
                    used_tokens: 1_307_987,
                },
            ),
            ev(
                Actor::Quark(QuarkId::new("opus")),
                None,
                Kind::EnergyReport { used_tokens: 5_338 },
            ),
        ];
        let view = project_with_team(&evs, &team, &Team::default());
        let row = |id: &str| view.roster.iter().find(|r| r.id == id).unwrap().clone();

        let acp = row("acp-claude");
        assert_eq!(
            acp.tokens, 0,
            "an ACP legacy total is a different unit — not fresh"
        );
        assert_eq!(
            acp.unknown_turns, 1,
            "and it is counted as unknown, not dropped"
        );

        let cli = row("opus");
        assert_eq!(
            cli.tokens, 5_338,
            "a CLI legacy total IS input+output — it counts"
        );
        assert_eq!(cli.unknown_turns, 0);
    }

    /// A provider that cannot see cache reports `None`, which means *unknown*.
    /// Counting it as 0 would claim we know it spent nothing.
    #[test]
    fn an_absent_component_is_not_counted_as_zero() {
        let mut reply = ev(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::Message {
                body: "done".into(),
            },
        );
        reply.usage = Some(hadron_lattice::Usage {
            model: Some("gemini-1.5-pro".to_string()),
            spend: hadron_lattice::TokenSpend {
                input: Some(5),
                output: Some(5),
                cache_read: None,
                cache_write: None,
            },
            context: None,
            quota: vec![],
        });
        let stats = project(&[reply]).session_stats();
        let (_, s) = &stats.per_quark[0];
        assert_eq!(s.fresh, 10);
        assert_eq!(s.cached, 0, "no cache reported — nothing added");
        assert!(s.context.is_none());
        assert!(
            s.quota.is_empty(),
            "empty means no quota concept, not exhausted"
        );
    }

    #[test]
    fn global_mode_and_per_quark_override_are_surfaced() {
        let evs = vec![
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
            ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Auto }), // global Auto
            ev(
                Actor::Human,
                Some("agy"),
                Kind::ModeSet { mode: Mode::Bypass },
            ), // agy override
        ];
        let view = project(&evs);
        assert_eq!(view.global_mode, Mode::Auto);
        let agy = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy.mode, Mode::Bypass);
        assert!(agy.mode_is_override);
    }

    #[test]
    fn roster_row_inherits_global_and_carries_team_legibility() {
        use hadron_lattice::{Flavor, Seat};
        let team = Team {
            quarks: vec![Seat::cli(
                QuarkId::new("agy"),
                "agy",
                "gemini-3-pro",
                Flavor::Worker,
            )],
            roster: vec![],
            max_exchanges: None,
        };
        let evs = vec![
            ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Write }),
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
        ];
        let view = project_with_team(&evs, &team, &Team::default());
        let agy = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy.mode, Mode::Write, "inherits the global default");
        assert!(!agy.mode_is_override);
        assert_eq!(agy.provider, "agy");
        assert_eq!(agy.model, "gemini-3-pro");
    }

    /// A catalogue quark that this repo has NOT adopted still gets a roster row —
    /// greyed (not adopted, inert), its legibility drawn from the catalogue — so the
    /// user sees "there to use when you want, but off".
    #[test]
    fn catalogue_quarks_not_adopted_here_show_as_available() {
        use hadron_lattice::{Flavor, Seat};
        // Resolved team: only "opus" is adopted here.
        let team = Team {
            quarks: vec![Seat::cli(
                QuarkId::new("opus"),
                "claude",
                "opus",
                Flavor::Orchestrator,
            )],
            roster: vec![],
            max_exchanges: None,
        };
        // Catalogue: "opus" (adopted) plus "gemini" (available, not adopted here).
        let global = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("opus"), "claude", "opus", Flavor::Orchestrator),
                Seat::cli(QuarkId::new("gemini"), "agy", "gemini-3-pro", Flavor::Worker),
            ],
            roster: vec![],
            max_exchanges: None,
        };
        let view = project_with_team(&[], &team, &global);
        let opus = view.roster.iter().find(|r| r.id == "opus").unwrap();
        assert!(opus.adopted, "the seated quark is adopted");
        assert!(opus.enabled);
        let gemini = view.roster.iter().find(|r| r.id == "gemini").unwrap();
        assert!(!gemini.adopted, "the catalogue-only quark is not adopted");
        assert!(!gemini.enabled, "and shows inert (grey dot)");
        assert_eq!(gemini.provider, "agy", "legibility comes from the catalogue");
    }

    #[test]
    fn mode_set_renders_as_a_row() {
        let view = project(&[ev(Actor::Human, None, Kind::ModeSet { mode: Mode::Bypass })]);
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].kind_label, "mode_set");
        assert!(view.messages[0].body.contains("bypass"));
    }

    #[test]
    fn message_becomes_a_row() {
        let view = project(&[ev(
            Actor::Human,
            Some("claude"),
            Kind::Message {
                body: "build it".into(),
            },
        )]);
        assert_eq!(view.messages.len(), 1);
        let row = &view.messages[0];
        assert_eq!(row.from, "human");
        assert_eq!(row.to.as_deref(), Some("claude"));
        assert_eq!(row.body, "build it");
        assert_eq!(row.kind_label, "message");
    }

    #[test]
    fn pending_permission_is_surfaced_then_cleared_by_a_grant() {
        let req = ev(
            Actor::Quark(QuarkId::new("agy")),
            None,
            Kind::PermissionReq {
                risk: hadron_gatekeeper::Risk::BashExec,
                description: "cargo publish".into(),
            },
        );
        // With an outstanding request, the view carries it (addressed to the asker).
        let view = project(std::slice::from_ref(&req));
        let pending = view
            .pending_permission
            .expect("outstanding request surfaced");
        assert_eq!(pending.quark, QuarkId::new("agy"));
        assert_eq!(pending.description, "cargo publish");

        // Once granted, the toast clears.
        let grant = ev(
            Actor::Human,
            Some("agy"),
            Kind::PermissionGrant {
                approved: true,
                remember: false,
            },
        );
        let view = project(&[req, grant]);
        assert!(view.pending_permission.is_none());
    }

    #[test]
    fn assign_becomes_a_row() {
        let view = project(&[ev(
            Actor::Human,
            Some("agy"),
            Kind::Assign {
                task: "work".into(),
                invariants: vec!["no errors".into()],
            },
        )]);
        assert_eq!(view.messages.len(), 1);
        let row = &view.messages[0];
        assert_eq!(row.kind_label, "assign");
        assert_eq!(row.body, "assigned: work (invariants: [\"no errors\"])");
    }

    #[test]
    fn latest_status_wins_in_roster() {
        let agy = || Actor::Quark(QuarkId::new("agy"));
        let view = project(&[
            ev(
                Actor::Human,
                Some("agy"),
                Kind::Message { body: "go".into() },
            ),
            ev(
                agy(),
                None,
                Kind::Status {
                    state: QuarkState::Excited,
                },
            ),
            ev(
                agy(),
                None,
                Kind::Status {
                    state: QuarkState::Ground,
                },
            ),
        ]);
        let agy_row = view.roster.iter().find(|r| r.id == "agy").unwrap();
        assert_eq!(agy_row.state, QuarkState::Ground); // latest wins
    }

    #[test]
    fn unknown_event_is_a_muted_row_not_dropped() {
        // Construct an Unknown by round-tripping a future-kind JSON line.
        let line = serde_json::to_string(&json!({
            "v": 2,
            "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "ts": "2026-07-10T14:00:00Z",
            "from": "gluon",
            "to": null,
            "kind": "edit_by_hash",
            "block_hash": "9f86d0"
        }))
        .unwrap();
        let e: Event = serde_json::from_str(&line).unwrap();
        let view = project(&[e]);
        assert_eq!(view.messages.len(), 1);
        assert_eq!(view.messages[0].kind_label, "unrecognized");
        assert!(view.messages[0].body.contains("edit_by_hash"));
    }

    #[test]
    fn roster_includes_authors_and_addressees() {
        let view = project(&[
            ev(
                Actor::Human,
                Some("orch"),
                Kind::Message { body: "go".into() },
            ),
            ev(
                Actor::Quark(QuarkId::new("orch")),
                Some("worker"),
                Kind::Message {
                    body: "@worker do it".into(),
                },
            ),
        ]);
        let ids: Vec<&str> = view.roster.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&"orch"));
        assert!(ids.contains(&"worker"));
        // human is not a quark → not on the roster.
        assert!(!ids.contains(&"human"));
    }
}
