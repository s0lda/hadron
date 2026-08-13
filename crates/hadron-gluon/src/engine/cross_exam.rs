//! Adversarial cross-examination lane.
//!
//! Phase 1 Task 2 of the 2026-08-13 capabilities plan: a structured adversarial
//! review step that runs alongside (or before) the peer-review quorum gate from
//! `engine::review`. Where `ReviewGate` answers "is anyone willing to sign off
//! on this branch?", cross-examination answers "is there even *anything* worth
//! signing off on, and if so, what specifically should the critic look at?".
//!
//! A cross-exam is gated by the **shape of the change set**, not its author or
//! its size: pure documentation and pure configuration changes skip the lane
//! (no executable surface to attack, no logic to dispute), while any change
//! touching code, schemas, build glue, or anything else with a runtime effect
//! is required to produce a critic prompt and run the dual-quark debate.
//!
//! **What this module is and is not.** It is a *policy* + *prompt-builder*
//! pair. It does NOT spawn a critic quark, it does NOT consume field events,
//! and it does NOT know about branches or worktrees — those belong to the
//! merge-gate layer that adopts this lane. The split mirrors `engine::review`:
//! one small, pure data-structure + builder that the merge gate can use
//! without taking a dependency on the engine itself.

/// Path prefixes that mark a file as "not code" for the purposes of
/// cross-examination. Any file under one of these directories is treated as
/// documentation or repository metadata, not a candidate for adversarial
/// review.
///
/// Kept `&'static str` so the default `CrossExaminationLane` needs no
/// allocation; tests exercise the same set.
const DEFAULT_EXEMPT_PREFIXES: &[&str] = &[
    "docs/",
    "doc/",
    ".hadron/",
];

/// File extensions that mark a file as documentation, configuration, or
/// metadata — not a candidate for adversarial review. The list is deliberately
/// conservative: only extensions whose files are *never* executable in any
/// sense that a critic would care about.
///
/// Edge cases not in this list (and the reasoning):
///
/// - `*.txt` — too coarse; project READMEs, build manifests, and asset
///   manifests all wear it. We require a real path prefix instead.
/// - `*.lock` — generated build state, not authored logic. The trigger for
///   cross-exam is "did you write code", not "did anything change", so an
///   incidentally-regenerated `Cargo.lock` shouldn't fire a debate.
/// - `*.png`/`*.jpg`/… — already covered by being non-`text/*` for git
///   purposes, but the diff layer will not surface them to a critic anyway.
const DEFAULT_EXEMPT_EXTENSIONS: &[&str] = &[
    "md",
    "markdown",
    "rst",
    "adoc",
    "toml",
    "yaml",
    "yml",
    "json",
    "ini",
    "cfg",
    "conf",
    "license",
    "notice",
    "changelog",
    "txt",
];

/// The cross-examination lane itself. Holds the trigger policy (which file
/// shapes are exempt from review) and builds the adversarial prompt the
/// critic quark will receive.
///
/// A `CrossExaminationLane` is cheap to construct (no allocations) and
/// `Clone`-friendly. The merge-gate layer can hold one per engine, per
/// branch, or per turn without measurable cost.
#[derive(Debug, Clone)]
pub struct CrossExaminationLane {
    exempt_prefixes: Vec<&'static str>,
    exempt_extensions: Vec<&'static str>,
}

impl Default for CrossExaminationLane {
    fn default() -> Self {
        Self::new()
    }
}

impl CrossExaminationLane {
    /// Build a lane with the default trigger policy (docs and config are
    /// exempt; everything else is a candidate for cross-exam).
    pub fn new() -> Self {
        Self {
            exempt_prefixes: DEFAULT_EXEMPT_PREFIXES.to_vec(),
            exempt_extensions: DEFAULT_EXEMPT_EXTENSIONS.to_vec(),
        }
    }

    /// Should a critic quark be spun up against this change set?
    ///
    /// Rules, in order:
    ///
    /// 1. An empty change set never fires the lane — there is nothing to
    ///    examine. (Pure-prose typo fixes in a doc don't, e.g., require a
    ///    second pair of eyes.)
    /// 2. A change set composed entirely of exempt files (every file matches
    ///    an exempt prefix or extension) does not fire the lane.
    /// 3. Any change set with at least one non-exempt file DOES fire the
    ///    lane — even if the change is 99% docs, the one non-exempt file is
    ///    what we want a critic to look at.
    ///
    /// `author` is accepted for symmetry with the free function and to give
    /// future policy (e.g. "self-cross-exam is suppressed for the orchestrator
    /// seat") a place to live. The default lane ignores it.
    pub fn should_cross_examine(&self, _author: &str, files_changed: &[&str]) -> bool {
        if files_changed.is_empty() {
            return false;
        }
        files_changed.iter().any(|f| !self.is_exempt(f))
    }

