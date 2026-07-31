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
    /// Consumes the rest of the *entire message*, newlines included, verbatim —
    /// nothing after it on a later line is scanned for further commands. The only
    /// arity that can carry a multi-line body (e.g. a skill file's front-matter
    /// block, whose indentation must survive).
    Body,
}

/// Where a `/command` argument gets its autocompletions from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgSource {
    /// Takes no argument (or no completion for argument).
    None,
    /// Argument completes from roster quarks (`@name`).
    Quark,
    /// Argument completes from archived sessions (`name` or `id`).
    Session,
    /// Argument completes from project files (`@file`) — `/add-skill`'s
    /// `@path/to/file.md` form.
    File,
}

/// One chat `/command`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The name typed after the slash. The lookup key, and unique.
    pub name: &'static str,
    /// The one-line gloss shown in the completion menu.
    pub detail: &'static str,
    pub arity: Arity,
    pub arg: ArgSource,
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
    Command { name: "help", detail: "List every chat command", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "skills", detail: "List the skills the engine can hand a quark, and their triggers", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "vocabulary", detail: "What each Hadron word means — quark, preon, field, gluon…", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "clear", detail: "Archive and clear the current chat history", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "exit", detail: "Exit Hadron Chamber", arity: Arity::None, arg: ArgSource::None, listed: true },
    // A working alias of `/exit`, kept so existing muscle memory does not break,
    // unlisted so the menu offers one way to do it.
    Command { name: "quit", detail: "Exit Hadron Chamber", arity: Arity::None, arg: ArgSource::None, listed: false },
    Command { name: "toggle-roster", detail: "Toggle the Roster sidebar", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "toggle-inspector", detail: "Toggle the Inspector sidebar", arity: Arity::None, arg: ArgSource::None, listed: true },
    // The skill commands. Each posts a message carrying the skill's own canonical
    // trigger, so the engine selects the procedure — see `skill_command_body`.
    Command { name: "brainstorm", detail: "Explore a design before any code (e.g. /brainstorm @Sonnet the new menu)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "writing-plans", detail: "Turn a settled design into an implementation plan", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "executing-plans", detail: "Work through an existing plan, task by task", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "team-brainstorm", detail: "Kick off brainstorming with the whole team", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "reboot", detail: "Force-restart a resident quark (e.g. /reboot @acp-claude or /reboot all)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "approve", detail: "Approve a pending permission request (e.g. /approve @worker or /approve @worker remember)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "deny", detail: "Deny a pending permission request (e.g. /deny @worker)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "toggle", detail: "Park or unpark a quark — keeps the seat, skips its turns (e.g. /toggle @Sonnet)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "rename", detail: "Name the current session (e.g. /rename bugfix-router)", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "resume", detail: "Reopen an archived session as the live one (e.g. /resume bugfix-router)", arity: Arity::Line, arg: ArgSource::Session, listed: true },
    Command { name: "limit", detail: "Set custom energy token limit for a seat (e.g. /limit @acp-claude 1000000)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "reset-energy", detail: "Reset used token ledger for a seat or all (e.g. /reset-energy @acp-claude or /reset-energy all)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "learn", detail: "Pin a lesson into this repo's nucleus (e.g. /learn always run cargo fmt first)", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "learn-global", detail: "Pin a lesson into your global nucleus, across every repo", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "learn-std-model", detail: "Add a standard law to this repo (appends to laws.md, never edits the Standard Model)", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "learn-std-model-global", detail: "Add a standard law across every repo you run Hadron in", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "gate-status", detail: "Show which branch the merge gate is running, since when, and time left", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "abandon", detail: "Archive-tag then discard a quark's pending branch (e.g. /abandon @acp-claude, then /abandon @acp-claude confirm to force)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "status", detail: "Show status (permission mode, current-field session tokens, quota, branch state) for a seat or global", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "mode", detail: "Set permission mode (ask, write, auto, bypass) for a seat or global default", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "whoami", detail: "Show active orchestrator, workspace root, and field path", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "nucleus", detail: "Show nucleus index size vs resolved budget, lesson count, notes count, and index path", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "health", detail: "Show daemon PID, daemon process state, repo root, and worktree count", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "sessions", detail: "List archived sessions with labels", arity: Arity::None, arg: ArgSource::None, listed: true },
    Command { name: "spend", detail: "Show spend per seat over a window: today (session), week, or all (e.g. /spend @acp-claude week)", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "search", detail: "Search this session's messages for text", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "diff", detail: "Summarize a seat's branch diff against the default branch, or the working tree if no seat is given", arity: Arity::Line, arg: ArgSource::Quark, listed: true },
    Command { name: "export", detail: "Export the current session (or a named archived session) as a Markdown transcript", arity: Arity::Line, arg: ArgSource::None, listed: true },
    Command { name: "add-skill", detail: "Add a custom skill (e.g. /add-skill @path/to/file.md, or /add-skill my-skill then paste the file content)", arity: Arity::Body, arg: ArgSource::File, listed: true },
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

/// Whether appending `new_line` to an index currently `current_len` bytes would push
/// it past `budget_bytes`. Pure, so `/learn`'s write-time refusal (`app/actions.rs`)
/// can be unit-tested without touching disk. The budget is checked when the prompt
/// is READ (`prompt::build`); nothing checked it when a line was APPENDED — this is
/// the write-time half, closing the gap a reader alone can only report, never prevent.
pub(crate) fn would_exceed_index_budget(current_len: usize, new_line: &str, budget_bytes: usize) -> bool {
    current_len + new_line.len() > budget_bytes
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
            Arity::Body => " <name> (then paste the file content on the following lines)",
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

/// Parse `/mode` arguments into `(Mode, Option<seat_name>)`.
pub fn parse_mode_arg(args: &str) -> Option<(hadron_lattice::Mode, Option<&str>)> {
    let parts: Vec<&str> = args.trim().split_whitespace().collect();
    if parts.is_empty() {
        return None;
    }
    let parse_mode = |s: &str| match s.to_lowercase().as_str() {
        "ask" => Some(hadron_lattice::Mode::Ask),
        "write" => Some(hadron_lattice::Mode::Write),
        "auto" => Some(hadron_lattice::Mode::Auto),
        "bypass" => Some(hadron_lattice::Mode::Bypass),
        _ => None,
    };

    if parts.len() == 1 {
        let mode = parse_mode(parts[0])?;
        Some((mode, None))
    } else {
        if let Some(mode) = parse_mode(parts[0]) {
            let seat = parts[1].trim_start_matches('@');
            Some((mode, if seat.is_empty() { None } else { Some(seat) }))
        } else if let Some(mode) = parse_mode(parts[1]) {
            let seat = parts[0].trim_start_matches('@');
            Some((mode, if seat.is_empty() { None } else { Some(seat) }))
        } else {
            None
        }
    }
}

/// Format status output for `/status`.
pub fn status_body(
    roster: &[crate::model::RosterRow],
    global_mode: hadron_lattice::Mode,
    target: Option<&str>,
    repo_root: &std::path::Path,
) -> String {
    let mut out = String::from("**System & Seat Status**\n\n");
    out.push_str(&format!("- **Global Permission Mode**: `{:?}`\n", global_mode));
    out.push_str("- **Token Window**: CURRENT-FIELD window (tokens in active field.jsonl)\n\n");

    let targets: Vec<&crate::model::RosterRow> = if let Some(t) = target {
        let trimmed = t.trim_start_matches('@');
        roster
            .iter()
            .filter(|r| r.id.eq_ignore_ascii_case(trimmed) || r.display_name.as_deref().map_or(false, |d| d.eq_ignore_ascii_case(trimmed)))
            .collect()
    } else {
        roster.iter().filter(|r| r.adopted).collect()
    };

    if targets.is_empty() {
        if let Some(t) = target {
            out.push_str(&format!("No seat on roster matches `{t}`.\n"));
        } else {
            out.push_str("No adopted seats on roster.\n");
        }
    } else {
        for r in targets {
            let mode_str = if r.mode_is_override {
                format!("`{:?}` (override)", r.mode)
            } else {
                format!("`{:?}` (inherited)", r.mode)
            };
            let wt_path = hadron_gluon::worktree::trees_dir(repo_root).join(&r.id);
            let branch_str = if wt_path.exists() {
                let base = hadron_gluon::worktree::default_branch(repo_root);
                match crate::vcs::commits_ahead(&wt_path, &base) {
                    Some(n) => format!("{n} commit(s) ahead of `{base}`"),
                    None => "worktree active".to_string(),
                }
            } else {
                "no worktree".to_string()
            };
            out.push_str(&format!(
                "- **@{}** ({}) — mode: {}, session tokens: {} (CURRENT-FIELD window), branch: {}\n",
                r.id,
                r.display_name.as_deref().unwrap_or(&r.id),
                mode_str,
                r.tokens,
                branch_str,
            ));
        }
    }
    out
}

/// Format `/whoami` output.
pub fn whoami_body(
    roster: &[crate::model::RosterRow],
    repo_root: &std::path::Path,
    field_path: &std::path::Path,
) -> String {
    let orchestrator = roster
        .iter()
        .find(|r| r.flavor == Some(hadron_lattice::Flavor::Orchestrator))
        .map(|r| format!("@{} ({})", r.id, r.display_name.as_deref().unwrap_or(&r.id)))
        .unwrap_or_else(|| "@orchestrator".to_string());

    format!(
        "**Whoami**\n\n\
         - **Orchestrator**: {}\n\
         - **Workspace Root**: `{}`\n\
         - **Field Path**: `{}`\n",
        orchestrator,
        repo_root.display(),
        field_path.display()
    )
}

/// Format `/nucleus` output.
pub fn nucleus_body(workspace_root: &std::path::Path, budget_bytes: usize) -> String {
    let index_path = workspace_root.join(".hadron").join("nucleus").join("index.md");
    let notes_dir = workspace_root.join(".hadron").join("nucleus").join("notes");

    let (index_bytes, lessons_count) = if let Ok(content) = std::fs::read_to_string(&index_path) {
        let bytes = content.len();
        // The ENGINE's predicate, not a local one — `/nucleus` and the over-budget
        // summary must agree on what counts, or the command is a second opinion.
        let lessons = content
            .lines()
            .filter(|l| hadron_gluon::nucleus_status::is_lesson_line(l))
            .count();
        (bytes, lessons)
    } else {
        (0, 0)
    };

    let notes_count = std::fs::read_dir(&notes_dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "md"))
                .count()
        })
        .unwrap_or(0);

    let pct = if budget_bytes > 0 {
        (index_bytes as f64 / budget_bytes as f64) * 100.0
    } else {
        0.0
    };

    format!(
        "**Nucleus Index**\n\n\
         - **Index Path**: `{}`\n\
         - **Size**: {} B / {} B ({:.1}% of resolved budget)\n\
         - **Lessons**: {}\n\
         - **Notes**: {}\n",
        index_path.display(),
        index_bytes,
        budget_bytes,
        pct,
        lessons_count,
        notes_count
    )
}

