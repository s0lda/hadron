//! Chat-body text preparation and plan parsing: `@mention` display-name resolution
//! (the colouring itself is done natively by the forked markdown renderer) and the
//! plan-checklist progress parser behind the Plan rail. Pure string work — no GPUI.

/// Resolve what the human typed after a `/command` — `@Sonnet`, `Sonnet`, or the bare
/// seat id `acp-claude-2` — to the roster row it names.
///
/// The chamber renders a quark by its **display name** while the field keys everything
/// on the **seat id**, so a command that only matched ids would reject the very name it
/// just printed. Matching is case-insensitive and accepts either key; a leading `@` is
/// optional because a human who just typed a mention will include it and one answering
/// a prompt often will not.
///
/// Returns the row, not the id, so the caller can also see `enabled`/`adopted` without a
/// second lookup — which is the difference between "toggled" and "there is nothing here
/// to toggle yet".
pub(super) fn seat_by_mention<'a>(
    roster: &'a [crate::model::RosterRow],
    target: &str,
) -> Option<&'a crate::model::RosterRow> {
    let want = target.trim().trim_start_matches('@').trim();
    if want.is_empty() {
        return None;
    }
    roster.iter().find(|r| {
        r.id.eq_ignore_ascii_case(want)
            || r.display_name.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(want))
    })
}

/// Parse a plan's markdown checklist into `(total, completed, items)`. Any line whose
/// trimmed form starts with `- [ ]` / `- [x]` (case-insensitive) is a checkbox; the
/// nearest preceding `## Task` / `### Task` heading is prefixed so the tracker shows
/// which task a step belongs to. Bold/backtick emphasis is stripped for a compact label.
pub(super) fn parse_plan_progress(content: &str) -> (usize, usize, Vec<(String, bool)>) {
    let mut total = 0usize;
    let mut completed = 0usize;
    let mut items = Vec::new();
    let mut current_task = String::new();

    for line in content.lines() {
        if line.starts_with("## Task") || line.starts_with("### Task") {
            current_task = line.trim_start_matches('#').trim().to_string();
            continue;
        }
        let trimmed = line.trim_start();
        let done = if trimmed.starts_with("- [ ]") {
            false
        } else if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            true
        } else {
            continue;
        };

        total += 1;
        if done {
            completed += 1;
        }
        // `- [ ]` and `- [x]` are both 5 bytes, so a fixed skip is safe for either case.
        let body = trimmed[5..].trim().trim_matches(|c| c == '*' || c == '`').trim();
        let label = if current_task.is_empty() {
            body.to_string()
        } else {
            format!("{current_task} — {body}")
        };
        items.push((label, done));
    }

    (total, completed, items)
}

/// Parse plan content into grouped tasks: a list of `(task_name, steps)` tuples.
/// Any step body is stripped of bold or backtick marks.
pub(super) fn parse_plan_tasks(content: &str) -> Vec<(String, Vec<(String, bool)>)> {
    let mut tasks = Vec::new();
    let mut current_task = String::new();
    let mut current_steps = Vec::new();

    for line in content.lines() {
        if line.starts_with("## Task") || line.starts_with("### Task") {
            if !current_steps.is_empty() || !current_task.is_empty() {
                tasks.push((current_task.clone(), std::mem::take(&mut current_steps)));
            }
            current_task = line.trim_start_matches('#').trim().to_string();
            continue;
        }
        let trimmed = line.trim_start();
        let done = if trimmed.starts_with("- [ ]") {
            false
        } else if trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            true
        } else {
            continue;
        };

        // `- [ ]` and `- [x]` are both 5 bytes, so a fixed skip is safe for either case.
        let body = trimmed[5..].trim().trim_matches(|c| c == '*' || c == '`').trim();
        current_steps.push((body.to_string(), done));
    }

    if !current_steps.is_empty() || !current_task.is_empty() {
        tasks.push((current_task, current_steps));
    }

    tasks
}

