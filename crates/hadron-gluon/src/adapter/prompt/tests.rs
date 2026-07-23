use super::*;
use hadron_lattice::{EnergyState, Event, Flavor, QuarkCard};

fn projection(task: &str) -> Projection {
    Projection {
        task: task.into(),
        invariants: "Snapshot before editing. Use @mentions.".into(),
        available_invariants: vec![],
        nucleus_digest: "## map.md\nauth lives in src/auth".into(),
        live_activities: vec![], roster: vec![QuarkCard {
            id: QuarkId::new("agy"),
            display_name: None,
            flavor: Flavor::Worker,
            energy: EnergyState::Available,
            provider: String::new(),
            model: String::new(),
            roles: vec![],
            exclusive: false,
            commands: Default::default(),
            energy_limit: None,
            deny_skills: vec![],
        }],
        field_window: vec![Event::new(
            Actor::Human,
            Some(QuarkId::new("claude")),
            Kind::Message { body: "start the auth work".into() },
        )],
        field_truncated: false,
        nucleus_index: String::new(),
        nucleus_index_path: std::path::PathBuf::new(),
        nucleus_index_truncated: false,
        nucleus_notes_dir: std::path::PathBuf::new(),
        git_diff: String::new(),
        isolated: true,
        cwd: std::path::PathBuf::from("/repo/.hadron/trees/agy"),
        mode: hadron_lattice::Mode::default(),
        role_body: None,
    }
}

/// The quark must be TOLD where it is working, not just silently placed there.
#[test]
fn prompt_names_the_working_directory() {
    let p = build(&projection("Build login"), &QuarkId::new("agy"));
    assert!(p.contains("# Where you are"));
    assert!(p.contains("/repo/.hadron/trees/agy"));
    assert!(p.contains("do NOT"), "and told not to touch the parent checkout");
}

/// Peers are named to a quark by their DISPLAY NAME when they have one, not the
/// raw seat id — so quarks address each other `@GoogleGirl`, not `@acp-claude`.
/// (The router resolves either back to the seat, so routing is unaffected.)
#[test]
fn peers_are_named_by_display_name_not_raw_id() {
    let mut proj = projection("coordinate");
    // The self (agy) gets a display name too — "Who you are" must use it.
    proj.roster[0].display_name = Some("Aggy".into());
    // A peer with a display name, and a transcript line it authored.
    proj.roster.push(QuarkCard {
        id: QuarkId::new("acp-claude"),
        display_name: Some("GoogleGirl".into()),
        flavor: Flavor::Worker,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        roles: vec![],
        exclusive: false,
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
    });
    proj.field_window.push(Event::new(
        Actor::Quark(QuarkId::new("acp-claude")),
        Some(QuarkId::new("agy")),
        Kind::Message { body: "ping".into() },
    ));
    let p = build(&proj, &QuarkId::new("agy"));

    assert!(p.contains("You are `@Aggy`"), "self named by display name:\n{p}");
    assert!(
        p.contains("**GoogleGirl → Aggy:** ping"),
        "peer + recipient named by display name in the transcript:\n{p}"
    );
    assert!(
        !p.contains("acp-claude"),
        "the raw id must not leak anywhere once a display name exists:\n{p}"
    );
}

/// The bug this pair of tests exists to stop recurring: for the whole of the
/// live swarm's life the prompt asserted isolation unconditionally, while the
/// engine actually placed every quark in the *shared* workspace root. A worker
/// obeying "your changes reach `main` only through the merge gate" then refused
/// to commit at all — and its work sat uncommitted in the human's tree, one
/// `git checkout` away from being lost. A quark in the shared tree must be told
/// to commit its own work, and must NOT be told a merge gate exists.
#[test]
fn a_quark_in_the_shared_tree_is_told_to_commit_its_own_work() {
    let mut proj = projection("Build login");
    proj.isolated = false;
    proj.cwd = std::path::PathBuf::from("/home/jake/dev/hadron");
    let p = build(&proj, &QuarkId::new("agy"));

    assert!(p.contains("shared checkout"), "it must know it shares the tree");
    assert!(p.contains("commit your own work"), "and that committing is ITS job");
    assert!(
        !p.contains("merge gate"),
        "there is no merge gate in the shared tree — promising one is what made agy \
         refuse to commit, every single time"
    );
    // Sharing a tree with in-flight quarks makes a blanket stage a footgun.
    assert!(p.contains("git add -A"), "and is warned off the blanket stage");
}

