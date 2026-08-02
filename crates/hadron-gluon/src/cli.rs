//! The gluon daemon.
//!
//! Watches a `field.jsonl`, and whenever a human message addresses a quark
//! (`@acp-claude …`), excites that quark over its real adapter and appends the
//! reply. Every seat comes from a `team.json`: there is **no** stand-in quark and
//! no mock mode — a daemon that cannot seat a real swarm refuses to start rather
//! than answering a human with fabricated work.
//!
//! Run this beside `hadron-chamber <field>` to see the two-process architecture
//! live: type `@<quark> build the login page` in the chamber and watch the reply
//! appear via the chamber's field tail.

use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::adapter::registry;
use crate::engine::Engine;
use crate::field::read_events;
use crate::quark::Quark;
use crate::reseat;
use hadron_lattice::secrets::SecretStore;
use hadron_lattice::term::{self, Source};
use hadron_lattice::{
    load_team, orphan_overrides, parse_team, resolve_team, team_config_path, Actor, Event, Flavor, Kind,
    Team,
};

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
/// `None` → no team file found → the daemon refuses to start.
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

/// Seat the quarks: real adapters, one per seat in `team.json`. Returns the quarks,
/// a mode label, and one [`term::SeatRow`] per successfully built seat — the caller prints them as
/// a single table once startup knows each seat's final enabled/chat-lane state,
/// instead of this function printing per-seat as it builds.
///
/// An empty team seats **nobody** — there is no stand-in quark to fall back to,
/// whether or not a file was found. A fake quark answering a human is worse than a
/// daemon that refuses to start: on 2026-08-01 a daemon too old to parse a catalogue
/// carrying `"transport": "http"` degraded it to an empty catalogue, orphaned every
/// repo `roster` override, and seated a fake "@Claude" that acknowledged a real
/// instruction it never did. `team_path` only chooses which refusal to print.
///
/// `store` resolves each seat's `secret_env` names to values. // TODO(next task):
/// swap `MemoryStore` for a real `KeyringStore` at the call site once one exists.
fn seat_quarks(
    team: &Team,
    team_path: Option<&Path>,
    live_dir: &Path,
    store: &dyn SecretStore,
) -> (Vec<Box<dyn Quark>>, &'static str, Vec<term::SeatRow>) {
    if team.is_empty() {
        match team_path {
            Some(p) => term::error(
                Source::Gluon,
                &format!(
                    "{} was found but resolves to NO seats. Usually the catalogue \
                     (~/.hadron/team.json) failed to parse, so every roster override is an \
                     orphan (see the warnings above); a build older than the file that \
                     describes it does exactly that.",
                    p.display()
                ),
            ),
            None => term::error(
                Source::Gluon,
                "no team.json found. Add one next to the field (e.g. .hadron/team.json) \
                 or pass --team to seat real quarks.",
            ),
        }
        return (Vec::new(), "no usable team", Vec::new());
    }
    let mut quarks: Vec<Box<dyn Quark>> = Vec::new();
    let mut rows = Vec::new();
    for seat in &team.quarks {
        match registry::build_seat_watched(seat, live_dir, store) {
            Ok(q) => {
                rows.push(term::SeatRow {
                    id: seat.id.as_str().to_string(),
                    agent: seat.vendor.clone(),
                    model: seat.model.clone(),
                    flavor: format!("{:?}", seat.flavor),
                    enabled: seat.enabled,
                    chat_lane: false,
                });
                quarks.push(q);
            }
            Err(e) => term::warn(Source::Gluon, &format!("skipped {}: {e:#}", seat.id.as_str())),
        }
    }
    (quarks, "live mode (real CLIs — real budget)", rows)
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
        term::info(
            Source::Gluon,
            &format!("{} {}", id.as_str(), if *on { "ENABLED" } else { "DISABLED (instance kept, session intact)" }),
        );
    }

    // A rename is metadata: update the roster card the router matches, keep the instance
    // (and any live ACP session). `out` was populated from the running seats above with
    // their OLD names, so correct it here too, or the next tick would diff the same rename.
    for (id, name) in &plan.renamed {
        engine.rename(id, name.clone());
        if let Some(seat) = out.quarks.iter_mut().find(|s| &s.id == id) {
            seat.display_name = name.clone();
        }
        term::info(
            Source::Gluon,
            &format!(
                "{} renamed to '{}' (instance kept, session intact)",
                id.as_str(),
                name.as_deref().unwrap_or("<id>")
            ),
        );
    }

    for id in &plan.removed {
        if engine.unseat(id) {
            term::info(Source::Gluon, &format!("unseated {}", id.as_str()));
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
                if seat.flavor == Flavor::Orchestrator {
                    match registry::build_seat_watched(seat, live_dir, store) {
                        Ok(chat) => {
                            engine.seat_chat_lane(&seat.id, chat);
                        }
                        Err(e) => term::warn(
                            Source::Gluon,
                            &format!("{} — could not seat a chat lane on reseat: {e:#}", seat.id.as_str()),
                        ),
                    }
                }
                out.quarks.push(seat.clone());
                term::info(
                    Source::Gluon,
                    &format!(
                        "seated {} — {} · {} ({:?})",
                        seat.id.as_str(),
                        seat.vendor,
                        seat.model,
                        seat.flavor
                    ),
                );
            }
            Err(e) => {
                // Not fatal, and deliberately not silent: the swarm keeps running with
                // the seats it has, and the human is told which one did not take.
                term::warn(Source::Gluon, &format!("could not seat {}: {e:#}", seat.id.as_str()));
                // A *replacement* that failed to build never displaced anything — the
                // old quark is still seated and still answering. Record the OLD seat, or
                // the daemon's picture of the swarm would disagree with the swarm.
                if let Some(old) = running.get(&seat.id) {
                    term::info(Source::Gluon, &format!("  → {} keeps its previous seat", seat.id.as_str()));
                    out.quarks.push(old.clone());
                }
            }
        }
    }
    out
}