/// Rewrite `@<seat-id>` mentions to `@<display-name>` so a mention typed or stored by
/// its raw seat id (e.g. `@acp-claude-2`) reads the way the human names the quark
/// (`@Sonnet`). Matching keys on the id; only the shown text changes. Everything else is
/// passed through untouched.
///
/// This deliberately does *not* colour anything. The forked markdown renderer marks
/// `@mention` and `/command` tokens natively at parse time (`push_mention_and_command_marks`
/// in gpui-component's `markdown.rs`), so wrapping them in HTML `<span>`s here would only
/// feed that parser tags it mangles — the bug that left mentions plain after chat moved to
/// `TextView::markdown`. We stay code-block-aware only so a mention inside a fence or inline
/// code is left as literal text rather than silently renamed.
pub(super) fn resolve_mention_names(body: &str, roster: &[crate::model::RosterRow]) -> String {
    let mut out = String::with_capacity(body.len());
    let mut chars = body.chars().peekable();
    let mut in_code_block = false;
    let mut in_inline_code = false;

    while let Some(c) = chars.next() {
        if c == '`' {
            let mut backtick_count = 1;
            while chars.peek() == Some(&'`') {
                chars.next();
                backtick_count += 1;
            }
            if backtick_count >= 3 {
                in_code_block = !in_code_block;
            } else if !in_code_block {
                in_inline_code = !in_inline_code;
            }
            for _ in 0..backtick_count {
                out.push('`');
            }
            continue;
        }

        if c == '@' && !in_code_block && !in_inline_code {
            let mut name = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '.' || nc == '/' || nc == '-' || nc == '_' || nc == ' ' || nc == '(' || nc == ')' {
                    name.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            // We greedily consumed spaces and parens, so `name` may run past the mention
            // (e.g. "@Agy said hi"). Show the longest roster display-name or id it starts
            // with; matching keys on the id, only the shown text changes. Anything with no
            // roster match (a plain `@word`, a `@file/path`) is emitted verbatim for the
            // renderer to mark — we no longer classify quark vs file, the renderer marks
            // every `@token` the same.
            let mut best_match_len = 0;
            let mut matched_display: Option<&str> = None;

            for q in roster {
                let q_id = &q.id;
                let q_name = q.display_name.as_deref().unwrap_or(q_id.as_str());
                if name.starts_with(q_name) && q_name.len() > best_match_len {
                    best_match_len = q_name.len();
                    matched_display = Some(q_name);
                } else if name.starts_with(q_id.as_str()) && q_id.len() > best_match_len {
                    best_match_len = q_id.len();
                    matched_display = Some(q_name);
                }
            }

            out.push('@');
            match matched_display {
                Some(shown) => {
                    out.push_str(shown);
                    out.push_str(&name[best_match_len..]);
                }
                None => out.push_str(&name),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RosterRow;
    use hadron_lattice::QuarkState;

    #[test]
    fn test_parse_plan_progress() {
        let content = "\
# A Plan

### Task 1: First
- [x] **Step 1: Do the thing**
- [ ] Step 2: Do the other thing

### Task 2: Second
  - [X] Nested done step
  - [ ] Nested pending step
";
        let (total, completed, items) = parse_plan_progress(content);
        assert_eq!(total, 4);
        assert_eq!(completed, 2);
        assert_eq!(items.len(), 4);
        assert_eq!(items[0], ("Task 1: First — Step 1: Do the thing".to_string(), true));
        assert_eq!(items[1], ("Task 1: First — Step 2: Do the other thing".to_string(), false));
        assert!(items[2].1); // nested [X] counts as done
        assert!(!items[3].1);
    }

    #[test]
    fn test_parse_plan_tasks() {
        let content = "\
# A Plan

### Task 1: First
- [x] **Step 1: Do the thing**
- [ ] Step 2: Do the other thing

### Task 2: Second
  - [X] Nested done step
  - [ ] Nested pending step
";
        let tasks = parse_plan_tasks(content);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].0, "Task 1: First");
        assert_eq!(tasks[0].1.len(), 2);
        assert_eq!(tasks[0].1[0], ("Step 1: Do the thing".to_string(), true));
        assert_eq!(tasks[0].1[1], ("Step 2: Do the other thing".to_string(), false));
        assert_eq!(tasks[1].0, "Task 2: Second");
        assert_eq!(tasks[1].1.len(), 2);
        assert_eq!(tasks[1].1[0], ("Nested done step".to_string(), true));
        assert_eq!(tasks[1].1[1], ("Nested pending step".to_string(), false));
    }

    #[test]
    fn test_first_incomplete_task() {
        let content = "\
### Task 1: First
- [x] Step 1
- [x] Step 2

### Task 2: Second
- [ ] Step 3

### Task 3: Third
- [ ] Step 4
";
        let tasks = parse_plan_tasks(content);
        let first_incomplete = tasks
            .iter()
            .find(|(_, steps)| steps.iter().any(|(_, done)| !*done))
            .map(|(name, _)| name.as_str());
        assert_eq!(first_incomplete, Some("Task 2: Second"));
    }

    fn opus_roster() -> Vec<RosterRow> {
        vec![RosterRow {
            id: "opus".to_string(),
            display_name: None,
            state: QuarkState::Excited,
            mode: hadron_lattice::Mode::Ask,
            mode_is_override: false,
            vendor: "anthropic".to_string(),
            model: "Claude Opus 4.6".to_string(),
            flavor: Some(hadron_lattice::Flavor::Worker),
            transport: hadron_lattice::Transport::Cli,
            effort: None,
            enabled: true,
            adopted: true,
            tokens: 0,
            unknown_turns: 0,
        }]
    }

    /// `/toggle @Sonnet` must find the seat the human NAMED, not only the id the field
    /// keys on — the chamber shows `@Sonnet` and never `@acp-claude-2`, so an id-only
    /// match would reject the one spelling the UI ever displays.
    #[test]
    fn a_seat_resolves_by_display_name_by_id_and_with_or_without_the_sigil() {
        let mut roster = opus_roster();
        roster[0].id = "acp-claude-2".to_string();
        roster[0].display_name = Some("Sonnet".to_string());

        for typed in ["@Sonnet", "Sonnet", "@sonnet", "@acp-claude-2", "acp-claude-2", " @Sonnet "] {
            assert_eq!(
                seat_by_mention(&roster, typed).map(|r| r.id.as_str()),
                Some("acp-claude-2"),
                "{typed:?} names this seat",
            );
        }
    }

    /// An empty or unmatched target must be `None`, not the first row: `/toggle` with a
    /// typo would otherwise silently park whichever quark happens to sort first.
    #[test]
    fn an_unknown_or_empty_target_resolves_to_nothing() {
        let roster = opus_roster();
        for typed in ["", "@", "   ", "@nobody", "opu"] {
            assert!(seat_by_mention(&roster, typed).is_none(), "{typed:?} names no seat");
        }
    }

    /// The renderer, not this pass, colours `@mention`s and `/command`s now. Text with no
    /// seat-id to resolve must come out byte-for-byte identical (no HTML, no `<span>`), so
    /// the forked markdown parser sees the bare tokens it marks natively.
    #[test]
    fn text_with_nothing_to_resolve_passes_through_untouched() {
        assert_eq!(
            resolve_mention_names("Please run /plan and /grill-me today.", &[]),
            "Please run /plan and /grill-me today."
        );
        // A quark whose id already matches the shown name is unchanged.
        assert_eq!(resolve_mention_names("Hello @opus!", &opus_roster()), "Hello @opus!");
        // Unknown mentions and file paths are left for the renderer to mark.
        assert_eq!(
            resolve_mention_names("@team and @orchestrator and @src/main.rs", &[]),
            "@team and @orchestrator and @src/main.rs"
        );
    }

    /// A mention typed or stored by its raw seat id (e.g. `@acp-claude-2`) must be rewritten
    /// to the quark's display name (`@Sonnet`) when one is set — matching still keys on the
    /// id, only the shown text changes. The renderer then colours the resulting `@Sonnet`.
    #[test]
    fn a_mention_by_raw_id_resolves_to_the_display_name() {
        let roster = vec![RosterRow {
            id: "acp-claude-2".to_string(),
            display_name: Some("Sonnet".to_string()),
            state: QuarkState::Excited,
            mode: hadron_lattice::Mode::Ask,
            mode_is_override: false,
            vendor: "anthropic".to_string(),
            model: "claude-sonnet-5".to_string(),
            flavor: Some(hadron_lattice::Flavor::Worker),
            transport: hadron_lattice::Transport::Acp,
            effort: None,
            enabled: true,
            adopted: true,
            tokens: 0,
            unknown_turns: 0,
        }];

        assert_eq!(resolve_mention_names("Hello @acp-claude-2!", &roster), "Hello @Sonnet!");
        // Trailing words after the mention are preserved.
        assert_eq!(
            resolve_mention_names("@acp-claude-2 please review", &roster),
            "@Sonnet please review"
        );
        // Mentioning by the display name itself is already resolved, unchanged.
        assert_eq!(resolve_mention_names("Hello @Sonnet!", &roster), "Hello @Sonnet!");
    }

    /// Resolution must not fire inside inline code or a fenced block: a `@seat-id` there is
    /// literal text a human wrote, not a mention to rename.
    #[test]
    fn resolution_is_suppressed_inside_code() {
        let roster = vec![RosterRow {
            id: "acp-claude-2".to_string(),
            display_name: Some("Sonnet".to_string()),
            ..opus_roster().pop().unwrap()
        }];

        assert_eq!(
            resolve_mention_names("Here is `@acp-claude-2` inline.", &roster),
            "Here is `@acp-claude-2` inline."
        );
        assert_eq!(
            resolve_mention_names("```\n@acp-claude-2\n```", &roster),
            "```\n@acp-claude-2\n```"
        );
        // Inside code stays literal; outside resolves in the same string.
        assert_eq!(
            resolve_mention_names("`@acp-claude-2` then @acp-claude-2.", &roster),
            "`@acp-claude-2` then @Sonnet."
        );
    }

    #[test]
    fn markdown_cache_invalidates_on_body_change_or_clear() {
        let mut cache = std::collections::HashMap::<usize, (String, String)>::new();
        let roster = vec![RosterRow {
            id: "acp-claude-2".to_string(),
            display_name: Some("Sonnet".to_string()),
            ..opus_roster().pop().unwrap()
        }];

        // Render index 0 with initial body
        let body1 = "Hello @acp-claude-2";
        let res1 = match cache.get(&0) {
            Some((b, c)) if b == body1 => c.clone(),
            _ => {
                let content = resolve_mention_names(body1, &roster);
                cache.insert(0, (body1.to_string(), content.clone()));
                content
            }
        };
        assert_eq!(res1, "Hello @Sonnet");

        // Index 0 reused with new body
        let body2 = "I'm Copilot";
        let res2 = match cache.get(&0) {
            Some((b, c)) if b == body2 => c.clone(),
            _ => {
                let content = resolve_mention_names(body2, &roster);
                cache.insert(0, (body2.to_string(), content.clone()));
                content
            }
        };
        assert_eq!(res2, "I'm Copilot");

        // Clear resets all cache entries
        cache.clear();
        assert!(cache.is_empty());
    }
}