/// Format `/health` output.
pub fn health_body(
    daemon_pid: Option<u32>,
    pid_alive: bool,
    repo_root: &std::path::Path,
    worktree_count: usize,
) -> String {
    let daemon_str = match daemon_pid {
        Some(pid) if pid_alive => format!("`{pid}` (running, `hadron-gluon`)"),
        Some(pid) => format!("`{pid}` (dead/stale PID)"),
        None => "none (stopped)".to_string(),
    };

    format!(
        "**System Health**\n\n\
         - **Daemon PID**: {}\n\
         - **Repo Root**: `{}`\n\
         - **Worktrees**: {}\n",
        daemon_str,
        repo_root.display(),
        worktree_count
    )
}

/// Format `/sessions` output.
pub fn sessions_body(sessions: &[crate::model::SessionInfo]) -> String {
    if sessions.is_empty() {
        return "No archived sessions found.".to_string();
    }
    let mut out = format!("**Archived Sessions ({})**\n\n", sessions.len());
    for s in sessions {
        if let Some(name) = &s.name {
            out.push_str(&format!("- `{}` — *{}*\n", s.id, name));
        } else {
            out.push_str(&format!("- `{}`\n", s.id));
        }
    }
    out
}

/// Parse `/spend` arguments into `(seat, window)`. Any token matching a window
/// keyword sets the window; any other token (with or without a leading `@`) names
/// the seat. `today` is accepted but is NOT a distinct calendar-day cutoff — no such
/// bucket exists in [`crate::model::StatsWindow`], and adding one would touch the
/// Stats tab's UI cycle (`StatsWindow::ALL`), out of scope for this command — so
/// `today` and `session` both resolve to [`crate::model::StatsWindow::Session`], the
/// live field since the last `/clear`. Absent a window keyword, defaults to `Session`.
pub fn parse_spend_arg(args: &str) -> (Option<&str>, crate::model::StatsWindow) {
    let mut seat = None;
    let mut window = crate::model::StatsWindow::Session;
    for tok in args.trim().split_whitespace() {
        match tok.to_lowercase().as_str() {
            "today" | "session" => window = crate::model::StatsWindow::Session,
            "week" => window = crate::model::StatsWindow::Week,
            "all" | "alltime" => window = crate::model::StatsWindow::AllTime,
            _ => {
                let s = tok.trim_start_matches('@');
                if !s.is_empty() {
                    seat = Some(s);
                }
            }
        }
    }
    (seat, window)
}

