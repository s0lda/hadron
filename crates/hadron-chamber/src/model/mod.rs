//! The chamber view-model: a pure projection of the field (`Vec<Event>`) into
//! what the UI renders — a chat row per event plus the derived per-quark roster
//! state. No GPUI here, so this is fully unit-tested.

use std::collections::HashMap;

use chrono::{DateTime, NaiveDate, TimeZone};
use hadron_gatekeeper::{global_mode, has_override, resolve_mode, Mode};
use hadron_lattice::{Actor, Event, Kind, QuarkId, QuarkState, Team};

mod stats;
pub mod tasks;
#[cfg(test)]
mod tests;

pub use tasks::{SwarmTask, TaskState};

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
    pub turn: Option<String>,
    pub severity: Option<hadron_lattice::Severity>,
}

impl MessageRow {
    /// Does the Chat tab show this row? The Log tab shows every projected row; Chat
    /// shows only the ones a human wrote or read. The single definition of that split —
    /// it used to be the literal `m.kind_label == "message"` copied into five places.
    pub fn is_chat(&self) -> bool {
        self.kind_label == "message"
    }
}

/// The indices into `messages` that the virtualized chat list addresses.
///
/// The chat is a `gpui::list` over a SUBSET of the projected rows, so it carries this
/// index list plus a `ListState` whose count must equal its length. Both are caches of
/// `view.messages` and every rebuild of the projection has to rebuild them too — the
/// `/resume` handler reprojected a whole archived session and left the index list
/// cleared, so the chat stayed empty until the next message rebuilt it.
pub fn chat_message_indices(messages: &[MessageRow]) -> Vec<usize> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(ix, m)| m.is_chat().then_some(ix))
        .collect()
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

/// The fresh (input+output, cache-excluded) tokens one message represents, or `None` if
/// it carries no spend. The seat's `transport` disambiguates a legacy bare `used_tokens`
/// (a CLI seat's is fresh; an ACP seat's was a cache-inclusive total we cannot split).
/// Shared by [`ChamberView::fold_stats`] and [`ChamberView::spend_timeline`] so both read
/// spend identically.
fn message_fresh(m: &MessageRow, transport: hadron_lattice::Transport) -> Option<u32> {
    if let Some(u) = &m.usage {
        u.spend.fresh()
    } else if let Some(used) = m.legacy_used_tokens {
        resolve_fresh(None, used, transport).ok()
    } else {
        None
    }
}

/// One roster entry: a quark, its latest lifecycle state, its effective
/// permission mode (and whether that's an explicit per-quark override vs the
/// inherited global), and its legibility (`vendor`/`model` from the team
/// config — empty strings when the seat isn't in `team.json`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RosterRow {
    pub id: String,
    pub display_name: Option<String>,
    pub state: QuarkState,
    pub mode: Mode,
    pub mode_is_override: bool,
    pub vendor: String,
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

impl RosterRow {
    /// What to display for this seat's model.
    ///
    /// An empty `model` on a **CLI** seat is not "unknown" — it means the seat passes
    /// no `--model`, so the tool's own config decides (for `agy`, that is
    /// `~/.gemini/antigravity-cli/settings.json`). Hadron cannot read the value back:
    /// `agy` writes the model it actually used only to its private per-invocation log,
    /// never to stdout/stderr, so the CLI adapter has nothing to capture. Saying
    /// "CLI default" states the true situation; a dash reads as broken.
    ///
    /// Still empty for a non-CLI seat with no configured model — callers keep their own
    /// placeholder for that. SSOT for the rule, so the roster row and the Stats table
    /// cannot drift.
    pub fn model_label(&self) -> String {
        if !self.model.is_empty() {
            self.model.clone()
        } else if self.transport == hadron_lattice::Transport::Cli {
            "CLI default".to_string()
        } else {
            String::new()
        }
    }
}

