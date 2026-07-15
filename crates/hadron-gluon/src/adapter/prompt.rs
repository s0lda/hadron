use hadron_lattice::{Actor, Flavor, Kind, Mode, Projection, QuarkId};

/// What the quark can actually do this turn, given its resolved permission mode.
/// The mode sets the CLI posture, but the *model* only narrates honestly if it is
/// told its constraints — otherwise a read-only turn confidently reports commits
/// and passing tests it never ran (observed live). This text keeps the narration
/// tied to reality.
fn mode_guidance(mode: Mode) -> &'static str {
    match mode {
        Mode::Ask => "**Ask (read-only) — you CANNOT edit files, run shell commands, or commit \
            this turn.** Propose what you would do and how. Do NOT claim to have made changes, \
            commits, or test runs — you have no way to perform them right now.",
        Mode::Write => "**Write — you may edit files, but you CANNOT run shell commands** (no \
            builds, tests, git, or other commands). Do not claim command output, test results, \
            or commits you cannot produce.",
        Mode::Auto => "**Auto — you may edit files; ungated shell commands are not available** \
            this turn. Do not claim results of commands you could not run.",
        Mode::Bypass => "**Bypass — full tool access** (edits and shell commands). Report only \
            what you actually ran and observed.",
    }
}

/// Render one field event as a Markdown transcript line: `**from → to:** body`.
fn render_event_line(from: &Actor, to: &Option<QuarkId>, body: &str) -> String {
    let from_s = match from {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => q.as_str().to_string(),
    };
    match to {
        Some(t) => format!("**{from_s} → {}:** {body}", t.as_str()),
        None => format!("**{from_s}:** {body}"),
    }
}