/// Format `/spend` output. `target` narrows to one seat; `None` shows every seat with
/// spend plus the team total. The window is the CURRENT-FIELD-or-wider window
/// [`SessionStats`](crate::model::SessionStats) was folded over — never the ledger's
/// all-time cumulative, which the chamber does not read
/// (`roster-tokens-and-depletion-are-different-windows`).
pub fn spend_body(stats: &crate::model::SessionStats, window_label: &str, target: Option<&str>) -> String {
    let mut out = format!("**Spend — {window_label} window**\n\n");
    let rows: Vec<&(String, crate::model::QuarkStats)> = match target {
        Some(t) => stats.per_quark.iter().filter(|(id, _)| id.eq_ignore_ascii_case(t)).collect(),
        None => stats.per_quark.iter().collect(),
    };
    if rows.is_empty() {
        out.push_str(&format!("No seat matches `{}`.\n", target.unwrap_or("")));
        return out;
    }
    for (id, qs) in &rows {
        let cost = qs.cost_usd.map(|c| format!("${c:.4}")).unwrap_or_else(|| "n/a".to_string());
        out.push_str(&format!(
            "- **@{id}** — {} turn(s), {} fresh token(s), cost {cost}\n",
            qs.turns, qs.fresh
        ));
    }
    if target.is_none() {
        out.push_str(&format!(
            "\n**Team total** — {} turn(s), {} fresh token(s)\n",
            stats.total_turns, stats.total_fresh
        ));
    }
    out
}

/// Format `/search` output: every message whose body contains `query`
/// (case-insensitive), newest constraint applied by the caller (this just filters).
pub fn search_body(messages: &[crate::model::MessageRow], query: &str) -> String {
    let q = query.to_lowercase();
    let hits: Vec<&crate::model::MessageRow> =
        messages.iter().filter(|m| m.body.to_lowercase().contains(&q)).collect();
    if hits.is_empty() {
        return format!("No matches for `{query}`.\n");
    }
    let mut out = format!("**Search: `{query}`** — {} match(es)\n\n", hits.len());
    for m in hits {
        let to = m.to.as_deref().unwrap_or("(broadcast)");
        let snippet: String = m.body.chars().take(160).collect();
        out.push_str(&format!(
            "- **{} → {}** ({}): {snippet}\n",
            m.from,
            to,
            m.ts.format("%Y-%m-%d %H:%M")
        ));
    }
    out
}

/// Format `/diff` output as a per-file summary — never the raw unified diff, which
/// would paste an unbounded blob into the field that every quark re-reads on every
/// later turn.
pub fn diff_body(label: &str, diffs: Option<&[crate::vcs::FileDiff]>) -> String {
    match diffs {
        None => format!("**Diff — {label}**\n\nNo diff available (git call failed, or nothing to diff against).\n"),
        Some(files) if files.is_empty() => format!("**Diff — {label}**\n\nNo changes.\n"),
        Some(files) => {
            let (added, removed) =
                files.iter().fold((0usize, 0usize), |(a, r), f| (a + f.added, r + f.removed));
            let mut out =
                format!("**Diff — {label}** ({} file(s), +{added} \u{2212}{removed})\n\n", files.len());
            for f in files {
                out.push_str(&format!("- `{}` (+{} \u{2212}{})\n", f.path, f.added, f.removed));
            }
            out
        }
    }
}

/// Render a session's chat messages as a standalone Markdown transcript, for `/export`.
/// Only chat rows ([`MessageRow::is_chat`](crate::model::MessageRow::is_chat)) — the
/// Log tab's internal events (status, energy reports) are not part of a human-readable
/// transcript.
pub fn render_session_markdown(messages: &[crate::model::MessageRow]) -> String {
    let mut out = String::new();
    for m in messages.iter().filter(|m| m.is_chat()) {
        let to = m.to.as_deref().unwrap_or("(broadcast)");
        out.push_str(&format!(
            "## {} — {} \u{2192} {}\n\n{}\n\n",
            m.ts.format("%Y-%m-%d %H:%M:%S"),
            m.from,
            to,
            m.body
        ));
    }
    out
}

/// Format `/export`'s confirmation.
pub fn export_body(dest: &std::path::Path, count: usize) -> String {
    format!("**Exported** {count} message(s) to `{}`\n", dest.display())
}

/// What `/add-skill`'s captured `Arity::Body` argument turned out to name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddSkillSource {
    /// `/add-skill @path/to/file.md` — copy an existing file's content.
    Path(String),
    /// `/add-skill <name>` followed by the file content on later lines.
    Inline { name: String, content: String },
}

/// Parse `/add-skill`'s argument (spec §10): the first line is either an
/// `@`-prefixed path or a bare skill name, and everything after the first
/// newline — if anything — is the inline content verbatim, untouched by this
/// function. `None` when there is nothing to act on (an empty first line).
pub fn parse_add_skill_args(args: &str) -> Option<AddSkillSource> {
    let (first, rest) = args.split_once('\n').unwrap_or((args, ""));
    let first = first.trim();
    if first.is_empty() {
        return None;
    }
    match first.strip_prefix('@') {
        Some(path) => Some(AddSkillSource::Path(path.to_string())),
        None => Some(AddSkillSource::Inline { name: first.to_string(), content: rest.to_string() }),
    }
}