/// The `hadron-gluon` daemon entrypoint, in the library rather than in a `[[bin]]`,
/// so the single installable package (`hadron`) can carry a bin target that calls it.
/// The chamber finds the daemon as a sibling of its own `current_exe`
/// (`chamber/src/main.rs:211`), so all three binaries must install together.
pub async fn run() {
    let _shutdown_guard = crate::proc::ShutdownGuard::new();

    let Some(args) = parse_args() else {
        term::error(Source::Gluon, "usage: hadron-gluon <field.jsonl> [--interval-ms N] [--team team.json]");
        term::error(Source::Gluon, "Team resolution: --team, else a team.json next to the field");
        term::error(Source::Gluon, "(e.g. .hadron/team.json), else ~/.hadron/team.json.");
        term::error(Source::Gluon, "With none, the daemon refuses to start — there are no stand-in quarks.");
        std::process::exit(2);
    };

    let field_dir = hadron_lattice::hadron_dir_of(&args.field_path);
    let _ = std::fs::create_dir_all(&field_dir);
    let lock_path = field_dir.join("gluon.lock");
    let lock_file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .open(&lock_path)
    {
        Ok(f) => f,
        Err(e) => {
            term::error(Source::Gluon, &format!("failed to open lock file: {}", e));
            std::process::exit(1);
        }
    };

    #[cfg(unix)]
    {
        use std::io::{Seek, SeekFrom, Write};
        use std::os::unix::io::AsRawFd;
        let fd = lock_file.as_raw_fd();
        let lock_res = unsafe { libc::flock(fd, libc::LOCK_EX | libc::LOCK_NB) };
        if lock_res != 0 {
            term::error(Source::Gluon, "another instance of gluon is already running.");
            std::process::exit(1);
        }
        let mut f = &lock_file;
        let _ = f.set_len(0);
        let _ = f.seek(SeekFrom::Start(0));
        let _ = writeln!(f, "{}", std::process::id());
        let _ = f.flush();
    }

    // Seat the team: explicit --team, else a sibling team.json next to the field
    // (the .hadron/ convention), else the config-dir default; refuse to start when none.
    let team_path = resolve_team_path(args.team_path.clone(), &args.field_path);
    match &team_path {
        Some(p) => term::info(Source::Gluon, &format!("team from {}", p.display())),
        None => term::info(Source::Gluon, "no team.json found (looked next to the field, then the config dir)"),
    }
    // The global catalogue holds the quark *definitions* a repo's role/state overrides
    // resolve against. Skip it when it IS the repo file (a bare ~/.hadron/team.json used
    // directly as the team) — there is nothing to resolve against itself.
    let global_path =
        team_config_path().filter(|g| Some(g.as_path()) != team_path.as_deref());
    if let Some(g) = &global_path {
        term::info(Source::Gluon, &format!("catalogue from {}", g.display()));
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
    // values live only in the keychain. See `crate::secrets` and the
    // Security note in the encrypted-secrets design doc.
    let secret_store = crate::secrets::KeyringStore::new();

    let (quarks, mode_label, mut seat_rows) =
        seat_quarks(&team, team_path.as_deref(), &live_dir, &secret_store);
    if quarks.is_empty() {
        term::error(Source::Gluon, "team.json had no usable quarks; nothing to run.");
        std::process::exit(2);
    }
    let max_exchanges = team.max_exchanges.unwrap_or(12);
    let repo_root = std::fs::canonicalize(hadron_lattice::repo_root_of(&args.field_path))
        .unwrap_or_else(|_| hadron_lattice::repo_root_of(&args.field_path).to_path_buf());

    // Self-healing: move the legacy `.hadron/memory/` lessons ledger into
    // `.hadron/nucleus/` before anything else reads either. Non-fatal — the
    // reader's own fallback (`read_nucleus_index_with_fallback`) covers a
    // failed or skipped migration.
    if let Err(e) = crate::engine::migrate_legacy_memory(&repo_root) {
        term::warn(Source::Gluon, &format!("memory→nucleus migration failed (non-fatal): {e:#}"));
    }

    // Startup reclamation. `worktree::reclaim` has existed (and been tested) since
    // the worktree module landed but had **no caller at all** — so a dirty tree left
    // by a crashed quark was only ever discovered when the next `ensure` refused it,
    // by which point the human is reading a routing warning instead of a diagnosis.
    // Report it up front, and sweep the merged `quark/*` branches that accumulate one
    // per turn forever (178 of them here, 156 already merged). Both are non-fatal:
    // a repo that cannot be swept is still a repo the daemon can serve.
    match crate::worktree::reclaim(&repo_root) {
        Ok(found) => {
            for wt in found.iter().filter(|w| w.dirty) {
                term::warn(
                    Source::Gluon,
                    &format!(
                        "{} left uncommitted work in {} (on {}) — inspect it before its next turn",
                        wt.quark.as_str(),
                        wt.path.display(),
                        wt.branch,
                    ),
                );
            }
        }
        Err(e) => term::warn(Source::Gluon, &format!("worktree reclamation failed (non-fatal): {e:#}")),
    }
    let base = crate::worktree::default_branch(&repo_root);
    // Then the disk bound. A worktree is stable per quark and nothing ever removed
    // one, so every seat the human switches off — or deletes from the roster — keeps a
    // full checkout of the repo forever. At twenty seats in a monorepo that is the
    // whole budget. Reap the trees of quarks not taking turns, but ONLY when the tree
    // holds nothing `base` already has; anything dirty or unlanded is spared and named.
    // Runs BEFORE the branch sweep on purpose: removing a tree un-holds its branch, so
    // the existing `-d` sweep below is what actually deletes the ref.
    //
    // Skipped entirely on an empty roster: `load_resolved_team` degrades a missing or
    // malformed team.json to empty, and that must not read as "no quark is seated".
    if team.quarks.is_empty() {
        term::info(Source::Gluon, "roster is empty — skipping the idle-worktree reap");
    } else {
        let keep: Vec<_> =
            team.quarks.iter().filter(|s| s.enabled).map(|s| s.id.clone()).collect();
        match crate::worktree::reap_idle_worktrees(&repo_root, &keep, &base) {
            Ok(reaped) => {
                for r in &reaped {
                    match r {
                        crate::worktree::Reap::Removed { quark, preserved, .. } => {
                            term::info(
                                Source::Gluon,
                                &format!(
                                    "reclaimed {}'s worktree — it takes no turns and \
                                     held nothing that is not on {base} (recreated on its next turn)",
                                    quark.as_str(),
                                ),
                            );
                            // Never silent: anything moved or pinned out of the tree is
                            // named, or the human cannot find it again.
                            for note in preserved {
                                term::info(Source::Gluon, &format!("  {note}"));
                            }
                        }
                        crate::worktree::Reap::Spared { quark, path, why } => term::info(
                            Source::Gluon,
                            &format!("keeping {}'s worktree at {} — {why}", quark.as_str(), path.display()),
                        ),
                    }
                }
            }
            Err(e) => term::warn(Source::Gluon, &format!("idle-worktree reap failed (non-fatal): {e:#}")),
        }
    }
    match crate::worktree::prune_merged_branches(&repo_root, &base) {
        Ok(pruned) if !pruned.is_empty() => {
            term::info(Source::Gluon, &format!("pruned {} merged quark branches", pruned.len()));
        }
        Ok(_) => {}
        Err(e) => term::warn(Source::Gluon, &format!("branch prune failed (non-fatal): {e:#}")),
    }
    // The shared build dir (`worktree::shared_build_env`) is a landfill: nothing has
    // ever swept it, and cargo has no target-dir GC of its own. Measured on this box
    // before this existed: target/debug was 107G, 22G of it untouched for >7 days.
    // Skips (not waits) if cargo already holds the target dir's lock — a build is in
    // flight, and deleting artifacts out from under it could remove a file mid-link.
    match crate::worktree::reap_build_artifacts(
        &repo_root,
        crate::worktree::ARTIFACT_REAP_MIN_AGE,
    ) {
        // Recorded to `<hadron dir>/artifact-sweeps.log` as well as printed: this
        // deletes tens of GB unattended, and the first real run's figure was lost
        // because `eprintln!` went to the launching terminal and nowhere else.
        Ok(Some(reap)) => {
            if reap.files_removed > 0 {
                term::info(
                    Source::Gluon,
                    &format!(
                        "swept {} stale build artifacts ({:.1} GB)",
                        reap.files_removed,
                        reap.bytes_removed as f64 / 1e9,
                    ),
                );
            }
            if let Err(e) = crate::worktree::record_artifact_sweep(&field_dir, &reap) {
                term::warn(Source::Gluon, &format!("could not record the artifact sweep (non-fatal): {e:#}"));
            }
        }
        Ok(None) => term::info(Source::Gluon, "build artifact sweep skipped — cargo is building right now"),
        Err(e) => term::warn(Source::Gluon, &format!("build artifact sweep failed (non-fatal): {e:#}")),
    }

    let engine = Engine::new(args.field_path.clone(), quarks, max_exchanges)
        .with_git(repo_root.clone())
        .with_merge_gate(std::sync::Arc::new(crate::merge::CargoMergeRunner))
        .with_nucleus_index_budget_bytes(crate::nucleus_status::resolve_budget_bytes(&team))
        .with_nucleus(crate::engine::build_nucleus_digest(&repo_root))
        // `Engine::new` defaults this to `None` (hermetic — see the field doc), so the
        // real daemon must opt in explicitly or custom global skills under
        // `~/.hadron/skills` would silently never load in production.
        .with_global_skills_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("skills")))
        // Same seam, same reason: without this the real daemon would never look in
        // `~/.hadron/preons`, so `@preon-name` mentions could only ever resolve
        // via the repo half.
        .with_global_preons_dir(hadron_lattice::user_hadron_dir().map(|d| d.join("preons")));

    // Security: We open the energy ledger database `ledger.db` under the parent directory
    // of `field_path` (which resides inside the repo's `.hadron/` directory in typical usage).
    // The path is derived using standard library functions to prevent traversal or access
    // to untrusted locations.
    let ledger_path = field_dir.join("ledger.db");
    let ledger = match crate::ledger::Ledger::open(&ledger_path) {
        Ok(l) => l,
        Err(e) => {
            term::error(Source::Gluon, &format!("failed to open ledger at {}: {e:#}", ledger_path.display()));
            std::process::exit(2);
        }
    };
    // No swarm-wide ceiling. The ledger meters every seat, but only a seat the
    // human gave an `energy_limit` in Settings can be stopped by it — a quark
    // must not be cut off mid-plan by a number nobody chose.
    let mut engine = engine.with_ledger(ledger, None);
    // A seat can boot switched OFF. It is still seated (still addressable, still owns
    // its instance) — it just does not take turns until the human enables it. Its row
    // already carries `enabled: false` from `seat_quarks`, so the table below shows it
    // as a column rather than this loop printing a separate line per disabled seat.
    for seat in &team.quarks {
        if !seat.enabled {
            engine.set_enabled(&seat.id, false);
        }
    }
    // Give every orchestrator-flavoured seat a second, chat-only lane (Task 6 Step 4
    // of the responsive-orchestrator plan) — built through the SAME construction path
    // as the work lane above, so it gets an identical adapter/config. A build failure
    // here is non-fatal: the seat still runs, just on today's single-lane behaviour.
    for seat in &team.quarks {
        if seat.flavor != Flavor::Orchestrator {
            continue;
        }
        match registry::build_seat_watched(seat, &live_dir, &secret_store) {
            Ok(chat) => {
                // `seat_chat_lane` calls `become_chat_lane` itself — it is the only way a
                // chat lane is ever attached, so telling the instance here as well would
                // be a second site to keep in step for no coverage gained.
                engine.seat_chat_lane(&seat.id, chat);
                if let Some(row) = seat_rows.iter_mut().find(|r| r.id == seat.id.as_str()) {
                    row.chat_lane = true;
                }
            }
            Err(e) => term::warn(
                Source::Gluon,
                &format!("{} — could not seat a chat lane: {e:#}", seat.id.as_str()),
            ),
        }
    }

    // The startup seating summary — one table, built from every row `seat_quarks`
    // (and the chat-lane loop above) collected, instead of the six-odd separate
    // "seated …" / "is seated but DISABLED" / "chat lane seated" lines this used to be.
    for line in term::seating_table(&seat_rows) {
        term::info(Source::Gluon, &line);
    }

    term::info(Source::Gluon, &format!("({mode_label}) watching {}", args.field_path.display()));
    term::info(Source::Gluon, "address quarks from the chamber: '@<id> …'. Ctrl-C to stop.");
    // DO-NOT-ACTIVATE toggle (spec §2 D): read once from HADRON_NO_HUMAN_MODE inside
    // `Engine::new`. Loud on purpose — a mode where the orchestrator, not a human,
    // adjudicates permission asks under global Bypass must never be silently on.
    if engine.no_human() {
        term::warn(
            Source::Gluon,
            "HADRON_NO_HUMAN_MODE is ON — under global Bypass, permission asks that would \
             stop for a human are instead adjudicated by the orchestrator. A human deny-list \
             entry remains absolute (never orchestrator-overridable).",
        );
    }

    // The team the engine is actually running, and the exact bytes we last read from
    // `team.json`. Change is detected on the *bytes*, not on the mtime: a coarse
    // filesystem clock can hide a fast save, and the file is tiny enough that reading it
    // once per interval costs nothing.
    let mut running_team = team;
    let mut last_seen_repo: Option<String> = team_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok());
    let mut last_seen_global: Option<String> = global_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok());

    let mut shutdown_signal = tokio::spawn(async move {
        #[cfg(unix)]
        wait_for_shutdown_signal().await;
        #[cfg(not(unix))]
        tokio::signal::ctrl_c().await.ok();
    });

    loop {
        if shutdown_signal.is_finished() {
            break;
        }

        let before = read_events(&args.field_path).map(|e| e.len()).unwrap_or(0);
        let excite_res = tokio::select! {
            _ = &mut shutdown_signal => {
                term::info(Source::Gluon, "shutting down daemon...");
                break;
            }
            res = engine.run_until_quiesce() => res,
        };

        if let Err(e) = excite_res {
            term::error(Source::Gluon, &format!("excite error (continuing): {e:#}"));
            let events_after = read_events(&args.field_path).map(|e| e.len()).unwrap_or(0);
            if events_after == before {
                let orch_exists =
                    engine.roster().iter().any(|c| c.flavor == Flavor::Orchestrator);
                let body = if orch_exists {
                    format!("@{} Gluon excite error: {e:#}", crate::router::ORCHESTRATOR_ALIAS)
                } else {
                    format!("Gluon excite error: {e:#}")
                };
                let _ = engine
                    .append(Event::new(Actor::Gluon, None, Kind::Message { body }).with_severity(hadron_lattice::Severity::Error))
                    .await;
            }
        }
        let after = read_events(&args.field_path).map(|e| e.len()).unwrap_or(0);
        if after > before {
            term::info(Source::Gluon, &format!("appended {} event(s) → {after} total", after - before));
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
                (Err(e), _) | (_, Err(e)) => term::warn(
                    Source::Gluon,
                    &format!("a team file changed but does not parse — keeping the running roster: {e}"),
                ),
                (Ok(repo), Ok(global)) => {
                    let repo = repo.unwrap_or_default();
                    let global = global.unwrap_or_default();
                    for id in orphan_overrides(&repo, &global) {
                        term::warn(
                            Source::Gluon,
                            &format!("repo override {} names no catalogue seat — ignored", id.as_str()),
                        );
                    }
                    let desired = resolve_team(&repo, &global);
                    // A team with nobody in it is never applied. It is almost certainly a
                    // mistake, and obeying it would leave a swarm the human cannot talk to.
                    // The guard is on the RESOLVED team, so an empty result from a bad
                    // merge (e.g. all-orphan overrides) is caught too.
                    if desired.is_empty() {
                        term::warn(
                            Source::Gluon,
                            &format!(
                                "team now seats nobody — keeping the running roster ({} quark(s))",
                                engine.seated_count()
                            ),
                        );
                    } else {
                        let max_exchanges = desired.max_exchanges.unwrap_or(12);
                        engine.set_max_exchanges(max_exchanges);
                        engine.set_nucleus_index_budget_bytes(
                            crate::nucleus_status::resolve_budget_bytes(&desired),
                        );
                        let plan = reseat::plan(&running_team, &desired);
                        if !plan.is_empty() {
                            term::info(Source::Gluon, &format!("team changed — re-seating [{}]", plan.summary()));
                            running_team =
                                apply_reseat(&mut engine, &running_team, &plan, &live_dir, &secret_store);
                            term::info(Source::Gluon, &format!("roster is now {} quark(s)", engine.seated_count()));
                        }
                    }
                }
            }
        }

        tokio::select! {
            _ = &mut shutdown_signal => {
                term::info(Source::Gluon, "shutting down daemon...");
                break;
            }
            _ = tokio::time::sleep(args.interval) => {}
        }
    }
}

