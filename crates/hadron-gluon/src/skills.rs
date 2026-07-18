//! Skills — the procedure a quark follows for a **kind** of work.
//!
//! The Standard Model says how to work at all; a skill says how to do *this sort of
//! thing*. Writing a plan, executing one, and reviewing one are three different jobs
//! with three different failure modes, and a quark handed all three sets of rules at
//! once follows none of them.
//!
//! Two properties make this real rather than decorative:
//!
//! 1. **The engine picks the skill, not the model.** [`select`] is a pure function of
//!    the task text, so the same task always yields the same procedure. A model that
//!    *decides* whether to follow a process is a model that skips it under pressure.
//! 2. **The reviewer is somebody else.** The thing this swarm keeps getting wrong is
//!    a quark grading its own homework — reporting a mechanism as live because it
//!    compiled. So the engine computes who *else* is available (see [`Handoff`]) and
//!    names them, and when a quark is handed a plan it wrote itself, it is told to
//!    hand the verification to a peer instead. That check reads ground truth — the
//!    `author:` line in the plan file on disk — not the model's word for it.
//!
//! Built-in bodies are `include_str!`d individually and never globbed: a skill file
//! that nobody lists in [`SKILLS`] is a skill that silently never loads, which is
//! the exact trap (`compiled-is-not-running`) this module exists to close. Custom
//! skills loaded from `~/.hadron/skills` and `<repo>/.hadron/skills` (see
//! [`load_skills`]) are the deliberate exception: those directories ARE globbed for
//! `*.md`, because a human placing a file there and expecting it to load is the
//! entire point of that feature — the trap being closed there is different (a
//! name-less file being silently mis-keyed by its filename instead of loudly
//! skipped), not "did anyone remember to list it".

use std::fs;
use std::path::{Path, PathBuf};

use hadron_lattice::QuarkId;

/// One procedure, compiled into the binary.
///
/// `triggers` are matched case-insensitively against the task text. Keep them
/// specific: a trigger that fires on the bare word "plan" would attach the
/// plan-writing procedure to every turn that merely mentions one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Skill {
    /// Stable name, also what the prompt shows the quark ("Skill for this turn").
    pub id: &'static str,
    /// Phrases that put a turn in this skill's phase.
    pub triggers: &'static [&'static str],
    /// The procedure itself, as handed to the model.
    pub body: &'static str,
}

