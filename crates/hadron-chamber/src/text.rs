//! Pure text logic shared by the UI — no `gpui`, no feature gate.
//!
//! `extract_completion_query` lived in `completions`, which imports `gpui` and is
//! therefore `#[cfg(feature = "gui")]`. That gating was incidental to the function
//! (it is pure `&str` work) but not to its **tests**: `cargo test --workspace` does
//! not build the `gui` feature, so the regression test guarding the emoji crash
//! never ran in the gate that decides whether we ship. A crash guard invisible to
//! the gate is a crash guard that will be broken again without anyone noticing.

/// The two `@names` that are **not** roster ids but still route: `@orchestrator`
/// resolves to whoever currently holds the Orchestrator seat, and `@team` fans out
/// to every quark. The chat field is the human's, and `@team` is human-only, so
/// both belong in its completion list — they were missing, so the one mention Jake
/// uses most often was the one the menu would not offer.
///
/// The names come from the router's own constants rather than being retyped here:
/// they are the single source of truth for what actually routes, and a completion
/// that offers a name the router does not resolve is worse than no completion.
pub const MENTION_ALIASES: [(&str, &str); 2] = [
    (
        hadron_gluon::router::ORCHESTRATOR_ALIAS,
        "Whoever holds the Orchestrator seat",
    ),
    (hadron_gluon::router::TEAM_ALIAS, "Everyone on the roster"),
];

/// Case-insensitive substring match, the same rule the quark and file rows use.
pub fn mention_matches(name: &str, query_lower: &str) -> bool {
    query_lower.is_empty() || name.to_lowercase().contains(query_lower)
}

/// How much of its line a `/command` consumes.
///
/// An enum rather than two `&[&str]` lists, because the lists could disagree with
/// the completion menu — and did, for six commands, silently (see [`COMMANDS`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    /// Takes no argument, so several can chain ahead of a message on one line.
    None,
    /// Consumes the rest of its line as its argument.
    Line,
}

/// One chat `/command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The name typed after the slash. The lookup key, and unique.
    pub name: &'static str,
    /// The one-line gloss shown in the completion menu.
    pub detail: &'static str,
    pub arity: Arity,
    /// Whether the completion menu offers it. `false` marks a working **alias**
    /// we simply do not advertise (`/quit` is `/exit`) — never an unimplemented
    /// command. Nothing may sit in this table that the handler does not handle.
    pub listed: bool,
}