#[cfg(unix)]
async fn wait_for_shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut sigint = signal(SignalKind::interrupt()).ok();
    let mut sigterm = signal(SignalKind::terminate()).ok();

    tokio::select! {
        _ = async {
            if let Some(s) = &mut sigint {
                s.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            term::info(Source::Gluon, "received SIGINT, shutting down...");
        }
        _ = async {
            if let Some(s) = &mut sigterm {
                s.recv().await;
            } else {
                std::future::pending::<()>().await;
            }
        } => {
            term::info(Source::Gluon, "received SIGTERM, shutting down...");
        }
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
        term::warn(
            Source::Gluon,
            &format!("repo override {} names no catalogue seat — ignored (not seated)", id.as_str()),
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
    use hadron_lattice::QuarkId;
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

    /// `seat_quarks`' whole reason to return rows instead of printing per-seat: the
    /// startup summary must be ONE table built from a known roster, not six-odd
    /// separate lines. Assert on the rows it hands back, and on what
    /// `term::seating_table` renders from them — the seam Task 2 converts.
    #[test]
    fn seat_quarks_returns_one_row_per_seat_with_its_enabled_state() {
        use hadron_lattice::secrets::MemoryStore;
        use hadron_lattice::Seat;

        let mut disabled = Seat::cli(QuarkId::new("cli-agy"), "agy", "", Flavor::Worker);
        disabled.enabled = false;
        let team = Team {
            quarks: vec![
                Seat::cli(QuarkId::new("opus"), "agy", "opus-4.8", Flavor::Orchestrator),
                disabled,
            ],
            ..Default::default()
        };
        let dir = tempdir().unwrap();
        let store = MemoryStore::new();

        let (quarks, mode_label, rows) = seat_quarks(&team, None, dir.path(), &store);

        assert_eq!(quarks.len(), 2, "both seats must build — neither is malformed");
        assert_eq!(mode_label, "live mode (real CLIs — real budget)");
        assert_eq!(rows.len(), 2, "one SeatRow per seat, not one line per fact about it");
        assert_eq!(rows[0].id, "opus");
        assert!(rows[0].enabled);
        assert!(!rows[1].enabled, "the disabled seat's row must carry that state");

        let table = term::seating_table(&rows);
        assert_eq!(table.len(), 2);
        assert!(table[0].contains("opus") && table[0].contains("opus-4.8"));
        assert!(table[1].contains("disabled"), "a disabled seat is a column, not a second line");
    }

    /// An empty team seats nobody, whether or not a file was found. There is no mock
    /// pair left to stand in: a fake '@Claude' acknowledging a real instruction is
    /// worse than a daemon that refuses to start, and "no file at all" is no different
    /// — a human with no team.json wants the wizard, not a puppet answering for them.
    #[test]
    fn an_empty_team_seats_nobody_and_is_never_mocked() {
        use hadron_lattice::secrets::MemoryStore;

        let dir = tempdir().unwrap();
        let store = MemoryStore::new();
        let found = dir.path().join("team.json");

        for team_path in [Some(found.as_path()), None] {
            let (quarks, label, rows) =
                seat_quarks(&Team::default(), team_path, dir.path(), &store);
            assert!(quarks.is_empty(), "no seats, and no fakes standing in for them");
            assert_eq!(label, "no usable team");
            assert!(rows.is_empty());
        }
    }
}
