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
    Command { name: "learn", detail: "Pin a lesson into this repo's nucleus (e.g. /learn always run cargo fmt first)", arity: Arity::Line, listed: true },
    Command { name: "learn-global", detail: "Pin a lesson into your global nucleus, across every repo", arity: Arity::Line, listed: true },
    Command { name: "learn-std-model", detail: "Add a standing law to this repo (appends to laws.md, never edits the Standard Model)", arity: Arity::Line, listed: true },
    Command { name: "learn-std-model-global", detail: "Add a standing law across every repo you run Hadron in", arity: Arity::Line, listed: true },
    Command { name: "gate-status", detail: "Show which branch the merge gate is running, since when, and time left", arity: Arity::None, listed: true },
    Command { name: "abandon", detail: "Archive-tag then discard a quark's pending branch (e.g. /abandon @acp-claude, then /abandon @acp-claude confirm to force)", arity: Arity::Line, listed: true },
];

/// A short kebab-case id for a lesson line: the first few words, lowercased,
/// stripped to alphanumerics and hyphens. Not guaranteed unique — the caller
/// appends a suffix on collision, since uniqueness needs to see the existing index.
pub(crate) fn slugify(text: &str) -> String {
    text.split_whitespace()
        .take(5)
        .collect::<Vec<_>>()
        .join("-")
        .to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect()
}

/// What a memory is FOR, which is not the same as what it says. Four fixed kinds,
/// as an enum rather than a string: the type tells a quark *how* to use the fact,
/// and a fifth spelling arriving by typo would silently mean nothing.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
// `/learn` writes `User` (the human typing a fact directly). The other three are
// the format's contract — what a quark writing a note by hand must choose from —
// so they are spelled once here rather than as free strings in a document.
#[allow(dead_code)]
pub enum MemoryType {
    /// Who the human is — role, expertise, standing preferences.
    User,
    /// Guidance on how to work, including the why behind a correction.
    Feedback,
    /// Ongoing work, goals or constraints not derivable from the code itself.
    Project,
    /// A pointer to something external — a URL, a dashboard, a ticket.
    Reference,
}

impl MemoryType {
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryType::User => "user",
            MemoryType::Feedback => "feedback",
            MemoryType::Project => "project",
            MemoryType::Reference => "reference",
        }
    }
}

/// How long an index hook may be. The index is force-loaded into every prompt of
/// every turn, and `.hadron/nucleus/index.md` reached 46 KB against the engine's
/// 32 KB `NUCLEUS_INDEX_BUDGET` — past which the prompt sends a per-section COUNT
/// and no lesson text at all. An unbounded hook is that failure with extra steps,
/// so the bound lives in the writer.
pub(crate) const HOOK_MAX_CHARS: usize = 100;

/// A one-line, length-bounded hook for the index. Truncation counts CHARACTERS and
/// never bytes — a byte slice into prose lands mid-character and panics (the char
/// boundary rule this file is otherwise full of).
pub(crate) fn hook(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= HOOK_MAX_CHARS {
        return flat;
    }
    flat.chars().take(HOOK_MAX_CHARS).chain(std::iter::once('…')).collect()
}

/// One nucleus-index line: a POINTER, never the lesson. `- [<slug>](notes/<slug>.md)
/// — <hook>`. Content in the index is a bug — the index is a routing table that every
/// quark pays for on every turn, and the fact belongs in the note the engine never
/// loads until someone opens it.
pub(crate) fn learn_line(slug: &str, hook: &str) -> String {
    format!("- [{slug}](notes/{slug}.md) — {hook}\n")
}

/// The note itself: frontmatter, then the fact. `description` is a **retrieval key**,
/// not a summary — its only job is letting a quark decide whether to open the file,
/// which is what keeps the index short enough to always send.
pub(crate) fn note_body(
    slug: &str,
    description: &str,
    kind: MemoryType,
    fact: &str,
) -> String {
    let kind = kind.as_str();
    let fact = fact.trim();
    format!(
        "---\nname: {slug}\ndescription: {description}\nmetadata:\n  type: {kind}\n---\n\n{fact}\n"
    )
}

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