/// Build the full Markdown prompt handed to a quark's CLI for one turn.
/// Deterministic and side-effect-free so it can be unit-tested exactly.
/// `self_id` is the quark's own handle — a human message can address several
/// quarks at once ("@opus X and @agy Y"), each of whom receives the whole
/// message, so each must know which mentions are its part.
pub fn build(projection: &Projection, self_id: &QuarkId) -> String {
    let mut p = String::new();

    // 0. Identity — which quark is being excited. A multi-addressee human
    // message hands the SAME text to each named quark, so the model must know
    // its own handle to act on only its part.
    p.push_str(&format!("# Who you are\nYou are `@{}` in this swarm.\n\n", self_id.as_str()));

    // 1. Invariants — the enforced working protocol.
    if !projection.invariants.trim().is_empty() {
        p.push_str("# Working protocol (Invariants)\n");
        p.push_str(projection.invariants.trim());
        p.push_str("\n\n");
    }

    // 2. Nucleus digest — project SSOT context.
    if !projection.nucleus_digest.trim().is_empty() {
        p.push_str("# Project knowledge (nucleus)\n");
        p.push_str(projection.nucleus_digest.trim());
        p.push_str("\n\n");
    }

    // 2b. Memory — what THIS quark learned, on earlier turns and in earlier sessions.
    //
    // The gap this closes is the one Jake named: one quark arrives with weeks of
    // accumulated context and another arrives with nothing, and we mistake the
    // difference for intelligence. It is not; it is persistence. Always emit the
    // section, even when empty — a quark that is never shown its memory file does not
    // know it HAS one, and will not write to it.
    if !projection.memory_path.as_os_str().is_empty() {
        p.push_str("# What the swarm has learned (memory index)\n");
        if projection.memory.trim().is_empty() {
            p.push_str("_Empty — nothing has been recorded here yet._\n\n");
        } else {
            p.push_str(projection.memory.trim());
            p.push_str("\n\n");
        }
        if projection.memory_truncated {
            p.push_str(
                "**The index above is CUT — it did not fit the budget, and lessons are \
                 missing from it.** Do not read the end of it as the end of what is known. \
                 It has stopped being an index: prune it to one line per lesson and move \
                 the long versions into notes.\n\n",
            );
        }
        p.push_str(&format!(
            "This memory is **shared by every quark**, and it persists across turns and \
             sessions. A lesson one of you paid for is a lesson none of you should pay for \
             twice, so write for the others, not just for yourself.\n\n\
             It is an **index**: one short line per lesson, because it is handed to every \
             quark on every turn and every word in it is a tax paid forever. A line that \
             needs more than a line names a **note** that holds the long version. The \
             engine does NOT load the notes — you open one yourself, with your own tools, \
             on the turn its line turns out to matter.\n\n\
             - The index lives at `{index}` — append to it (create it if it does not exist).\n\
             - Notes live in `{notes}/` — one file per lesson, named after its slug.\n\
             - A line is `- **<slug>** — <the lesson, in one sentence>` and, if it has a \
             note, ends with ` → `{notes}/<slug>.md`. Many lessons need no note at all.\n\n\
             Record what you could only learn by getting it wrong: a fact about this \
             codebase that cost you effort, a rule you were given that turned out to be \
             false, a mistake worth not repeating. Do not record what the code already \
             says — it will still say it tomorrow.\n\n",
            index = projection.memory_path.display(),
            notes = projection.memory_notes_dir.display()
        ));
    }

    // 3. The task.
    p.push_str("# Your task\n");
    p.push_str(projection.task.trim());
    p.push_str("\n\n");

    // 3b. Authority this turn — what the current mode actually permits, so the
    // model narrates honestly instead of confabulating actions it cannot take.
    p.push_str("# Your authority this turn\n");
    p.push_str(mode_guidance(projection.mode));
    p.push_str("\n\n");

    // 3c. Where you are. The CLI already *runs* in this directory (the engine sets
    // the process cwd to the quark's own worktree), but a model that isn't told its
    // constraints narrates around them — same failure class as `mode_guidance`. Say
    // it plainly, so nobody goes looking for the parent checkout.
    if !projection.cwd.as_os_str().is_empty() {
        p.push_str("# Where you are\n");
        if projection.isolated {
            p.push_str(&format!(
                "You are working in `{}` — your own checkout, isolated from every other quark \
                 and from the human's tree. Do all your work here. Do NOT `cd` out of it, and \
                 do NOT touch the parent checkout: your changes reach `main` only through the \
                 merge gate.\n\n",
                projection.cwd.display()
            ));
        } else {
            p.push_str(&format!(
                "You are working in `{}` — the **shared checkout**, the same working tree as \
                 the human and every other quark. There is no separate worktree for you and \
                 nothing to route your work through: whatever you leave uncommitted is at risk, \
                 and whatever you commit lands on the branch that is checked out right now. So \
                 **commit your own work** on the current branch when it is done and green. Two \
                 consequences of sharing the tree, and they are not optional. **Stage by \
                 explicit path — `git add <path> <path>` — listing only the files you \
                 authored this turn.** Never `git add -A`, `git add .` or `git commit -a`: \
                 those sweep up another quark's in-flight edits and the human's, and commit \
                 them under your name. Leave no scratch files behind — check `git ls-files \
                 --others --exclude-standard` before you report.\n\n",
                projection.cwd.display()
            ));
        }
    }

    // 3d. Live Activity. What other quarks are doing *right now*.
    if !projection.live_activities.is_empty() {
        p.push_str("# Live Activity\n");
        p.push_str("The following quarks are currently working in parallel:\n");
        for act in &projection.live_activities {
            p.push_str(&format!(
                "- **@{id}** is {doing}: {detail}\n",
                id = act.quark.as_str(),
                doing = act.doing.label(),
                detail = act.detail
            ));
        }
        p.push_str("\n");
    }

    // 4. Recent field transcript. If older events were dropped to fit the byte
    // budget, SAY SO. A silent truncation is indistinguishable, from inside the
    // model, from the human never having said the thing — and it acts accordingly.
    if !projection.field_window.is_empty() {
        p.push_str("# Recent field (most recent last)\n");
        if projection.field_truncated {
            p.push_str(
                "_Older events were dropped to fit the context budget — this transcript is \
                 INCOMPLETE. If what you need to act correctly is not here, say so and ask, \
                 rather than assuming it was never said._\n\n",
            );
        }
        for e in &projection.field_window {
            if let Kind::Message { body } = &e.kind {
                p.push_str(&render_event_line(&e.from, &e.to, body));
                p.push('\n');
            }
        }
        p.push('\n');
    }

    // 5. Working diff — ATTRIBUTED. In the shared tree this diff is the whole
    // tree's uncommitted state: the human's edits and other quarks' in-flight work,
    // not the quark's own prior output. Handed over unlabelled, a quark reads it as
    // its own and "continues" work it never did. Same fiction class as the merge gate.
    if !projection.git_diff.trim().is_empty() {
        p.push_str("# Current working diff\n");
        if projection.isolated {
            p.push_str("This is the uncommitted diff in your own worktree — your work.\n\n");
        } else {
            p.push_str(
                "This is the uncommitted diff of the **whole shared tree**. It is NOT \
                 necessarily yours: it may contain the human's edits and other quarks' \
                 in-flight work. Do not assume you wrote it, and do not commit any part of \
                 it you did not author.\n\n",
            );
        }
        p.push_str("```diff\n");
        p.push_str(projection.git_diff.trim());
        p.push_str("\n```\n\n");
    }

    // 6. Handoff reminder — how to keep the loop coordinating / quiescing.
    p.push_str("# How to respond\n");
    p.push_str(
        "Reply in Markdown. If a message addresses several quarks (e.g. `@opus do X and @agy \
         do Y`), act ONLY on the part directed at you — the others handle theirs. To delegate, \
         start a line with `@<quark-id>` and the request (only a mention at the START of a line \
         routes — mentions inside prose are ignored).\n\n",
    );

    // The length cap. Stated here so the quark can meet it deliberately.
    p.push_str(&format!(
        "**Be short.** We want Quarks to write shorter, meaningful messages without arbitrary limits. \
         A reply should ideally fit within {lines} lines \
         and {chars} characters. Put the \
         outcome in the first line, keep the evidence to the command and its result, and \
         cut every restatement, preamble, and summary-of-your-summary. TLDR is no good; be concise but complete.\n\n",
        lines = crate::brevity::MAX_LINES,
        chars = crate::brevity::MAX_CHARS,
    ));

    // 6a. Who a finished turn goes back TO — and this depends on the role, because
    // an unaddressed reply excites nobody: it lands in the field for the human.
    //
    // Telling a worker "you report to the orchestrator" and then "drop the @mention
    // when you are done" is a contradiction, and the worker obeys the second one:
    // it finishes, drops the mention, and the orchestrator is never woken. Observed
    // live — agy reported a completed task and the orchestrator took no action,
    // because it was never told there was one. A worker therefore hands back UP the
    // chain; only the quark that actually answers to the human hands back to them.
    if is_worker(projection, self_id) {
        p.push_str(
            "**When your task is complete, start a line with `@orchestrator` and report there.** \
             You report to the orchestrator, not to the human — a reply with no `@mention` \
             excites nobody and your work stops dead in the field. Do not hand back to the \
             human directly.\n\n",
        );
    } else {
        p.push_str(
            "When the overall task is complete, reply WITHOUT any `@mention` to hand control \
             back to the human.\n\n",
        );
    }

    // 6b. Escalation — a worker owns execution, not the call. Addressed by ROLE
    // (`@orchestrator`), never by a hardcoded id, so re-flavouring the team in
    // `team.json` retargets escalation without touching this text.
    if is_worker(projection, self_id) {
        p.push_str(
            "You are a **worker**: you execute the work you are handed, and you report to the \
             orchestrator, not to the human. Resolve implementation details yourself — naming, \
             layout, which helper to use, how to structure a function. Those are yours; make \
             the call and move on. Escalate ONLY what is genuinely not yours to settle: an \
             architectural fork, a scope change, a constraint you cannot satisfy, or a fact \
             that contradicts your task. To escalate, start a line with `@orchestrator` and put \
             the question there; it routes to whoever currently holds that role. Do NOT guess \
             on those, and do NOT stall on the small ones.\n\n",
        );
    }

    // 6c. Availability — the orchestrator is the human's conversational partner, so
    // its turn is what the human waits on. The engine runs turns serially, so a long
    // orchestrator turn IS the chat freezing. Keep its turn short by construction:
    // dispatch the long work and hand back, rather than doing it inline.
    if is_orchestrator(projection, self_id) {
        p.push_str(
            "You are the **orchestrator**: you are the human's conversational partner, the \
             workers report to you, and you carry their work to the human. Three duties.\n\n\
             **Stay available.** The human waits on your turn, and turns run serially — a long \
             orchestrator turn IS the chat freezing. When a request implies long work, do NOT \
             grind through it inline: decide, hand it to a worker (start a line with \
             `@<quark-id>`) or to your own sub-agents, and reply promptly.\n\n\
             **But do not bounce trivial work.** If a task is one or two steps — a small edit, \
             a direct question, a decision you can settle now — just do it. Delegating \
             something you could have finished in the time it took to write the handoff wastes \
             a turn and the human's patience.\n\n\
             **Verify before you relay.** A worker's report is a claim, not a fact. You are the \
             human's only reliable narrator: before you tell the human something is done, check \
             it against the repo — the commit exists, the tests actually pass, the file really \
             changed. If a worker's claim and the repo disagree, the repo wins; say so plainly \
             and without blame. If you did not verify a claim, pass it on as the worker's claim, \
             not as your own finding.\n\n",
        );
    }

    p.push_str(
        "Be truthful about your actions: report only what you actually did and verified this \
         turn, and clearly separate what you PROPOSE from what you have DONE. Never state \
         completed work — commits, passing tests, file edits — that you did not perform. If you \
         could not do something, say so.\n\n\
         # CRITICAL: Response Format Requirement\n\
         You MUST structure your final response exactly as follows. Do NOT skip the evidence (you must copy-paste actual command lines and a concise summary of the output from tests/checks — do NOT dump full logs or entire test suites; keep it under 10 lines of summary output):\n\n\
         **Done**: [Brief outcome summary, including commit hash]\n\n\
         - **Done**:\n\
           - [Brief list of key completed tasks and files changed]\n\
         - **Evidence**: [Copy-paste the exact command and a concise/trimmed summary of the test/check output showing it works — keep it to the summary or last few lines]\n\
         - **Risks**: [Rule 7 security risk note or 'no new attack surface' with explanation]\n\
         - **What I did not verify / clean up**: [Explicitly specify what you did not check or clean up]\n\n\
         Lead directly with the outcome. Avoid preambles like 'I have completed the task', 'Here is the report', or general pleasantries.\n",
    );

    p
}

