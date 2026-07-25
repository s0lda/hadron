
use hadron_lattice::{
    Event, Kind,
};

use std::fs;

use super::*;

/// The swarm's lessons INDEX for this project — one file, shared by every quark.
///
/// It was one file *per quark*, which meant a lesson agy paid for in blood was one
/// opus would pay for again. Shared, so the swarm learns once.
///
/// Lives under `.hadron/nucleus/` — the single knowledge root, alongside
/// `invariants/` — not the old `.hadron/memory/`. See
/// [`read_nucleus_index_with_fallback`] for what covers the window before a
/// project's legacy `memory/` has been migrated.
pub(super) fn nucleus_index_path(workspace_root: &std::path::Path) -> std::path::PathBuf {
    nucleus_lessons_dir(workspace_root).join("index.md")
}

/// Where the long-form notes live: `.hadron/nucleus/notes/<slug>.md`. The index names
/// them; the engine never loads them. That is the whole token argument — an index of
/// one-liners stays cheap forever, and the detail is paid for only on the turn a quark
/// actually opens it.
pub(super) fn nucleus_notes_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    nucleus_lessons_dir(workspace_root).join("notes")
}

fn nucleus_lessons_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    workspace_root.join(".hadron").join("nucleus")
}

fn legacy_memory_dir(workspace_root: &std::path::Path) -> std::path::PathBuf {
    workspace_root.join(".hadron").join("memory")
}

/// The index is in **every** prompt of **every** turn, so its size is a tax paid
/// forever — and the tax is *context*, not money. Prompt caching makes re-sending it
/// cheap; it does not make it free, because every token here is a token the model is
/// not spending on the task. It is also the wrong thing to grow: the index is a
/// routing table (one line per lesson) and the detail belongs in `notes/`, which the
/// engine never loads. A file that outgrows this has stopped being an index.
pub(crate) use crate::nucleus_status::BUDGET_BYTES as NUCLEUS_INDEX_BUDGET;

/// Whether an index line names a lesson, in EITHER shape the index has had.
///
/// The pointer form — `- [slug](notes/slug.md) — hook` — is what the chamber writes
/// now (`text::learn_line`); the bold form — `- **slug** — lesson` — is what every
/// line written before the migration looks like, and those files are on disk in
/// projects nobody is going to rewrite by hand. A counter that knew only one shape
/// would report `0 lesson(s)` for a full index, which is a lie in the one place a
/// quark cannot check: the summary it is shown INSTEAD of the index.
fn is_lesson_line(line: &str) -> bool {
    let line = line.trim_start();
    line.starts_with("- **") || line.starts_with("- [")
}

/// A few hundred bytes: one heading per `## ` section in the index, with a count
/// of lessons under it. What the quark sees instead of the full index when the
/// index has grown past a size worth always sending in full.
pub(crate) fn tag_manifest(index_text: &str) -> String {
    let mut out = String::new();
    let mut current: Option<(&str, usize)> = None;
    for line in index_text.lines() {
        if let Some(heading) = line.strip_prefix("## ") {
            if let Some((h, n)) = current.take() {
                out.push_str(&format!("- {h}: {n} lesson(s)\n"));
            }
            current = Some((heading, 0));
        } else if is_lesson_line(line) {
            if current.is_none() {
                current = Some(("General", 0));
            }
            if let Some((_, n)) = current.as_mut() {
                *n += 1;
            }
        }
    }
    if let Some((h, n)) = current {
        out.push_str(&format!("- {h}: {n} lesson(s)\n"));
    }
    out
}

/// Read the lessons index. Returns full text and false (truncation removed in Task 4).
pub(super) fn read_nucleus_index(path: &std::path::Path) -> (String, bool) {
    let raw = fs::read_to_string(path).unwrap_or_default();
    (raw, false)
}


/// Read the lessons index from its home (`.hadron/nucleus/index.md`),
/// falling back to the pre-migration legacy location
/// (`.hadron/memory/index.md`) if nucleus is empty — so a quark is never
/// shown an empty index in the window before `Engine::migrate_legacy_memory`
/// has run at daemon boot.
pub(super) fn read_nucleus_index_with_fallback(workspace_root: &std::path::Path) -> (String, bool) {
    let (text, truncated) = read_nucleus_index(&nucleus_index_path(workspace_root));
    if !text.trim().is_empty() {
        return (text, truncated);
    }
    read_nucleus_index(&legacy_memory_dir(workspace_root).join("index.md"))
}

fn home_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(std::path::PathBuf::from(home))
}

/// Where a user's own always-on rules live, under their home directory. Their
/// preferences, across every project they run Hadron in.
fn global_invariants_dir() -> Option<std::path::PathBuf> {
    Some(home_dir()?.join(".hadron").join("nucleus").join("invariants"))
}

/// Standing laws the human pinned with `/learn-std-model[-global]` — appended
/// verbatim into every prompt, unlike a named `invariants/` rule which loads only
/// when `always.md` or requested by name. A law is meant to be unconditional, the
/// same way the Standard Model itself is.
fn global_laws_path() -> Option<std::path::PathBuf> {
    Some(home_dir()?.join(".hadron").join("nucleus").join("laws.md"))
}

fn repo_laws_path(workspace_root: &std::path::Path) -> std::path::PathBuf {
    workspace_root.join(".hadron").join("nucleus").join("laws.md")
}