/// Every skill the engine can inject. Adding a file to `invariants/skills/` does
/// nothing until it is listed here — that is deliberate, and the test below asserts
/// each listed body is non-empty so a typo'd path fails the gate instead of shipping
/// an empty rule.
pub const SKILLS: &[Skill] = &[
    Skill {
        id: "writing-plans",
        // A trigger that MISSES is invisible — the turn just silently gets no
        // procedure — so cover the phrasings a tired human actually types, not the
        // one canonical form.
        triggers: &[
            "write a plan",
            "write the plan",
            "write up a plan",
            "writing a plan",
            "draft a plan",
            "draft the plan",
            "create a plan",
            "make a plan",
            "prepare a plan",
            "come up with a plan",
            "put together a plan",
            "implementation plan",
            "plan out",
            "plan for",
        ],
        body: include_str!("../invariants/skills/writing-plans.md"),
    },
    Skill {
        id: "executing-plans",
        triggers: &[
            "execute the plan",
            "execute this plan",
            "execute plan",
            "executing the plan",
            "implement the plan",
            "implement this plan",
            "carry out the plan",
            "work through the plan",
            "follow the plan",
            "pick up the plan",
            "start on the plan",
            "run the plan",
            // A bare path to a plan file is itself an instruction to execute it.
            "docs/plans/",
        ],
        body: include_str!("../invariants/skills/executing-plans.md"),
    },
    Skill {
        id: "reviewing-work",
        triggers: &[
            "review the plan",
            "review this plan",
            "review the work",
            "review his work",
            "review her work",
            "review my work",
            "review the commit",
            "review the change",
            "peer review",
            "code review",
            "verify the work",
            "verify the plan",
            "verify his",
            "verify her",
            "check his work",
            "check her work",
            "check the plan",
            "test the plan",
            "double-check the work",
        ],
        body: include_str!("../invariants/skills/reviewing-work.md"),
    },
    Skill {
        id: "brainstorming",
        triggers: &["brainstorm", "explore ideas", "think about", "design feature", "design the feature"],
        body: include_str!("../invariants/skills/brainstorming.md"),
    },
    Skill {
        id: "dispatching-parallel-agents",
        triggers: &["dispatch agents", "parallel tasks", "run in parallel", "do these in parallel"],
        body: include_str!("../invariants/skills/dispatching-parallel-agents.md"),
    },
    Skill {
        id: "finishing-a-development-branch",
        triggers: &["finish the branch", "ready to merge", "create pr", "cleanup branch", "wrap up the branch"],
        body: include_str!("../invariants/skills/finishing-a-development-branch.md"),
    },
    Skill {
        id: "receiving-code-review",
        triggers: &["received code review", "review feedback", "address comments", "fix the review comments"],
        body: include_str!("../invariants/skills/receiving-code-review.md"),
    },
    Skill {
        id: "requesting-code-review",
        triggers: &["request review", "ready for review", "please review", "needs review"],
        body: include_str!("../invariants/skills/requesting-code-review.md"),
    },
    Skill {
        id: "subagent-driven-development",
        triggers: &["use subagents", "dispatch subagents", "subagent driven"],
        body: include_str!("../invariants/skills/subagent-driven-development.md"),
    },
    Skill {
        id: "systematic-debugging",
        triggers: &["debug", "fix bug", "test failure", "investigate error", "why does it fail", "systematic debugging"],
        body: include_str!("../invariants/skills/systematic-debugging.md"),
    },
    Skill {
        id: "test-driven-development",
        triggers: &["tdd", "write tests first", "test driven"],
        body: include_str!("../invariants/skills/test-driven-development.md"),
    },
    Skill {
        id: "using-git-worktrees",
        triggers: &["use worktree", "create worktree", "isolated workspace", "git worktree"],
        body: include_str!("../invariants/skills/using-git-worktrees.md"),
    },
    Skill {
        id: "using-superpowers",
        triggers: &["use superpowers", "what are your superpowers"],
        body: include_str!("../invariants/skills/using-superpowers.md"),
    },
    Skill {
        id: "verification-before-completion",
        triggers: &["verify completion", "check before done", "verify before", "double check completion"],
        body: include_str!("../invariants/skills/verification-before-completion.md"),
    },
    Skill {
        id: "writing-skills",
        triggers: &["write a skill", "create skill", "edit skill", "create a skill", "new skill"],
        body: include_str!("../invariants/skills/writing-skills.md"),
    },
];

/// An owned skill — the same shape as [`Skill`], but with `String`/`Vec<String>`
/// instead of `&'static` slices so a skill parsed off disk at runtime (which has no
/// `'static` backing) can sit in the same corpus as the compiled-in set.
///
/// [`builtins`] maps [`SKILLS`] into this shape; [`load_skills`] extends that with
/// whatever `.md` files it finds under the global and repo skill directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedSkill {
    /// Stable name; the merge key. A repo skill with the same `id` as a built-in
    /// REPLACES it (see [`load_skills`]).
    pub id: String,
    /// Phrases that put a turn in this skill's phase (see [`select`]).
    pub triggers: Vec<String>,
    /// The procedure itself, as handed to the model. For a built-in this is the
    /// `include_str!`'d file verbatim (front-matter and all — preserved for
    /// byte-for-byte back-compat with the pre-`ResolvedSkill` renderer). For a
    /// loaded `.md` file this is the body ONLY, front-matter stripped (see
    /// [`load_skills`]).
    pub body: String,
    /// The one-liner from front-matter `description:`, quoted in [`index`].
    pub description: Option<String>,
    /// Front-matter `tools:` — engine-level tool gating while this skill is active
    /// (spec §3.2). Not enforced yet; carried here so a later task can read it.
    /// Always empty for built-ins.
    pub tools: Vec<String>,
}

/// The compiled-in [`SKILLS`], mapped into owned [`ResolvedSkill`]s. The starting
/// point for [`load_skills`], and what every existing call site uses today (Task 1
/// wires callers to `&builtins()`; Task 2 adds the real directory loading).
pub fn builtins() -> Vec<ResolvedSkill> {
    SKILLS
        .iter()
        .map(|s| ResolvedSkill {
            id: s.id.to_string(),
            triggers: s.triggers.iter().map(|t| t.to_string()).collect(),
            body: s.body.to_string(),
            description: description(s.body).map(str::to_string),
            tools: Vec::new(),
        })
        .collect()
}

