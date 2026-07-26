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
        has_forge_tools: false,
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
        nucleus_index_budget_bytes: hadron_lattice::DEFAULT_NUCLEUS_INDEX_BUDGET_BYTES,
        nucleus_notes_dir: std::path::PathBuf::new(),
        git_diff: String::new(),
        isolated: true,
        cwd: std::path::PathBuf::from("/repo/.hadron/trees/agy"),
        mode: hadron_lattice::Mode::default(),
        role_body: None,
        active_skill: None,
        named_specifically: true,
        has_forge_tools: false,
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
        has_forge_tools: false,
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
        has_forge_tools: false,
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
        has_forge_tools: false,
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
        has_forge_tools: false,
    });

    let orch_prompt = build(&proj, &QuarkId::new("opus"));
    assert!(orch_prompt.contains("You are the **orchestrator**"));
    assert!(orch_prompt.contains("Stay available"));

    let worker_prompt = build(&proj, &QuarkId::new("agy"));
    assert!(!worker_prompt.contains("You are the **orchestrator**"));
}

/// **The "deploy first" property.** On a multi-task request the orchestrator's
/// own guidance must tell it to emit `@worker <task>` delegation lines BEFORE
/// starting its own implementation, so workers run in parallel instead of
/// picking up sub-tasks only after the orchestrator has already spent the turn
/// on its own slice.
#[test]
fn orchestrator_is_told_to_delegate_before_implementing() {
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
        has_forge_tools: false,
    });

    let prompt = build(&proj, &QuarkId::new("opus"));
    assert!(
        prompt.contains("before you start your own implementation work"),
        "the orchestrator must be told to fan out delegations first:\n{prompt}"
    );
}

/// **Broadcast means think, not do.** A worker reached only via `@team` (or any
/// unaddressed message) — `named_specifically == false` — must be told to
/// analyse and report to the orchestrator, not to implement, edit files, or
/// commit. This is what stopped every worker independently implementing the
/// same `@team` brainstorm ask.
#[test]
fn a_broadcast_reached_worker_is_told_to_think_not_do() {
    let mut proj = projection("x");
    proj.named_specifically = false;
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
        has_forge_tools: false,
    });

    let worker_prompt = build(&proj, &QuarkId::new("agy"));
    assert!(
        worker_prompt.contains("do not implement it"),
        "a broadcast-reached worker must be told to think, not do:\n{worker_prompt}"
    );
    assert!(worker_prompt.contains("Do not edit files"));
    assert!(worker_prompt.contains("commit, or open a branch"));

    // The orchestrator is never "broadcast-only" — an unaddressed message
    // defaults to it — so it must never get this clause regardless of the flag.
    let orch_prompt = build(&proj, &QuarkId::new("opus"));
    assert!(!orch_prompt.contains("do not implement it"));
}

/// A worker specifically named — by `@id`, display name, or role — gets the
/// ordinary work directive; the broadcast-only clause is absent.
#[test]
fn a_specifically_named_worker_gets_no_broadcast_clause() {
    let mut proj = projection("x");
    proj.named_specifically = true;
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
        has_forge_tools: false,
    });

    let worker_prompt = build(&proj, &QuarkId::new("agy"));
    assert!(!worker_prompt.contains("do not implement it"));
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
        has_forge_tools: false,
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
        has_forge_tools: false,
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
        has_forge_tools: false,
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

/// The budget the prompt enforces is whatever `Projection::nucleus_index_budget_bytes`
/// carries, not always the shipped default — an index well under the 32 KiB default
/// but over a smaller CONFIGURED budget must still degrade to counts. This is the
/// whole point of threading the resolved value through rather than reading a
/// hardcoded constant at the call site.
#[test]
fn a_configured_smaller_budget_is_enforced_even_under_the_default() {
    let mut proj = projection("x");
    proj.nucleus_index_path = std::path::PathBuf::from("/repo/.hadron/nucleus/index.md");
    proj.nucleus_index_budget_bytes = 64;
    proj.nucleus_index = "- [a-lesson](notes/a-lesson.md) — well under the 32 KiB shipped default\n".into();
    assert!(proj.nucleus_index.len() > 64, "fixture must exceed the configured budget");
    assert!(
        proj.nucleus_index.len() < crate::engine::nucleus::NUCLEUS_INDEX_BUDGET,
        "fixture must stay under the shipped default, to prove it's the configured value that fired"
    );

    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("COUNTS, not lessons"), "a smaller configured budget must still be enforced");
}

/// Symmetric case: a larger configured budget must let an index PAST the shipped
/// default through in full.
#[test]
fn a_configured_larger_budget_admits_an_index_past_the_default() {
    let mut proj = projection("x");
    proj.nucleus_index_path = std::path::PathBuf::from("/repo/.hadron/nucleus/index.md");
    proj.nucleus_index_budget_bytes = 128 * 1024;
    let big = "- [x](notes/x.md) — ".to_string()
        + &"a".repeat(crate::engine::nucleus::NUCLEUS_INDEX_BUDGET + 1000);
    proj.nucleus_index = big.clone();

    let p = build(&proj, &QuarkId::new("agy"));
    assert!(!p.contains("COUNTS, not lessons"), "a larger configured budget must admit the full index");
    assert!(p.contains(&big));
}