/// **The single source of truth for what a `/command` is.**
///
/// A command used to live in three places that could disagree: the completion
/// rows here, the `ZERO_ARG_CMDS`/`LINE_ARG_CMDS` lists in `app::input`, and the
/// `match` in `app::actions`. Six commands (`teamwork-preview`, `plan`, `goal`,
/// `grill-me`, `schedule`, `learn`) sat in the menu and in neither list, so
/// choosing one from the menu silently posted the whole line as chat. They were
/// ported from another CLI's command surface and never had handlers here.
///
/// Now the menu and the parser both read this table, so "offered but unparsed"
/// cannot be written. The `match` arm still cannot be checked by the compiler —
/// `every_listed_command_is_handled` in `app::input` is the guard that closes it.
pub const COMMANDS: &[Command] = &[
    Command { name: "help", detail: "List every chat command", arity: Arity::None, listed: true },
    Command { name: "skills", detail: "List the skills the engine can hand a quark, and their triggers", arity: Arity::None, listed: true },
    Command { name: "vocabulary", detail: "What each Hadron word means — quark, preon, field, gluon…", arity: Arity::None, listed: true },
    Command { name: "clear", detail: "Archive and clear the current chat history", arity: Arity::None, listed: true },
    Command { name: "exit", detail: "Exit Hadron Chamber", arity: Arity::None, listed: true },
    // A working alias of `/exit`, kept so existing muscle memory does not break,
    // unlisted so the menu offers one way to do it.
    Command { name: "quit", detail: "Exit Hadron Chamber", arity: Arity::None, listed: false },
    Command { name: "toggle-roster", detail: "Toggle the Roster sidebar", arity: Arity::None, listed: true },
    Command { name: "toggle-inspector", detail: "Toggle the Inspector sidebar", arity: Arity::None, listed: true },
    // The skill commands. Each posts a message carrying the skill's own canonical
    // trigger, so the engine selects the procedure — see `skill_command_body`.
    Command { name: "brainstorm", detail: "Explore a design before any code (e.g. /brainstorm @Sonnet the new menu)", arity: Arity::Line, listed: true },
    Command { name: "writing-plans", detail: "Turn a settled design into an implementation plan", arity: Arity::Line, listed: true },
    Command { name: "executing-plans", detail: "Work through an existing plan, task by task", arity: Arity::Line, listed: true },
    Command { name: "team-brainstorm", detail: "Kick off brainstorming with the whole team", arity: Arity::Line, listed: true },
    Command { name: "reboot", detail: "Force-restart a resident quark (e.g. /reboot @acp-claude or /reboot all)", arity: Arity::Line, listed: true },
    Command { name: "approve", detail: "Approve a pending permission request (e.g. /approve @worker or /approve @worker remember)", arity: Arity::Line, listed: true },
    Command { name: "deny", detail: "Deny a pending permission request (e.g. /deny @worker)", arity: Arity::Line, listed: true },
    Command { name: "toggle", detail: "Park or unpark a quark — keeps the seat, skips its turns (e.g. /toggle @Sonnet)", arity: Arity::Line, listed: true },
    Command { name: "rename", detail: "Name the current session (e.g. /rename bugfix-router)", arity: Arity::Line, listed: true },
    Command { name: "resume", detail: "Reopen an archived session as the live one (e.g. /resume bugfix-router)", arity: Arity::Line, listed: true },
    Command { name: "limit", detail: "Set custom energy token limit for a seat (e.g. /limit @acp-claude 1000000)", arity: Arity::Line, listed: true },
    Command { name: "reset-energy", detail: "Reset used token ledger for a seat or all (e.g. /reset-energy @acp-claude or /reset-energy all)", arity: Arity::Line, listed: true },
];

/// Look a command up by the name typed after the slash.
pub fn command(name: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.name == name)
}

/// The markdown `/help` prints.
///
/// Pure and here rather than in the handler so the property that makes it safe to
/// print is testable: it must contain **no line beginning with `@`**. `/help` is
/// posted by `Actor::Gluon`, and the only way such an event reaches a seat is an
/// addressee parsed out of the body — so a stray leading mention would turn the
/// help text into a dispatch.
pub fn help_body() -> String {
    let mut out = String::from("**Chat commands**\n\n");
    for c in COMMANDS.iter().filter(|c| c.listed) {
        let arg = match c.arity {
            Arity::None => "",
            Arity::Line => " <…>",
        };
        out.push_str(&format!("- `/{}{}` — {}\n", c.name, arg, c.detail));
    }
    out.push_str(
        "\nA command may start any line, not just the first — so you can write a \
         paragraph and put the command underneath it. Inside a ``` fence it stays \
         plain text.\n",
    );
    out
}

/// Split an optional leading `@target` off a skill command's argument.
///
/// `/brainstorm @Sonnet the menu` → `(Some("Sonnet"), "the menu")`, and
/// `/brainstorm the menu` → `(None, "the menu")`. The caller substitutes the
/// orchestrator alias when there is no target, which is the rule Jake asked for:
/// the mentioned quark, or the orchestrator when none is named.
pub fn split_target(args: &str) -> (Option<&str>, &str) {
    let args = args.trim();
    let Some(rest) = args.strip_prefix('@') else {
        return (None, args);
    };
    // `find` returns a char boundary by construction, so `split_at` cannot land
    // mid-character — the crash class this file is otherwise full of.
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    let (target, task) = rest.split_at(end);
    if target.is_empty() {
        // A bare `@` with nothing attached is not a target. Return the text AFTER
        // it, not the original argument: returning `args` leaked the stray `@`
        // into the task, so `/brainstorm @ fix the router` posted "… Let's
        // brainstorm: @ fix the router".
        (None, task.trim())
    } else {
        (Some(target), task.trim())
    }
}

