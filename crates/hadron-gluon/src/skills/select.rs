use hadron_lattice::QuarkId;

use super::parse::{front_matter_value, split_front_matter};
use super::ResolvedSkill;

/// The skill a turn is in, and the phrase that decided it.
///
/// A turn is in exactly ONE phase — you are writing a plan or executing one, never
/// both — so this is an `Option<Match>`, not a `Vec`. Handing a quark the
/// plan-writing and plan-executing procedures together is how you get a plan that
/// half-implements itself.
///
/// Owns its data (rather than borrowing from the `skills` slice `select` was given)
/// so a caller can pass a freshly-built, short-lived `Vec<ResolvedSkill>` (e.g.
/// `&skills::builtins()`) without fighting the borrow checker to keep it alive
/// through a later `render` call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub id: String,
    pub body: String,
    /// The trigger phrase that matched, quoted back to the quark so a misfire is
    /// visible and debuggable rather than a mysterious extra section in the prompt.
    pub trigger: String,
}

/// Pick the skill for a task, or `None` — which leaves the prompt exactly as it was.
///
/// **Earliest-mentioned action wins.** "Write a plan for X, then have someone review
/// it" is a plan-writing turn: the verb aimed at *this* quark comes first, and the
/// review is what happens next, to somebody else. Ties break by declaration order in
/// `skills`, so the choice is total and deterministic.
pub fn select(task: &str, skills: &[ResolvedSkill]) -> Option<Match> {
    let lower = task.to_lowercase();

    let (_, skill, trigger) = skills
        .iter()
        .filter_map(|skill| {
            skill
                .triggers
                .iter()
                .filter_map(|t| lower.find(t.as_str()).map(|at| (at, t.clone())))
                .min()
                .map(|(at, trigger)| (at, skill, trigger))
        })
        .min_by_key(|(at, _, _)| *at)?;

    Some(Match {
        id: skill.id.clone(),
        body: skill.body.clone(),
        trigger,
    })
}

/// The role a task of this kind prefers (spec 2026-07-20 §3.2). None = no preference.
pub fn preferred_role(skill_name: &str) -> Option<&'static str> {
    match skill_name {
        "writing-plans" | "brainstorming" => Some("architect"),
        "requesting-code-review" | "reviewing-work" => Some("reviewer"),
        "executing-plans" | "subagent-driven-development" => Some("executor"),
        _ => None,
    }
}

/// Who can take the next step, computed by the engine from the live roster — not
/// guessed by the model.
///
/// The whole "another quark tests it" property rests on this being *true at dispatch
/// time*: a disabled seat keeps its roster card (`disable-is-not-unseat`), so naming
/// it as a reviewer would route the work into a void that answers with a gluon
/// warning and nothing else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Handoff {
    /// Quarks that can actually take a turn right now: enabled, not depleted, not you.
    pub peers: Vec<QuarkId>,
    /// The `author:` recorded inside the plan file this task names, read off disk.
    /// `Some` only when the task actually points at a plan that exists.
    pub plan_author: Option<QuarkId>,
}

/// The path of a plan file named in the task, if any — the hook that lets the engine
/// check a *fact* (who wrote it) instead of trusting the turn to admit it.
///
/// Deliberately narrow: a token that mentions a `plans/` directory and ends in `.md`.
/// Punctuation and markdown backticks are trimmed, because a task written by a human
/// says "execute `docs/plans/2026-07-14-foo.md`." with the quotes and the full stop.
pub fn plan_ref(task: &str) -> Option<String> {
    task.split(|c: char| c.is_whitespace() || c == '(' || c == ')' || c == '[' || c == ']' || c == '<' || c == '>' || c == '"' || c == '\'' || c == '`')
        .map(|tok| tok.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '/' && c != '.' && c != '-' && c != '_'))
        .find(|tok| tok.contains("plans/") && tok.ends_with(".md"))
        .map(str::to_string)
}

/// The `author:` line from a plan's YAML front-matter.
///
/// Only the leading `---` block is honoured: an `author:` mentioned in the prose of a
/// plan is discussion, not provenance, and must not be able to reassign authorship.
pub fn plan_author(markdown: &str) -> Option<QuarkId> {
    let (front, _) = split_front_matter(markdown);
    front_matter_value(front?, "author").map(QuarkId::new)
}

/// The `description:` line from a skill's YAML front-matter — the one-liner that goes in
/// the always-on index. Absent front-matter yields `None` rather than a guess.
pub fn description(body: &str) -> Option<&str> {
    let (front, _) = split_front_matter(body);
    front_matter_value(front?, "description")
}