/// The isolated arm must keep its old contract intact: a quark that really does
/// have its own worktree still routes through the gate rather than committing
/// onto whatever branch the human happens to have checked out.
#[test]
fn a_quark_in_its_own_worktree_still_routes_through_the_merge_gate() {
    let p = build(&projection("Build login"), &QuarkId::new("agy"));
    assert!(p.contains("merge gate"));
    assert!(!p.contains("shared checkout"));
}

#[test]
fn prompt_contains_all_sections() {
    let p = build(&projection("Build login"), &QuarkId::new("agy"));
    assert!(p.contains("# Working protocol (Invariants)"));
    assert!(p.contains("Snapshot before editing"));
    assert!(p.contains("# Project knowledge (nucleus)"));
    assert!(p.contains("auth lives in src/auth"));
    assert!(p.contains("# Your task"));
    assert!(p.contains("Build login"));
    assert!(p.contains("# Recent field"));
    assert!(p.contains("**human → claude:** start the auth work"));
    assert!(p.contains("@<name>"), "the addressing instruction is present");
}

#[test]
fn prompt_states_the_quarks_own_identity_and_multi_addressee_rule() {
    // A quark must know its handle to act on only its slice of a message that
    // named several quarks (the multi-dispatch case).
    let p = build(&projection("x"), &QuarkId::new("opus"));
    assert!(p.contains("# Who you are"));
    assert!(p.contains("You are `@opus`"));
    assert!(p.contains("act ONLY on the part directed at you"));
}

#[test]
fn prompt_states_mode_authority_and_demands_honesty() {
    let mut proj = projection("x");
    // Read-only mode tells the quark it cannot act…
    proj.mode = hadron_lattice::Mode::Ask;
    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("# Your authority this turn"));
    assert!(p.contains("Ask (read-only)"));
    // …and every mode demands truthful reporting (the anti-confabulation rule).
    assert!(p.contains("Never state completed work"));

    proj.mode = hadron_lattice::Mode::Bypass;
    assert!(build(&proj, &QuarkId::new("agy")).contains("Bypass — full tool access"));
}

/// A worker is told to escalate by ROLE; the orchestrator is not told to
/// escalate at all (it *is* the escalation target — it hands back to the human).
#[test]
fn worker_is_told_to_escalate_to_the_orchestrator_role() {
    let mut proj = projection("x");
    proj.roster.push(QuarkCard {
        id: QuarkId::new("opus"),
        display_name: None,
        flavor: Flavor::Orchestrator,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        roles: vec![],
        exclusive: false,
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
    });

    let worker_prompt = build(&proj, &QuarkId::new("agy"));
    assert!(worker_prompt.contains("You are a **worker**"));
    assert!(worker_prompt.contains("`@orchestrator`"));

    let orch_prompt = build(&proj, &QuarkId::new("opus"));
    assert!(!orch_prompt.contains("You are a **worker**"));
}

/// A finished worker turn must wake the orchestrator, not fall through to the
/// human. An unaddressed reply excites nobody, so telling a worker both "you
/// report to the orchestrator" and "drop the @mention when you are done" made it
/// obey the second: it reported a completed task into the void and the
/// orchestrator never acted on it. Observed live.
#[test]
fn a_finished_worker_reports_up_and_is_never_told_to_drop_the_mention() {
    let mut proj = projection("x");
    proj.roster.push(QuarkCard {
        id: QuarkId::new("opus"),
        display_name: None,
        flavor: Flavor::Orchestrator,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        roles: vec![],
        exclusive: false,
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
    });

    let worker = build(&proj, &QuarkId::new("agy"));
    assert!(
        worker.contains(
            "When your task is complete or if your turn encounters an error, start a line with `@orchestrator`"
        ),
        "a worker must be told to report its completed work UP to the orchestrator"
    );
    assert!(
        !worker.contains("reply WITHOUT any `@mention`"),
        "a worker told to drop the mention hands its result to nobody"
    );

    // The orchestrator answers to the human, so it keeps the original rule.
    let orch = build(&proj, &QuarkId::new("opus"));
    assert!(orch.contains("reply WITHOUT any `@mention`"));
    assert!(!orch.contains("start a line with `@orchestrator` and report there"));
}

/// With no orchestrator seated there is nobody to report to, so a lone quark must
/// still hand back to the human rather than address a role that does not exist.
#[test]
fn with_no_orchestrator_a_quark_still_hands_back_to_the_human() {
    let p = build(&projection("x"), &QuarkId::new("agy"));
    assert!(p.contains("reply WITHOUT any `@mention`"));
    assert!(!p.contains("start a line with `@orchestrator` and report there"));
}