#[test]
fn an_over_budget_nucleus_index_emits_tag_manifest_and_relevant_lessons() {
    let mut proj = projection("fix GUI rendering bug");
    proj.nucleus_index_path = std::path::PathBuf::from("/repo/.hadron/nucleus/index.md");

    let mut big_index = String::from("## GUI\n- **gui-bug** — fixed [tag:gui]\n- **pinned-item** — important [pinned]\n## IPC\n");
    while big_index.len() <= crate::engine::nucleus::NUCLEUS_INDEX_BUDGET {
        big_index.push_str("- **other** — padding line\n");
    }
    proj.nucleus_index = big_index;

    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("GUI:"));
    assert!(p.contains("IPC:"));
    assert!(p.contains("Relevant & Pinned Lessons"));
    assert!(p.contains("gui-bug"));
    assert!(p.contains("pinned-item"));
    // A degraded section that does not announce itself is how the nucleus went
    // inert unnoticed: counts printed under "What the swarm has learned" read as
    // the truth about what is known.
    assert!(p.contains("COUNTS, not lessons"), "the substitution must announce itself");
    let notice = p.find("COUNTS, not lessons").unwrap();
    assert!(notice < p.find("GUI:").unwrap(), "the notice must precede the counts");
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
    assert!(!p.contains("Project knowledge"));
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

#[test]
fn measure_reports_nonzero_sizes_for_present_sections() {
    let mut proj = projection("do the thing");
    proj.invariants = "some rule".to_string();
    proj.nucleus_digest = String::new();
    let b = measure(&proj, &QuarkId::new("agy"));
    assert!(b.standard_model > 0, "standard model is always present");
    assert!(b.invariants > 0);
    assert!(b.task > 0);
    assert_eq!(b.nucleus_digest, 0, "empty digest measures as zero");
}

/// The sum of every measured section must not exceed `build()`'s total length —
/// `measure` must not double-count a section `build` only writes once.
#[test]
fn measure_and_build_agree_on_section_boundaries() {
    let proj = projection("do the thing");
    let id = QuarkId::new("agy");
    let built = build(&proj, &id);
    let b = measure(&proj, &id);
    let total = b.standard_model + b.invariants + b.nucleus_digest + b.nucleus_index + b.task + b.field_window;
    assert!(total <= built.len());
}

/// The safety net under the budget cliff was silently disabled by the index migration.
/// Over budget, the prompt substitutes counts for the index and then re-adds the lines
/// that match the task — `line_matches_task_or_pinned`. That filter required a line to
/// start `- **`, the OLD index shape. `c449aef` moved every line to
/// `- [slug](notes/slug.md) — hook` and fixed `tag_manifest`'s identical coupling, but
/// not this one, so a quark past the budget got counts and an EMPTY relevant-lessons
/// list: zero lessons, no partial recovery.
#[test]
fn a_pointer_line_still_matches_the_task_it_names() {
    let lower = "the merge gate keeps hanging on a rebase".to_lowercase();
    assert!(
        super::line_matches_task_or_pinned(
            "- [the-merge-gate](notes/the-merge-gate.md) — a hung suite in the target project",
            &lower
        ),
        "a pointer line whose slug appears in the task must still be surfaced"
    );
    // The old shape has to keep working — a user's index may not be migrated.
    assert!(super::line_matches_task_or_pinned("- **the-merge-gate** — a hung suite", &lower));
    // And an unrelated lesson must NOT be dragged in.
    assert!(!super::line_matches_task_or_pinned(
        "- [gpui-hsla-takes-normalised-hue-not-degrees](notes/x.md) — hue clamps to 0..1",
        &lower
    ));
}

#[test]
fn prompt_contains_hadron_forge_tools_section_when_enabled() {
    let mut proj = projection("x");
    proj.has_forge_tools = true;
    let p = build(&proj, &QuarkId::new("agy"));
    assert!(p.contains("# Available Hadron Forge Tools"));
    assert!(p.contains("hadron_forge_read_file"));
    assert!(p.contains("hadron_forge_list_dir"));
    assert!(p.contains("hadron_forge_grep"));
    assert!(p.contains("hadron_forge_exec"));
    assert!(p.contains("hadron_forge_diagnostics"));
    assert!(p.contains("hadron_forge_cargo_tree"));
    assert!(p.contains("hadron_forge_query_nucleus"));
    assert!(p.contains("hadron_forge_read_blocks"));
    assert!(p.contains("hadron_forge_edit"));
}

#[test]
fn a_cli_transport_projection_does_not_contain_forge_tools_section() {
    let proj = projection("x");
    assert!(!proj.has_forge_tools, "CLI projection defaults to has_forge_tools = false");
    let p = build(&proj, &QuarkId::new("agy"));
    assert!(!p.contains("# Available Hadron Forge Tools"));
    assert!(!p.contains("hadron_forge_"));
}

#[test]
fn prompt_renders_active_skill_immediately_before_your_task() {
    let mut proj = projection("Build auth feature");
    proj.active_skill = Some("\n# Skill for this turn: executing-plans\n\nLoad plan and execute.".to_string());
    let p = build(&proj, &QuarkId::new("agy"));

    let skill_pos = p.find("# Skill for this turn: executing-plans").expect("active skill header present");
    let task_pos = p.find("# Your task").expect("task header present");
    let inv_pos = p.find("# Working protocol (Invariants)").expect("invariants header present");

    assert!(inv_pos < skill_pos, "invariants precede active skill");
    assert!(skill_pos < task_pos, "active skill precedes task");
}