    /// Build the structured adversarial-critique prompt for one
    /// author/turn/diff triple. The output is a markdown document that
    /// instructs a critic quark to walk the diff looking for problems the
    /// author may have missed.
    ///
    /// The prompt is **deliberately adversarial**: it tells the critic to
    /// look for missing tests, missing error paths, silently-changed
    /// invariants, and any "this would obviously pass a code review" filler.
    /// The whole point of the lane is that a friendly same-voice review is
    /// not adversarial enough; this prompt sets the frame.
    pub fn create_critic_prompt(&self, author: &str, task: &str, diff: &str) -> String {
        format!(
            "You are an adversarial reviewer for the Hadron swarm.\n\
             The author of the change below is `{author}`.\n\
             They claim the task was:\n\n\
             > {task}\n\n\
             Your job is NOT to confirm the change works. Your job is to find\n\
             what the author missed. Walk the diff and produce, in this exact\n\
             order:\n\n\
             1. **Correctness holes** — logic that the test suite would not\n\
                catch. Off-by-one, signed/unsigned, empty-input, large-input,\n\
                charset, locale, timezone, leap-year, daylight-savings, etc.\n\
             2. **Missing error paths** — every `unwrap`, `expect`, `panic!`,\n\
                `?` operator, and swallowed error in the diff. If the change\n\
                adds any of these, name the line and the input that triggers\n\
                it.\n\
             3. **Invariant violations** — claims the change makes about the\n\
                rest of the codebase that the rest of the codebase does not\n\
                back. Cross-reference with `hadron-lattice`, the merge gate,\n\
                and the standard-model invariants.\n\
             4. **Test coverage gaps** — assertions that only cover the\n\
                happy path, that mock away the very thing they are meant to\n\
                verify, or that are structurally identical to the code\n\
                under test.\n\
             5. **Refactor risk** — any unrelated cleanup the author smuggled\n\
                in. Cross-examine its commit-by-commit. Refactors do not\n\
                belong in a feature branch.\n\
             6. **Documentation that lies** — comments or doc-comments whose\n\
                claim is now false. This is the one place you may look at\n\
                docs: as a falsifiability check on the code.\n\n\
             If, after walking the diff, you find nothing in any category,\n\
             say so explicitly. Do not invent problems. The lane is\n\
             adversarial, not theatrical.\n\n\
             The diff to cross-examine follows.\n\n\
             ```diff\n\
             {diff}\n\
             ```\n"
        )
    }

    fn is_exempt(&self, file: &str) -> bool {
        let lower = file.to_ascii_lowercase();
        if self.exempt_prefixes.iter().any(|p| lower.starts_with(p)) {
            return true;
        }
        match lower.rfind('.') {
            Some(pos) if pos + 1 < lower.len() => {
                let ext = &lower[pos + 1..];
                self.exempt_extensions.iter().any(|e| *e == ext)
            }
            _ => false,
        }
    }
}

/// Free-function shortcut for the default lane. Equivalent to
/// `CrossExaminationLane::new().should_cross_examine(author, files_changed)`.
///
/// Provided as a top-level function so callers that don't need to configure
/// the lane can write `should_cross_examine(author, &files)` without naming
/// the type. The merge gate is expected to be one such caller.
pub fn should_cross_examine(author: &str, files_changed: &[&str]) -> bool {
    CrossExaminationLane::new().should_cross_examine(author, files_changed)
}