/// Load custom skills from disk and merge them with the built-ins.
///
/// Starts from [`builtins`], then walks `global_dir` and then `repo_dir` (each
/// `None` is simply skipped — a missing directory is not an error), reading every
/// `*.md` file in each (sorted by filename within a directory, for determinism).
/// Each file is upserted into the corpus **by id**: a later source replaces an
/// earlier one with the same id, so the precedence is
/// `built-in < global < repo` — a repo skill wins over a global skill wins over a
/// built-in, and within one directory the last file wins if two files somehow
/// declare the same `name:` (sorted order makes that deterministic too).
///
/// With `global_dir: None, repo_dir: None` this returns exactly [`builtins`] — no
/// directory is walked, so behaviour with no custom skills installed is unchanged.
pub fn load_skills(global_dir: Option<&Path>, repo_dir: Option<&Path>) -> Vec<ResolvedSkill> {
    let mut skills = builtins();

    for dir in [global_dir, repo_dir].into_iter().flatten() {
        for loaded in load_dir(dir) {
            upsert(&mut skills, loaded);
        }
    }

    skills
}

/// Insert `skill`, replacing any existing entry with the same `id` in place (so
/// later sources keep their position relative to unrelated skills — only the
/// overridden id's content changes, not the corpus order).
fn upsert(skills: &mut Vec<ResolvedSkill>, skill: ResolvedSkill) {
    if let Some(existing) = skills.iter_mut().find(|s| s.id == skill.id) {
        *existing = skill;
    } else {
        skills.push(skill);
    }
}

/// Read every `*.md` file directly under `dir` (non-recursive) as a candidate
/// skill. A missing or unreadable directory yields no skills, silently — the
/// caller passes `None` for "not configured" and an absent `~/.hadron/skills` on a
/// machine that has never used custom skills is the same case, not an error.
fn load_dir(dir: &Path) -> Vec<ResolvedSkill> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    // Sorted so two files in the same directory resolve deterministically, and so
    // the merge order (and hence any same-directory override) doesn't depend on
    // the OS's directory-listing order.
    paths.sort();

    paths
        .into_iter()
        .filter_map(|path| {
            let text = fs::read_to_string(&path).ok()?;
            match parse_skill_file(&text) {
                Some(skill) => Some(skill),
                None => {
                    // A skill file with no `name:` front-matter has no merge key —
                    // rather than guess one from the filename (which would make the
                    // id depend on how the file happens to be named, not on what
                    // its author declared), it is skipped. Silent would be worse: a
                    // typo'd or missing `name:` should be visible, not a skill that
                    // silently never loads.
                    eprintln!(
                        "hadron-gluon: skipping skill file {} — missing required `name:` front-matter",
                        path.display()
                    );
                    None
                }
            }
        })
        .collect()
}

/// Parse one skill `.md` file's front-matter + body into a [`ResolvedSkill`].
/// `None` when there is no front-matter block at all, or the block has no `name:`
/// — both cases the caller reports as "skipped: missing `name:`".
fn parse_skill_file(text: &str) -> Option<ResolvedSkill> {
    let (front, body) = split_front_matter(text);
    let front = front?;

    let id = front_matter_value(front, "name")?.to_string();
    let description = front_matter_value(front, "description").map(str::to_string);
    // Lowercased here, once, at parse time: `select` lowercases the task text before
    // matching, and built-in triggers are already all-lowercase in source — a custom
    // skill authored with `triggers: [Foo]` must not silently never match because its
    // author capitalised it.
    let triggers = front_matter_value(front, "triggers")
        .map(parse_list_value)
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect();
    let tools = front_matter_value(front, "tools").map(parse_list_value).unwrap_or_default();

    Some(ResolvedSkill {
        id,
        triggers,
        body: body.trim().to_string(),
        description,
        tools,
    })
}