/// The chat message a skill command posts.
///
/// `trigger` is the skill's own canonical trigger phrase, taken from
/// `hadron_gluon::skills` rather than retyped here: the engine selects a skill by
/// matching that phrase against the task text (`skills::select` is a pure function
/// of the text), so a copy in the chamber would silently stop selecting the skill
/// the day someone edited the trigger list.
pub fn skill_command_body(trigger: &str, target: &str, task: &str) -> String {
    if task.is_empty() {
        format!("@{target} Let's {trigger}.")
    } else {
        format!("@{target} Let's {trigger}: {task}")
    }
}

/// Find the `@`/`:` completion trigger immediately before the cursor.
///
/// Returns the trigger char, the query typed after it, and its byte index.
///
/// **`offset` is a BYTE offset from the editor and may land anywhere** — including
/// past the end of the string, or in the middle of a multi-byte character. Slicing
/// `&text[..offset]` on either panics, and that panic is the emoji crash: type an
/// emoji into the chat box and the cursor sits at a byte offset that is not a char
/// boundary. Clamp into range, then walk back to the nearest boundary.
pub fn extract_completion_query(text: &str, offset: usize) -> Option<(char, String, usize)> {
    let mut safe_offset = offset.min(text.len());
    while safe_offset > 0 && !text.is_char_boundary(safe_offset) {
        safe_offset -= 1;
    }

    let before_cursor = &text[..safe_offset];
    for (idx, c) in before_cursor.char_indices().rev() {
        let is_slash_cmd = c == '/'
            && (idx == 0
                || before_cursor[..idx]
                    .chars()
                    .next_back()
                    .map_or(false, |ch| ch.is_whitespace() || ch == '\n'));
        if c == '@' || c == ':' || is_slash_cmd {
            let query = before_cursor[idx + c.len_utf8()..].to_string();
            return Some((c, query, idx));
        }
        if c.is_whitespace() {
            break;
        }
    }
    None

}

/// The label and filter text of one completion-menu row, built so the menu cannot
/// panic on it.
///
/// **Jake's `::` crash.** The completion menu highlights the matched prefix of a row
/// by byte range — `0..filter_text.len()` (gpui-component `completion_menu.rs`) — and
/// gpui `debug_assert!`s that both ends of a highlight range are char boundaries
/// (`gpui/src/elements/text.rs`). We used to build emoji rows as `"📺 :tv"`: the label
/// *starts* with a 4-byte emoji, and `":tv".len()` is 3, so the range `0..3` lands
/// **inside** the emoji and the assert fires. Instant crash, any query that surfaces
/// an emoji whose shortcode is shorter than the leading emoji is wide.
///
/// The fix is structural, not a check: a label that **begins with its own filter
/// text** always has a char boundary at `filter_text.len()`, because the filter text
/// is a prefix of it. This type is the only way to construct a row, so the crashing
/// shape cannot be written.
#[allow(dead_code)] // char-boundary guard type, exercised only by tests
pub struct MenuRow {
    label: String,
    filter_text: String,
}

#[allow(dead_code)] // char-boundary guard type, exercised only by tests
impl MenuRow {
    /// `filter_text` is what the user is matching against and what the menu
    /// highlights; `trailing` is anything shown after it (an emoji glyph, a hint).
    pub fn new(filter_text: impl Into<String>, trailing: &str) -> Self {
        let filter_text = filter_text.into();
        let label = if trailing.is_empty() {
            filter_text.clone()
        } else {
            format!("{filter_text} {trailing}")
        };
        Self { label, filter_text }
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn filter_text(&self) -> &str {
        &self.filter_text
    }

    /// The byte range the menu will highlight. Always a char boundary, by construction.
    pub fn highlight_len(&self) -> usize {
        self.filter_text.len()
    }
}

/// One row the completion card offers: what to show, a short right-hand hint, and
/// the exact text that replaces the query span when the row is accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub label: String,
    pub detail: String,
    pub new_text: String,
}