/// The exact prefix `engine::run.rs`'s superseded-assignment arm posts before it
/// blocks the dispatch loop on the merge gate — see the "The Gate Runs an
/// Untrusted Command on the Dispatch Path" invariant. Matched literally rather
/// than with a regex: it is one `format!` call in one place, and a change there
/// should break this parser loudly (a test pins the exact shape) rather than
/// silently stop matching.
const GATING_PREFIX: &str = "gating `";
const GATING_ASSIGNMENT_OF: &str = "assignment of `";

/// `/gate-status`'s answer, built by re-reading the field rather than asking the
/// daemon: the chamber has no live channel into the engine's in-memory state, and
/// the gate's own notice (`GATING_PREFIX`) plus every completion message it can
/// produce (landed / already-landed / conflicted / hand-back) all quote the
/// branch name in backticks — so "still running" is exactly "a gating notice with
/// no later message mentioning that branch".
///
/// `now` and `deadline` are parameters rather than read from the clock/constant
/// here so the function stays pure and testable against synthetic events.
pub fn gate_status_body(
    events: &[hadron_lattice::Event],
    now: chrono::DateTime<chrono::Utc>,
    deadline: std::time::Duration,
) -> String {
    use hadron_lattice::Kind;

    struct Notice {
        ts: chrono::DateTime<chrono::Utc>,
        branch: String,
        quark: String,
    }

    let mut notices = Vec::new();
    for e in events {
        let Kind::Message { body } = &e.kind else { continue };
        let Some(rest) = body.strip_prefix(GATING_PREFIX) else { continue };
        let Some(branch_end) = rest.find('`') else { continue };
        let branch = rest[..branch_end].to_string();
        let Some(q_start) = rest.find(GATING_ASSIGNMENT_OF) else { continue };
        let after = &rest[q_start + GATING_ASSIGNMENT_OF.len()..];
        let Some(q_end) = after.find('`') else { continue };
        notices.push(Notice { ts: e.ts, branch, quark: after[..q_end].to_string() });
    }

    if notices.is_empty() {
        return "No merge gate has run yet this session.".to_string();
    }

    // A notice is "still running" when no message AFTER it names its branch —
    // every terminal outcome (landed, already-landed, conflicted, handed back)
    // quotes the branch, so a mention closes it out regardless of which one fired.
    let active: Vec<&Notice> = notices
        .iter()
        .filter(|n| {
            !events.iter().any(|e| {
                e.ts > n.ts
                    && matches!(&e.kind, Kind::Message { body }
                        if body.contains(&format!("`{}`", n.branch)) && !body.starts_with(GATING_PREFIX))
            })
        })
        .collect();

    if active.is_empty() {
        return "No merge gate is currently running.".to_string();
    }

    let mut out = String::from("**Merge gate status**\n\n");
    for n in active {
        let elapsed = (now - n.ts).to_std().unwrap_or_default();
        if elapsed > deadline {
            out.push_str(&format!(
                "- `{}` (a previous assignment of `{}`) — no outcome recorded — the daemon probably restarted\n",
                n.branch,
                n.quark,
            ));
        } else {
            let remaining = deadline - elapsed;
            out.push_str(&format!(
                "- `{}` (a previous assignment of `{}`) — running {}s, {}s left of {}s\n",
                n.branch,
                n.quark,
                elapsed.as_secs(),
                remaining.as_secs(),
                deadline.as_secs(),
            ));
        }
    }
    out
}

/// Longest line `skills_body` will emit. The chat column is narrow, and the
/// first version of `/skills` printed every trigger of every skill: **4,687
/// characters, a 339-character line, and nine lines over 200** for the real
/// 15-skill corpus — measured, not guessed. A reference that has to be scrolled
/// sideways answers nothing.
const SKILL_LINE_MAX: usize = 110;

/// Clip to `max` CHARACTERS (not bytes) and mark the cut.
///
/// `chars().take()` cannot split a multi-byte character, which is the crash
/// class this whole file is built around — a byte-slice `&s[..max]` here would
/// panic on the first skill description containing an em dash, and most of them
/// contain one.
fn clip(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", kept.trim_end())
}

