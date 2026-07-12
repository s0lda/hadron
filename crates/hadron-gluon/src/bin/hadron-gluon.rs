//! The gluon daemon — mock mode.
//!
//! Watches a `field.jsonl`, and whenever a human message addresses a quark
//! (`@claude …`, `@agy …`), excites that quark and appends its reply — the same
//! coordination loop the real adapters will drive, but with deterministic
//! **mock quarks** that echo the task back. Zero-spend: no CLI is ever invoked.
//!
//! Run this beside `hadron-chamber <field>` to see the two-process architecture
//! live: type `@claude build the login page` in the chamber and watch the reply
//! appear via the chamber's field tail.
//!
//! ## Not yet here (deliberately)
//! Real-adapter mode — `adapter::registry::build` per configured quark over a
//! `ProcessRunner`, `nucleus::load`/`digest`, `Engine::with_git(repo)` — is the
//! glue described in Plan 3's notes. It invokes real CLIs (real budget), so it
//! is held for a human-present session (Plan 3 Task 6). One open decision when
//! it lands: whether an excite error aborts the human turn or appends a gluon
//! error message and quiesces (see the plan's watch-items).

use std::path::{Path, PathBuf};
use std::time::Duration;

use async_trait::async_trait;
use hadron_gluon::adapter::registry;
use hadron_gluon::engine::Engine;
use hadron_gluon::field::read_events;
use hadron_gluon::quark::Quark;
use hadron_lattice::{
    load_team, team_config_path, EnergyState, Flavor, Projection, QuarkId, Team, TurnOutcome,
};

/// A deterministic stand-in for a real adapter: it acknowledges whatever task it
/// was handed, labelled so the reply is unmistakably a mock. It never errors and
/// never emits an `@mention`, so a burst always quiesces (bounded anyway by the
/// engine's per-turn exchange budget).
struct DemoQuark {
    id: QuarkId,
    flavor: Flavor,
    /// Human-facing name used to sign the reply (e.g. "Claude").
    label: String,
}

impl DemoQuark {
    fn new(id: &str, flavor: Flavor, label: &str) -> Self {
        DemoQuark { id: QuarkId::new(id), flavor, label: label.to_string() }
    }
}

#[async_trait]
impl Quark for DemoQuark {
    fn id(&self) -> QuarkId {
        self.id.clone()
    }
    fn flavor(&self) -> Flavor {
        self.flavor.clone()
    }
    fn energy(&self) -> EnergyState {
        EnergyState::Available
    }
    async fn excite(&mut self, turn: Projection) -> anyhow::Result<TurnOutcome> {
        let task = turn.task.trim();
        let body = if task.is_empty() {
            format!("[{}] standing by — what should I work on?", self.label)
        } else {
            format!("[{}] acknowledged: \"{task}\" (mock reply — no real work performed)", self.label)
        };
        Ok(TurnOutcome { message: Some(body), used_tokens: 0, permission: None })
    }
}

/// Parsed command line: the field path, poll interval, and optional team path.
struct Args {
    field_path: PathBuf,
    interval: Duration,
    team_path: Option<PathBuf>,
}

fn parse_args() -> Option<Args> {
    let mut field_path: Option<PathBuf> = None;
    let mut interval_ms: u64 = 400;
    let mut team_path: Option<PathBuf> = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--interval-ms" => interval_ms = it.next()?.parse().ok()?,
            "--team" => team_path = Some(PathBuf::from(it.next()?)),
            "-h" | "--help" => return None,
            other if !other.starts_with('-') => field_path = Some(PathBuf::from(other)),
            _ => return None,
        }
    }
    Some(Args {
        field_path: field_path?,
        interval: Duration::from_millis(interval_ms),
        team_path,
    })
}

/// Resolve which `team.json` to load, in priority order:
/// 1. an explicit `--team <path>`;
/// 2. a `team.json` sitting **next to the field** — the per-project
///    `.hadron/team.json` convention (field at `.hadron/field.jsonl` → team at
///    `.hadron/team.json`), so opening a project's field just works;
/// 3. the user-level default (`~/.hadron/team.json`).
///
/// `None` → no team file found → mock quarks.
fn resolve_team_path(explicit: Option<PathBuf>, field_path: &Path) -> Option<PathBuf> {
    if explicit.is_some() {
        return explicit;
    }
    if let Some(sibling) = field_path.parent().map(|d| d.join("team.json")) {
        if sibling.exists() {
            return Some(sibling);
        }
    }
    team_config_path().filter(|p| p.exists())
}