/// The always-on skill index: one line per skill, injected every turn so a quark always
/// knows the full set of procedures available to it and can invoke the right one as the
/// work crosses phases. This is hadron's analog of the Superpowers `using-superpowers`
/// bootstrap (the SessionStart hook), which does not exist over ACP.
pub fn index(skills: &[ResolvedSkill]) -> String {
    let mut out = String::from(
        "\n# Your skills\n\n\
         These are your built-in procedures. The engine hands you a starting one below; \
         invoke any of the others yourself as the work crosses into its kind (a bug while \
         you execute → systematic-debugging; work done → requesting-code-review).\n\n",
    );
    for s in skills {
        out.push_str(&format!("- **{}** — {}\n", s.id, s.description.as_deref().unwrap_or("")));
    }
    out
}

/// Render the selected skill into the working-protocol block: the procedure, who is
/// actually available to take the next step, and — when the engine can prove it — the
/// refusal to let a quark verify its own plan.
///
/// `include_body` is `true` for every quark that actually has a matched skill this turn
/// (both resident/ACP and one-shot/CLI get the full body now, in full, right here —
/// see [`index`] for the always-on menu the rest of the library is known by). `false`
/// names the skill without inlining its body, for a caller that only wants the pointer.
///
/// Takes no `skills` slice: [`Match`] already owns the resolved id/body ([`select`]
/// cloned them out of the slice it was given), so there is nothing left to look up.
pub fn render(m: &Match, self_id: &QuarkId, handoff: &Handoff, include_body: bool) -> String {
    let mut out = String::new();

    let procedure = if include_body {
        format!("\n\n{}\n", m.body.trim())
    } else {
        "\nSee the skill index above for what this procedure covers.\n".to_string()
    };
    out.push_str(&format!(
        "\n# Skill for this turn: {id}\n\n\
         The engine selected this procedure because your task says \"{trigger}\". It is \
         part of your working protocol for this turn — not a suggestion, and not \
         optional. If it is the wrong procedure for what you were actually asked, say so \
         in your report instead of half-following it.\n{procedure}",
        id = m.id,
        trigger = m.trigger,
    ));

    // Who can take the next step. A named peer is a handoff; "@<quark-id>" is a wish.
    out.push_str("\n## Who can take the next step\n\n");
    if handoff.peers.is_empty() {
        out.push_str(
            "**Nobody. You are the only quark that can take a turn right now** — every \
             other seat is disabled, depleted, or absent. Do the work, and say plainly in \
             your report that it has NOT been independently reviewed and by whom it still \
             should be. Do not invent a reviewer: a line addressed to a disabled quark \
             excites nobody and the work stops dead in the field.\n",
        );
    } else {
        let named = handoff
            .peers
            .iter()
            .map(|p| format!("`@{}`", p.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!(
            "These quarks can take a turn right now: {named}. They are enabled and not \
             depleted — the engine checked at dispatch, so a line starting with one of \
             these names will actually reach them.\n\n\
             Hand the next step to one of them **by name**, and say what you want checked. \
             Do not carry the whole cycle yourself: the failure this swarm keeps repeating \
             is one quark writing, executing, and grading its own work, and reporting it \
             green.\n"
        ));
    }

    // The one thing here the engine can *prove*: who wrote the plan.
    if let Some(author) = &handoff.plan_author {
        if author == self_id {
            out.push_str(&format!(
                "\n## Separation of duties — you wrote this plan\n\n\
                 **The plan you were handed records `author: {a}`, which is you.** You may \
                 not be both its author and the quark who verifies it: you would be \
                 checking it against the same assumptions that produced it, which is how a \
                 mechanism gets reported as working on the day it is written and turns out \
                 never to have been wired.\n\n",
                a = author.as_str(),
            ));
            if handoff.peers.is_empty() {
                out.push_str(
                    "No peer is available to take it, so proceed — but report explicitly \
                     that this work was written and verified by the same quark, and is \
                     therefore unreviewed.\n",
                );
            } else {
                out.push_str(
                    "Hand the verification to one of the peers named above, by name, and \
                     tell them what to check.\n",
                );
            }
        } else {
            out.push_str(&format!(
                "\n## Provenance\n\nThis plan was written by `@{a}`, not by you — so you are \
                 an independent pair of eyes on it, which is the point. If it turns out to be \
                 wrong about the code, say so and hand it back to `@{a}` rather than quietly \
                 working around it.\n",
                a = author.as_str(),
            ));
        }
    }

    out
}