/// The orchestrator is told to stay available and dispatch long work; a worker
/// is not (a worker grinding away blocks nobody's conversation).
#[test]
fn only_the_orchestrator_is_told_to_stay_available() {
    let mut proj = projection("x");
    proj.roster.push(QuarkCard {
        id: QuarkId::new("opus"),
        display_name: None,
        flavor: Flavor::Orchestrator,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        roles: vec![],
        exclusive: false,
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
    });

    let orch_prompt = build(&proj, &QuarkId::new("opus"));
    assert!(orch_prompt.contains("You are the **orchestrator**"));
    assert!(orch_prompt.contains("Stay available"));

    let worker_prompt = build(&proj, &QuarkId::new("agy"));
    assert!(!worker_prompt.contains("You are the **orchestrator**"));
}

#[test]
fn bypass_orchestrator_gets_autonomous_loop_directives() {
    let mut proj = projection("x");
    proj.mode = hadron_lattice::Mode::Bypass;
    proj.roster.push(QuarkCard {
        id: QuarkId::new("opus"),
        display_name: None,
        flavor: Flavor::Orchestrator,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        roles: vec![],
        exclusive: false,
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
    });

    let prompt = build(&proj, &QuarkId::new("opus"));
    assert!(prompt.contains("Autonomous Bypass Execution Loop"));
    assert!(prompt.contains("update the active plan file on disk"));
    assert!(prompt.contains("dispatch the next unchecked task"));
}

/// Task 5: in Bypass the orchestrator must recover worktree-fixable blockers
/// itself and never hand the human a menu of options. The escalation exception
/// is narrowed to "only the human can unblock", closing the loophole that turned
/// a stranded merge (recoverable) into an option-menu handback.
#[test]
fn bypass_completion_gate_forbids_menus() {
    let mut proj = projection("x");
    proj.mode = hadron_lattice::Mode::Bypass;
    proj.roster.push(QuarkCard {
        id: QuarkId::new("opus"),
        display_name: None,
        flavor: Flavor::Orchestrator,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        roles: vec![],
        exclusive: false,
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
    });

    let prompt = build(&proj, &QuarkId::new("opus"));
    assert!(prompt.contains("NEVER hand the human a menu"));
    assert!(prompt.contains("carry the commit forward onto THIS turn's branch"));
    assert!(prompt.contains("\"Unrecoverable\" means the human is the sole unblocker"));
}


/// No orchestrator on the roster → don't point the worker at a target that
/// cannot be reached.
#[test]
fn no_escalation_clause_without_an_orchestrator() {
    // The base projection's roster is a lone worker (`agy`).
    let p = build(&projection("x"), &QuarkId::new("agy"));
    assert!(!p.contains("You are a **worker**"));
}

/// A dropped event must be *announced*. Silent truncation is the merge-gate bug
/// wearing a different hat: the quark cannot tell "never said" from "can't see it",
/// so it acts on the partial field with full confidence.
#[test]
fn a_truncated_field_window_says_so() {
    let mut proj = projection("x");
    assert!(!build(&proj, &QuarkId::new("agy")).contains("INCOMPLETE"));

    proj.field_truncated = true;
    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("INCOMPLETE"), "the quark must be told the transcript is partial");
    assert!(p.contains("rather than assuming it was never said"));
}

/// In the shared tree the diff is the WHOLE tree's uncommitted state — the human's
/// edits and other quarks' in-flight work. Unlabelled, a quark reads it as its own
/// prior output and "continues" work it never did.
#[test]
fn the_working_diff_is_attributed_to_the_tree_it_came_from() {
    let mut proj = projection("x");
    proj.git_diff = "--- a/x\n+++ b/x".into();

    proj.isolated = true;
    let own = build(&proj, &QuarkId::new("agy"));
    assert!(own.contains("your own worktree"));

    proj.isolated = false;
    let shared = build(&proj, &QuarkId::new("agy"));
    assert!(shared.contains("whole shared tree"));
    assert!(shared.contains("It is NOT necessarily yours"));
    assert!(shared.contains("do not commit any part of it you did not author"));
}

