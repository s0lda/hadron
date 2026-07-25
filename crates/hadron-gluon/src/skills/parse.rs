use std::fs;
use std::path::{Path, PathBuf};

use super::ResolvedSkill;

/// Insert `skill`, replacing any existing entry with the same `id` in place (so
/// later sources keep their position relative to unrelated skills — only the
/// overridden id's content changes, not the corpus order).
pub(super) fn upsert(skills: &mut Vec<ResolvedSkill>, skill: ResolvedSkill) {
    if let Some(existing) = skills.iter_mut().find(|s| s.id == skill.id) {
        *existing = skill;
    } else {
        skills.push(skill);
    }
}

/// Read every `*.md` file directly under `dir` (non-recursive) as a candidate
/// skill. A missing or unreadable directory yields no skills, silently — the
/// caller passes `None` for "not configured" and an absent `~/.hadron/skills` on a
/// machine that has never used custom skills is the same case, not an error.
pub(super) fn load_dir(dir: &Path) -> Vec<ResolvedSkill> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("md"))
        .collect();
    // Sorted so two files in the same directory resolve deterministically, and so
    // the merge order (and hence any same-directory override) doesn't depend on
    // the OS's directory-listing order.
    paths.sort();

    paths
        .into_iter()
        .filter_map(|path| {
            let text = match fs::read_to_string(&path) {
                Ok(t) => t,
                // Unreadable (non-UTF-8, permissions, …) — warn rather than skip
                // silently, mirroring the missing-`name:` case below: an authoring
                // mistake should be visible, not a skill that never loads with no clue.
                Err(e) => {
                    eprintln!("skills: skipping {} — could not read it: {e}", path.display());
                    return None;
                }
            };
            match parse_skill_file(&text) {
                Some(skill) => Some(skill),
                None => {
                    // A skill file with no `name:` front-matter has no merge key —
                    // rather than guess one from the filename (which would make the
                    // id depend on how the file happens to be named, not on what
                    // its author declared), it is skipped. Silent would be worse: a
                    // typo'd or missing `name:` should be visible, not a skill that
                    // silently never loads.
                    eprintln!(
                        "hadron-gluon: skipping skill file {} — missing required `name:` front-matter",
                        path.display()
                    );
                    None
                }
            }
        })
        .collect()
}

/// Parse one skill `.md` file's front-matter + body into a [`ResolvedSkill`].
/// `None` when there is no front-matter block at all, or the block has no `name:`
/// — both cases the caller reports as "skipped: missing `name:`".
pub(super) fn parse_skill_file(text: &str) -> Option<ResolvedSkill> {
    let (front, body) = split_front_matter(text);
    let front = front?;

    let id = front_matter_value(front, "name")?.to_string();
    let description = front_matter_value(front, "description").map(str::to_string);
    // Lowercased here, once, at parse time: `select` lowercases the task text before
    // matching, and built-in triggers are already all-lowercase in source — a custom
    // skill authored with `triggers: [Foo]` must not silently never match because its
    // author capitalised it.
    let triggers = front_matter_value(front, "triggers")
        .map(parse_list_value)
        .unwrap_or_default()
        .into_iter()
        .map(|t| t.to_lowercase())
        .collect();
    let tools = front_matter_value(front, "tools").map(parse_list_value).unwrap_or_default();

    Some(ResolvedSkill {
        id,
        triggers,
        body: body.trim().to_string(),
        description,
        tools,
    })
}

/// Split a `.md` file's leading `---`-fenced front-matter from its body. Returns
/// `(front_matter_lines, body)`; `front_matter_lines` is `None` when the text does
/// not open with a front-matter block, and `body` is then the whole input
/// unchanged. This is the one place the `---` fence is parsed — [`description`],
/// [`plan_author`], and [`crate::preons::load_preons`] all go through it
/// rather than re-splitting themselves.
pub(crate) fn split_front_matter(markdown: &str) -> (Option<&str>, &str) {
    match markdown.strip_prefix("---").and_then(|rest| rest.split_once("\n---")) {
        Some((front, body)) => (Some(front), body.trim_start_matches('\n')),
        None => (None, markdown),
    }
}

/// The value of a `key:` line within a front-matter block, or `None` if the key is
/// absent or its value is empty. Shared by every single-line front-matter field
/// (`name:`, `description:`, `author:`, `triggers:`, `tools:`).
pub(crate) fn front_matter_value<'a>(front: &'a str, key: &str) -> Option<&'a str> {
    let prefix = format!("{key}:");
    front.lines().find_map(|line| {
        let value = line.trim().strip_prefix(prefix.as_str())?.trim();
        (!value.is_empty()).then_some(value)
    })
}

/// Parse a front-matter list value in either of two forms:
/// - a YAML inline list: `[foo, bar, "baz qux"]`
/// - a bare comma-separated string: `foo, bar, baz qux`
///
/// Multi-line YAML lists (`triggers:\n  - foo\n  - bar`) are NOT supported — every
/// entry must be on the `key:` line itself. Surrounding quotes on an entry are
/// stripped; empty entries (a trailing comma, `[]`) are dropped.
pub(super) fn parse_list_value(raw: &str) -> Vec<String> {
    let raw = raw.trim();
    let inner = raw.strip_prefix('[').and_then(|s| s.strip_suffix(']')).unwrap_or(raw);

    inner
        .split(',')
        .map(|s| s.trim().trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}