/// Whether `s` could escape a directory it's joined into, or a filename it's
/// interpolated into — a path separator, or a literal `..` anywhere in it.
/// Shared by every place that turns a chat-typed argument into a filesystem
/// name (`/add-skill`'s name, `/export`'s session argument): one definition,
/// not two similar checks that started identical and drifted (rule 3).
fn contains_path_escape(s: &str) -> bool {
    s.contains(['/', '\\']) || s.contains("..")
}

/// Turn a `/add-skill <name>` argument into a safe `<name>.md` filename —
/// never a path. `load_skills` keys a skill by its front-matter `name:`, not
/// its filename, so this only needs to be a filesystem-safe label; it must
/// never let the human's typed name escape `.hadron/skills/` (e.g. `../..`).
pub fn add_skill_filename(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name == "." || contains_path_escape(name) {
        return None;
    }
    Some(format!("{name}.md"))
}

/// Format `/add-skill`'s confirmation, including the `tools:` warning when the
/// written content declared one (spec §10: `ResolvedSkill.tools` is parsed but
/// **not enforced anywhere** — see `hadron_gluon::skills::is_tool_allowed`'s own
/// doc comment — so accepting the field silently would ship a security-shaped
/// lie about what the skill restricts).
pub fn add_skill_written_body(dest: &std::path::Path, has_tools_field: bool, has_name_field: bool) -> String {
    let mut out = format!("**Wrote skill** to `{}`\n", dest.display());
    if !has_name_field {
        out.push_str(
            "\n⚠️ No `name:` front-matter found — the skill loader requires one and will \
             skip this file (silently, to its own stderr) until you add it.\n",
        );
    }
    if has_tools_field {
        out.push_str(
            "\n⚠️ This skill declares a `tools:` front-matter line. That field is parsed \
             but **not enforced anywhere** — every quark can still use every tool while \
             this skill is active. Do not rely on it to restrict tool access.\n",
        );
    }
    out
}