/// The global `ModeSet` `/clear` must append to re-arm the human's standing
/// permission mode, or `None` when the fresh field already resolves to it.
///
/// The effective mode is folded from the field's `ModeSet` events
/// (`hadron_gatekeeper::global_mode`), and `/clear` truncates the field — so
/// every mode the human had set went with it and each new session silently reopened
/// on `Mode::Ask`. This is the re-seed.
///
/// `None` for `Mode::default()` on purpose: an empty field ALREADY resolves to that,
/// so seeding it would write a row that changes nothing and would put a second
/// definition of the floor next to `Mode::default()`. Same pure-and-tested shape as
/// [`post_clear_reboots`], so the GPUI command handler stays a thin caller.
pub fn default_mode_seed(default_mode: Mode) -> Option<Event> {
    (default_mode != Mode::default())
        .then(|| Event::new(Actor::Human, None, Kind::ModeSet { mode: default_mode }))
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

/// Metadata about one archived session: its directory id (the sortable
/// `YYYYMMDD_HHMMSS` timestamp `/clear` names it with) and, if `/rename` was ever
/// run in it, the name it was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionInfo {
    pub id: String,
    pub name: Option<String>,
}

impl SessionInfo {
    /// What a human should see for this session: the name `/rename` gave it, or the
    /// `YYYYMMDD_HHMMSS` directory id rewritten as a date. An id that does not have that
    /// shape (nothing writes one today, but nothing stops a human creating one either) is
    /// returned verbatim — a half-parsed timestamp would read as a confidently wrong date.
    pub fn label(&self) -> String {
        if let Some(name) = &self.name {
            return name.clone();
        }
        match chrono::NaiveDateTime::parse_from_str(&self.id, "%Y%m%d_%H%M%S") {
            Ok(ts) => ts.format("%Y-%m-%d %H:%M").to_string(),
            Err(_) => self.id.clone(),
        }
    }
}

/// List archived sessions under `sessions_dir`, oldest first (directory names are
/// timestamp-sortable, same order `load_archived_messages` folds them in). A
/// session's name is the LAST `Kind::SessionName` event in its field — `/rename`
/// can run more than once, and the latest one is what a human should see, the same
/// "latest wins" rule `project_with_team` already applies to status/mode. A missing
/// directory or an unreadable session field yields no rows, not an error.
pub fn list_sessions(sessions_dir: &std::path::Path) -> Vec<SessionInfo> {
    let mut dirs: Vec<std::path::PathBuf> = match std::fs::read_dir(sessions_dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => return Vec::new(),
    };
    dirs.sort();

    dirs.into_iter()
        .map(|dir| {
            let id = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let events = hadron_lattice::io::read_events(&dir.join("field.jsonl")).unwrap_or_default();
            let name = events.iter().rev().find_map(|e| match &e.kind {
                Kind::SessionName { name } => Some(name.clone()),
                _ => None,
            });
            SessionInfo { id, name }
        })
        .collect()
}

/// Resolve a `/resume` target against the session list: the id (the timestamp
/// directory name, always unique) matches first, then a case-insensitive match on
/// the name set by `/rename` — a human retyping a name they saw on screen should
/// not have to match its case exactly.
pub fn find_session<'a>(sessions: &'a [SessionInfo], target: &str) -> Option<&'a SessionInfo> {
    sessions
        .iter()
        .find(|s| s.id == target)
        .or_else(|| sessions.iter().find(|s| s.name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(target))))
}

/// Whether any roster quark is mid-turn right now. `/resume` swaps out
/// `.hadron/field.jsonl` — the live bus a running daemon appends to — so it must
/// refuse while a quark could still be writing to it.
pub fn any_quark_mid_turn(roster: &[RosterRow]) -> bool {
    roster.iter().any(|r| matches!(r.state, QuarkState::Excited | QuarkState::Thinking))
}

/// One row in the Process Manager overlay: the daemon, or one adopted quark seat,
/// with a human status line and which of the codebase's *existing* control
/// mechanisms apply (`Kind::Reboot` for a resident ACP seat, the `Seat.enabled`
/// toggle for any adopted seat) — not a fabricated OS-level kill switch, which
/// this view has no wiring for (the chamber never holds a quark's child PID; the
/// daemon does).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessRow {
    pub id: String,
    pub label: String,
    pub status: String,
    /// Force-restart is meaningful only for a resident (ACP) seat that is
    /// currently enabled — a disabled or one-shot CLI seat holds nothing to reap.
    pub can_restart: bool,
    /// Whether this row's participation can be flipped at all (every adopted
    /// quark; not the daemon, which the chamber has no start/stop path for).
    pub can_toggle: bool,
    pub enabled: bool,
}

