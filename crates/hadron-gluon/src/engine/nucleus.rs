
use hadron_lattice::term::{self, Source};
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
///
/// The shipped DEFAULT only — production code enforces the RESOLVED (possibly
/// configured) budget carried on `Projection::nucleus_index_budget_bytes` instead
/// (see `Engine::nucleus_index_budget_bytes`), so this re-export is `#[cfg(test)]`:
/// nothing outside a test fixture should still be reading the unconfigurable default.
#[cfg(test)]
pub(crate) use crate::nucleus_status::BUDGET_BYTES as NUCLEUS_INDEX_BUDGET;

// "What is a lesson line" lives in `nucleus_status`, next to the budget, because the
// CHAMBER needs the same predicate for `/nucleus` and cannot reach into `engine`.
// It was private here and `/nucleus` grew its own looser `starts_with("- ")`, which
// counted the index preamble's prose bullets as lessons — a second opinion on the one
// number a quark is shown instead of the index.
use crate::nucleus_status::is_lesson_line;

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
                    term::warn(
                        Source::Gluon,
                        &format!("invariant exists but could not be read: {} — {err}", e.path().display()),
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

    // Standard laws, right after the Standard Model and before the invariant
    // tiers — a law pinned via `/learn-std-model[-global]` is unconditional, the
    // same as the Standard Model itself, not an opt-in named rule. The heading
    // wording matches `text::COMMANDS`' gloss for those two commands ("standard
    // law"): the human types the command and reads the heading, and two spellings
    // of one concept read as two features.
    if let Some(path) = global_laws_path() {
        if let Ok(body) = fs::read_to_string(&path) {
            if !body.trim().is_empty() {
                combined.push_str(&format!("\n# Your standard laws\n{}\n", body.trim()));
            }
        }
    }
    if let Ok(body) = fs::read_to_string(repo_laws_path(workspace_root)) {
        if !body.trim().is_empty() {
            combined.push_str(&format!("\n# This project's standard laws\n{}\n", body.trim()));
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

fn extract_slug_from_line(line: &str) -> Option<&str> {
    let start = line.find('[');
    let end = line.find(']');
    if let (Some(s), Some(e)) = (start, end) {
        if e > s + 1 {
            return Some(&line[s + 1..e]);
        }
    }
    None
}

fn extract_description_from_note(content: &str) -> String {
    if content.starts_with("---") {
        if let Some(end_fm) = content[3..].find("---") {
            let fm = &content[3..3 + end_fm];
            for line in fm.lines() {
                if let Some(desc) = line.strip_prefix("description:") {
                    return desc.trim().to_string();
                }
            }
        }
    }
    // Fallback: first non-empty line outside frontmatter
    let body = if content.starts_with("---") {
        if let Some(end_fm) = content[3..].find("---") {
            &content[3 + end_fm + 3..]
        } else {
            content
        }
    } else {
        content
    };
    body.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

trait LowercaseWords {
    fn lowercase_words(&self) -> Vec<String>;
}

impl LowercaseWords for str {
    fn lowercase_words(&self) -> Vec<String> {
        let stopwords: std::collections::HashSet<&str> = [
            "the", "and", "for", "with", "this", "that", "from", "you", "are", "have", "not", "all",
            "was", "will", "can", "has", "but", "about", "into", "over", "more", "then", "them",
        ]
        .into_iter()
        .collect();

        self.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
            .map(|w| w.to_lowercase())
            .filter(|w| w.len() >= 3 && !stopwords.contains(w.as_str()))
            .collect()
    }
}

pub(crate) fn rank_lessons(
    index_text: &str,
    notes_dir: &std::path::Path,
    query: &str,
    budget_bytes: usize,
) -> String {
    let max_output_bytes = budget_bytes / 8;
    let query_terms = query.lowercase_words();

    struct Section {
        heading: String,
        is_pinned_section: bool,
        lines: Vec<LessonLine>,
    }

    struct LessonLine {
        slug: String,
        line_text: String,
        is_pinned: bool,
        score: f64,
    }

    let mut sections: Vec<Section> = Vec::new();
    let mut current_section: Option<Section> = None;

    for line in index_text.lines() {
        if line.starts_with("## ") {
            if let Some(sec) = current_section.take() {
                sections.push(sec);
            }
            let heading = line[3..].trim().to_string();
            let is_pinned_section = heading.to_lowercase().contains("how we get things wrong");
            current_section = Some(Section {
                heading,
                is_pinned_section,
                lines: Vec::new(),
            });
        } else if is_lesson_line(line) {
            if current_section.is_none() {
                current_section = Some(Section {
                    heading: "General".to_string(),
                    is_pinned_section: false,
                    lines: Vec::new(),
                });
            }
            if let Some(sec) = current_section.as_mut() {
                if let Some(slug) = extract_slug_from_line(line) {
                    let is_pinned = sec.is_pinned_section;
                    sec.lines.push(LessonLine {
                        slug: slug.to_string(),
                        line_text: line.to_string(),
                        is_pinned,
                        score: if is_pinned { f64::INFINITY } else { 0.0 },
                    });
                }
            }
        }
    }
    if let Some(sec) = current_section.take() {
        sections.push(sec);
    }

    // Score lessons
    for sec in &mut sections {
        for lesson in &mut sec.lines {
            if lesson.is_pinned {
                continue;
            }
            let note_path = notes_dir.join(format!("{}.md", lesson.slug));
            let (desc, body) = if let Ok(content) = std::fs::read_to_string(&note_path) {
                let desc = extract_description_from_note(&content);
                (desc, content)
            } else {
                (String::new(), String::new())
            };

            let slug_terms = lesson.slug.lowercase_words();
            let hook_terms = lesson.line_text.lowercase_words();
            let desc_terms = desc.lowercase_words();
            let body_terms = body.lowercase_words();

            let mut score = 0.0;
            for q in &query_terms {
                let slug_matches = slug_terms.iter().filter(|t| t == &q).count() as f64;
                let hook_matches = hook_terms.iter().filter(|t| t == &q).count() as f64;
                let desc_matches = desc_terms.iter().filter(|t| t == &q).count() as f64;
                let body_matches = body_terms.iter().filter(|t| t == &q).count() as f64;

                score += slug_matches * 4.0
                    + desc_matches * 3.0
                    + hook_matches * 2.0
                    + body_matches * 1.0;
            }
            lesson.score = score;
        }
    }

    // Collect all lesson pointers and sort by relevance
    struct Candidate {
        section_idx: usize,
        line_idx: usize,
        score: f64,
        is_pinned: bool,
    }

    let mut candidates: Vec<Candidate> = Vec::new();
    for (sec_i, sec) in sections.iter().enumerate() {
        for (line_i, lesson) in sec.lines.iter().enumerate() {
            candidates.push(Candidate {
                section_idx: sec_i,
                line_idx: line_i,
                score: lesson.score,
                is_pinned: lesson.is_pinned,
            });
        }
    }

    // Sort: pinned first, then by score descending, then original index order
    candidates.sort_by(|a, b| {
        if a.is_pinned != b.is_pinned {
            return b.is_pinned.cmp(&a.is_pinned);
        }
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.section_idx.cmp(&b.section_idx))
            .then_with(|| a.line_idx.cmp(&b.line_idx))
    });

    // Select candidates within budget
    let mut selected_indices: std::collections::HashSet<(usize, usize)> =
        std::collections::HashSet::new();

    let header = "# Memory index\n\nShared by every quark, and injected into every prompt.\n\n";
    let mut current_bytes = header.len();

    for cand in candidates {
        if !cand.is_pinned && cand.score <= 0.0 {
            continue;
        }

        let sec = &sections[cand.section_idx];
        let line_text = &sec.lines[cand.line_idx].line_text;
        
        let sec_header_needed = !sections[cand.section_idx]
            .lines
            .iter()
            .enumerate()
            .any(|(li, _)| selected_indices.contains(&(cand.section_idx, li)));

        let sec_header_bytes = if sec_header_needed {
            format!("## {}\n\n", sec.heading).len()
        } else {
            0
        };

        let line_bytes = line_text.len() + 1; // including newline

        if current_bytes + sec_header_bytes + line_bytes > max_output_bytes {
            if cand.is_pinned {
                // Pinned items must fit if possible
                selected_indices.insert((cand.section_idx, cand.line_idx));
                current_bytes += sec_header_bytes + line_bytes;
            } else {
                // Non-pinned items cap at max_output_bytes
                continue;
            }
        } else {
            selected_indices.insert((cand.section_idx, cand.line_idx));
            current_bytes += sec_header_bytes + line_bytes;
        }
    }

    // Render selected lessons in original index order
    let mut out = String::new();
    out.push_str("# Memory index\n\nShared by every quark, and injected into every prompt.\n\n");

    for (sec_i, sec) in sections.iter().enumerate() {
        let selected_in_sec: Vec<&LessonLine> = sec
            .lines
            .iter()
            .enumerate()
            .filter(|(line_i, _)| selected_indices.contains(&(sec_i, *line_i)))
            .map(|(_, line)| line)
            .collect();

        if !selected_in_sec.is_empty() {
            out.push_str(&format!("## {}\n\n", sec.heading));
            for line in selected_in_sec {
                out.push_str(&line.line_text);
                out.push('\n');
            }
            out.push('\n');
        }
    }

    out.trim_end().to_string()
}


#[cfg(test)]
mod tests {
    use super::rank_lessons;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_notes() -> (TempDir, String) {
        let temp = TempDir::new().unwrap();
        let notes_dir = temp.path().join("notes");
        fs::create_dir_all(&notes_dir).unwrap();

        // Create sample note files
        fs::write(
            notes_dir.join("compiled-is-not-running.md"),
            "---\ndescription: A patch that compiles is not a feature that runs\n---\nFind its caller before reporting it works.",
        ).unwrap();

        fs::write(
            notes_dir.join("git-worktrees.md"),
            "---\ndescription: Git worktree management and checkout isolation\n---\nWorktrees isolate branches cleanly.",
        ).unwrap();

        fs::write(
            notes_dir.join("unindexed-note.md"),
            "---\ndescription: Unindexed note about database migration\n---\nDatabase migrations need locks.",
        ).unwrap();

        let index_text = r#"# Memory index

## How we get things wrong

- [compiled-is-not-running](notes/compiled-is-not-running.md) — A patch that compiles is not a feature that runs; find caller

## The shared tree

- [git-worktrees](notes/git-worktrees.md) — Git worktree management and checkout isolation
"#;

        (temp, index_text.to_string())
    }

    #[test]
    fn rank_lessons_pins_wrong_section_and_ranks_relevant_notes() {
        let (temp, index_text) = setup_test_notes();
        let notes_dir = temp.path().join("notes");

        let result = rank_lessons(&index_text, &notes_dir, "worktree isolation", 100_000);
        assert!(result.contains("## How we get things wrong"), "Must contain pinned section");
        assert!(result.contains("compiled-is-not-running"), "Must contain pinned lesson");
        assert!(result.contains("git-worktrees"), "Must rank relevant note for worktree query");
    }

    #[test]
    fn rank_lessons_determinism() {
        let (temp, index_text) = setup_test_notes();
        let notes_dir = temp.path().join("notes");

        let res1 = rank_lessons(&index_text, &notes_dir, "worktree query", 100_000);
        let res2 = rank_lessons(&index_text, &notes_dir, "worktree query", 100_000);
        assert_eq!(res1, res2, "Scoring and ranking must be deterministic");
    }

    #[test]
    fn rank_lessons_negative_control_returns_pinned_only() {
        let (temp, index_text) = setup_test_notes();
        let notes_dir = temp.path().join("notes");

        let result = rank_lessons(&index_text, &notes_dir, "xyzabc_nonexistent_query", 100_000);
        assert!(result.contains("## How we get things wrong"), "Must contain pinned section");
        assert!(result.contains("compiled-is-not-running"), "Must contain pinned lesson");
        assert!(!result.contains("git-worktrees"), "Must NOT contain unrelated lesson");
    }

    #[test]
    fn rank_lessons_preserves_index_order() {
        let (temp, index_text) = setup_test_notes();
        let notes_dir = temp.path().join("notes");

        let result = rank_lessons(&index_text, &notes_dir, "compiles worktree", 100_000);
        let pos_wrong = result.find("## How we get things wrong").unwrap();
        let pos_tree = result.find("## The shared tree").unwrap();
        assert!(pos_wrong < pos_tree, "Index order must be preserved");
    }

    #[test]
    fn rank_lessons_respects_budget_fraction() {
        let (temp, index_text) = setup_test_notes();
        let notes_dir = temp.path().join("notes");

        // Budget 2400 bytes -> max output 300 bytes. Pinned section takes 227 bytes.
        // Adding non-pinned git-worktrees would take >350 bytes, so it must be capped out.
        let result = rank_lessons(&index_text, &notes_dir, "compiles worktree database", 2400);
        assert!(result.len() <= 300, "Result length {} must be <= budget / 8 (300)", result.len());
        assert!(!result.contains("git-worktrees"), "Non-pinned lesson must be excluded due to budget cap");
    }
}