/// Validate whether a session argument for `/export` is safe — rejects any
/// input that could escape `.hadron/exports/` (the write path) or
/// `.hadron/sessions/` (the read path). An empty argument is safe: it means
/// "the current session", not a path component.
pub fn is_safe_session_arg(arg: &str) -> bool {
    let s = arg.trim();
    s.is_empty() || !contains_path_escape(s)
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

/// Body for a Gluon chat notice when a human types at a busy CLI seat mid-turn.
///
/// **The seat is named WITHOUT an `@` sigil, deliberately.** This is posted as
/// `Actor::Gluon` with `to: None`, and per the "Printing Without Waking the Swarm"
/// invariant that only avoids costing a turn while no LINE of the body begins with
/// `@` — `unaddressed_message_targets` resolves addressees out of the body. An
/// opening `@agy …` would dispatch a turn to the very seat this notice exists to
/// say is busy, queued behind the human's own message, so the seat runs twice.
pub fn uninterruptible_cli_notice(target: &str) -> String {
    format!(
        "{target} is a CLI seat currently mid-turn and cannot be interrupted — your message will be picked up when its turn completes."
    )
}

/// Extract target handles from all line-start `@mentions` in `text`.
///
/// Ignores lines inside fenced code blocks (```) and bold `**@` mentions, matching the router's rule.
pub fn line_start_mentions(text: &str) -> Vec<&str> {
    let mut targets = Vec::new();
    let mut in_fence = false;
    let fenced = text
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        % 2
        == 0;

    for line in text.lines() {
        let trimmed = line.trim_start();
        if fenced && trimmed.starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('@') {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (target, _) = rest.split_at(end);
            if !target.is_empty() {
                targets.push(target);
            }
        }
    }
    targets
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

/// The trigger character or command argument context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionTrigger {
    Mention,
    Emoji,
    Command,
    Arg(ArgSource),
}

/// Find the `@`/`:`/`/` completion trigger immediately before the cursor or a
/// `/command` argument context.
///
/// Returns the trigger type, the query typed after it, and its byte index.
pub fn extract_completion_query(text: &str, offset: usize) -> Option<(CompletionTrigger, String, usize)> {
    let mut safe_offset = offset.min(text.len());
    while safe_offset > 0 && !text.is_char_boundary(safe_offset) {
        safe_offset -= 1;
    }

    let before_cursor = &text[..safe_offset];
    let line_start = before_cursor.rfind('\n').map_or(0, |i| i + 1);
    let current_line = &before_cursor[line_start..];

    // Check if the current line starts with a command that takes arguments (e.g. `/resume bug`)
    if current_line.starts_with('/') {
        let after_slash = &current_line[1..];
        if let Some(space_pos) = after_slash.find(char::is_whitespace) {
            let cmd_name = &after_slash[..space_pos];
            if let Some(cmd) = command(cmd_name) {
                if cmd.arg != ArgSource::None {
                    let arg_part = &after_slash[space_pos..];
                    let arg_trimmed = arg_part.trim_start();
                    let leading_spaces = arg_part.len() - arg_trimmed.len();
                    let arg_start = line_start + 1 + space_pos + leading_spaces;
                    let query = arg_trimmed.to_string();
                    return Some((CompletionTrigger::Arg(cmd.arg), query, arg_start));
                }
            }
        }
    }

    for (idx, c) in before_cursor.char_indices().rev() {
        let is_slash_cmd = c == '/'
            && (idx == 0
                || before_cursor[..idx]
                    .chars()
                    .next_back()
                    .map_or(false, |ch| ch.is_whitespace() || ch == '\n'));
        if c == '@' {
            let query = before_cursor[idx + c.len_utf8()..].to_string();
            return Some((CompletionTrigger::Mention, query, idx));
        } else if c == ':' {
            let query = before_cursor[idx + c.len_utf8()..].to_string();
            return Some((CompletionTrigger::Emoji, query, idx));
        } else if is_slash_cmd {
            let query = before_cursor[idx + c.len_utf8()..].to_string();
            return Some((CompletionTrigger::Command, query, idx));
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
    sessions: &[crate::model::SessionInfo],
) -> Option<Completions> {
    let (trigger, query, start) = extract_completion_query(text, cursor)?;
    let query_lower = query.to_lowercase();
    let mut out: Vec<Candidate> = Vec::new();

    match trigger {
        CompletionTrigger::Mention => {
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
        CompletionTrigger::Emoji => {
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
        CompletionTrigger::Command => {
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
        CompletionTrigger::Arg(ArgSource::Session) => {
            for session in sessions {
                if out.len() >= MAX_CANDIDATES {
                    break;
                }
                let label = session.label();
                let name_or_id = session.name.as_deref().unwrap_or(&session.id);
                let label_lower = label.to_lowercase();
                let id_lower = session.id.to_lowercase();
                if query_lower.is_empty()
                    || label_lower.contains(&query_lower)
                    || id_lower.contains(&query_lower)
                {
                    out.push(Candidate {
                        label,
                        detail: "Session".into(),
                        new_text: format!("{name_or_id} "),
                    });
                }
            }
        }
        CompletionTrigger::Arg(ArgSource::Quark) => {
            let q_query = query_lower.strip_prefix('@').unwrap_or(&query_lower);
            for (id, display) in quarks {
                if out.len() >= MAX_CANDIDATES {
                    break;
                }
                let name = display.as_ref().unwrap_or(id);
                let name_l = name.to_lowercase();
                let id_l = id.to_lowercase();
                if q_query.is_empty()
                    || name_l.contains(q_query)
                    || id_l.contains(q_query)
                {
                    out.push(Candidate {
                        label: format!("@{name}"),
                        detail: "Quark".into(),
                        new_text: format!("@{name} "),
                    });
                }
            }
        }
        CompletionTrigger::Arg(ArgSource::File) => {
            let f_query = query_lower.strip_prefix('@').unwrap_or(&query_lower);
            for file in files {
                if out.len() >= MAX_CANDIDATES {
                    break;
                }
                if f_query.is_empty() || file.to_lowercase().contains(f_query) {
                    out.push(Candidate {
                        label: format!("@{file}"),
                        detail: "File".into(),
                        new_text: format!("@{file} "),
                    });
                }
            }
        }
        CompletionTrigger::Arg(ArgSource::None) => {}
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
        let c = completion_candidates("@ag", 3, &quarks(), &[], &[]).expect("has rows");
        assert_eq!(c.start, 0, "replace span starts at the '@'");
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"@Agy"), "matched quark offered: {labels:?}");
        // The accepted text carries the sigil and a trailing space, ready to type on.
        let agy = c.candidates.iter().find(|c| c.label == "@Agy").unwrap();
        assert_eq!(agy.new_text, "@Agy ");
    }

    #[test]
    fn an_empty_mention_query_offers_the_routing_aliases_first() {
        let c = completion_candidates("@", 1, &quarks(), &[], &[]).expect("has rows");
        assert_eq!(
            c.candidates[0].label,
            format!("@{}", hadron_gluon::router::ORCHESTRATOR_ALIAS),
            "aliases lead the list"
        );
    }

    #[test]
    fn a_file_query_offers_files() {
        let files = vec!["src/app.rs".to_string(), "README.md".to_string()];
        let c = completion_candidates("@app", 4, &[], &files, &[]).expect("has rows");
        assert_eq!(c.candidates.len(), 1);
        assert_eq!(c.candidates[0].new_text, "@src/app.rs ");
    }

    #[test]
    fn a_bare_emoji_trigger_is_capped_not_thousands() {
        let c = completion_candidates(":", 1, &[], &[], &[]).expect("has rows");
        assert!(
            c.candidates.len() <= MAX_CANDIDATES,
            "a bare ':' must not build thousands of rows: got {}",
            c.candidates.len()
        );
    }

    #[test]
    fn an_emoji_query_accepts_the_glyph_not_the_shortcode() {
        let c = completion_candidates(":rofl", 5, &[], &[], &[]).expect("has rows");
        let first = &c.candidates[0];
        assert!(first.label.starts_with(":rofl"));
        // Accepting inserts the glyph itself, not the `:rofl:` text.
        assert!(!first.new_text.starts_with(':'));
    }

    #[test]
    fn an_emoji_query_searches_the_entire_crate() {
        // "globe_with_meridians" is a real but less common emoji in the emojis crate.
        let c = completion_candidates(":globe_with_meridians", 21, &[], &[], &[]).expect("has rows");
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.iter().any(|l| l.starts_with(":globe_with_meridians")), "should search the whole crate: {labels:?}");
    }

    #[test]
    fn a_bare_emoji_query_returns_curated_modern_emojis() {
        let c = completion_candidates(":", 1, &[], &[], &[]).expect("has rows");
        // Verify we got the curated 50 emojis (or at least capped to MAX_CANDIDATES).
        assert_eq!(c.candidates.len(), 50);
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels[0].starts_with(":rofl"), "first emoji should be rofl: {:?}", labels[0]);
    }

    #[test]
    fn a_resume_query_offers_archived_sessions() {
        let sessions = vec![
            crate::model::SessionInfo {
                id: "20260725_120000".into(),
                name: Some("bugfix-router".into()),
            },
            crate::model::SessionInfo {
                id: "20260724_093000".into(),
                name: None,
            },
        ];
        let c = completion_candidates("/resume ", 8, &[], &[], &sessions).expect("has rows");
        assert_eq!(c.start, 8);
        assert_eq!(c.candidates.len(), 2);
        assert_eq!(c.candidates[0].label, "bugfix-router");
        assert_eq!(c.candidates[0].new_text, "bugfix-router ");
        assert_eq!(c.candidates[1].label, "2026-07-24 09:30");
        assert_eq!(c.candidates[1].new_text, "20260724_093000 ");

        let c2 = completion_candidates("/resume bug", 11, &[], &[], &sessions).expect("has rows");
        assert_eq!(c2.start, 8);
        assert_eq!(c2.candidates.len(), 1);
        assert_eq!(c2.candidates[0].label, "bugfix-router");
        assert_eq!(c2.candidates[0].new_text, "bugfix-router ");
    }

    #[test]
    fn a_reboot_query_offers_quarks() {
        let c = completion_candidates("/reboot ", 8, &quarks(), &[], &[]).expect("has rows");
        assert_eq!(c.start, 8);
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"@Agy"));
        assert!(labels.contains(&"@acp-claude"));
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

    /// **This notice must not wake the seat it is about.** It is posted as
    /// `Actor::Gluon` with `to: None`, and the "Printing Without Waking the Swarm"
    /// invariant is precise about what that buys: nothing, if any LINE of the body
    /// begins with `@`. `unaddressed_message_targets` resolves addressees found in
    /// the body, so a notice opening `@agy …` dispatches a turn to the very seat it
    /// has just told the human is busy — on top of the human's own message, so the
    /// seat is queued twice.
    ///
    /// The seat is still named, because a notice that does not say WHICH seat is
    /// useless; it just may not be named at the start of a line with a sigil.
    #[test]
    fn uninterruptible_cli_notice_names_the_seat_without_waking_it() {
        let msg = uninterruptible_cli_notice("agy");
        assert!(msg.contains("agy"), "the notice must say which seat it is about");
        assert!(msg.contains("cannot be interrupted"));
        assert!(
            !msg.lines().any(|l| l.trim_start().starts_with('@')),
            "a Gluon notice whose line starts with @ dispatches a turn to that seat: {msg:?}"
        );
    }

    #[test]
    fn line_start_mentions_filters_fences_bold_and_mid_sentence() {
        assert_eq!(line_start_mentions("@agy fix the router"), vec!["agy"]);
        assert_eq!(line_start_mentions("  @Sonnet  do this"), vec!["Sonnet"]);
        assert_eq!(line_start_mentions("contact @agy for info"), Vec::<&str>::new());
        assert_eq!(line_start_mentions("**@agy** fix the router"), Vec::<&str>::new());
        assert_eq!(
            line_start_mentions("```\n@agy fix the router\n```"),
            Vec::<&str>::new()
        );
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
        let c = completion_candidates("/tog", 4, &[], &[], &[]).expect("has rows");
        let labels: Vec<&str> = c.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"/toggle-roster"), "matched toggle-roster offered: {labels:?}");
        // Was `/goa` — `/goal` was one of six rows the menu offered with no handler,
        // so choosing it posted the line as chat. Retired; `/brainstorm` is a real one.
        assert!(completion_candidates("/brain", 6, &[], &[], &[]).is_some());

        let c_reboot = completion_candidates("/reb", 4, &[], &[], &[]).expect("has rows");
        let labels_reboot: Vec<&str> = c_reboot.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels_reboot.contains(&"/reboot"), "matched reboot offered: {labels_reboot:?}");

        let c_approve = completion_candidates("/app", 4, &[], &[], &[]).expect("has rows");
        let labels_approve: Vec<&str> = c_approve.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels_approve.contains(&"/approve"), "matched approve offered: {labels_approve:?}");

        let c_deny = completion_candidates("/den", 4, &[], &[], &[]).expect("has rows");
        let labels_deny: Vec<&str> = c_deny.candidates.iter().map(|c| c.label.as_str()).collect();
        assert!(labels_deny.contains(&"/deny"), "matched deny offered: {labels_deny:?}");
        
        // Mid-line `/` at a word boundary IS a trigger.
        assert!(completion_candidates("hi /brain", 9, &[], &[], &[]).is_some());
        // Path slashes are not triggers.
        assert!(completion_candidates("src/app.rs", 10, &[], &[], &[]).is_none());
    }


    #[test]
    fn no_trigger_yields_no_card() {
        assert!(completion_candidates("just talking", 12, &quarks(), &[], &[]).is_none());
    }

    #[test]
    fn a_cursor_past_the_end_or_mid_emoji_does_not_panic() {
        let hostile = "hi 🌍 @a";
        for cursor in 0..=hostile.len() + 4 {
            let _ = completion_candidates(hostile, cursor, &quarks(), &[], &[]);
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
            Some((CompletionTrigger::Mention, "op".to_string(), 4))
        );
        assert_eq!(
            extract_completion_query("nice :smi", 9),
            Some((CompletionTrigger::Emoji, "smi".to_string(), 5))
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
            Some((CompletionTrigger::Mention, "world".to_string(), 11))
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
        assert_eq!(trigger, CompletionTrigger::Mention);
        assert_eq!(idx, 0);
        assert_eq!(query, "ab", "the partial emoji is dropped, not sliced");
    }

    #[test]
    fn a_slash_command_is_offered_at_word_boundaries_mid_text() {
        let text = "hello /plan";
        assert_eq!(
            extract_completion_query(text, text.len()),
            Some((CompletionTrigger::Command, "plan".to_string(), 6))
        );

        let multi_line = "first line\n/reboot";
        assert_eq!(
            extract_completion_query(multi_line, multi_line.len()),
            Some((CompletionTrigger::Command, "reboot".to_string(), 11))
        );

        // Path slashes must NOT trigger completions.
        let path = "src/app.rs";
        assert_eq!(extract_completion_query(path, path.len()), None);
    }

    #[test]
    fn exit_command_completion_candidates() {
        let completions = completion_candidates("/ex", 3, &[], &[], &[]).unwrap();
        assert!(completions.candidates.iter().any(|c| c.label == "/exit"));
    }

    #[test]
    fn rename_and_resume_command_completion_candidates() {
        let completions = completion_candidates("/ren", 4, &[], &[], &[]).unwrap();
        assert!(completions.candidates.iter().any(|c| c.label == "/rename"));

        let completions = completion_candidates("/res", 4, &[], &[], &[]).unwrap();
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

    // -- would_exceed_index_budget: /learn's write-time refusal --

    #[test]
    fn would_exceed_index_budget_is_false_comfortably_under() {
        assert!(!would_exceed_index_budget(100, "- [x](notes/x.md) — short\n", 1000));
    }

    #[test]
    fn would_exceed_index_budget_is_true_when_the_append_crosses_the_line() {
        assert!(would_exceed_index_budget(995, "123456\n", 1000));
    }

    #[test]
    fn would_exceed_index_budget_is_false_at_exactly_the_budget() {
        // Landing exactly on the budget is not OVER it — `>` not `>=`, matching
        // `index_over_budget`'s own `len() > budget_bytes` check.
        let line = "1234567890\n"; // 11 bytes
        assert_eq!(line.len(), 11);
        assert!(!would_exceed_index_budget(989, line, 1000));
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

    #[test]
    fn parse_mode_arg_parses_global_and_seat_modes() {
        assert_eq!(parse_mode_arg("write"), Some((hadron_lattice::Mode::Write, None)));
        assert_eq!(
            parse_mode_arg("bypass @acp-claude"),
            Some((hadron_lattice::Mode::Bypass, Some("acp-claude")))
        );
        assert_eq!(
            parse_mode_arg("@Sonnet auto"),
            Some((hadron_lattice::Mode::Auto, Some("Sonnet")))
        );
        assert_eq!(parse_mode_arg("invalid"), None);
        assert_eq!(parse_mode_arg(""), None);
    }

    #[test]
    fn status_body_labels_current_field_window() {
        let body = status_body(&[], hadron_lattice::Mode::Ask, None, std::path::Path::new("/tmp"));
        assert!(body.contains("CURRENT-FIELD window"));
        assert!(body.contains("Global Permission Mode"));
    }

    #[test]
    fn whoami_body_formats_paths_and_orchestrator() {
        let body = whoami_body(
            &[],
            std::path::Path::new("/workspace"),
            std::path::Path::new("/workspace/.hadron/field.jsonl"),
        );
        assert!(body.contains("Workspace Root"));
        assert!(body.contains("/workspace"));
        assert!(body.contains("/workspace/.hadron/field.jsonl"));
    }

    #[test]
    fn nucleus_body_uses_resolved_budget() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus_dir = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus_dir).unwrap();
        std::fs::write(nucleus_dir.join("index.md"), "- [test](notes/test.md) — hook\n").unwrap();
        let body = nucleus_body(dir.path(), 64 * 1024);
        assert!(body.contains("65536 B"));
        assert!(body.contains("Lessons"));
    }

    /// `/nucleus` must count lessons the way the ENGINE counts them, or the number it
    /// prints is a second opinion on "what is a lesson line". The engine's predicate
    /// (`nucleus_status::is_lesson_line`) accepts only the two pointer shapes — `- [` and
    /// `- **`. A plain `- ` bullet is prose: the index preamble has several, and counting
    /// them told the reader there were more lessons than the prompt would ever deliver.
    #[test]
    fn nucleus_body_counts_only_what_the_engine_calls_a_lesson() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus_dir = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(&nucleus_dir).unwrap();
        std::fs::write(
            nucleus_dir.join("index.md"),
            "# Memory index\n\n\
             - The index lives at `.hadron/nucleus/index.md` — prose, not a lesson.\n\
             - Notes live in `notes/` — also prose.\n\n\
             ## A section\n\n\
             - [one](notes/one.md) — hook\n\
             - **two** — the older shape, still counted\n",
        )
        .unwrap();
        let body = nucleus_body(dir.path(), 32 * 1024);
        assert!(
            body.contains("**Lessons**: 2"),
            "two pointer lines and two prose bullets must count as 2, got:\n{body}"
        );
    }

    #[test]
    fn health_body_formats_running_or_stopped_daemon() {
        let body_live = health_body(Some(1234), true, std::path::Path::new("/repo"), 2);
        assert!(body_live.contains("`1234` (running, `hadron-gluon`)"));
        assert!(body_live.contains("Worktrees**: 2"));

        let body_dead = health_body(None, false, std::path::Path::new("/repo"), 0);
        assert!(body_dead.contains("none (stopped)"));
    }

    #[test]
    fn sessions_body_formats_session_list() {
        let sessions = vec![
            crate::model::SessionInfo { id: "20260726_010000".into(), name: Some("test-session".into()) },
            crate::model::SessionInfo { id: "20260726_020000".into(), name: None },
        ];
        let body = sessions_body(&sessions);
        assert!(body.contains("20260726_010000"));
        assert!(body.contains("test-session"));
        assert!(body.contains("20260726_020000"));
    }

    fn msg(from: &str, to: Option<&str>, body: &str, kind_label: &'static str) -> crate::model::MessageRow {
        crate::model::MessageRow {
            from: from.to_string(),
            to: to.map(str::to_string),
            body: body.to_string(),
            kind_label,
            usage: None,
            ts: chrono::Utc::now(),
            legacy_used_tokens: None,
            turn: None,
            severity: None,
        }
    }

    // -- parse_spend_arg --

    #[test]
    fn parse_spend_arg_defaults_to_session_window_with_no_seat() {
        assert_eq!(parse_spend_arg(""), (None, crate::model::StatsWindow::Session));
    }

    #[test]
    fn parse_spend_arg_today_and_session_both_resolve_to_session_window() {
        assert_eq!(parse_spend_arg("today").1, crate::model::StatsWindow::Session);
        assert_eq!(parse_spend_arg("session").1, crate::model::StatsWindow::Session);
    }

    #[test]
    fn parse_spend_arg_parses_week_and_all_windows() {
        assert_eq!(parse_spend_arg("week").1, crate::model::StatsWindow::Week);
        assert_eq!(parse_spend_arg("all").1, crate::model::StatsWindow::AllTime);
    }

    #[test]
    fn parse_spend_arg_parses_a_seat_alongside_a_window() {
        assert_eq!(parse_spend_arg("@Sonnet week"), (Some("Sonnet"), crate::model::StatsWindow::Week));
        assert_eq!(parse_spend_arg("week @Sonnet"), (Some("Sonnet"), crate::model::StatsWindow::Week));
    }

    // -- spend_body --

    #[test]
    fn spend_body_labels_the_window_and_lists_per_seat_spend() {
        let stats = crate::model::SessionStats {
            per_quark: vec![(
                "acp-claude".to_string(),
                crate::model::QuarkStats { turns: 3, fresh: 1500, cost_usd: Some(0.02), ..Default::default() },
            )],
            total_turns: 3,
            total_fresh: 1500,
            ..Default::default()
        };
        let body = spend_body(&stats, "Week", None);
        assert!(body.contains("Week window"));
        assert!(body.contains("@acp-claude"));
        assert!(body.contains("1500 fresh token"));
        assert!(body.contains("Team total"));
    }

    #[test]
    fn spend_body_narrows_to_one_seat_and_omits_the_team_total() {
        let stats = crate::model::SessionStats {
            per_quark: vec![
                ("a".to_string(), crate::model::QuarkStats { turns: 1, fresh: 10, ..Default::default() }),
                ("b".to_string(), crate::model::QuarkStats { turns: 2, fresh: 20, ..Default::default() }),
            ],
            ..Default::default()
        };
        let body = spend_body(&stats, "All time", Some("a"));
        assert!(body.contains("@a"));
        assert!(!body.contains("@b"));
        assert!(!body.contains("Team total"));
    }

    #[test]
    fn spend_body_reports_no_match_for_an_unknown_seat() {
        let stats = crate::model::SessionStats::default();
        let body = spend_body(&stats, "Session", Some("nobody"));
        assert!(body.contains("No seat matches"));
    }

    // -- search_body --

    #[test]
    fn search_body_finds_case_insensitive_matches() {
        let messages = vec![
            msg("acp-claude", Some("acp-agy"), "fix the merge gate", "message"),
            msg("acp-agy", Some("acp-claude"), "unrelated reply", "message"),
        ];
        let body = search_body(&messages, "MERGE GATE");
        assert!(body.contains("1 match"));
        assert!(body.contains("fix the merge gate"));
        assert!(!body.contains("unrelated reply"));
    }

    #[test]
    fn search_body_reports_no_matches() {
        let body = search_body(&[], "anything");
        assert!(body.contains("No matches"));
    }

    // -- diff_body --

    #[test]
    fn diff_body_summarises_files_never_the_raw_hunks() {
        let files = vec![
            crate::vcs::FileDiff { path: "src/a.rs".into(), added: 5, removed: 2, hunks: vec![] },
            crate::vcs::FileDiff { path: "src/b.rs".into(), added: 1, removed: 0, hunks: vec![] },
        ];
        let body = diff_body("acp-claude", Some(&files));
        assert!(body.contains("2 file(s)"));
        assert!(body.contains("src/a.rs"));
        assert!(body.contains("src/b.rs"));
    }

    #[test]
    fn diff_body_reports_no_changes_for_an_empty_diff() {
        let body = diff_body("acp-claude", Some(&[]));
        assert!(body.contains("No changes"));
    }

    #[test]
    fn diff_body_reports_unavailable_when_the_git_call_failed() {
        let body = diff_body("acp-claude", None);
        assert!(body.contains("No diff available"));
    }

    // -- render_session_markdown / export_body --

    #[test]
    fn render_session_markdown_includes_only_chat_rows() {
        let messages = vec![
            msg("human", Some("acp-claude"), "do the thing", "message"),
            msg("acp-claude", None, "status update", "status"),
        ];
        let md = render_session_markdown(&messages);
        assert!(md.contains("do the thing"));
        assert!(!md.contains("status update"));
    }

    #[test]
    fn export_body_names_the_destination_and_count() {
        let body = export_body(std::path::Path::new("/tmp/x.md"), 7);
        assert!(body.contains("7 message"));
        assert!(body.contains("/tmp/x.md"));
    }

    #[test]
    fn add_skill_args_reads_an_at_prefixed_path() {
        assert_eq!(
            parse_add_skill_args("@path/to/file.md"),
            Some(AddSkillSource::Path("path/to/file.md".to_string()))
        );
    }

    #[test]
    fn add_skill_args_reads_a_name_and_inline_body_verbatim() {
        let args = "my-skill\n---\nname: my-skill\n---\n  indented step\nDo the thing.";
        let parsed = parse_add_skill_args(args);
        assert_eq!(
            parsed,
            Some(AddSkillSource::Inline {
                name: "my-skill".to_string(),
                // The interior is untouched: indentation on `  indented step`
                // must survive, or a front-matter block written this way is
                // a broken skill file.
                content: "---\nname: my-skill\n---\n  indented step\nDo the thing.".to_string(),
            })
        );
    }

    #[test]
    fn add_skill_args_with_no_second_line_has_empty_content() {
        assert_eq!(
            parse_add_skill_args("my-skill"),
            Some(AddSkillSource::Inline { name: "my-skill".to_string(), content: String::new() })
        );
    }

    #[test]
    fn add_skill_args_rejects_an_empty_first_line() {
        assert_eq!(parse_add_skill_args(""), None);
        assert_eq!(parse_add_skill_args("   \nbody"), None);
    }

    #[test]
    fn add_skill_filename_accepts_a_plain_name() {
        assert_eq!(add_skill_filename("my-skill"), Some("my-skill.md".to_string()));
    }

    #[test]
    fn add_skill_filename_rejects_path_traversal() {
        assert_eq!(add_skill_filename("../../etc/passwd"), None);
        assert_eq!(add_skill_filename("a/b"), None);
        assert_eq!(add_skill_filename("a\\b"), None);
        assert_eq!(add_skill_filename(".."), None);
        assert_eq!(add_skill_filename("."), None);
        assert_eq!(add_skill_filename(""), None);
        assert_eq!(add_skill_filename("   "), None);
    }

    #[test]
    fn add_skill_written_body_warns_about_unenforced_tools() {
        let body = add_skill_written_body(std::path::Path::new("/tmp/x.md"), true, true);
        assert!(body.contains("not enforced"));
    }

    #[test]
    fn add_skill_written_body_warns_about_a_missing_name_field() {
        let body = add_skill_written_body(std::path::Path::new("/tmp/x.md"), false, false);
        assert!(body.contains("No `name:`"));
        assert!(!body.contains("not enforced"));
    }

    #[test]
    fn add_skill_written_body_stays_quiet_with_neither_warning() {
        let body = add_skill_written_body(std::path::Path::new("/tmp/x.md"), false, true);
        assert!(!body.contains('⚠'));
    }

    #[test]
    fn is_safe_session_arg_accepts_valid_labels_and_rejects_traversal() {
        assert!(is_safe_session_arg(""));
        assert!(is_safe_session_arg("   "));
        assert!(is_safe_session_arg("session-1"));
        assert!(is_safe_session_arg("my_session_2026"));
        assert!(!is_safe_session_arg("../foo"));
        assert!(!is_safe_session_arg("foo/bar"));
        assert!(!is_safe_session_arg("foo\\bar"));
        assert!(!is_safe_session_arg(".."));
    }
}