/// Jake's ask, made structural: the orchestrator is the human's narrator, so a
/// worker's claim must be checked against the repo before it is relayed as fact.
/// This was my habit; a habit is not architecture.
#[test]
fn the_orchestrator_must_verify_a_workers_claim_before_relaying_it() {
    let mut proj = projection("x");
    proj.roster.push(QuarkCard {
        id: QuarkId::new("opus"),
        display_name: None,
        flavor: Flavor::Orchestrator,
        energy: EnergyState::Available,
        provider: String::new(),
        model: String::new(),
        roles: vec![],
        exclusive: false,
        commands: Default::default(),
        energy_limit: None,
        deny_skills: vec![],
    });

    let orch = build(&proj, &QuarkId::new("opus"));
    assert!(orch.contains("Verify before you relay"));
    assert!(orch.contains("the repo wins"), "ground truth beats a self-report");
    // …and the floor, so it doesn't become a lazy delegator.
    assert!(orch.contains("do not bounce trivial work"));

    // The worker is told to settle small calls itself rather than stalling.
    let worker = build(&proj, &QuarkId::new("agy"));
    assert!(worker.contains("Resolve implementation details yourself"));
    assert!(worker.contains("do NOT stall on the small ones"));
}

/// The asymmetry Jake named: one quark arrives with weeks of accumulated context,
/// another with nothing, and we mistake persistence for intelligence. A quark must
/// be shown the nucleus index AND told where to write it — "remember this" without
/// a path is an instruction it cannot obey. It must also be told where the NOTES
/// are, or the index's `→ path` lines point at something it was never told it may
/// open.
#[test]
fn a_quark_is_shown_the_nucleus_index_and_told_where_to_write_it() {
    let mut proj = projection("x");
    proj.nucleus_index_path = std::path::PathBuf::from("/repo/.hadron/nucleus/index.md");
    proj.nucleus_notes_dir = std::path::PathBuf::from("/repo/.hadron/nucleus/notes");

    // Empty index still emits the section — otherwise the quark never learns it HAS one.
    let empty = build(&proj, &QuarkId::new("agy"));
    assert!(empty.contains("# What the swarm has learned (nucleus index)"));
    assert!(empty.contains("nothing has been recorded here yet"));
    assert!(empty.contains("/repo/.hadron/nucleus/index.md"), "it must know the path");
    assert!(empty.contains("/repo/.hadron/nucleus/notes"), "and where notes live");
    assert!(empty.contains("shared by every quark"), "and that it is not private");
    assert!(empty.contains("append to it"), "and that writing is its job");

    // A populated index comes back verbatim.
    proj.nucleus_index = "- **forge-unwired** — the forge has zero consumers.".into();
    let carried = build(&proj, &QuarkId::new("agy"));
    assert!(carried.contains("- **forge-unwired** — the forge has zero consumers."));
    assert!(!carried.contains("nothing has been recorded here yet"));
    assert!(!carried.contains("index above is CUT"), "it was not truncated");
}

/// A nucleus index cut for size must SAY it was cut. Silent truncation is the
/// failure we killed in the field window: the quark cannot tell "never learned"
/// from "not shown" and treats the last line it can see as the end of what is known.
#[test]
fn a_truncated_nucleus_index_says_so() {
    let mut proj = projection("x");
    proj.nucleus_index_path = std::path::PathBuf::from("/repo/.hadron/nucleus/index.md");
    proj.nucleus_index = "- **a** — a lesson.".into();
    proj.nucleus_index_truncated = true;

    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("index above is CUT"));
}

/// No nucleus index path (a mock adapter, an old snapshot) → no section, rather
/// than a section telling the quark to write to nowhere.
#[test]
fn no_nucleus_index_section_without_a_path() {
    let p = build(&projection("x"), &QuarkId::new("agy"));
    assert!(!p.contains("your memory"));
}

/// Prohibition alone did not stop a blanket stage; give the positive form too.
#[test]
fn the_shared_tree_prescribes_staging_by_explicit_path() {
    let mut proj = projection("x");
    proj.isolated = false;
    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("git add <path>"), "say what TO do, not only what not to");
    assert!(p.contains("git add -A"), "and still name the footgun");
}

#[test]
fn empty_optional_sections_are_omitted() {
    let mut proj = projection("t");
    proj.invariants = String::new();
    proj.nucleus_digest = String::new();
    proj.git_diff = String::new();
    let p = build(&proj, &QuarkId::new("agy"));
    assert!(!p.contains("Invariants"));
    assert!(!p.contains("nucleus"));
    assert!(!p.contains("working diff"));
    assert!(p.contains("# Your task"));
}

#[test]
fn prompt_contains_your_role_when_present() {
    let mut proj = projection("Build login");
    proj.role_body = Some("You must act as the lead architect and write design docs.".into());
    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("# Your role"));
    assert!(p.contains("You must act as the lead architect and write design docs."));
}