/// The live completion query and the rows that answer it.
///
/// `start` is the BYTE offset of the trigger char (`@`/`:`/`/`); accepting a row
/// replaces `text[start..cursor]` with the row's `new_text`. Both offsets are
/// UTF-8 byte offsets — the same unit the editor's `cursor()`/`set_selected_range`
/// use — so the accept is a plain string splice with no UTF-16 conversion.
pub struct Completions {
    pub start: usize,
    pub candidates: Vec<Candidate>,
}

/// Cap on rows built. A bare `:` matches every emoji (thousands); without this the
/// card would be thousands of rows tall. The card scrolls, but building them all is
/// pointless — the user narrows with a query.
pub const MAX_CANDIDATES: usize = 50;

/// Build the completion rows for the trigger immediately before `cursor`, or `None`
/// when there is no live trigger (so the card closes).
///
/// This is the single source of truth for what the chat completion card offers. It
/// is pure `&str`/`&[..]` work — no `gpui` — so its behaviour is pinned by tests in
/// the workspace gate, unlike the fork's `CompletionProvider` which only compiled
/// under `--features gui`.
pub fn completion_candidates(
    text: &str,
    cursor: usize,
    quarks: &[(String, Option<String>)],
    files: &[String],
) -> Option<Completions> {
    let (trigger, query, start) = extract_completion_query(text, cursor)?;
    let query_lower = query.to_lowercase();
    let mut out: Vec<Candidate> = Vec::new();

    match trigger {
        '@' => {
            // Routing aliases first — `@team`/`@orchestrator` are what the human
            // reaches for most, and neither is a roster id.
            for (alias, detail) in MENTION_ALIASES {
                if mention_matches(alias, &query_lower) {
                    out.push(Candidate {
                        label: format!("@{alias}"),
                        detail: detail.to_string(),
                        new_text: format!("@{alias} "),
                    });
                }
            }
            for (id, display) in quarks {
                let name = display.as_ref().unwrap_or(id);
                let name_l = name.to_lowercase();
                let id_l = id.to_lowercase();
                if query_lower.is_empty()
                    || name_l.contains(&query_lower)
                    || id_l.contains(&query_lower)
                {
                    out.push(Candidate {
                        label: format!("@{name}"),
                        detail: "Quark".into(),
                        new_text: format!("@{name} "),
                    });
                }
            }
            for file in files {
                if out.len() >= MAX_CANDIDATES {
                    break;
                }
                if query_lower.is_empty() || file.to_lowercase().contains(&query_lower) {
                    out.push(Candidate {
                        label: format!("@{file}"),
                        detail: "File".into(),
                        new_text: format!("@{file} "),
                    });
                }
            }
        }
        ':' => {
            if query_lower.is_empty() {
                const MODERN_EMOJIS: &[(&str, &str)] = &[
                    ("rofl", "🤣"),
                    ("skull", "💀"),
                    ("sob", "😭"),
                    ("fire", "🔥"),
                    ("100", "💯"),
                    ("sparkles", "✨"),
                    ("rocket", "🚀"),
                    ("eyes", "👀"),
                    ("thinking", "🤔"),
                    ("thumbsup", "👍"),
                    ("tada", "🎉"),
                    ("warning", "⚠️"),
                    ("white_check_mark", "✅"),
                    ("x", "❌"),
                    ("bulb", "💡"),
                    ("bug", "🪲"),
                    ("clown", "🤡"),
                    ("coffee", "☕"),
                    ("exploding_head", "🤯"),
                    ("nerd", "🤓"),
                    ("salute", "🫡"),
                    ("shrug", "🤷"),
                    ("pleading", "🥺"),
                    ("heart_hands", "🫶"),
                    ("raised_hands", "🙌"),
                    ("poop", "💩"),
                    ("handshake", "🤝"),
                    ("computer", "💻"),
                    ("hammer_and_wrench", "🛠️"),
                    ("art", "🎨"),
                    ("zap", "⚡"),
                    ("dart", "🎯"),
                    ("sweat_smile", "😅"),
                    ("heart_eyes", "😍"),
                    ("sunglasses", "😎"),
                    ("partying", "🥳"),
                    ("scream", "😱"),
                    ("roll_eyes", "🙄"),
                    ("cry", "😢"),
                    ("rage", "😡"),
                    ("sleepy", "😪"),
                    ("wave", "👋"),
                    ("clap", "👏"),
                    ("pray", "🙏"),
                    ("heart", "❤️"),
                    ("broken_heart", "💔"),
                    ("star", "⭐"),
                    ("money_bag", "💰"),
                    ("key", "🔑"),
                    ("lock", "🔒"),
                ];

                for &(shortcode, glyph) in MODERN_EMOJIS {
                    if out.len() >= MAX_CANDIDATES {
                        break;
                    }
                    out.push(Candidate {
                        label: format!(":{shortcode} {glyph}"),
                        detail: "Emoji".into(),
                        new_text: glyph.to_string(),
                    });
                }
            } else {
                let mut matched_emojis = Vec::new();
                for emoji in emojis::iter() {
                    if let Some(shortcode) = emoji.shortcode() {
                        if shortcode.to_lowercase().contains(&query_lower) {
                            matched_emojis.push((shortcode, emoji.as_str()));
                        }
                    }
                }
                matched_emojis.sort_by_key(|&(shortcode, _)| (shortcode.len(), shortcode.to_string()));

                for (shortcode, glyph) in matched_emojis {
                    if out.len() >= MAX_CANDIDATES {
                        break;
                    }
                    out.push(Candidate {
                        label: format!(":{shortcode} {glyph}"),
                        detail: "Emoji".into(),
                        new_text: glyph.to_string(),
                    });
                }
            }
        }
        '/' => {
            // Straight off `COMMANDS`, so the menu cannot offer a command the
            // parser does not recognise.
            for cmd in COMMANDS.iter().filter(|c| c.listed) {
                if query_lower.is_empty() || cmd.name.contains(&query_lower) {
                    out.push(Candidate {
                        label: format!("/{}", cmd.name),
                        detail: cmd.detail.to_string(),
                        new_text: format!("/{} ", cmd.name),
                    });
                }
            }
        }
        _ => {}
    }

    if out.is_empty() {
        return None;
    }
    Some(Completions { start, candidates: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quarks() -> Vec<(String, Option<String>)> {
        vec![
            ("acp-claude".into(), None),
            ("agy".into(), Some("Agy".into())),
        ]
    }

    #[test]
    fn a_mention_query_offers_matching_quarks_and_aliases() {
        let c = completion_candidates("@ag", 3, &quarks(), &[]).expect("has rows");
        assert_eq!(c.start, 0, "replace span starts at the '@'");
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"@Agy"), "matched quark offered: {labels:?}");
        // The accepted text carries the sigil and a trailing space, ready to type on.
        let agy = c.candidates.iter().find(|c| c.label == "@Agy").unwrap();
        assert_eq!(agy.new_text, "@Agy ");
    }

    #[test]
    fn an_empty_mention_query_offers_the_routing_aliases_first() {
        let c = completion_candidates("@", 1, &quarks(), &[]).expect("has rows");
        assert_eq!(
            c.candidates[0].label,
            format!("@{}", hadron_gluon::router::ORCHESTRATOR_ALIAS),
            "aliases lead the list"
        );
    }

    #[test]
    fn a_file_query_offers_files() {
        let files = vec!["src/app.rs".to_string(), "README.md".to_string()];
        let c = completion_candidates("@app", 4, &[], &files).expect("has rows");
        assert_eq!(c.candidates.len(), 1);
        assert_eq!(c.candidates[0].new_text, "@src/app.rs ");
    }

    #[test]
    fn a_bare_emoji_trigger_is_capped_not_thousands() {
        let c = completion_candidates(":", 1, &[], &[]).expect("has rows");
        assert!(
            c.candidates.len() <= MAX_CANDIDATES,
            "a bare ':' must not build thousands of rows: got {}",
            c.candidates.len()
        );
    }

    #[test]
    fn an_emoji_query_accepts_the_glyph_not_the_shortcode() {
        let c = completion_candidates(":rofl", 5, &[], &[]).expect("has rows");
        let first = &c.candidates[0];
        assert!(first.label.starts_with(":rofl"));
        // Accepting inserts the glyph itself, not the `:rofl:` text.
        assert!(!first.new_text.starts_with(':'));
    }

    #[test]
    fn an_emoji_query_searches_the_entire_crate() {
        // "globe_with_meridians" is a real but less common emoji in the emojis crate.
        let c = completion_candidates(":globe_with_meridians", 21, &[], &[]).expect("has rows");
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with(":globe_with_meridians")), "should search the whole crate: {labels:?}");
    }

    #[test]
    fn a_bare_emoji_query_returns_curated_modern_emojis() {
        let c = completion_candidates(":", 1, &[], &[]).expect("has rows");
        // Verify we got the curated 50 emojis (or at least capped to MAX_CANDIDATES).
        assert_eq!(c.candidates.len(), 50);
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels[0].starts_with(":rofl"), "first emoji should be rofl: {:?}", labels[0]);
    }

    /// Found in review of `c3b978d`: a multi-byte char after `@` must not panic,
    /// and a bare `@ ` must not leak into the task text.
    #[test]
    fn split_target_handles_the_awkward_at_signs() {
        assert_eq!(split_target("@Sonnet the menu"), (Some("Sonnet"), "the menu"));
        assert_eq!(split_target("no target here"), (None, "no target here"));
        // Multi-byte first char: `find` returns a char boundary, so no panic.
        assert_eq!(split_target("@😀name task"), (Some("😀name"), "task"));
        // A bare `@` followed by space is not a target, and does not survive.
        assert_eq!(split_target("@ fix the router"), (None, "fix the router"));
        // A lone `@` has no text after it at all.
        assert_eq!(split_target("@"), (None, ""));
    }

    /// `/help` is posted by `Actor::Gluon`, which reaches a seat ONLY through an
    /// addressee parsed out of the body. A line beginning with `@` would therefore
    /// turn the help text into a dispatch — and answering "what can I type" must
    /// never cost a turn.
    #[test]
    fn the_help_text_addresses_nobody() {
        let body = help_body();
        for line in body.lines() {
            assert!(
                !line.trim_start().starts_with('@'),
                "help text would route to a seat: {line:?}"
            );
        }
        // And it really does describe the live table, not a frozen copy.
        for c in COMMANDS.iter().filter(|c| c.listed) {
            assert!(
                body.contains(&format!("`/{}", c.name)),
                "/{} is offered by the menu but missing from /help",
                c.name
            );
        }
        assert!(
            !body.contains("`/quit"),
            "unlisted aliases stay out of the help text"
        );
    }

    #[test]
    fn a_slash_command_is_offered_only_at_the_line_start() {
        let c = completion_candidates("/tog", 4, &[], &[]).expect("has rows");
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"/toggle-roster"), "matched toggle-roster offered: {labels:?}");
        // Was `/goa` — `/goal` was one of six rows the menu offered with no handler,
        // so choosing it posted the line as chat. Retired; `/brainstorm` is a real one.
        assert!(completion_candidates("/brain", 6, &[], &[]).is_some());


        let c_reboot = completion_candidates("/reb", 4, &[], &[]).expect("has rows");
        let labels_reboot: Vec<&str> = c_reboot.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels_reboot.contains(&"/reboot"), "matched reboot offered: {labels_reboot:?}");

        let c_approve = completion_candidates("/app", 4, &[], &[]).expect("has rows");
        let labels_approve: Vec<&str> = c_approve.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels_approve.contains(&"/approve"), "matched approve offered: {labels_approve:?}");

        let c_deny = completion_candidates("/den", 4, &[], &[]).expect("has rows");
        let labels_deny: Vec<&str> = c_deny.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels_deny.contains(&"/deny"), "matched deny offered: {labels_deny:?}");
        
        // Mid-line `/` at a word boundary IS a trigger.
        assert!(completion_candidates("hi /brain", 9, &[], &[]).is_some());
        // Path slashes are not triggers.
        assert!(completion_candidates("src/app.rs", 10, &[], &[]).is_none());
    }


    #[test]
    fn no_trigger_yields_no_card() {
        assert!(completion_candidates("just talking", 12, &quarks(), &[]).is_none());
    }

    #[test]
    fn a_cursor_past_the_end_or_mid_emoji_does_not_panic() {
        let hostile = "hi 🌍 @a";
        for cursor in 0..=hostile.len() + 4 {
            let _ = completion_candidates(hostile, cursor, &quarks(), &[]);
        }
    }

    /// `@team` and `@orchestrator` route, but the completion menu never offered
    /// them: it only listed roster ids, and neither alias is one. The names are
    /// asserted against the router's own constants, so a rename over there breaks
    /// this test rather than silently leaving the menu offering a dead mention.
    #[test]
    fn the_routing_aliases_are_offered_and_are_the_ones_that_actually_route() {
        let names: Vec<&str> = MENTION_ALIASES.iter().map(|(n, _)| *n).collect();
        assert!(
            names.contains(&hadron_gluon::router::TEAM_ALIAS),
            "@team routes but was not offered: {names:?}"
        );
        assert!(
            names.contains(&hadron_gluon::router::ORCHESTRATOR_ALIAS),
            "@orchestrator routes but was not offered: {names:?}"
        );

        // Typing `@te` must reach `team`, and `@ORCH` must reach `orchestrator` —
        // the router matches case-insensitively, so the menu must too.
        assert!(mention_matches(hadron_gluon::router::TEAM_ALIAS, "te"));
        assert!(mention_matches(
            hadron_gluon::router::ORCHESTRATOR_ALIAS,
            "orch"
        ));
        assert!(mention_matches(hadron_gluon::router::TEAM_ALIAS, ""));
        assert!(!mention_matches(hadron_gluon::router::TEAM_ALIAS, "zzz"));
    }

    /// The guard for Jake's `::` crash, over **every emoji the picker can offer** —
    /// not a hand-picked one. `:tv`, `:id` and `:vs` are the short shortcodes that
    /// actually fired it; an exhaustive sweep does not depend on us guessing right.
    #[test]
    fn no_emoji_row_can_put_the_menus_highlight_inside_a_multibyte_char() {
        for emoji in emojis::iter() {
            let Some(shortcode) = emoji.shortcode() else {
                continue;
            };
            let row = MenuRow::new(format!(":{shortcode}"), emoji.as_str());
            assert!(
                row.label().is_char_boundary(row.highlight_len()),
                "menu would slice {:?} at byte {} — mid-character, gpui panics",
                row.label(),
                row.highlight_len()
            );
        }
    }

    /// The mechanism itself, pinned: the label shape we *used* to build is the crash.
    /// If this ever stops holding, the bug was fixed somewhere else and `MenuRow` can go.
    #[test]
    fn the_old_emoji_first_label_really_does_land_mid_character() {
        let tv = emojis::get_by_shortcode("tv").expect(":tv is in the emojis crate");
        let old_label = format!("{} :tv", tv.as_str()); // what completions.rs built
        assert!(
            !old_label.is_char_boundary(":tv".len()),
            "the premise of the crash: byte 3 of {old_label:?} is inside the emoji"
        );
    }

    /// A row whose trailing text is a bare mention has nothing after the filter text,
    /// and the highlight covers the whole label.
    #[test]
    fn a_row_with_no_trailing_text_is_all_highlight() {
        let row = MenuRow::new("@opus", "");
        assert_eq!(row.label(), "@opus");
        assert_eq!(row.highlight_len(), row.label().len());
    }

    #[test]
    fn finds_a_mention_and_an_emoji_trigger() {
        assert_eq!(
            extract_completion_query("hey @op", 7),
            Some(('@', "op".to_string(), 4))
        );
        assert_eq!(
            extract_completion_query("nice :smi", 9),
            Some((':', "smi".to_string(), 5))
        );
        assert_eq!(extract_completion_query("no trigger", 10), None);
    }

    /// **Jake's crash.** Every byte offset into a string full of multi-byte
    /// characters must be survivable — including offsets that land *inside* an
    /// emoji, and offsets past the end. Exhaustive rather than exemplary: a single
    /// hand-picked offset proves nothing about the one the editor actually sends.
    #[test]
    fn no_byte_offset_into_a_multibyte_string_can_panic() {
        // A 4-byte emoji, a ZWJ family sequence, an accented char, a CJK char.
        let hostile = "Hi 🌍 @wor 👨‍👩‍👧‍👦 café 日本 :smi 🙃";

        for offset in 0..=hostile.len() + 8 {
            // Must not panic at ANY offset, boundary or not, in range or past the end.
            let _ = extract_completion_query(hostile, offset);
        }
    }

    /// Agy's original regression test, moved here from `completions` so it runs in
    /// `cargo test --workspace` rather than only under `--features gui`.
    #[test]
    fn text_with_emoji_does_not_panic_on_offset() {
        let text = "Hello 🌍 @world";
        // 🌍 occupies bytes 6..10.
        assert_eq!(extract_completion_query(text, 10), None);
        assert_eq!(
            extract_completion_query(text, text.len()),
            Some(('@', "world".to_string(), 11))
        );
        // An offset landing INSIDE the emoji must not crash.
        assert_eq!(extract_completion_query(text, 8), None);
    }

    /// The clamp must not silently change the answer for well-formed input: walking
    /// back from a mid-emoji offset should still find the trigger that precedes it.
    #[test]
    fn walking_back_from_inside_an_emoji_still_finds_the_trigger() {
        let text = "@ab🌍";
        let mid_emoji = text.len() - 2; // inside the 4-byte emoji
        assert!(!text.is_char_boundary(mid_emoji), "the test's premise");

        let (trigger, query, idx) = extract_completion_query(text, mid_emoji).unwrap();
        assert_eq!(trigger, '@');
        assert_eq!(idx, 0);
        assert_eq!(query, "ab", "the partial emoji is dropped, not sliced");
    }

    #[test]
    fn a_slash_command_is_offered_at_word_boundaries_mid_text() {
        let text = "hello /plan";
        assert_eq!(
            extract_completion_query(text, text.len()),
            Some(('/', "plan".to_string(), 6))
        );

        let multi_line = "first line\n/reboot";
        assert_eq!(
            extract_completion_query(multi_line, multi_line.len()),
            Some(('/', "reboot".to_string(), 11))
        );

        // Path slashes must NOT trigger completions.
        let path = "src/app.rs";
        assert_eq!(extract_completion_query(path, path.len()), None);
    }

    #[test]
    fn exit_command_completion_candidates() {
        let completions = completion_candidates("/ex", 3, &[], &[]).unwrap();
        assert!(completions.candidates.iter().any(|c| c.label == "/exit"));
    }

    #[test]
    fn rename_and_resume_command_completion_candidates() {
        let completions = completion_candidates("/ren", 4, &[], &[]).unwrap();
        assert!(completions.candidates.iter().any(|c| c.label == "/rename"));

        let completions = completion_candidates("/res", 4, &[], &[]).unwrap();
        assert!(completions.candidates.iter().any(|c| c.label == "/resume"));
    }
}