/// Builds the Process Manager's row list: the `hadron-gluon` daemon first (its
/// running state is a live flock probe the caller supplies — only the OS knows),
/// then every *adopted* roster seat. A not-adopted quark holds no process at
/// all, so — unlike the roster pane, which greys it out — this omits it entirely.
pub fn build_process_rows(gluon_running: bool, roster: &[RosterRow]) -> Vec<ProcessRow> {
    let mut rows = vec![ProcessRow {
        id: "gluon".to_string(),
        label: "hadron-gluon (daemon)".to_string(),
        status: if gluon_running { "Running".to_string() } else { "Stopped".to_string() },
        can_restart: false,
        can_toggle: false,
        enabled: gluon_running,
    }];
    rows.extend(roster.iter().filter(|r| r.adopted).map(|r| {
        let status = if !r.enabled {
            "Disabled".to_string()
        } else {
            match r.state {
                QuarkState::Excited => "Excited".to_string(),
                QuarkState::Thinking => "Thinking".to_string(),
                QuarkState::Waiting => "Waiting".to_string(),
                QuarkState::Blocked => "Blocked".to_string(),
                QuarkState::Error => "Error".to_string(),
                QuarkState::Ground => "Idle".to_string(),
            }
        };
        ProcessRow {
            id: r.id.clone(),
            label: r.display_name.clone().unwrap_or_else(|| r.id.clone()),
            status,
            can_restart: r.enabled && matches!(r.transport, hadron_lattice::Transport::Acp),
            can_toggle: true,
            enabled: r.enabled,
        }
    }));
    rows
}

/// What one quark spent this session. `context` and `quota` are the *latest*
/// the provider reported, not a sum: occupancy and remaining allowance are
/// levels, and adding them up would mean nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct TurnSpend {
    pub turn: u64,
    pub fresh: u32,
    pub cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct QuarkStats {
    pub turns: u64,
    pub fresh: u64,
    pub cached: u64,
    pub cost_usd: Option<f64>,
    pub context: Option<hadron_lattice::ContextUsage>,
    pub quota: Vec<hadron_lattice::QuotaBucket>,
    pub first_seen: Option<chrono::DateTime<chrono::Utc>>,
    pub last_active: Option<chrono::DateTime<chrono::Utc>>,
    pub spend_history: Vec<TurnSpend>,
}

/// A time window over the swarm's telemetry. `Session` is bounded by *source* — the
/// live field only, i.e. the current post-`/clear` session — while the wider windows
/// fold archived sessions in and bound by *time* ([`StatsWindow::cutoff`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsWindow {
    /// The current run: since the last human message. No archives, no time bound.
    Current,
    /// The live field: this session since the last `/clear`. No archives, no time bound.
    Session,
    /// Rolling `now − 7 days`, across the live field and archived sessions.
    Week,
    /// Rolling `now − 30 days`, across the live field and archived sessions.
    Month,
    /// Everything, live field and every archived session, no lower bound.
    AllTime,
}

impl StatsWindow {
    /// In tab order.
    pub const ALL: [StatsWindow; 5] = [
        StatsWindow::Current,
        StatsWindow::Session,
        StatsWindow::Week,
        StatsWindow::Month,
        StatsWindow::AllTime,
    ];

    pub fn label(self) -> &'static str {
        match self {
            StatsWindow::Current => "Current",
            StatsWindow::Session => "Session",
            StatsWindow::Week => "Week",
            StatsWindow::Month => "Month",
            StatsWindow::AllTime => "All time",
        }
    }

    /// The inclusive lower bound for an event's `ts`. `None` = no lower bound. `Session`/`Current`
    /// is `None` too: it is bounded by its source, not by a timestamp.
    pub fn cutoff(self, now: DateTime<chrono::Utc>) -> Option<DateTime<chrono::Utc>> {
        match self {
            StatsWindow::Current | StatsWindow::Session | StatsWindow::AllTime => None,
            StatsWindow::Week => Some(now - chrono::Duration::days(7)),
            StatsWindow::Month => Some(now - chrono::Duration::days(30)),
        }
    }

    /// Whether this window folds in archived sessions or reads the live field alone.
    pub fn includes_archives(self) -> bool {
        !matches!(self, StatsWindow::Current | StatsWindow::Session)
    }
}

/// Per-quark session statistics plus the totals that are meaningful to add.
/// Deliberately no total context or quota — see [`QuarkStats`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SessionStats {
    /// In roster order, so the tab and the roster agree.
    pub per_quark: Vec<(String, QuarkStats)>,
    pub total_turns: u64,
    pub total_fresh: u64,
    pub total_cached: u64,
    pub total_cost_usd: Option<f64>,
    pub spend_history: Vec<TurnSpend>,
}