/// Split a `.md` file's leading `---`-fenced front-matter from its body. Returns
/// `(front_matter_lines, body)`; `front_matter_lines` is `None` when the text does
/// not open with a front-matter block, and `body` is then the whole input
/// unchanged. This is the one place the `---` fence is parsed — [`description`]
/// and [`plan_author`] both go through it rather than re-splitting themselves.
fn split_front_matter(markdown: &str) -> (Option<&str>, &str) {
    match markdown.strip_prefix("---").and_then(|rest| rest.split_once("\n---")) {
        Some((front, body)) => (Some(front), body.trim_start_matches('\n')),
        None => (None, markdown),
    }
}

/// The value of a `key:` line within a front-matter block, or `None` if the key is
/// absent or its value is empty. Shared by every single-line front-matter field
/// (`name:`, `description:`, `author:`, `triggers:`, `tools:`).
fn front_matter_value<'a>(front: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    front.lines().find_map(|line| {
        let value = line.trim().strip_prefix(prefix.as_str())?.trim();
        (!value.is_empty()).then_some(value)
    })
}

/// Parse a front-matter list value in either of two forms:
/// - a YAML inline list: `[foo, bar, "baz qux"]`
/// - a bare comma-separated string: `foo, bar, baz qux`
///
/// Multi-line YAML lists (`triggers:\n  - foo\n  - bar`) are NOT supported — every
/// entry must be on the `key:` line itself. Surrounding quotes on an entry are
/// stripped; empty entries (a trailing comma, `[]`) are dropped.
fn parse_list_value(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    let inner = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(raw);

    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

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
    task.split_whitespace()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// A skill whose `include_str!` points at the wrong path would compile only if the
    /// file existed, but an EMPTY file compiles fine and injects nothing — a rule the
    /// quark is silently never given.
    #[test]
    fn every_listed_skill_has_a_body() {
        assert!(!SKILLS.is_empty());
        for s in SKILLS {
            assert!(
                s.body.len() > 200,
                "skill `{}` has an empty or stub body ({} bytes) — it would inject nothing",
                s.id,
                s.body.len()
            );
            assert!(!s.triggers.is_empty(), "skill `{}` can never be selected", s.id);
        }
    }

    /// The always-on index names every skill with the description from its front-matter
    /// — the "here are your procedures" list a quark gets every turn.
    #[test]
    fn the_index_lists_every_skill_with_its_description() {
        let idx = index(&builtins());
        for s in SKILLS {
            assert!(idx.contains(s.id), "index is missing skill `{}`", s.id);
        }
        assert!(
            description(SKILLS[0].body).is_some(),
            "a skill must carry a `description:` line for the index to quote"
        );
    }

    /// **The self-contained invariant.** No skill body may point at a companion file the
    /// quark cannot open — every quark now gets only the index (a one-line summary) plus
    /// the single active skill's body, so a body that leans on a sibling file or another
    /// skill's prose has nothing to resolve against. "in this directory" and
    /// "references/" are the Superpowers dangling-reference shapes; if one creeps back
    /// with the next skill sync, this fails instead of shipping a dead end.
    #[test]
    fn no_skill_body_dangles_a_reference_the_quark_cannot_follow() {
        for s in SKILLS {
            let b = s.body.to_lowercase();
            assert!(!b.contains("in this directory"), "`{}` still says 'in this directory'", s.id);
            assert!(!b.contains("references/"), "`{}` still points at 'references/'", s.id);
        }
    }

    /// `render`'s `include_body` toggle: `true` inlines the procedure (what every quark
    /// gets now, resident or one-shot alike); `false` only names the skill and points back
    /// at the always-on index, without repeating the body.
    #[test]
    fn render_include_body_toggles_the_procedure_text() {
        let m = select("write a plan for X", &builtins()).unwrap();
        let me = QuarkId::new("opus");

        let full = render(&m, &me, &Handoff::default(), true);
        assert!(full.contains("author: <your quark id>"), "include_body=true must carry the body");

        let pointer = render(&m, &me, &Handoff::default(), false);
        assert!(pointer.contains("writing-plans"), "the skill is still named");
        assert!(
            !pointer.contains("author: <your quark id>"),
            "include_body=false must NOT repeat the body"
        );
        assert!(pointer.contains("skill index above"), "and must point back at the index, not a corpus");
    }

    #[test]
    fn a_task_with_no_trigger_selects_nothing() {
        // The no-match path must be a true no-op: unchanged behaviour for every turn
        // that isn't plan work.
        assert!(select("fix the completion popup so it stops being clipped", &builtins()).is_none());
        assert!(select("", &builtins()).is_none());
    }

    #[test]
    fn writing_a_plan_selects_the_writing_skill() {
        let m = select("Write a plan for the ACP auth work", &builtins()).expect("should match");
        assert_eq!(m.id, "writing-plans");
        assert_eq!(m.trigger, "write a plan");
    }

    #[test]
    fn executing_a_plan_selects_the_executing_skill() {
        let m = select("Execute the plan at docs/plans/2026-07-14-acp-auth.md", &builtins()).expect("match");
        assert_eq!(m.id, "executing-plans");
    }

    #[test]
    fn a_bare_plan_path_is_enough_to_select_executing() {
        let m = select("take docs/plans/2026-07-14-acp-auth.md and get it done", &builtins()).expect("match");
        assert_eq!(m.id, "executing-plans");
    }

    #[test]
    fn reviewing_selects_the_review_skill() {
        let m = select("Review the plan agy wrote and tell me if it holds", &builtins()).expect("match");
        assert_eq!(m.id, "reviewing-work");
    }

    /// The turn has ONE phase. "Write a plan, then someone reviews it" is a writing
    /// turn — the review is the next quark's job, and handing this quark both
    /// procedures at once is how it ends up doing neither.
    #[test]
    fn the_earliest_mentioned_action_wins() {
        let m = select("Write a plan for X, then have someone code review it", &builtins()).expect("match");
        assert_eq!(m.id, "writing-plans", "the verb aimed at THIS quark comes first");

        let m = select("Review the plan, do not write a plan yourself", &builtins()).expect("match");
        assert_eq!(m.id, "reviewing-work");
    }

    /// A MISSED trigger is silent — the turn simply gets no procedure and nobody can
    /// tell — so the phrasings a tired human actually types must all land.
    #[test]
    fn the_phrasings_a_human_actually_types_all_land() {
        let skills = builtins();
        for task in [
            "write up a plan for the auth work",
            "draft the plan first",
            "put together a plan for #34",
            "come up with a plan",
            "plan out the ACP auth work",
        ] {
            assert_eq!(
                select(task, &skills).map(|m| m.id),
                Some("writing-plans".to_string()),
                "missed: {task:?}"
            );
        }

        for task in ["implement this plan", "pick up the plan", "run the plan and commit"] {
            assert_eq!(
                select(task, &skills).map(|m| m.id),
                Some("executing-plans".to_string()),
                "missed: {task:?}"
            );
        }

        for task in ["review my work", "check the plan agy wrote", "verify the plan"] {
            assert_eq!(
                select(task, &skills).map(|m| m.id),
                Some("reviewing-work".to_string()),
                "missed: {task:?}"
            );
        }
    }

    #[test]
    fn selection_is_case_insensitive() {
        assert_eq!(select("WRITE A PLAN for X", &builtins()).unwrap().id, "writing-plans");
    }

    #[test]
    fn plan_ref_finds_a_path_through_human_punctuation() {
        assert_eq!(
            plan_ref("execute `docs/plans/2026-07-14-foo.md`, please."),
            Some("docs/plans/2026-07-14-foo.md".to_string())
        );
        assert_eq!(plan_ref("no plan here"), None);
        // A directory is not a plan, and a non-markdown file is not one either.
        assert_eq!(plan_ref("look in docs/plans/ for it"), None);
        assert_eq!(plan_ref("see docs/plans/foo.txt"), None);
    }

    #[test]
    fn plan_author_reads_the_front_matter() {
        let md = "---\nauthor: opus\nstatus: draft\n---\n\n# A plan\n";
        assert_eq!(plan_author(md), Some(QuarkId::new("opus")));
    }

    /// Provenance is what the engine *checks*, so it must not be forgeable by prose:
    /// a plan that merely discusses an author has no author.
    #[test]
    fn an_author_in_the_prose_is_not_provenance() {
        let md = "# A plan\n\nauthor: opus\n";
        assert_eq!(plan_author(md), None);

        let md = "---\nstatus: draft\n---\n\nauthor: agy\n";
        assert_eq!(plan_author(md), None);
    }

    #[test]
    fn a_quark_handed_its_own_plan_is_told_to_hand_off() {
        let me = QuarkId::new("opus");
        let m = select("execute the plan", &builtins()).unwrap();
        let handoff = Handoff {
            peers: vec![QuarkId::new("agy")],
            plan_author: Some(QuarkId::new("opus")),
        };

        let out = render(&m, &me, &handoff, true);
        assert!(out.contains("you wrote this plan"), "must refuse self-verification:\n{out}");
        assert!(out.contains("`@agy`"), "must name the peer who can take it:\n{out}");
    }

    /// The inverse, and the one that must NOT fire: a plan written by someone else is
    /// exactly the case we want executed, not bounced.
    #[test]
    fn a_plan_written_by_a_peer_is_not_a_conflict() {
        let me = QuarkId::new("opus");
        let m = select("execute the plan", &builtins()).unwrap();
        let handoff = Handoff {
            peers: vec![QuarkId::new("agy")],
            plan_author: Some(QuarkId::new("agy")),
        };

        let out = render(&m, &me, &handoff, true);
        assert!(!out.contains("you wrote this plan"));
        assert!(out.contains("independent pair of eyes"));
    }

    /// Live state, not a hypothetical: opus is disabled and acp-agy cannot boot, so a
    /// lone quark is a real Saturday-night roster. It must not be told to hand work to
    /// nobody.
    #[test]
    fn a_lone_quark_is_told_it_is_alone_rather_than_routed_into_the_void() {
        let me = QuarkId::new("opus");
        let m = select("write a plan for X", &builtins()).unwrap();
        let out = render(&m, &me, &Handoff::default(), true);

        assert!(out.contains("only quark"), "must say it is alone:\n{out}");
        assert!(out.contains("NOT been independently reviewed"));
    }

    /// Not an assertion — an affordance. `--ignored --nocapture` prints exactly what a
    /// quark is handed for a given task, so a misfiring trigger is something you can
    /// LOOK at instead of guess about.
    #[test]
    #[ignore = "prints the rendered skill; run with --ignored --nocapture"]
    fn show_me_what_a_quark_actually_receives() {
        let task = "execute the plan at docs/plans/2026-07-14-acp-auth.md";
        let m = select(task, &builtins()).expect("should select a skill");
        let out = render(
            &m,
            &QuarkId::new("opus"),
            &Handoff {
                peers: vec![QuarkId::new("agy"), QuarkId::new("acp-claude")],
                plan_author: Some(QuarkId::new("opus")),
            },
            true,
        );
        println!("=== task: {task}\n{out}");
    }

    #[test]
    fn the_selected_body_is_actually_injected() {
        let m = select("write a plan for X", &builtins()).unwrap();
        let out = render(&m, &QuarkId::new("opus"), &Handoff::default(), true);
        assert!(out.contains("Skill for this turn: writing-plans"));
        assert!(out.contains("author: <your quark id>"), "the real body must be present");
        assert!(out.contains("\"write a plan\""), "the trigger must be quoted back");
    }

    // --- Additive back-compat proof --------------------------------------------
    //
    // These pin the EXACT text `index`/`render` produce for a controlled, made-up
    // skill set — not just "contains" assertions — so a change to the format
    // strings (as opposed to a change to some real skill's prose, which is not
    // this refactor's concern) fails these tests. Using a synthetic skill set
    // rather than `builtins()` keeps the pin independent of the actual invariant
    // files' wording, which is free to change for unrelated reasons.

    #[test]
    #[ignore = "temp: regenerate the real-builtins() index fixture"]
    fn zzz_dump_real_index_fixture() {
        std::fs::write(
            concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/builtins_index_snapshot.txt"),
            index(&builtins()),
        )
        .unwrap();
    }

    /// **The actual back-compat pin.** Unlike the synthetic-skill test below (which
    /// pins the format independent of any real skill's prose), this pins `index`'s
    /// output for the REAL `builtins()` — the whole reason this refactor exists is
    /// that the engine's byte-stable prompt prefix (and its cache) must not move.
    /// The fixture is checked in; regenerate it deliberately (via
    /// `zzz_dump_real_index_fixture`, `--ignored`) when a skill's `name`/
    /// `description`/`SKILLS` order actually changes — never to silence this test.
    #[test]
    fn index_over_builtins_matches_the_pinned_fixture() {
        let fixture = include_str!("../testdata/builtins_index_snapshot.txt");
        assert_eq!(index(&builtins()), fixture, "index(&builtins()) output moved — was that intentional?");
    }

    #[test]
    fn index_snapshot_pins_the_exact_line_format() {
        let skills = vec![
            ResolvedSkill {
                id: "alpha".to_string(),
                triggers: vec!["do alpha".to_string()],
                body: "alpha body".to_string(),
                description: Some("does alpha things".to_string()),
                tools: vec![],
            },
            ResolvedSkill {
                id: "beta".to_string(),
                triggers: vec!["do beta".to_string()],
                body: "beta body".to_string(),
                description: None,
                tools: vec![],
            },
        ];

        let expected = "\n# Your skills\n\nThese are your built-in procedures. The engine hands you a starting one below; invoke any of the others yourself as the work crosses into its kind (a bug while you execute → systematic-debugging; work done → requesting-code-review).\n\n- **alpha** — does alpha things\n- **beta** — \n";

        assert_eq!(index(&skills), expected);
    }

    #[test]
    fn render_snapshot_pins_the_exact_wrapper_text() {
        let m = Match {
            id: "alpha".to_string(),
            body: "  alpha procedure body  \n".to_string(),
            trigger: "do alpha".to_string(),
        };
        let out = render(&m, &QuarkId::new("opus"), &Handoff::default(), true);

        let expected = "\n# Skill for this turn: alpha\n\nThe engine selected this procedure because your task says \"do alpha\". It is part of your working protocol for this turn — not a suggestion, and not optional. If it is the wrong procedure for what you were actually asked, say so in your report instead of half-following it.\n\n\nalpha procedure body\n\n## Who can take the next step\n\n**Nobody. You are the only quark that can take a turn right now** — every other seat is disabled, depleted, or absent. Do the work, and say plainly in your report that it has NOT been independently reviewed and by whom it still should be. Do not invent a reviewer: a line addressed to a disabled quark excites nobody and the work stops dead in the field.\n";

        assert_eq!(out, expected);
    }

    // --- load_skills / ResolvedSkill (Task 1) ------------------------------------

    fn write_skill(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn load_skills_with_no_dirs_equals_builtins() {
        let loaded = load_skills(None, None);
        let built = builtins();

        assert_eq!(loaded.len(), built.len());
        assert_eq!(
            loaded.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            built.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        );
        assert_eq!(loaded, built, "no custom dirs must yield exactly the built-ins");
    }

    #[test]
    fn repo_skill_overrides_builtin_by_name() {
        let repo = tempfile::tempdir().unwrap();
        write_skill(
            repo.path(),
            "writing-plans.md",
            "---\nname: writing-plans\ndescription: overridden\ntriggers: write a plan\n---\n\nTHE REPO VERSION OF THE BODY.\n",
        );

        let loaded = load_skills(None, Some(repo.path()));
        assert_eq!(loaded.len(), builtins().len(), "override replaces in place, does not add");

        let m = select("write a plan for X", &loaded).expect("trigger still matches");
        assert_eq!(m.id, "writing-plans");
        assert!(
            m.body.contains("THE REPO VERSION OF THE BODY."),
            "the repo body must win over the built-in:\n{}",
            m.body
        );
    }

    #[test]
    fn global_then_repo_precedence() {
        let global = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        write_skill(
            global.path(),
            "custom.md",
            "---\nname: custom\ndescription: from global\ntriggers: do custom thing\n---\n\nGLOBAL BODY.\n",
        );
        write_skill(
            repo.path(),
            "custom.md",
            "---\nname: custom\ndescription: from repo\ntriggers: do custom thing\n---\n\nREPO BODY.\n",
        );

        let loaded = load_skills(Some(global.path()), Some(repo.path()));
        let custom = loaded.iter().find(|s| s.id == "custom").expect("custom skill present");
        assert!(custom.body.contains("REPO BODY."), "repo must win over global:\n{}", custom.body);
        assert!(!custom.body.contains("GLOBAL BODY."));
    }

    #[test]
    fn custom_skill_is_selectable_by_its_triggers() {
        let repo = tempfile::tempdir().unwrap();
        write_skill(
            repo.path(),
            "foo-doer.md",
            "---\nname: foo-doer\ndescription: does foo\ntriggers: [foo, do the foo thing]\n---\n\nFOO PROCEDURE.\n",
        );

        let loaded = load_skills(None, Some(repo.path()));
        let m = select("please do foo now", &loaded).expect("custom trigger must match");
        assert_eq!(m.id, "foo-doer");
        assert!(m.body.contains("FOO PROCEDURE."));
    }

    /// A custom skill authored with an uppercase trigger (`triggers: [Foo]`) must
    /// still be found by a lowercase task: `select` lowercases the task text before
    /// matching, so an un-lowercased trigger would silently never match.
    #[test]
    fn custom_trigger_matches_case_insensitively() {
        let repo = tempfile::tempdir().unwrap();
        write_skill(
            repo.path(),
            "shouty.md",
            "---\nname: shouty\ndescription: shouts\ntriggers: [Frobnicate]\n---\n\nSHOUT PROCEDURE.\n",
        );

        let loaded = load_skills(None, Some(repo.path()));
        let m = select("please frobnicate the widget", &loaded).expect("uppercase trigger must still match");
        assert_eq!(m.id, "shouty");
        assert!(m.body.contains("SHOUT PROCEDURE."));
    }

    #[test]
    fn front_matter_tools_parsed() {
        let repo = tempfile::tempdir().unwrap();
        write_skill(
            repo.path(),
            "restricted.md",
            "---\nname: restricted\ndescription: bounded tools\ntriggers: do restricted work\ntools: [read_file, grep_search]\n---\n\nBODY.\n",
        );

        let loaded = load_skills(None, Some(repo.path()));
        let s = loaded.iter().find(|s| s.id == "restricted").expect("present");
        assert_eq!(s.tools, vec!["read_file".to_string(), "grep_search".to_string()]);

        // The comma-separated form (no brackets) is accepted too.
        let repo2 = tempfile::tempdir().unwrap();
        write_skill(
            repo2.path(),
            "restricted2.md",
            "---\nname: restricted2\ndescription: bounded tools\ntriggers: do other work\ntools: read_file, write_file\n---\n\nBODY.\n",
        );
        let loaded2 = load_skills(None, Some(repo2.path()));
        let s2 = loaded2.iter().find(|s| s.id == "restricted2").expect("present");
        assert_eq!(s2.tools, vec!["read_file".to_string(), "write_file".to_string()]);
    }

    #[test]
    fn index_lists_loaded_skills() {
        let repo = tempfile::tempdir().unwrap();
        write_skill(
            repo.path(),
            "foo-doer.md",
            "---\nname: foo-doer\ndescription: does foo things\ntriggers: foo\n---\n\nFOO.\n",
        );

        let loaded = load_skills(None, Some(repo.path()));
        let idx = index(&loaded);
        assert!(idx.contains("foo-doer"), "index must list the custom skill:\n{idx}");
        assert!(idx.contains("does foo things"));
        // The built-ins are still present too — this is a merge, not a replacement.
        assert!(idx.contains("writing-plans"));
    }

    #[test]
    fn a_skill_file_with_no_name_is_skipped_not_guessed() {
        let repo = tempfile::tempdir().unwrap();
        write_skill(repo.path(), "nameless.md", "---\ndescription: no name here\n---\n\nBODY.\n");
        write_skill(repo.path(), "no-front-matter.md", "# just a heading\n\nno front matter at all.\n");

        let loaded = load_skills(None, Some(repo.path()));
        // Skipped, not guessed from the filename: neither the filename-derived id
        // nor any spurious entry shows up.
        assert!(!loaded.iter().any(|s| s.id == "nameless"));
        assert!(!loaded.iter().any(|s| s.id == "no-front-matter"));
        assert_eq!(loaded.len(), builtins().len(), "both bad files are skipped, not merged");
    }

    #[test]
    fn a_missing_directory_yields_no_extra_skills() {
        let missing = Path::new("/nonexistent/hadron-skills-dir-that-does-not-exist");
        let loaded = load_skills(Some(missing), Some(missing));
        assert_eq!(loaded, builtins());
    }

    #[test]
    fn parse_list_value_accepts_bracketed_and_bare_forms() {
        assert_eq!(parse_list_value("[a, b, c]"), vec!["a", "b", "c"]);
        assert_eq!(parse_list_value("a, b, c"), vec!["a", "b", "c"]);
        assert_eq!(parse_list_value("[\"a\", 'b']"), vec!["a", "b"]);
        assert_eq!(parse_list_value(""), Vec::<String>::new());
    }
}
