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
use hadron_gluon::reseat;
use hadron_lattice::secrets::SecretStore;
use hadron_lattice::{
    load_team, orphan_overrides, parse_team, resolve_team, team_config_path, EnergyState, Flavor,
    Projection, QuarkId, Team,
    TurnOutcome,
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
        Ok(TurnOutcome { message: Some(body), permission: None, usage: Default::default() })
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
///
/// `store` resolves each seat's `secret_env` names to values. // TODO(next task):
/// swap `MemoryStore` for a real `KeyringStore` at the call site once one exists.
fn seat_quarks(team: &Team, live_dir: &Path, store: &dyn SecretStore) -> (Vec<Box<dyn Quark>>, &'static str) {
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
        match registry::build_seat_watched(seat, live_dir, store) {
            Ok(q) => {
                eprintln!(
                    "  seated {} — {} · {} ({:?})",
                    seat.id.as_str(),
                    seat.vendor,
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

/// Apply a [`reseat::ReseatPlan`] to the live engine, and return the team that is
/// **actually seated** afterwards.
///
/// The return value is the seated truth, not the requested one: a seat whose adapter
/// fails to build (an unknown provider, an ACP seat with no boot command) is reported
/// and left out, so the daemon's idea of the running team never claims a quark that
/// isn't there. If it did, the seat would never be retried and `@its-id` would resolve
/// to nobody — the exact failure this whole mechanism exists to end.
fn apply_reseat(
    engine: &mut Engine,
    running: &Team,
    plan: &reseat::ReseatPlan,
    live_dir: &Path,
    store: &dyn SecretStore,
) -> Team {
    let mut out = Team::default();

    // Everything the plan does not mention keeps its exact quark instance — and, for an
    // ACP seat, its live session. This is the whole point of reconciling.
    for seat in &running.quarks {
        let touched =
            plan.removed.contains(&seat.id) || plan.replaced.iter().any(|r| r.id == seat.id);
        if !touched {
            // A toggled seat is NOT rebuilt — it keeps its exact instance, and for an
            // ACP seat that means its live subprocess and its conversation survive. The
            // only thing that changes is the flag, here and in the engine. Its new
            // `enabled` must be recorded in the seated truth too, or the next tick would
            // diff the same difference and toggle it forever.
            let mut seat = seat.clone();
            if let Some((_, on)) = plan.toggled.iter().find(|(id, _)| id == &seat.id) {
                seat.enabled = *on;
            }
            out.quarks.push(seat);
        }
    }

    for (id, on) in &plan.toggled {
        engine.set_enabled(id, *on);
        eprintln!("  {} {}", id.as_str(), if *on { "ENABLED" } else { "DISABLED (instance kept, session intact)" });
    }

    // A rename is metadata: update the roster card the router matches, keep the instance
    // (and any live ACP session). `out` was populated from the running seats above with
    // their OLD names, so correct it here too, or the next tick would diff the same rename.
    for (id, name) in &plan.renamed {
        engine.rename(id, name.clone());
        if let Some(seat) = out.quarks.iter_mut().find(|s| &s.id == id) {
            seat.display_name = name.clone();
        }
        eprintln!(
            "  {} renamed to '{}' (instance kept, session intact)",
            id.as_str(),
            name.as_deref().unwrap_or("<id>")
        );
    }

    for id in &plan.removed {
        if engine.unseat(id) {
            eprintln!("  unseated {}", id.as_str());
        }
    }

    for seat in plan.added.iter().chain(plan.replaced.iter()) {
        match registry::build_seat_watched(seat, live_dir, store) {
            Ok(q) => {
                engine.seat(q);
                // Set participation explicitly, both ways. A seat added or re-pointed
                // while switched off must not start answering; and one switched back on
                // must not inherit a stale disabled flag from the id it replaced.
                engine.set_enabled(&seat.id, seat.enabled);
                out.quarks.push(seat.clone());
                eprintln!(
                    "  seated {} — {} · {} ({:?})",
                    seat.id.as_str(),
                    seat.vendor,
                    seat.model,
                    seat.flavor
                );
            }
            Err(e) => {
                // Not fatal, and deliberately not silent: the swarm keeps running with
                // the seats it has, and the human is told which one did not take.
                eprintln!("  ⚠ could not seat {}: {e:#}", seat.id.as_str());
                // A *replacement* that failed to build never displaced anything — the
                // old quark is still seated and still answering. Record the OLD seat, or
                // the daemon's picture of the swarm would disagree with the swarm.
                if let Some(old) = running.get(&seat.id) {
                    eprintln!("    → {} keeps its previous seat", seat.id.as_str());
                    out.quarks.push(old.clone());
                }
            }
        }
    }
    out
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

    let lock_path = args.field_path.parent().unwrap_or(std::path::Path::new(".")).join("gluon.lock");
    let lock_file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            eprintln!("hadron-gluon: failed to open lock file: {}", e);
            std::process::exit(1);
        }
    };

    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        let lock_res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if lock_res != 0 {
            eprintln!("hadron-gluon: another instance of gluon is already running.");
            std::process::exit(1);
        }
    }

    // Seat the team: explicit --team, else a sibling team.json next to the field
    // (the .hadron/ convention), else the config-dir default; mock when none.
    let team_path = resolve_team_path(args.team_path.clone(), &args.field_path);
    match &team_path {
        Some(p) => eprintln!("hadron-gluon: team from {}", p.display()),
        None => eprintln!("hadron-gluon: no team.json found (looked next to the field, then the config dir)"),
    }
    // The global catalogue holds the quark *definitions* a repo's role/state overrides
    // resolve against. Skip it when it IS the repo file (a bare ~/.hadron/team.json used
    // directly as the team) — there is nothing to resolve against itself.
    let global_path =
        team_config_path().filter(|g| Some(g.as_path()) != team_path.as_deref());
    if let Some(g) = &global_path {
        eprintln!("hadron-gluon: catalogue from {}", g.display());
    }
    let team = load_resolved_team(team_path.as_deref(), global_path.as_deref());

    // Where quarks publish what they are doing mid-turn, for the chamber to render.
    // Derived from the field path, so both processes agree without a second setting.
    //
    // A daemon killed mid-turn leaves its quarks' activity behind. `live::read`
    // already refuses to believe a stale file, but sweeping on boot means the
    // chamber does not show a ghost for the two minutes it takes to go stale.
    let live_dir = hadron_lattice::live::live_dir(&args.field_path);
    for seat in &team.quarks {
        let _ = hadron_lattice::live::clear(&live_dir, &seat.id);
    }

    // Real seats get real keys: `KeyringStore` resolves each seat's
    // `secret_env` names against the OS credential store (Keychain /
    // Credential Manager / Secret Service). team.json only ever holds names;
    // values live only in the keychain. See `hadron_gluon::secrets` and the
    // Security note in the encrypted-secrets design doc.
    let secret_store = hadron_gluon::secrets::KeyringStore::new();

    let (quarks, mode_label) = seat_quarks(&team, &live_dir, &secret_store);
    if quarks.is_empty() {
        eprintln!("hadron-gluon: team.json had no usable quarks; nothing to run.");
        std::process::exit(2);
    }
    let max_exchanges = team.max_exchanges.unwrap_or(12);
    let repo_root = std::fs::canonicalize(hadron_lattice::repo_root_of(&args.field_path))
        .unwrap_or_else(|_| hadron_lattice::repo_root_of(&args.field_path).to_path_buf());
    let engine = Engine::new(args.field_path.clone(), quarks, max_exchanges)
        .with_git(repo_root)
        .with_merge_gate(std::sync::Arc::new(hadron_gluon::merge::CargoMergeRunner))
        // `Engine::new` defaults this to `None` (hermetic — see the field doc), so the
        // real daemon must opt in explicitly or custom global skills under
        // `~/.hadron/skills` would silently never load in production.
        .with_global_skills_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("skills")))
        // Same seam, same reason: without this the real daemon would never look in
        // `~/.hadron/agents`, so `@persona-name` mentions could only ever resolve
        // via the repo half.
        .with_global_agents_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("agents")));

    // Security: We open the energy ledger database `ledger.db` under the parent directory
    // of `field_path` (which resides inside the repo's `.hadron/` directory in typical usage).
    // The path is derived using standard library functions to prevent traversal or access
    // to untrusted locations.
    let ledger_path = args.field_path.parent().unwrap_or(std::path::Path::new(".")).join("ledger.db");
    let ledger = match hadron_gluon::ledger::Ledger::open(&ledger_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("hadron-gluon: failed to open ledger at {}: {e:#}", ledger_path.display());
            std::process::exit(2);
        }
    };
    let global_limit = 500_000u32;
    let mut engine = engine.with_ledger(ledger, global_limit);
    // A seat can boot switched OFF. It is still seated (still addressable, still owns
    // its instance) — it just does not take turns until the human enables it.
    for seat in &team.quarks {
        if !seat.enabled {
            engine.set_enabled(&seat.id, false);
            eprintln!("  {} is seated but DISABLED — it will not take turns", seat.id.as_str());
        }
    }

    eprintln!(
        "hadron-gluon ({mode_label}) watching {}",
        args.field_path.display()
    );
    eprintln!("  address quarks from the chamber: '@<id> …'. Ctrl-C to stop.");
    // DO-NOT-ACTIVATE toggle (spec §2 D): read once from HADRON_NO_HUMAN_MODE inside
    // `Engine::new`. Loud on purpose — a mode where the orchestrator, not a human,
    // adjudicates permission asks under global Bypass must never be silently on.
    if engine.no_human() {
        eprintln!(
            "  ⚠️  HADRON_NO_HUMAN_MODE is ON — under global Bypass, permission asks that would \
             stop for a human are instead adjudicated by the orchestrator. A human deny-list \
             entry remains absolute (never orchestrator-overridable)."
        );
    }

    // The team the engine is actually running, and the exact bytes we last read from
    // `team.json`. Change is detected on the *bytes*, not on the mtime: a coarse
    // filesystem clock can hide a fast save, and the file is tiny enough that reading it
    // once per interval costs nothing.
    // True when the roster is the DemoQuark pair rather than anything from a file.
    let mut mock_mode = team.is_empty();
    let mut running_team = team;
    let mut last_seen_repo: Option<String> = team_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok());
    let mut last_seen_global: Option<String> = global_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok());

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

        // ---- THE SAFE POINT ---------------------------------------------------
        //
        // `run_until_quiesce` has returned, and it only returns when the field has no
        // pending work AND its `JoinSet` is empty — every turn it spawned has been
        // *joined*, not merely abandoned. So right here, and nowhere else, there is
        // provably no quark mid-turn and no turn task holding a clone of a quark's
        // `Arc`. Re-seating anywhere else could tear an agent out from under a running
        // turn; that is why `Engine::seat`/`unseat` take `&mut self`, which the borrow
        // checker will not hand us while a turn is in flight.
        // Watch BOTH the repo team.json and the global catalogue: a definition edited
        // in the catalogue (e.g. a model change for an already-adopted quark) must
        // reconcile even when the repo file is untouched. Reconcile when EITHER file's
        // bytes change. `poll_team_file` updates `last_seen` on every change, so an
        // unchanged file's `last_seen` still holds its current content to parse against.
        let repo_changed = team_path
            .as_deref()
            .and_then(|p| poll_team_file(p, &mut last_seen_repo));
        let global_changed = global_path
            .as_deref()
            .and_then(|p| poll_team_file(p, &mut last_seen_global));
        if repo_changed.is_some() || global_changed.is_some() {
            let repo_parsed = last_seen_repo.as_deref().map(parse_team).transpose();
            let global_parsed = last_seen_global.as_deref().map(parse_team).transpose();
            match (repo_parsed, global_parsed) {
                // A file caught mid-write parses as garbage on EITHER side. Keep the
                // swarm exactly as it is — treating an unparseable file as empty would
                // unseat everybody.
                (Err(e), _) | (_, Err(e)) => eprintln!(
                    "gluon: a team file changed but does not parse — keeping the running roster: {e}"
                ),
                (Ok(repo), Ok(global)) => {
                    let repo = repo.unwrap_or_default();
                    let global = global.unwrap_or_default();
                    for id in orphan_overrides(&repo, &global) {
                        eprintln!(
                            "  ⚠ repo override {} names no catalogue seat — ignored",
                            id.as_str()
                        );
                    }
                    let desired = resolve_team(&repo, &global);
                    // A team with nobody in it is never applied. It is almost certainly a
                    // mistake, and obeying it would leave a swarm the human cannot talk to.
                    // The guard is on the RESOLVED team, so an empty result from a bad
                    // merge (e.g. all-orphan overrides) is caught too.
                    if desired.is_empty() {
                        eprintln!(
                            "gluon: team now seats nobody — keeping the running roster ({} quark(s))",
                            engine.seated_count()
                        );
                    } else {
                        let max_exchanges = desired.max_exchanges.unwrap_or(12);
                        engine.set_max_exchanges(max_exchanges);
                        // Booted with no usable team.json ⇒ the roster is the DemoQuark
                        // pair. Those answer to no `Seat`, so no team-vs-team diff can
                        // see them: evict them by hand the first time a real team lands,
                        // or a fake '@claude' outlives the real one forever.
                        if mock_mode {
                            for id in engine.seated_ids() {
                                if desired.get(&id).is_none() && engine.unseat(&id) {
                                    eprintln!("  unseated mock {}", id.as_str());
                                }
                            }
                            mock_mode = false;
                        }
                        let plan = reseat::plan(&running_team, &desired);
                        if !plan.is_empty() {
                            eprintln!("gluon: team changed — re-seating [{}]", plan.summary());
                            running_team =
                                apply_reseat(&mut engine, &running_team, &plan, &live_dir, &secret_store);
                            eprintln!("gluon: roster is now {} quark(s)", engine.seated_count());
                        }
                    }
                }
            }
        }

        tokio::time::sleep(args.interval).await;
    }
}

