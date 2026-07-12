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

    // 3. The task.
    p.push_str("# Your task\n");
    p.push_str(projection.task.trim());
    p.push_str("\n\n");

    // 3b. Authority this turn — what the current mode actually permits, so the
    // model narrates honestly instead of confabulating actions it cannot take.
    p.push_str("# Your authority this turn\n");
    p.push_str(mode_guidance(projection.mode));
    p.push_str("\n\n");

    // 4. Recent field transcript.
    if !projection.field_window.is_empty() {
        p.push_str("# Recent field (most recent last)\n");
        for e in &projection.field_window {
            if let Kind::Message { body } = &e.kind {
                p.push_str(&render_event_line(&e.from, &e.to, body));
                p.push('\n');
            }
        }
        p.push('\n');
    }

    // 5. Working diff.
    if !projection.git_diff.trim().is_empty() {
        p.push_str("# Current working diff\n```diff\n");
        p.push_str(projection.git_diff.trim());
        p.push_str("\n```\n\n");
    }

    // 6. Handoff reminder — how to keep the loop coordinating / quiescing.
    p.push_str("# How to respond\n");
    p.push_str(
        "Reply in Markdown. If a message addresses several quarks (e.g. `@opus do X and @agy \
         do Y`), act ONLY on the part directed at you — the others handle theirs. To delegate, \
         start a line with `@<quark-id>` and the request (only a mention at the START of a line \
         routes — mentions inside prose are ignored). When the overall task is complete, reply \
         WITHOUT any `@mention` to hand control back to the human.\n\n",
    );

    // 6b. Escalation — a worker owns execution, not the call. Addressed by ROLE
    // (`@orchestrator`), never by a hardcoded id, so re-flavouring the team in
    // `team.json` retargets escalation without touching this text.
    if is_worker(projection, self_id) {
        p.push_str(
            "You are a **worker**: you execute the work you are handed. If you hit a decision, \
             an ambiguity, or a question that is not yours to settle — a design fork, a scope \
             change, anything the task does not already answer — do NOT guess. Start a line with \
             `@orchestrator` and put the question there; it routes to whoever currently holds \
             that role.\n\n",
        );
    }

    // 6c. Availability — the orchestrator is the human's conversational partner, so
    // its turn is what the human waits on. The engine runs turns serially, so a long
    // orchestrator turn IS the chat freezing. Keep its turn short by construction:
    // dispatch the long work and hand back, rather than doing it inline.
    if is_orchestrator(projection, self_id) {
        p.push_str(
            "You are the **orchestrator**: you are the human's conversational partner, and the \
             human waits on your turn. Stay available. When a request implies long work, do NOT \
             grind through it inline — decide, hand it to a worker (start a line with \
             `@<quark-id>`) or to your own sub-agents, and reply promptly so the human can keep \
             talking to you. Doing the long work yourself is what makes the chat feel frozen. \
             Short answers, quick questions, and decisions you can settle immediately are yours \
             to answer directly — it is the long, grinding work that should be dispatched.\n\n",
        );
    }

    p.push_str(
        "Be truthful about your actions: report only what you actually did and verified this \
         turn, and clearly separate what you PROPOSE from what you have DONE. Never state \
         completed work — commits, passing tests, file edits — that you did not perform. If you \
         could not do something, say so.\n",
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
            roster: vec![QuarkCard {
                id: QuarkId::new("agy"),
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
            git_diff: String::new(),
            mode: hadron_lattice::Mode::default(),
        }
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

    /// The orchestrator is told to stay available and dispatch long work; a worker
    /// is not (a worker grinding away blocks nobody's conversation).
    #[test]
    fn only_the_orchestrator_is_told_to_stay_available() {
        let mut proj = projection("x");
        proj.roster.push(QuarkCard {
            id: QuarkId::new("opus"),
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