/// Read every `*.md` in a directory, sorted by name so the prompt is deterministic
/// (a projection that reorders itself between turns busts every prompt cache).
/// A directory that isn't there is not an error — the tier is simply absent.
fn read_invariant_dir(dir: &std::path::Path) -> Vec<(String, String)> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };

    let mut found: Vec<(String, String)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let stem = name.strip_suffix(".md")?.to_string();
            match fs::read_to_string(e.path()) {
                Ok(body) => Some((stem, body)),
                Err(err) => {
                    // Loud, not silent: an unreadable rule file is a rule the quark
                    // is not being given, and nobody would otherwise ever know.
                    eprintln!(
                        "warning: invariant exists but could not be read: {} — {err}",
                        e.path().display()
                    );
                    None
                }
            }
        })
        .collect();

    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

/// Assemble the three tiers of working protocol handed to a quark.
///
/// 1. **Hardcoded** — [`STANDARD_MODEL`], compiled in, always present, not optional.
/// 2. **Global** — `~/.hadron/nucleus/invariants/`, the user's own standing rules.
/// 3. **Repo** — `<workspace>/.hadron/nucleus/invariants/`, this project's rules.
///
/// Ordered narrowest-last, and the prompt *says* which tier each block came from:
/// a quark that cannot tell a shipped rule from a project rule cannot reason about
/// which one to question when they conflict. `requested` names extra repo rules to
/// pull in for this specific turn.
pub(super) fn build_invariants(workspace_root: &std::path::Path, requested: &[String]) -> (String, Vec<String>) {
    let mut combined = String::new();

    // Tier 1 — always, whatever else is or isn't on disk.
    combined.push_str(STANDARD_MODEL.trim());
    combined.push('\n');

    // Standing laws, right after the Standard Model and before the invariant
    // tiers — a law pinned via `/learn-std-model[-global]` is unconditional, the
    // same as the Standard Model itself, not an opt-in named rule.
    if let Some(path) = global_laws_path() {
        if let Ok(body) = fs::read_to_string(&path) {
            if !body.trim().is_empty() {
                combined.push_str(&format!("\n# Your standing laws\n{}\n", body.trim()));
            }
        }
    }
    if let Ok(body) = fs::read_to_string(repo_laws_path(workspace_root)) {
        if !body.trim().is_empty() {
            combined.push_str(&format!("\n# This project's standing laws\n{}\n", body.trim()));
        }
    }

    // Tier 2 — the user's preferences, across all their projects.
    if let Some(global_dir) = global_invariants_dir() {
        for (name, body) in read_invariant_dir(&global_dir) {
            combined.push_str(&format!("\n# Your rule: {name}\n{}\n", body.trim()));
        }
    }

    // Tier 3 — this project. A cybersecurity repo and an indie game do not want the
    // same rules, so the repo tier is where the domain gets to speak.
    let repo_dir = workspace_root.join(".hadron").join("nucleus").join("invariants");
    let repo_rules = read_invariant_dir(&repo_dir);

    let mut available: Vec<String> = repo_rules.iter().map(|(n, _)| n.clone()).collect();
    available.sort();

    let mut requested_sorted = requested.to_vec();
    requested_sorted.sort();

    for (name, body) in &repo_rules {
        // Repo rules named `always.md` load unconditionally; the rest load when the
        // turn asks for them by name, so a big rulebook doesn't blow the budget.
        if name == "always" || requested_sorted.contains(name) {
            combined.push_str(&format!("\n# Project rule: {name}\n{}\n", body.trim()));
        }
    }

    (combined.trim().to_string(), available)
}

/// How many bytes of *rendered field transcript* a projection may carry.
///
/// Two hard reasons, not a taste:
///
/// 1. **`execve` rejects a long argv element.** `agy` has no stdin in print mode
///    and no `--prompt-file`, so its whole prompt is one argv element — and Linux
///    caps a single element at `MAX_ARG_STRLEN` = 128 KiB, unraisable. The field
///    window used to be `events.to_vec()` (the *entire* field), which grew past
///    that in normal use and killed every agy turn with E2BIG in ~0.7 ms, before
///    any subprocess started.
/// 2. **Tokens are money.** Re-sending the whole field on every turn of every quark
///    is quadratic in the swarm's lifetime, and the oldest events are the least
///    useful.
///
/// 48 KiB keeps a generous multi-turn transcript while leaving room under the
/// adapter's own [safety net](crate::adapter::cli) for the diff, nucleus and task.
/// A *byte* budget, not an event count: one long message can blow an event count.
pub const FIELD_WINDOW_BUDGET_BYTES: usize = 48 * 1024;

/// What one event costs the rendered prompt. Only `Message` bodies are rendered
/// (`prompt::build`), plus a small allowance for the `**from → to:**` prefix.
pub(super) fn event_cost(e: &Event) -> usize {
    let body = match &e.kind {
        Kind::Message { body } => body.len(),
        _ => 0,
    };
    body + 32
}

/// The most recent events that fit in `budget` bytes, in field order.
///
/// Most-recent-wins: the driving message and the freshest context are the ones a
/// quark actually needs; the oldest are the ones to drop. Always yields at least
/// the single newest event, even if that one event is itself over budget — a quark
/// with no transcript at all cannot act, and the adapter's guard bounds the final
/// argv anyway.
pub(super) fn bounded_window(events: &[Event], budget: usize) -> Vec<Event> {
    let mut spent = 0usize;
    let mut keep = 0usize;
    for e in events.iter().rev() {
        let cost = event_cost(e);
        if keep > 0 && spent + cost > budget {
            break;
        }
        spent += cost;
        keep += 1;
    }
    events[events.len().saturating_sub(keep)..].to_vec()
}