/// Whether this quark holds the orchestrator role. Only it gets the stay-available
/// clause: a worker grinding through long work blocks nobody's conversation, but the
/// orchestrator doing the same is the human staring at a silent chat.
fn is_orchestrator(projection: &Projection, self_id: &QuarkId) -> bool {
    projection
        .roster
        .iter()
        .any(|c| &c.id == self_id && c.flavor == Flavor::Orchestrator)
}

/// Whether this quark is a worker *and* someone holds the orchestrator role — the
/// only case where telling it to escalate to `@orchestrator` is honest. On a roster
/// with no orchestrator the alias resolves to nobody, so the clause is omitted
/// rather than pointing the worker at a target that cannot be reached.
fn is_worker(projection: &Projection, self_id: &QuarkId) -> bool {
    let self_is_worker = projection
        .roster
        .iter()
        .any(|c| &c.id == self_id && c.flavor == Flavor::Worker);
    let has_orchestrator = projection.roster.iter().any(|c| c.flavor == Flavor::Orchestrator);
    self_is_worker && has_orchestrator
}

#[cfg(test)]
mod tests {
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
            }],
            field_window: vec![Event::new(
                Actor::Human,
                Some(QuarkId::new("claude")),
                Kind::Message { body: "start the auth work".into() },
            )],
            field_truncated: false,
            memory: String::new(),
            memory_path: std::path::PathBuf::new(),
            memory_truncated: false,
            memory_notes_dir: std::path::PathBuf::new(),
            git_diff: String::new(),
            isolated: true,
            cwd: std::path::PathBuf::from("/repo/.hadron/trees/agy"),
            mode: hadron_lattice::Mode::default(),
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
        assert!(p.contains("@<quark-id>"));
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
        });

        let worker = build(&proj, &QuarkId::new("agy"));
        assert!(
            worker.contains("When your task is complete, start a line with `@orchestrator`"),
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
        });

        let orch_prompt = build(&proj, &QuarkId::new("opus"));
        assert!(orch_prompt.contains("You are the **orchestrator**"));
        assert!(orch_prompt.contains("Stay available"));

        let worker_prompt = build(&proj, &QuarkId::new("agy"));
        assert!(!worker_prompt.contains("You are the **orchestrator**"));
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
    /// be shown the memory AND told where to write it — "remember this" without a path
    /// is an instruction it cannot obey. It must also be told where the NOTES are, or
    /// the index's `→ path` lines point at something it was never told it may open.
    #[test]
    fn a_quark_is_shown_the_memory_index_and_told_where_to_write_it() {
        let mut proj = projection("x");
        proj.memory_path = std::path::PathBuf::from("/repo/.hadron/memory/index.md");
        proj.memory_notes_dir = std::path::PathBuf::from("/repo/.hadron/memory/notes");

        // Empty memory still emits the section — otherwise the quark never learns it HAS one.
        let empty = build(&proj, &QuarkId::new("agy"));
        assert!(empty.contains("# What the swarm has learned (memory index)"));
        assert!(empty.contains("nothing has been recorded here yet"));
        assert!(empty.contains("/repo/.hadron/memory/index.md"), "it must know the path");
        assert!(empty.contains("/repo/.hadron/memory/notes"), "and where notes live");
        assert!(empty.contains("shared by every quark"), "and that it is not private");
        assert!(empty.contains("append to it"), "and that writing is its job");

        // A populated index comes back verbatim.
        proj.memory = "- **forge-unwired** — the forge has zero consumers.".into();
        let carried = build(&proj, &QuarkId::new("agy"));
        assert!(carried.contains("- **forge-unwired** — the forge has zero consumers."));
        assert!(!carried.contains("nothing has been recorded here yet"));
        assert!(!carried.contains("index above is CUT"), "it was not truncated");
    }

    /// A memory cut for size must SAY it was cut. Silent truncation is the failure we
    /// killed in the field window: the quark cannot tell "never learned" from "not shown"
    /// and treats the last line it can see as the end of what is known.
    #[test]
    fn a_truncated_memory_index_says_so() {
        let mut proj = projection("x");
        proj.memory_path = std::path::PathBuf::from("/repo/.hadron/memory/index.md");
        proj.memory = "- **a** — a lesson.".into();
        proj.memory_truncated = true;

        let p = build(&proj, &QuarkId::new("agy"));
        assert!(p.contains("index above is CUT"));
    }

    /// No memory path (a mock adapter, an old snapshot) → no section, rather than a
    /// section telling the quark to write to nowhere.
    #[test]
    fn no_memory_section_without_a_path() {
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
}
