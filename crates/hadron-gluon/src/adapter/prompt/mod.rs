use hadron_lattice::{Actor, Flavor, Kind, Mode, Projection, QuarkCard, QuarkId};

/// How a peer is NAMED to a quark: its display name when it has one, else its raw
/// id. Quarks addressed each other by raw id (`@acp-claude`) because the prompt only
/// ever showed ids; naming peers by display name here makes them write `@GoogleGirl`
/// instead. Safe because the router resolves a display-name mention back to the seat
/// (`match_longest_mention` tries id AND display_name), so nothing stops resolving.
fn display_for(roster: &[QuarkCard], id: &QuarkId) -> String {
    roster
        .iter()
        .find(|c| &c.id == id)
        .and_then(|c| c.display_name.clone())
        .unwrap_or_else(|| id.as_str().to_string())
}

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
/// Quark endpoints are named by display name (via `roster`) when they have one.
fn render_event_line(
    from: &Actor,
    to: &Option<QuarkId>,
    body: &str,
    roster: &[QuarkCard],
) -> String {
    let from_s = match from {
        Actor::Human => "human".to_string(),
        Actor::Gluon => "gluon".to_string(),
        Actor::Quark(q) => display_for(roster, q),
    };
    match to {
        Some(t) => format!("**{from_s} → {}:** {body}", display_for(roster, t)),
        None => format!("**{from_s}:** {body}"),
    }
}

/// Build the full Markdown prompt handed to a quark's CLI for one turn.
/// Deterministic and side-effect-free so it can be unit-tested exactly.
/// `self_id` is the quark's own handle — a human message can address several
/// quarks at once ("@alpha X and @beta Y"), each of whom receives the whole
/// message, so each must know which mentions are its part.
pub fn build(projection: &Projection, self_id: &QuarkId) -> String {
    let mut p = String::new();

    p.push_str(
        "# CRITICAL DIRECTIVE: FOLLOW THE STANDARD MODEL AND ITS SKILLS\n\
         You are a quark in the hadron chamber. You MUST obey the Standard Model invariants \
         below and follow the skills they hand you — the procedures for planning, executing, \
         debugging, and reviewing work. These are built in to this prompt; do not rely on any \
         tooling of your own. Do NOT ignore the invariants under any circumstances.\n\n"
    );

    // 0. Identity — which quark is being excited. A multi-addressee human
    // message hands the SAME text to each named quark, so the model must know
    // its own handle to act on only its part.
    p.push_str(&format!(
        "# Who you are\nYou are `@{}` in this swarm.\n\n",
        display_for(&projection.roster, self_id)
    ));

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
                id = display_for(&projection.roster, &act.quark),
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
                p.push_str(&render_event_line(&e.from, &e.to, body, &projection.roster));
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
        "Reply in Markdown. If a message addresses several quarks (e.g. `@alpha do X and @beta \
         do Y`), act ONLY on the part directed at you — the others handle theirs. To delegate, \
         start a line with `@<name>` and the request — use a peer's name exactly as it appears \
         in Live Activity and the transcript above (its display name when it has one, otherwise \
         its id). Only a mention at the START of a line routes — mentions inside prose are \
         ignored.\n\n",
    );

    // Brevity as discipline, not a hard cut. The engine does NOT trim replies — a
    // truncated report can hide the one line the human needed — so shortness is asked
    // for and explained, never enforced by cutting bytes.
    p.push_str(
        "**Be short.** Write shorter, meaningful messages. Put the outcome in the first \
         line, keep the evidence to the command and its result, and cut every restatement, \
         preamble, and summary-of-your-summary. No TL;DR of your own answer, and no wall of \
         text for a trivial ask — answer at the length the question deserves. Be concise but \
         complete: never drop a critical detail just to be brief.\n\n",
    );

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
         If you performed actual work (e.g. edited files, ran commands, or committed changes) this turn, you MUST structure your final response exactly as follows. Do NOT skip the evidence (you must copy-paste actual command lines and a concise summary of the output from tests/checks — do NOT dump full logs or entire test suites; keep it under 10 lines of summary output):\n\n\
         **Done**: [Brief outcome summary, including commit hash]\n\n\
         - **Done**:\n\
           - [Brief list of key completed tasks and files changed]\n\
         - **Evidence**: [Copy-paste the exact command and a concise/trimmed summary of the test/check output showing it works — keep it to the summary or last few lines]\n\
         - **Risks**: [Rule 7 security risk note or 'no new attack surface' with explanation]\n\
         - **What I did not verify / clean up**: [Explicitly specify what you did not check or clean up]\n\n\
         If this turn was a normal conversation, query, or liveness check without any workspace modifications or tool execution, do NOT use this structured format. Instead, reply with a short, direct, and concise answer.\n\n\
         Lead directly with the outcome or answer. Avoid preambles like 'I have completed the task', 'Here is the report', or general pleasantries.\n",
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
mod tests;