/// One point on the [`SpendTimeline`]: the **cumulative** fresh spend, per quark and in
/// total, as of one spend event. `per_quark` is aligned to [`SpendTimeline::quarks`].
#[derive(Debug, Clone, PartialEq)]
pub struct SpendPoint {
    /// 1-based chronological index of the spend event (the chart's x).
    pub step: u32,
    /// Running cumulative fresh spend per quark, aligned to [`SpendTimeline::quarks`].
    pub per_quark: Vec<f64>,
    /// Running cumulative fresh spend across the whole team (the sum).
    pub team: f64,
}

/// Cumulative fresh spend over turns, per quark and team, for the combined spend area
/// chart. Each quark is a series (`quarks[i]` ↔ `points[_].per_quark[i]`); the team total
/// is `points[_].team` and, being the sum, sits above every quark curve. Empty when
/// nothing spent in the window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpendTimeline {
    /// Series order: the quark ids that spent in the window (all-zero columns dropped).
    pub quarks: Vec<String>,
    pub points: Vec<SpendPoint>,
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
    /// The swarm's live task feed — see [`tasks::swarm_tasks`]. Newest-first.
    pub tasks: Vec<SwarmTask>,
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
            format!("permission requested ({risk:?}): {description}"),
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
        Kind::SessionName { name } => (format!("session renamed to \"{name}\""), "session_name"),
        Kind::Unknown { kind, .. } => (format!("unrecognized event: {kind}"), "unrecognized"),
    };
    let severity = e.severity.or_else(|| {
        if matches!(e.kind, Kind::PermissionReq { .. }) {
            Some(hadron_lattice::Severity::Warning)
        } else {
            None
        }
    });
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
        turn: e.turn.map(|t| t.to_string()),
        severity,
    }
}

/// Project the field into a renderable view, with no team annotations
/// (`provider`/`model` blank). Convenience for tests and callers without a team.
#[cfg_attr(not(test), allow(dead_code))]
pub fn project(events: &[Event]) -> ChamberView {
    project_with_team(events, &Team::default(), &Team::default())
}

/// Project every archived session under `sessions_dir` into one flat, chronological
/// `Vec<MessageRow>` for the wider [`StatsWindow`]s (`/clear` writes each cleared field
/// to `sessions/<timestamp>/field.jsonl`).
///
/// Each session is projected **independently** and its rows concatenated — turn numbers
/// are scoped per field, so projecting sessions separately keeps one session's turn
/// usage from colliding with another's. The session directories are timestamp-named, so
/// sorting their paths yields chronological order. A missing `sessions_dir` (nothing
/// cleared yet) or an unreadable session is not an error — it contributes no rows.
pub fn load_archived_messages(sessions_dir: &std::path::Path) -> Vec<MessageRow> {
    let mut dirs: Vec<std::path::PathBuf> = match std::fs::read_dir(sessions_dir) {
        Ok(rd) => rd
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect(),
        Err(_) => return Vec::new(),
    };
    dirs.sort();

    let mut out = Vec::new();
    for dir in dirs {
        let field = dir.join("field.jsonl");
        let events = hadron_lattice::io::read_events(&field).unwrap_or_default();
        out.extend(project(&events).messages);
    }
    out
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
            let (display_name, vendor, model, flavor, transport, effort, enabled, adopted) =
                match (team.get(&qid), global.get(&qid)) {
                    (Some(s), _) => (
                        s.display_name.clone(),
                        s.vendor.clone(),
                        s.model.clone(),
                        Some(s.flavor.clone()),
                        s.transport,
                        s.effort.clone(),
                        s.enabled,
                        true,
                    ),
                    (None, Some(g)) => (
                        g.display_name.clone(),
                        g.vendor.clone(),
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
                vendor,
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
        pending_permission: hadron_gatekeeper::any_pending_permission(events),
        tasks: tasks::swarm_tasks(events),
    }
}

// NOTE: the human's `@mention` routing lives in the daemon now
// (`hadron_gluon::router::human_mentions`), not the chamber. The chamber writes
// the raw message with `to: None` and leaves mentions in the body, so ONE message
// can address several quarks; the daemon resolves and fans them out. (A former
// `parse_mention` here lifted a single leading mention into `to` — which silently
// dropped the second addressee — and has been removed.)

