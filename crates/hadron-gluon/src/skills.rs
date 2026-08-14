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

use std::path::Path;

mod parse;
mod select;
#[cfg(test)]
mod tests;

pub use select::{description, index, plan_author, plan_ref, render, select, preferred_role, Handoff, Match};
// `pub`, not `pub(crate)`: `/add-skill` (hadron-chamber) needs to inspect a
// candidate skill's front-matter (the `tools:` warning, spec §10) before writing
// it, and re-implementing this parser there would be a second definition of the
// same rule (rule 3) — reuse the one this module already tests.
pub use parse::{front_matter_value, split_front_matter};

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
    Skill {
        id: "release",
        triggers: &["release", "project release", "execute release", "run release", "make release"],
        body: include_str!("../invariants/skills/release.md"),
    },
    Skill {
        id: "absorb",
        triggers: &[
            "absorb",
            "absorb memories",
            "absorb skills",
            "migrate assistant",
            "migrate memories",
            "migrate skills",
            "import assistant",
            "import memories",
            "import skills",
            "absorb assistant",
        ],
        body: include_str!("../invariants/skills/absorb.md"),
    },
];

/// The canonical trigger phrase for a built-in skill id, or `None` if no such id.
///
/// The **first** trigger is canonical by construction: [`select`] matches any of a
/// skill's triggers, so a caller that needs to *produce* text which will select this
/// skill needs exactly one of them, and the first is the one written to read naturally
/// in a sentence.
///
/// This exists so the chamber's `/brainstorm`, `/writing-plans` and `/executing-plans`
/// commands can build their message from the engine's own trigger instead of retyping
/// the phrase. A copy in the chamber would keep compiling and silently stop selecting
/// the skill the day someone edited [`SKILLS`] — the drift rule 3 exists to prevent.
pub fn canonical_trigger(id: &str) -> Option<&'static str> {
    SKILLS
        .iter()
        .find(|s| s.id == id)
        .and_then(|s| s.triggers.first().copied())
}

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
/// point for [`load_skills`], onto which custom global/repo skills are merged.
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

/// Whether a quark may use `tool` while `skill` is the active skill for the turn.
///
/// The decision the engine's tool-gating consults (spec §3.2). It is deliberately a
/// PURE function of the tool name and the active skill's declared `tools:` allow-list,
/// so it is fully unit-testable without any transport:
/// - A skill that declares **no** `tools` (empty list) imposes NO restriction — every
///   tool is allowed. This is the default (all built-in skills), so tool-gating is
///   opt-in per skill and changes nothing until a skill actually lists tools.
/// - A skill that declares a `tools` allow-list permits ONLY those tools, matched
///   case-insensitively by name; anything else is denied.
///
/// ENFORCEMENT (not built here — it is transport-specific and rides later work):
/// - **SDK quarks**: n/a — the `sdk` transport is unsupported and has no native adapter
///   (see [`hadron_lattice::Transport::Sdk`]); every provider runs over ACP or CLI.
/// - **ACP/CLI quarks**: reject a disallowed tool at permission-request time (in
///   `acp.rs`'s `on_receive_request`), or escalate via the §2 gate. This is the same
///   "notional until a real per-tool ask exists" situation §2's adjudication is in.
/// This function is the shared, tested decision all of those will call.
pub fn is_tool_allowed(tool: &str, skill: &ResolvedSkill) -> bool {
    skill.tools.is_empty() || skill.tools.iter().any(|t| t.eq_ignore_ascii_case(tool))
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
        for loaded in parse::load_dir(dir) {
            parse::upsert(&mut skills, loaded);
        }
    }

    skills
}