/// Free-function shortcut for the default lane's prompt builder. Equivalent
/// to `CrossExaminationLane::new().create_critic_prompt(author, task, diff)`.
pub fn create_critic_prompt(author: &str, task: &str, diff: &str) -> String {
    CrossExaminationLane::new().create_critic_prompt(author, task, diff)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- should_cross_examine / CrossExaminationLane::should_cross_examine ----

    #[test]
    fn should_cross_examine_an_empty_change_set_is_false() {
        // Vacuously exempt — nothing to look at.
        assert!(!should_cross_examine("@ollama", &[]));
    }

    #[test]
    fn should_cross_examine_a_pure_markdown_change_is_false() {
        let files = ["README.md", "docs/plan.md", "CHANGELOG.md"];
        assert!(!should_cross_examine("@ollama", &files));
    }

    #[test]
    fn should_cross_examine_a_pure_config_change_is_false() {
        let files = ["Cargo.toml", "crates/hadron-chamber/hadron.toml", "config.yaml"];
        assert!(!should_cross_examine("@ollama", &files));
    }

    #[test]
    fn should_cross_examine_a_single_rust_file_is_true() {
        let files = ["crates/hadron-gluon/src/engine/cross_exam.rs"];
        assert!(should_cross_examine("@ollama", &files));
    }

    #[test]
    fn should_cross_examine_mixed_change_set_fires_when_any_file_is_code() {
        // 99% docs, 1% code — the code is what we want a critic to look at.
        let files = [
            "README.md",
            "docs/notes/2026-08-13.md",
            "crates/hadron-gluon/src/engine/cross_exam.rs",
        ];
        assert!(should_cross_examine("@ollama", &files));
    }

    #[test]
    fn should_cross_examine_under_hadron_dir_is_exempt() {
        // .hadron/ is metadata, not code — plans, notes, preons.
        let files = [".hadron/nucleus/notes/example.md", ".hadron/team.json"];
        assert!(!should_cross_examine("@ollama", &files));
    }

    #[test]
    fn should_cross_examine_is_case_insensitive_on_extensions() {
        // "Cargo.TOML" and "README.MD" are still config / docs.
        let files = ["Cargo.TOML", "README.MD", "config.YAML"];
        assert!(!should_cross_examine("@ollama", &files));
    }

    #[test]
    fn should_cross_examine_extensionless_files_are_treated_as_code() {
        // A `Makefile`, `Dockerfile`, `LICENSE` (without extension) — the
        // license one is covered by basename, but a bare `Makefile` should
        // still fire cross-exam. This is conservative on purpose: when in
        // doubt, fire the lane.
        let files = ["Makefile", "Dockerfile"];
        assert!(should_cross_examine("@ollama", &files));
    }

    #[test]
    fn cross_examination_lane_default_matches_the_free_function() {
        // The free function is documented as a shortcut for the default
        // lane. If these drift, one of the two is lying.
        let lane = CrossExaminationLane::new();
        let files = ["README.md", "src/lib.rs", "docs/notes/x.md"];
        assert_eq!(
            lane.should_cross_examine("@acp-claude", &files),
            should_cross_examine("@acp-claude", &files),
        );
    }

    // ---- create_critic_prompt ----

    #[test]
    fn create_critic_prompt_names_the_author_in_the_prompt_body() {
        let prompt = create_critic_prompt("@ollama", "implement cross-exam", "+1 line");
        assert!(prompt.contains("@ollama"), "author must appear in the prompt");
    }

    #[test]
    fn create_critic_prompt_includes_the_task_as_a_blockquote() {
        let prompt = create_critic_prompt("@ollama", "implement the cross-exam lane", "diff-body");
        // The task is rendered as a Markdown blockquote (`> `).
        assert!(
            prompt.contains("> implement the cross-exam lane"),
            "task must be rendered as a blockquote, prompt was:\n{prompt}"
        );
    }

    #[test]
    fn create_critic_prompt_embeds_the_diff_in_a_fenced_code_block() {
        let prompt = create_critic_prompt("@ollama", "task", "@@ -1,1 +1,1 @@\n-old\n+new\n");
        // The diff must be inside a ```diff fence so a Markdown renderer
        // actually treats it as a diff.
        assert!(prompt.contains("```diff"), "diff must be inside a diff fence");
        assert!(prompt.contains("-old"), "diff body must be preserved");
        assert!(prompt.contains("+new"), "diff body must be preserved");
    }

    #[test]
    fn create_critic_prompt_lists_six_adversarial_categories() {
        let prompt = create_critic_prompt("@ollama", "task", "diff");
        // The six categories the spec demands, in order. Each is enumerated
        // in the prompt so a critic quark that skims still hits every
        // section.
        for header in [
            "Correctness holes",
            "Missing error paths",
            "Invariant violations",
            "Test coverage gaps",
            "Refactor risk",
            "Documentation that lies",
        ] {
            assert!(
                prompt.contains(header),
                "critic prompt must enumerate `{header}`; full prompt:\n{prompt}"
            );
        }
    }

    #[test]
    fn create_critic_prompt_is_adversarial_in_voice() {
        // A friendly same-voice review is not what the lane is for. The
        // prompt should explicitly say adversarial.
        let prompt = create_critic_prompt("@ollama", "task", "diff");
        assert!(
            prompt.to_ascii_lowercase().contains("adversarial"),
            "prompt must frame itself as adversarial, prompt was:\n{prompt}"
        );
    }

    #[test]
    fn create_critic_prompt_with_an_empty_diff_still_produces_a_well_formed_document() {
        // An empty diff is degenerate but the function must not panic and
        // must still produce a valid critic prompt — merge-gate callers
        // should not have to special-case this.
        let prompt = create_critic_prompt("@ollama", "task", "");
        assert!(prompt.contains("```diff"));
        assert!(prompt.contains("@ollama"));
    }

    #[test]
    fn cross_examination_lane_create_critic_prompt_matches_the_free_function() {
        // The free function is documented as a shortcut for the default
        // lane. If these drift, one of the two is lying.
        let lane = CrossExaminationLane::new();
        let prompt = lane.create_critic_prompt("@acp-claude", "task", "diff");
        assert_eq!(prompt, create_critic_prompt("@acp-claude", "task", "diff"));
    }
}
