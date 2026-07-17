//! Chat-body text decoration and plan parsing: `@mention` / `/command` colouring
//! (as inline HTML the forked markdown renderer understands) and the plan-checklist
//! progress parser behind the Plan rail. Pure string work — no GPUI, no `Chamber`.

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

const MENTION_QUARK_OPEN: &str = "<span style=\"color: pink-400\"><strong>";
const MENTION_FILE_OPEN: &str = "<span style=\"color: purple-400\"><strong>";
const MENTION_CLOSE: &str = "</strong></span>";

/// Routing names that address the swarm rather than one quark. They are mentions of
/// *us*, so they read as quarks even though no roster row carries the id.
const SWARM_MENTIONS: [&str; 2] = ["team", "orchestrator"];

pub(super) fn color_mentions(body: &str, roster: &[crate::model::RosterRow]) -> String {
    let mut out = String::with_capacity(body.len() + 100);
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

        // A `/command` at a word boundary (start of body, or after whitespace) is a
        // slash command — colour it like a mention. A bare "/" or a slash mid-token
        // (a path like `foo/bar`) is left untouched.
        if c == '/'
            && !in_code_block
            && !in_inline_code
            && (out.is_empty() || out.ends_with(' ') || out.ends_with('\n'))
        {
            let mut cmd = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '-' || nc == '_' {
                    cmd.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            if cmd.is_empty() {
                out.push('/');
            } else {
                out.push_str(&format!(
                    "<span style=\"color: fuchsia-400\"><strong>/{cmd}</strong></span>"
                ));
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

            // Because we greedily consumed spaces and parens, the name might have trailing characters
            // that aren't actually part of a mention (e.g., "@Agy said hi").
            // We need to find the longest matching display name or id.
            let mut matched_quark = false;
            let mut best_match_len = 0;

            for q in roster {
                let q_id = &q.id;
                let q_name = q.display_name.as_ref().unwrap_or(q_id);
                if name.starts_with(q_name) && q_name.len() > best_match_len {
                    best_match_len = q_name.len();
                    matched_quark = true;
                } else if name.starts_with(q_id) && q_id.len() > best_match_len {
                    best_match_len = q_id.len();
                    matched_quark = true;
                }
            }

            let is_swarm = SWARM_MENTIONS.iter().find(|&&m| name.starts_with(m));
            if let Some(m) = is_swarm {
                if m.len() > best_match_len {
                    best_match_len = m.len();
                    matched_quark = true;
                }
            }

            if matched_quark {
                let matched_name = name[..best_match_len].to_string();
                let remainder = name[best_match_len..].to_string();
                out.push_str(&format!("{}@{}{}", MENTION_QUARK_OPEN, matched_name, MENTION_CLOSE));
                out.push_str(&remainder);
            } else {
                // If it's a file, we shouldn't have consumed spaces/parens.
                // We fallback to the old behavior for files: break at first space/paren.
                let mut file_name = String::new();
                for c in name.chars() {
                    if c.is_alphanumeric() || c == '.' || c == '/' || c == '-' || c == '_' {
                        file_name.push(c);
                    } else {
                        break;
                    }
                }
                if file_name.is_empty() {
                    out.push('@');
                    out.push_str(&name);
                } else {
                    let remainder = name[file_name.len()..].to_string();
                    out.push_str(&format!("{}@{}{}", MENTION_FILE_OPEN, file_name, MENTION_CLOSE));
                    out.push_str(&remainder);
                }
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
    fn test_color_commands() {
        let colored = color_mentions("Please run /plan and /grill-me today.", &[]);
        assert!(colored.contains("<span style=\"color: fuchsia-400\"><strong>/plan</strong></span>"));
        assert!(colored.contains("<span style=\"color: fuchsia-400\"><strong>/grill-me</strong></span>"));

        // Should ignore slashes inside code blocks
        let code = color_mentions("Code `/plan` inside.", &[]);
        assert!(!code.contains("color: fuchsia-400"));
    }

    #[test]
    fn test_mentions_parse_as_raw_html() {
        let body = "Hello @opus!";
        let roster = vec![RosterRow {
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
        }];

        let colored = color_mentions(body, &roster);
        assert_eq!(
            colored,
            "Hello <span style=\"color: pink-400\"><strong>@opus</strong></span>!"
        );

        let options = markdown::Options {
            compile: markdown::CompileOptions {
                allow_dangerous_html: true,
                ..markdown::CompileOptions::default()
            },
            parse: markdown::ParseOptions::gfm(),
        };
        let html = markdown::to_html_with_options(&colored, &options).unwrap();
        // The HTML should literally contain the span, meaning it wasn't escaped
        assert!(html.contains("<span style=\"color: pink-400\"><strong>@opus</strong></span>"));
    }

    /// `@team` and `@orchestrator` route to the swarm rather than to a roster row, so
    /// nothing in the roster carries those ids — they must still read as quarks, not
    /// as filenames.
    #[test]
    fn swarm_mentions_colour_as_quarks_not_files() {
        let colored = color_mentions("@team and @orchestrator and @src/main.rs", &[]);
        assert!(colored.contains(&format!("{MENTION_QUARK_OPEN}@team")));
        assert!(colored.contains(&format!("{MENTION_QUARK_OPEN}@orchestrator")));
        assert!(colored.contains(&format!("{MENTION_FILE_OPEN}@src/main.rs")));
    }

    #[test]
    fn color_mentions_ignores_mentions_in_code() {
        let roster = vec![crate::model::RosterRow {
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
        }];

        // Inline code mention
        let colored_inline = color_mentions("Here is `@opus` inside inline code.", &roster);
        assert_eq!(colored_inline, "Here is `@opus` inside inline code.");

        // Code block mention
        let colored_block = color_mentions("```rust\n@opus\n```", &roster);
        assert_eq!(colored_block, "```rust\n@opus\n```");

        // Code block mention with multiple ticks and mixed text
        let colored_mixed = color_mentions("Before `@opus` inside, and @opus outside.", &roster);
        assert!(colored_mixed.contains(&format!("{MENTION_QUARK_OPEN}@opus")));
        assert!(colored_mixed.contains("`@opus`"));
    }

    /// An unparseable colour is not an error in gpui-component — it is silently
    /// dropped. And `TextMark::color` exists only in our fork (see the `[patch]` in
    /// the workspace Cargo.toml): if that patch ever stops applying, we fall back to
    /// upstream, mentions render as plain text, and nothing else would notice.
    #[test]
    fn mention_colours_are_ones_the_forked_renderer_can_apply() {
        for markup in [MENTION_QUARK_OPEN, MENTION_FILE_OPEN] {
            let color = markup
                .split_once("color: ")
                .and_then(|(_, rest)| rest.split_once('"'))
                .map(|(c, _)| c)
                .expect("mention markup carries a color declaration");
            let parsed = gpui_component::try_parse_color(color)
                .unwrap_or_else(|e| panic!("{color} is not a colour gpui-component accepts: {e}"));

            // Fails to compile against upstream gpui-component, which has no `color`.
            let mark = gpui_component::text::TextMark::default().color(parsed);
            assert_eq!(mark.color, Some(parsed));
        }
    }
}