/// The markdown `/skills` prints: one compact line per skill — its id, the
/// phrase that selects it, and a short gloss.
///
/// Only the **canonical** trigger is shown, not all of them. A skill has up to
/// fourteen triggers and they are alternatives, so listing them all is a wall of
/// synonyms; one phrase answers the question the reader actually has, which is
/// "what do I type to get this".
///
/// Pure, so the line-length property is testable against the real corpus rather
/// than eyeballed — see `the_skills_list_fits_the_chat_column`.
pub fn skills_body(rows: &[(&str, Option<&str>, Option<&str>)]) -> String {
    let mut out = format!("**Skills** — {} loaded\n\n", rows.len());
    for (id, description, trigger) in rows {
        // Budget the gloss with what the id and trigger have already spent, so a
        // long id cannot push the line over on its own.
        let prefix = match trigger {
            Some(t) => format!("- **{id}** — `{t}`"),
            None => format!("- **{id}**"),
        };
        match description {
            Some(d) if !d.is_empty() => {
                let room = SKILL_LINE_MAX.saturating_sub(prefix.chars().count() + 3);
                out.push_str(&format!("{prefix} — {}\n", clip(d, room)));
            }
            _ => out.push_str(&format!("{prefix}\n")),
        }
    }
    out.push_str("\nThe engine picks the skill from your task text — the phrase above selects it.\n");
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

/// The chat message a skill command posts, or `None` when the human gave it no
/// task of their own.
///
/// `trigger` is the skill's own canonical trigger phrase, taken from
/// `hadron_gluon::skills` rather than retyped here: the engine selects a skill by
/// matching that phrase against the task text (`skills::select` is a pure function
/// of the text), so a copy in the chamber would silently stop selecting the skill
/// the day someone edited the trigger list.
///
/// **`None` is the whole point of the signature.** This message is posted as
/// `Actor::Human` — it has to be, because that is the only actor whose `@mentions`
/// wake a seat — so every word in it is attributed to the human on screen and is
/// what the dispatched quark reads as its task. With an empty `task` the old
/// version composed `"@team Let's brainstorm."`, a sentence the human never wrote:
/// it hid the `/team-brainstorm` they typed, showed them prose under their own
/// name, and — because `unaddressed_message_targets` hands each seat its most
/// recent unserved mention — was the entire task three workers were dispatched on
/// while the human's real question, typed next and mentioning nobody, reached only
/// the orchestrator. Returning `None` makes "a skill message carrying no human
/// words" a state the caller cannot post.
pub fn skill_command_body(trigger: &str, target: &str, task: &str) -> Option<String> {
    let task = task.trim();
    if task.is_empty() {
        return None;
    }
    Some(format!("@{target} Let's {trigger}: {task}"))
}

/// What the chamber prints when a skill command was typed with no task.
///
/// Printed rather than swallowed to stderr: the command visibly does nothing
/// otherwise, which is exactly how the empty `/team-brainstorm` went unnoticed.
/// Carries no line beginning with `@`, so it reaches no seat (see the "Printing
/// Without Waking the Swarm" invariant) — the point is that nobody was woken.
pub fn skill_command_needs_a_task(cmd: &str) -> String {
    format!(
        "`/{cmd}` needs the task itself — try `/{cmd} <what you want looked at>`. \
         Nothing was posted and no quark was woken."
    )
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
    use hadron_lattice::{Actor, Event, Kind};

    fn gating_notice(branch: &str, quark: &str, ts: chrono::DateTime<chrono::Utc>) -> Event {
        let mut e = Event::new(
            Actor::Gluon,
            None,
            Kind::Message {
                body: format!(
                    "gating `{branch}` (a previous assignment of `{quark}`) before its next \
                     turn — this can take minutes."
                ),
            },
        );
        e.ts = ts;
        e
    }

    fn landed_notice(branch: &str, ts: chrono::DateTime<chrono::Utc>) -> Event {
        let mut e = Event::new(
            Actor::Gluon,
            None,
            Kind::Message { body: format!("landed `{branch}` onto `main`.") },
        );
        e.ts = ts;
        e
    }

    #[test]
    fn no_notices_reads_as_no_gate_ever_run() {
        assert_eq!(
            gate_status_body(&[], chrono::Utc::now(), std::time::Duration::from_secs(900)),
            "No merge gate has run yet this session."
        );
    }

    #[test]
    fn a_notice_with_no_later_mention_is_still_running() {
        let now = chrono::Utc::now();
        let t0 = now - chrono::Duration::seconds(60);
        let events = vec![gating_notice("quark/acp-claude/01K", "acp-claude", t0)];
        let body = gate_status_body(&events, now, std::time::Duration::from_secs(900));
        assert!(body.contains("quark/acp-claude/01K"), "{body}");
        assert!(body.contains("acp-claude"), "{body}");
        assert!(body.contains("840s left of 900s"), "{body}");
    }

    #[test]
    fn a_later_mention_of_the_branch_closes_the_notice() {
        let now = chrono::Utc::now();
        let t0 = now - chrono::Duration::seconds(60);
        let t1 = now - chrono::Duration::seconds(10);
        let events = vec![
            gating_notice("quark/acp-claude/01K", "acp-claude", t0),
            landed_notice("quark/acp-claude/01K", t1),
        ];
        assert_eq!(
            gate_status_body(&events, now, std::time::Duration::from_secs(900)),
            "No merge gate is currently running."
        );
    }

    #[test]
    fn a_gate_past_its_deadline_is_reported_as_stale_no_outcome_recorded() {
        let now = chrono::Utc::now();
        let t0 = now - chrono::Duration::seconds(1000);
        let events = vec![gating_notice("quark/x/01K", "x", t0)];
        let body = gate_status_body(&events, now, std::time::Duration::from_secs(900));
        assert!(
            body.contains("no outcome recorded — the daemon probably restarted"),
            "{body}"
        );
        assert!(!body.contains("running"), "{body}");
        assert!(!body.contains("1000s"), "{body}");
        assert!(!body.contains("past its deadline"), "{body}");
    }

    /// The invariant this command exists to respect: it prints as `Actor::Gluon`
    /// with `to: None`, so no line may begin with `@` or it would route.
    #[test]
    fn gate_status_never_addresses_a_seat() {
        let now = chrono::Utc::now();
        let t0 = now - chrono::Duration::seconds(60);
        let events = vec![gating_notice("quark/acp-claude/01K", "acp-claude", t0)];
        let body = gate_status_body(&events, now, std::time::Duration::from_secs(900));
        for line in body.lines() {
            assert!(!line.trim_start().starts_with('@'), "would route: {line:?}");
        }
    }

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

    /// The regression this fixes was measured, so the guard is measured too: run
    /// it against the **real** built-in corpus, not a fixture, or it guards a
    /// shape nobody ships. Before the cap: 4,687 chars, longest line 339, nine
    /// lines over 200.
    #[test]
    fn the_skills_list_fits_the_chat_column() {
        let corpus = hadron_gluon::skills::builtins();
        assert!(!corpus.is_empty(), "no built-in skills to render");
        let rows: Vec<(&str, Option<&str>, Option<&str>)> = corpus
            .iter()
            .map(|s| {
                (
                    s.id.as_str(),
                    s.description.as_deref(),
                    s.triggers.first().map(String::as_str),
                )
            })
            .collect();
        let body = skills_body(&rows);

        for line in body.lines() {
            assert!(
                line.chars().count() <= SKILL_LINE_MAX,
                "a {}-char line would need sideways scrolling: {line:?}",
                line.chars().count()
            );
        }
        assert!(
            body.chars().count() < 2_000,
            "/skills is a wall again: {} chars",
            body.chars().count()
        );
        // Every skill still appears — capping must not silently drop rows.
        for s in &corpus {
            assert!(body.contains(&s.id), "{} vanished from /skills", s.id);
        }
        // And it still cannot route (same rule as `/help` — the Gluon channel).
        for line in body.lines() {
            assert!(!line.trim_start().starts_with('@'), "would route: {line:?}");
        }
    }

    /// Clipping happens on character boundaries — a byte slice here would panic
    /// on the first description containing an em dash, which most of them do.
    #[test]
    fn clip_never_splits_a_character() {
        assert_eq!(clip("short", 10), "short");
        let clipped = clip("a — dash and an emoji 😀 beyond the cut", 12);
        assert!(clipped.ends_with('…'), "cut is marked: {clipped:?}");
        assert!(clipped.chars().count() <= 12);
        // The pathological case: cutting exactly where a multi-byte char sits.
        for n in 1..12 {
            let _ = clip("😀😀😀😀😀", n); // must not panic
        }
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

    /// The message is posted as `Actor::Human`, so every word in it is attributed
    /// to the human and is what the dispatched quark reads as its task. A skill
    /// command with no task of its own therefore has no message to post at all —
    /// the old version composed one ("@team Let's brainstorm."), which is how three
    /// workers came to be dispatched on a sentence nobody typed.
    #[test]
    fn a_skill_command_with_no_task_composes_nothing() {
        assert_eq!(skill_command_body("brainstorm", "team", ""), None);
        assert_eq!(skill_command_body("brainstorm", "team", "   \n "), None);
        assert_eq!(skill_command_body("write a plan", "Sonnet", ""), None);
    }

    /// With a task, the body carries the human's own words verbatim after the
    /// trigger the engine matches on.
    #[test]
    fn a_skill_command_carries_the_humans_own_words() {
        assert_eq!(
            skill_command_body("brainstorm", "team", "  the session menu  ").as_deref(),
            Some("@team Let's brainstorm: the session menu"),
        );
    }

    /// The refusal notice prints from `Actor::Gluon`, so — like `/help` — no line
    /// in it may begin with `@`, or declining to wake a seat would wake one.
    #[test]
    fn the_needs_a_task_notice_addresses_nobody() {
        let body = skill_command_needs_a_task("team-brainstorm");
        assert!(body.contains("team-brainstorm"));
        assert!(!body.lines().any(|l| l.trim_start().starts_with('@')));
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

    #[test]
    fn slugify_makes_a_short_kebab_case_id() {
        assert_eq!(
            slugify("Always run cargo fmt before commit"),
            "always-run-cargo-fmt-before"
        );
    }

    /// The index is force-loaded into every prompt of every turn, so a line in it
    /// is a pointer and nothing else. It names the note; the note holds the fact.
    #[test]
    fn an_index_line_points_at_a_note_and_carries_no_content() {
        assert_eq!(
            learn_line("always-run-cargo-fmt-before", "Always run cargo fmt before commit"),
            "- [always-run-cargo-fmt-before](notes/always-run-cargo-fmt-before.md) — \
             Always run cargo fmt before commit\n"
        );
    }

    /// The whole failure this format exists to prevent: `.hadron/nucleus/index.md`
    /// grew to 46 KB against a 32 KB budget, so every quark got a per-section COUNT
    /// instead of any lesson at all. A hook that can grow without bound is that bug
    /// with extra steps, so the cap lives in the writer, not in a reviewer's memory.
    #[test]
    fn a_hook_is_capped_and_never_splits_a_character() {
        let long = "é".repeat(HOOK_MAX_CHARS * 2);
        let capped = hook(&long);
        assert_eq!(capped.chars().count(), HOOK_MAX_CHARS + 1, "cap plus the ellipsis");
        assert!(capped.ends_with('…'));
        // Round-tripping through `str` at all proves no slice landed mid-character.
        assert!(capped.chars().all(|c| c == 'é' || c == '…'));
    }

    #[test]
    fn a_short_hook_is_left_alone_and_flattened_to_one_line() {
        assert_eq!(hook("  two\nlines  "), "two lines");
    }

    /// `description` is a retrieval key: its only job is letting a quark decide
    /// whether to open the file. The fact itself lives in the body, below the
    /// frontmatter, and is never loaded until then.
    #[test]
    fn a_note_carries_frontmatter_then_the_fact() {
        let note = note_body(
            "always-run-cargo-fmt-before",
            "Formatting must precede a commit",
            MemoryType::User,
            "Always run cargo fmt before commit.",
        );
        assert!(note.starts_with("---\nname: always-run-cargo-fmt-before\n"));
        assert!(note.contains("description: Formatting must precede a commit\n"));
        assert!(note.contains("metadata:\n  type: user\n"));
        assert!(note.ends_with("Always run cargo fmt before commit.\n"));
    }

    /// The four types are fixed. A string here would let a fifth appear by typo,
    /// and the type is what tells a quark HOW to use the fact.
    #[test]
    fn every_memory_type_has_exactly_one_spelling() {
        let all = [
            MemoryType::User,
            MemoryType::Feedback,
            MemoryType::Project,
            MemoryType::Reference,
        ];
        let names: Vec<_> = all.iter().map(|t| t.as_str()).collect();
        assert_eq!(names, ["user", "feedback", "project", "reference"]);
    }
}