/// Read `team.json` and return its text **only if it differs from the last read**.
///
/// `last_seen` is updated on every successful read, good content or bad. That is
/// deliberate: if it tracked the last *valid* content instead, a file left malformed
/// would be re-read, re-rejected and re-logged on every tick, forever. The human gets
/// one warning per change, which is what a warning is worth.
/// Load the repo team and the global catalogue and fold them with [`resolve_team`]
/// into the concrete team to seat. Warns about any override that names no catalogue
/// seat — it is dropped and never seated, which is what keeps a not-adopted quark out
/// of the running swarm. A missing/malformed file degrades to empty, like [`load_team`].
fn load_resolved_team(repo_path: Option<&Path>, global_path: Option<&Path>) -> Team {
    let repo = repo_path.map(load_team).unwrap_or_default();
    let global = global_path.map(load_team).unwrap_or_default();
    for id in orphan_overrides(&repo, &global) {
        eprintln!(
            "  ⚠ repo override {} names no catalogue seat — ignored (not seated)",
            id.as_str()
        );
    }
    resolve_team(&repo, &global)
}

fn poll_team_file(path: &Path, last_seen: &mut Option<String>) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    if last_seen.as_deref() == Some(text.as_str()) {
        return None;
    }
    *last_seen = Some(text.clone());
    Some(text)
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

    /// Change detection is on the bytes, and it fires **once** per change.
    #[test]
    fn the_team_file_is_only_reported_when_it_actually_changes() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        std::fs::write(&path, "{\"quarks\":[]}").unwrap();

        let mut last_seen = None;
        assert!(poll_team_file(&path, &mut last_seen).is_some(), "first read is a change");
        assert!(poll_team_file(&path, &mut last_seen).is_none(), "unchanged file must be silent");

        std::fs::write(&path, "{\"quarks\":[],\"x\":1}").unwrap();
        assert!(poll_team_file(&path, &mut last_seen).is_some(), "a real edit must be seen");
        assert!(poll_team_file(&path, &mut last_seen).is_none());
    }

    /// A malformed file must not re-warn on every tick — `last_seen` tracks the last
    /// bytes *read*, not the last bytes that parsed.
    #[test]
    fn a_file_left_malformed_is_reported_once_not_forever() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("team.json");
        std::fs::write(&path, "{ this is not json").unwrap();

        let mut last_seen = None;
        let text = poll_team_file(&path, &mut last_seen).expect("the change is seen");
        assert!(parse_team(&text).is_err(), "and it is rejected, not read as an empty team");
        assert!(
            poll_team_file(&path, &mut last_seen).is_none(),
            "the same broken bytes must not be re-reported every interval"
        );
    }

    /// The torn-write guard, stated as the property that matters: a half-written
    /// `team.json` is an ERROR, never an empty team. If it parsed as empty, the daemon
    /// would unseat the entire swarm mid-save.
    #[test]
    fn a_truncated_team_file_is_an_error_and_not_an_empty_team() {
        let full = "{\"quarks\":[{\"id\":\"opus\",\"provider\":\"claude\",\"model\":\"opus\",\"flavor\":\"orchestrator\"}]}";
        let torn = &full[..full.len() / 2];
        assert!(parse_team(torn).is_err(), "a torn read must not be mistaken for 'nobody is seated'");
        assert_eq!(parse_team(full).unwrap().quarks.len(), 1);
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
