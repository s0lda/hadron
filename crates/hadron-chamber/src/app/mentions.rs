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
/// nearest preceding `## ` / `### ` heading is prefixed so the tracker shows
/// which task a step belongs to. Bold/backtick emphasis is stripped for a compact label.
pub(super) fn parse_plan_progress(content: &str) -> (usize, usize, Vec<(String, bool)>) {
    let mut total = 0usize;
    let mut completed = 0usize;
    let mut items = Vec::new();
    let mut current_task = String::new();

    for line in content.lines() {
        if line.starts_with("## ") || line.starts_with("### ") {
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
        if line.starts_with("## ") || line.starts_with("### ") {
            let heading = line.trim_start_matches('#').trim().to_string();
            if !current_steps.is_empty() {
                tasks.push((
                    if current_task.is_empty() {
                        "General Tasks".to_string()
                    } else {
                        current_task.clone()
                    },
                    std::mem::take(&mut current_steps),
                ));
            }
            current_task = heading;
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

    if !current_steps.is_empty() {
        tasks.push((
            if current_task.is_empty() {
                "General Tasks".to_string()
            } else {
                current_task
            },
            current_steps,
        ));
    }

    tasks
}

/// Extract introductory overview or architecture summary text from a plan markdown document.
/// Collects prose paragraphs under `## Overview`, `## Architecture`, or before the first checklist.
pub(super) fn parse_plan_overview(content: &str) -> Option<String> {
    let mut overview_lines = Vec::new();
    let mut collecting = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") || trimmed.starts_with("- [X]") {
            break;
        }
        if trimmed.starts_with("## Overview")
            || trimmed.starts_with("## Architecture")
            || trimmed.starts_with("## Summary")
            || trimmed.starts_with("### Overview")
            || trimmed.starts_with("### Summary")
        {
            collecting = true;
            continue;
        }
        if collecting {
            if trimmed.starts_with("## ") || trimmed.starts_with("### ") || trimmed.starts_with("---") {
                if !overview_lines.is_empty() {
                    break;
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                overview_lines.push(trimmed);
            }
        }
    }

    if overview_lines.is_empty() {
        let mut intro = Vec::new();
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("## ") || trimmed.starts_with("### ") || trimmed.starts_with("- [ ]") || trimmed.starts_with("- [x]") {
                break;
            }
            if !trimmed.starts_with('#') && !trimmed.starts_with("---") && !trimmed.starts_with("- **") && !trimmed.is_empty() {
                intro.push(trimmed);
            }
        }
        if !intro.is_empty() {
            return Some(intro.join("\n\n"));
        }
        None
    } else {
        Some(overview_lines.join("\n\n"))
    }
}

/// Preprocess Markdown alert callouts (e.g. `> [!NOTE]`, `> [!TIP]`, `> [!WARNING]`,
/// `> [!IMPORTANT]`, `> [!CAUTION]`, `> [!DANGER]`, `> [!INFO]`) inside blockquotes
/// into clean, readable styled headers (`> **Note:**`, `> **Important:**`, etc.).
///
/// Fenced code blocks are preserved unmodified.
pub(super) fn format_markdown_callouts(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 32);
    let mut in_code_block = false;

    for (i, line) in text.lines().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_block = !in_code_block;
            out.push_str(line);
            continue;
        }
        if in_code_block {
            out.push_str(line);
            continue;
        }

        if let Some(bq_content) = trimmed.strip_prefix('>') {
            let bq_trimmed = bq_content.trim_start();
            if bq_trimmed.starts_with("[!") {
                if let Some(end_bracket) = bq_trimmed.find(']') {
                    let tag = &bq_trimmed[2..end_bracket];
                    let after_tag = bq_trimmed[end_bracket + 1..].trim();
                    let default_title = match tag.to_ascii_uppercase().as_str() {
                        "NOTE" | "INFO" => "Note",
                        "TIP" => "Tip",
                        "IMPORTANT" => "Important",
                        "WARNING" => "Warning",
                        "CAUTION" | "DANGER" => "Caution",
                        _ => "",
                    };
                    if !default_title.is_empty() {
                        let header = if after_tag.is_empty() {
                            format!("**{default_title}:**")
                        } else {
                            format!("**{default_title}: {after_tag}**")
                        };
                        let indent_len = line.len() - trimmed.len();
                        let indent = &line[..indent_len];
                        out.push_str(indent);
                        out.push_str(&format!("> {header}"));
                        continue;
                    }
                }
            }
        }
        out.push_str(line);
    }
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Rewrite `@<seat-id>` mentions to `@<display-name>` for quarks in the roster, and
/// rewrite file mentions (like `@path/to/file.md`, `@src/main.rs:123`, `@Cargo.toml`)
/// into clickable markdown links (`[file.md](file:///abs/path "path/to/file.md")`)
/// so that users can open referenced files with one click from chat while hovering shows
/// the full path in a tooltip.
///
/// Code blocks (fenced ``` and inline `...`) are passed through unmodified.
pub(super) fn resolve_mention_names(
    body: &str,
    roster: &[crate::model::RosterRow],
    repo_root: Option<&std::path::Path>,
) -> String {
    let preprocessed_callouts = format_markdown_callouts(body);
    let preprocessed = crate::mermaid::plugin::format_html_wrappers(&preprocessed_callouts);
    let body = preprocessed.as_str();
    let mut out = String::with_capacity(body.len() + 64);
    let mut chars = body.char_indices().peekable();
    let mut in_code_block = false;
    let mut in_inline_code = false;
    let mut prev_char: Option<char> = None;

    while let Some((idx, c)) = chars.next() {
        if c == '`' {
            let mut backtick_count = 1;
            while let Some(&(_, '`')) = chars.peek() {
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
            prev_char = Some('`');
            continue;
        }

        let is_mention_start = c == '@'
            && !in_code_block
            && !in_inline_code
            && !prev_char.is_some_and(|ch| ch.is_alphanumeric());

        if is_mention_start {
            let rest = &body[idx + 1..];
            let mut matched_quark: Option<(&str, usize)> = None;

            for q in roster {
                let q_id = q.id.as_str();
                let q_name = q.display_name.as_deref().unwrap_or(q_id);
                for candidate_str in [q_name, q_id] {
                    if rest.starts_with(candidate_str) {
                        let len = candidate_str.len();
                        let is_boundary = rest[len..]
                            .chars()
                            .next()
                            .map_or(true, |nc| !nc.is_alphanumeric() && nc != '-' && nc != '_');
                        if is_boundary && matched_quark.map_or(true, |(_, clen)| len > clen) {
                            matched_quark = Some((q_name, len));
                        }
                    }
                }
            }

            if let Some((shown, consumed)) = matched_quark {
                out.push('@');
                out.push_str(shown);
                let target_idx = idx + 1 + consumed;
                while let Some(&(curr_idx, _)) = chars.peek() {
                    if curr_idx < target_idx {
                        chars.next();
                    } else {
                        break;
                    }
                }
                prev_char = shown.chars().next_back();
                continue;
            }

            let mut matched_alias = false;
            for (alias, _) in crate::text::MENTION_ALIASES {
                if rest.starts_with(alias) {
                    let len = alias.len();
                    let is_boundary = rest[len..]
                        .chars()
                        .next()
                        .map_or(true, |nc| !nc.is_alphanumeric() && nc != '-' && nc != '_');
                    if is_boundary {
                        out.push('@');
                        out.push_str(alias);
                        let target_idx = idx + 1 + len;
                        while let Some(&(curr_idx, _)) = chars.peek() {
                            if curr_idx < target_idx {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        prev_char = alias.chars().next_back();
                        matched_alias = true;
                        break;
                    }
                }
            }
            if matched_alias {
                continue;
            }

            let mut raw_token = String::new();
            while let Some(&(_, nc)) = chars.peek() {
                if !nc.is_whitespace()
                    && nc != '<'
                    && nc != '>'
                    && nc != '"'
                    && nc != '\''
                    && nc != '`'
                    && nc != '['
                    && nc != '{'
                {
                    raw_token.push(nc);
                    chars.next();
                } else {
                    break;
                }
            }

            if raw_token.is_empty() {
                out.push('@');
                prev_char = Some('@');
                continue;
            }

            let mut token = raw_token.as_str();
            let mut trailing = String::new();
            while !token.is_empty() {
                let last_char = token.chars().next_back().unwrap();
                if matches!(last_char, ',' | ';' | '!' | '?' | ')' | ']' | '}' | '>' | '\'' | '"') {
                    trailing.insert(0, last_char);
                    token = &token[..token.len() - last_char.len_utf8()];
                } else if last_char == '.' {
                    let rest_str = &token[..token.len() - 1];
                    if rest_str.contains('.') {
                        trailing.insert(0, last_char);
                        token = rest_str;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }

            if is_file_mention(token, repo_root) {
                let (path_str, line_frag, line_display) = parse_line_fragment(token);
                let is_dir = path_str.ends_with('/') || path_str.ends_with('\\');
                let bare_path = path_str.trim_end_matches(['/', '\\']);
                let file_name = std::path::Path::new(bare_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(bare_path);
                let display_name = if is_dir {
                    format!("{file_name}/{line_display}")
                } else {
                    format!("{file_name}{line_display}")
                };

                let abs_path = if path_str.starts_with('/') {
                    std::path::PathBuf::from(path_str)
                } else if let Some(root) = repo_root {
                    root.join(path_str)
                } else {
                    std::path::Path::new("/").join(path_str)
                };

                let url = format!("file://{}{}", abs_path.display(), line_frag);
                let title = token.replace('"', "&quot;");
                out.push_str(&format!("[{display_name}]({url} \"{title}\")"));
                out.push_str(&trailing);
                prev_char = trailing.chars().next_back().or_else(|| ")".chars().next());
            } else {
                out.push('@');
                out.push_str(token);
                out.push_str(&trailing);
                prev_char = trailing.chars().next_back().or_else(|| token.chars().next_back());
            }
            continue;
        }

        out.push(c);
        prev_char = Some(c);
    }
    out
}

fn parse_line_fragment(token: &str) -> (&str, String, String) {
    if let Some((p, num)) = token.rsplit_once(':') {
        if !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) {
            return (p, format!("#L{num}"), format!(":{num}"));
        }
    }
    if let Some((p, frag)) = token.split_once('#') {
        return (p, format!("#{frag}"), format!("#{frag}"));
    }
    (token, String::new(), String::new())
}

fn is_file_mention(token: &str, repo_root: Option<&std::path::Path>) -> bool {
    let (path_str, _, _) = parse_line_fragment(token);
    if path_str.is_empty() || path_str == "." || path_str == ".." {
        return false;
    }
    if path_str.starts_with('/') || path_str.starts_with('~') || path_str.starts_with("./") || path_str.starts_with("../") {
        return true;
    }
    if path_str.contains('/') || path_str.contains('\\') {
        return true;
    }
    if path_str.starts_with('.') && path_str.len() > 1 && !path_str.chars().skip(1).all(|c| c == '.') {
        return true;
    }
    if let Some((stem, ext)) = path_str.rsplit_once('.') {
        if !stem.is_empty() && !ext.is_empty() && ext.len() <= 10 && ext.chars().all(|c| c.is_ascii_alphanumeric()) {
            return true;
        }
    }
    if let Some(root) = repo_root {
        if root.join(path_str).exists() {
            return true;
        }
    }
    false
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

    #[test]
    fn test_parse_plan_overview_and_sections() {
        let content = "\
# Hadron Master Plan

- **Date**: 2026-08-19
- **Status**: In Planning

## Overview & Architecture

This is the overview description for the master plan.
It outlines 20 new capabilities across the swarm.

## Roadmap & Index

Some table notes.

## Phase 1: Nucleus & Quick DX
- [x] Task 1.1: Context Breadcrumbs
- [ ] Task 1.2: REPL Overlay

## Phase 2: Observability
- [ ] Task 2.1: Plan DAG
";
        let overview = parse_plan_overview(content);
        assert!(overview.is_some());
        let ov = overview.unwrap();
        assert!(ov.contains("This is the overview description"));
        assert!(ov.contains("It outlines 20 new capabilities"));

        let tasks = parse_plan_tasks(content);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].0, "Phase 1: Nucleus & Quick DX");
        assert_eq!(tasks[0].1.len(), 2);
        assert_eq!(tasks[1].0, "Phase 2: Observability");
        assert_eq!(tasks[1].1.len(), 1);
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
    /// seat-id or file-mention to resolve must come out byte-for-byte identical (no HTML, no `<span>`), so
    /// the forked markdown parser sees the bare tokens it marks natively.
    #[test]
    fn text_with_nothing_to_resolve_passes_through_untouched() {
        assert_eq!(
            resolve_mention_names("Please run /plan and /grill-me today.", &[], None),
            "Please run /plan and /grill-me today."
        );
        // A quark whose id already matches the shown name is unchanged.
        assert_eq!(
            resolve_mention_names("Hello @opus!", &opus_roster(), None),
            "Hello @opus!"
        );
        // Routing aliases and plain non-file mentions stay plain tokens.
        assert_eq!(
            resolve_mention_names("@team and @orchestrator and @everyone", &[], None),
            "@team and @orchestrator and @everyone"
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

        assert_eq!(
            resolve_mention_names("Hello @acp-claude-2!", &roster, None),
            "Hello @Sonnet!"
        );
        // Trailing words after the mention are preserved.
        assert_eq!(
            resolve_mention_names("@acp-claude-2 please review", &roster, None),
            "@Sonnet please review"
        );
        // Mentioning by the display name itself is already resolved, unchanged.
        assert_eq!(
            resolve_mention_names("Hello @Sonnet!", &roster, None),
            "Hello @Sonnet!"
        );
    }

    /// File mentions like `@path/to/some/file.md` or `@src/main.rs:123` must be rewritten
    /// into clickable markdown links (`[file.md](file:///... "path/to/some/file.md")`)
    /// showing the basename in chat and full path in a tooltip.
    #[test]
    fn file_mentions_resolve_to_interactive_markdown_links_with_tooltips() {
        let root = std::path::Path::new("/workspace");

        // Relative file path with directory
        assert_eq!(
            resolve_mention_names("@path/to/some/file.md", &[], Some(root)),
            "[file.md](file:///workspace/path/to/some/file.md \"path/to/some/file.md\")"
        );

        // Root file
        assert_eq!(
            resolve_mention_names("@Cargo.toml", &[], Some(root)),
            "[Cargo.toml](file:///workspace/Cargo.toml \"Cargo.toml\")"
        );

        // Dotfile
        assert_eq!(
            resolve_mention_names("@.gitignore", &[], Some(root)),
            "[.gitignore](file:///workspace/.gitignore \".gitignore\")"
        );

        // File with line number
        assert_eq!(
            resolve_mention_names("@src/main.rs:123", &[], Some(root)),
            "[main.rs:123](file:///workspace/src/main.rs#L123 \"src/main.rs:123\")"
        );

        // File with hash fragment
        assert_eq!(
            resolve_mention_names("@src/lib.rs#L42", &[], Some(root)),
            "[lib.rs#L42](file:///workspace/src/lib.rs#L42 \"src/lib.rs#L42\")"
        );

        // Absolute path
        assert_eq!(
            resolve_mention_names("@/home/Jake/dev/hadron/Cargo.lock", &[], Some(root)),
            "[Cargo.lock](file:///home/Jake/dev/hadron/Cargo.lock \"/home/Jake/dev/hadron/Cargo.lock\")"
        );

        // Directory mention
        assert_eq!(
            resolve_mention_names("@crates/hadron-chamber/", &[], Some(root)),
            "[hadron-chamber/](file:///workspace/crates/hadron-chamber/ \"crates/hadron-chamber/\")"
        );

        // Trailing sentence punctuation is preserved outside the link
        assert_eq!(
            resolve_mention_names("Check @src/main.rs. And (@crates/Cargo.toml), see @file.md!", &[], Some(root)),
            "Check [main.rs](file:///workspace/src/main.rs \"src/main.rs\"). And ([Cargo.toml](file:///workspace/crates/Cargo.toml \"crates/Cargo.toml\")), see [file.md](file:///workspace/file.md \"file.md\")!"
        );

        // Mixed with quark mentions
        let roster = vec![RosterRow {
            id: "acp-claude-2".to_string(),
            display_name: Some("Sonnet".to_string()),
            ..opus_roster().pop().unwrap()
        }];
        assert_eq!(
            resolve_mention_names("Hey @acp-claude-2, please look at @crates/hadron-chamber/src/app/mod.rs:50", &roster, Some(root)),
            "Hey @Sonnet, please look at [mod.rs:50](file:///workspace/crates/hadron-chamber/src/app/mod.rs#L50 \"crates/hadron-chamber/src/app/mod.rs:50\")"
        );
    }

    #[test]
    fn email_addresses_are_not_treated_as_mentions() {
        assert_eq!(
            resolve_mention_names("Contact support@hadron.dev for help.", &[], None),
            "Contact support@hadron.dev for help."
        );
    }

    /// Resolution must not fire inside inline code or a fenced block: a `@seat-id` or file mention
    /// there is literal text a human wrote, not a mention to rename.
    #[test]
    fn resolution_is_suppressed_inside_code() {
        let roster = vec![RosterRow {
            id: "acp-claude-2".to_string(),
            display_name: Some("Sonnet".to_string()),
            ..opus_roster().pop().unwrap()
        }];

        assert_eq!(
            resolve_mention_names("Here is `@acp-claude-2` and `@src/main.rs` inline.", &roster, None),
            "Here is `@acp-claude-2` and `@src/main.rs` inline."
        );
        assert_eq!(
            resolve_mention_names("```\n@acp-claude-2\n@src/main.rs\n```", &roster, None),
            "```\n@acp-claude-2\n@src/main.rs\n```"
        );
        // Inside code stays literal; outside resolves in the same string.
        assert_eq!(
            resolve_mention_names("`@acp-claude-2` then @acp-claude-2 with @src/main.rs.", &roster, None),
            "`@acp-claude-2` then @Sonnet with [main.rs](file:///src/main.rs \"src/main.rs\")."
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
                let content = resolve_mention_names(body1, &roster, None);
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
                let content = resolve_mention_names(body2, &roster, None);
                cache.insert(0, (body2.to_string(), content.clone()));
                content
            }
        };
        assert_eq!(res2, "I'm Copilot");

        // Clear resets all cache entries
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn format_markdown_callouts_formats_all_alert_types() {
        let input = "> [!NOTE]\n> This is a note.\n\n> [!TIP] Pro Tip\n> This is a tip.\n\n> [!WARNING]\n> Watch out!\n\n> [!IMPORTANT]\n> Critical info.\n\n> [!CAUTION]\n> Danger!";
        let formatted = format_markdown_callouts(input);
        assert_eq!(
            formatted,
            "> **Note:**\n> This is a note.\n\n> **Tip: Pro Tip**\n> This is a tip.\n\n> **Warning:**\n> Watch out!\n\n> **Important:**\n> Critical info.\n\n> **Caution:**\n> Danger!"
        );

        // Code block callout suppression
        let code_input = "```markdown\n> [!NOTE]\n> Inside code\n```\n> [!NOTE]\n> Outside code";
        let code_formatted = format_markdown_callouts(code_input);
        assert_eq!(
            code_formatted,
            "```markdown\n> [!NOTE]\n> Inside code\n```\n> **Note:**\n> Outside code"
        );
    }
}