/// Seat the quarks: real adapters from `team.json` when present, else the
/// deterministic mock pair (zero-spend). Returns the quarks and a mode label.
fn seat_quarks(team: &Team) -> (Vec<Box<dyn Quark>>, &'static str) {
    if team.is_empty() {
        eprintln!(
            "  ⚠ no usable team.json — running MOCK quarks: only '@claude' and '@agy' exist \
             and their replies are fake. Add a team.json next to the field \
             (e.g. .hadron/team.json) or pass --team to seat real CLI quarks."
        );
        let quarks: Vec<Box<dyn Quark>> = vec![
            Box::new(DemoQuark::new("claude", Flavor::Orchestrator, "Claude")),
            Box::new(DemoQuark::new("agy", Flavor::Worker, "Antigravity")),
        ];
        return (quarks, "mock mode");
    }
    let mut quarks: Vec<Box<dyn Quark>> = Vec::new();
    for seat in &team.quarks {
        match registry::build_seat(seat) {
            Ok(q) => {
                eprintln!(
                    "  seated {} — {} · {} ({:?})",
                    seat.id.as_str(),
                    seat.provider,
                    seat.model,
                    seat.flavor
                );
                quarks.push(q);
            }
            Err(e) => eprintln!("  skipped {}: {e:#}", seat.id.as_str()),
        }
    }
    (quarks, "live mode (real CLIs — real budget)")
}

#[tokio::main]
async fn main() {
    let Some(args) = parse_args() else {
        eprintln!("usage: hadron-gluon <field.jsonl> [--interval-ms N] [--team team.json]");
        eprintln!("  Team resolution: --team, else a team.json next to the field");
        eprintln!("  (e.g. .hadron/team.json), else ~/.hadron/team.json.");
        eprintln!("  With none, runs deterministic mock quarks (ids: claude, agy).");
        std::process::exit(2);
    };

    // Seat the team: explicit --team, else a sibling team.json next to the field
    // (the .hadron/ convention), else the config-dir default; mock when none.
    let team_path = resolve_team_path(args.team_path.clone(), &args.field_path);
    match &team_path {
        Some(p) => eprintln!("hadron-gluon: team from {}", p.display()),
        None => eprintln!("hadron-gluon: no team.json found (looked next to the field, then the config dir)"),
    }
    let team = team_path.as_deref().map(load_team).unwrap_or_default();
    let (quarks, mode_label) = seat_quarks(&team);
    if quarks.is_empty() {
        eprintln!("hadron-gluon: team.json had no usable quarks; nothing to run.");
        std::process::exit(2);
    }
    let mut engine = Engine::new(args.field_path.clone(), quarks, 12);

    eprintln!(
        "hadron-gluon ({mode_label}) watching {}",
        args.field_path.display()
    );
    eprintln!("  address quarks from the chamber: '@<id> …'. Ctrl-C to stop.");

    loop {
        let before = read_events(&args.field_path).map(|e| e.len()).unwrap_or(0);
        // Mock quarks never error; a real daemon would decide abort-vs-continue here.
        if let Err(e) = engine.run_until_quiesce().await {
            eprintln!("gluon: excite error (continuing): {e:#}");
        }
        let after = read_events(&args.field_path).map(|e| e.len()).unwrap_or(0);
        if after > before {
            eprintln!("gluon: appended {} event(s) → {after} total", after - before);
        }
        tokio::time::sleep(args.interval).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn explicit_team_wins() {
        let dir = tempdir().unwrap();
        let field = dir.path().join(".hadron").join("field.jsonl");
        let explicit = dir.path().join("custom-team.json");
        assert_eq!(
            resolve_team_path(Some(explicit.clone()), &field),
            Some(explicit)
        );
    }

    #[test]
    fn discovers_team_json_next_to_the_field() {
        // The .hadron/ convention: team.json sits beside the field.
        let dir = tempdir().unwrap();
        let hadron = dir.path().join(".hadron");
        std::fs::create_dir_all(&hadron).unwrap();
        let field = hadron.join("field.jsonl");
        let sibling = hadron.join("team.json");
        std::fs::write(&sibling, "{}").unwrap();
        assert_eq!(resolve_team_path(None, &field), Some(sibling));
    }

    #[test]
    fn no_sibling_and_no_config_falls_through_to_none() {
        // No team.json beside the field. (The config-dir default is env-dependent;
        // in a clean temp field dir with no sibling, discovery must not invent one.)
        let dir = tempdir().unwrap();
        let field = dir.path().join(".hadron").join("field.jsonl");
        let resolved = resolve_team_path(None, &field);
        // Either None (no config file) or the real config path — never the sibling.
        if let Some(p) = resolved {
            assert_ne!(p, field.parent().unwrap().join("team.json"));
        }
    }
}
