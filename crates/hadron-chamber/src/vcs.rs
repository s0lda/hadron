//! The chamber's read-only view of git.
//!
//! Deliberately NOT `hadron_gluon::snapshot`. That module lives in the engine crate,
//! which links bundled SQLite, the tokio runtime, the file watcher and the CLI process
//! adapters — none of which the UI has any business carrying just to read a diff. The
//! chamber renders the field; it does not drive the swarm, so it must not depend on the
//! crate that does. Shelling out to git is the whole implementation.

use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub hunks: Vec<Hunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

/// `git diff HEAD` for the working tree — what the Changes rail shows.
///
/// A repository with no commits yet has no HEAD to diff against, so it has no changes
/// to show rather than an error to report.
pub fn working_diff(repo_root: &Path) -> Option<Vec<FileDiff>> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    Some(parse_diff(&raw))
}

/// What a branch adds over its merge-base with `base` (`git diff base...branch`,
/// three-dot) — the "what would land if this merged" view, not the raw two-endpoint
/// delta. A branch already merged (or `base` itself) yields no files, which the panel
/// renders as "no changes relative to <base>" rather than an error.
pub fn branch_diff(repo_root: &Path, base: &str, branch: &str) -> Option<Vec<FileDiff>> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["diff", &format!("{base}...{branch}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&out.stdout);
    Some(parse_diff(&raw))
}

/// How many commits the branch/worktree at `wt_path` is ahead of `base`.
pub fn commits_ahead(wt_path: &Path, base: &str) -> Option<usize> {
    let range = format!("{base}..HEAD");
    let out = Command::new("git")
        .current_dir(wt_path)
        .args(["rev-list", "--count", &range])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// The patch diff for a single commit (`git show --patch <commit>`).
pub fn commit_diff(repo_root: &Path, commit: &str) -> Option<Vec<FileDiff>> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["show", "--patch", commit])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(parse_diff(&String::from_utf8_lossy(&out.stdout)))
}

/// The full raw commit message for a single commit (`git show -s --format=%B <commit>`).
pub fn commit_message(repo_root: &Path, commit: &str) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["show", "-s", "--format=%B", commit])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// Read a file's content at a specific git ref (`git show <ref>:<path>`).
pub fn show_file_at_ref(repo_root: &Path, git_ref: &str, file_path: &str) -> Option<String> {
    let spec = format!("{git_ref}:{file_path}");
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["show", &spec])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn parse_diff(raw: &str) -> Vec<FileDiff> {
    let mut files = Vec::new();
    let mut current_file: Option<FileDiff> = None;
    let mut current_hunk: Option<Hunk> = None;

    for line in raw.lines() {
        if line.starts_with("diff --git ") {
            if let Some(mut file) = current_file.take() {
                if let Some(hunk) = current_hunk.take() {
                    file.hunks.push(hunk);
                }
                files.push(file);
            }

            // For cases where there are spaces, we will fallback to parsing +++ and ---,
            // but we can try to extract path from diff --git a/path b/path
            let path = if let Some(b_part) = line.split(" b/").last() {
                b_part.to_string()
            } else {
                String::new()
            };

            current_file = Some(FileDiff {
                path,
                added: 0,
                removed: 0,
                hunks: Vec::new(),
            });
        } else if line.starts_with("@@ ") {
            if let Some(file) = current_file.as_mut() {
                if let Some(hunk) = current_hunk.take() {
                    file.hunks.push(hunk);
                }
                current_hunk = Some(Hunk {
                    header: line.to_string(),
                    lines: Vec::new(),
                });
            }
        } else if line.starts_with("+++ b/") {
            if let Some(file) = current_file.as_mut() {
                file.path = line["+++ b/".len()..].to_string();
            }
        } else if line.starts_with("--- a/") {
            if let Some(file) = current_file.as_mut() {
                if file.path.is_empty() {
                    file.path = line["--- a/".len()..].to_string();
                }
            }
        } else if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("index ")
        {
            // skip other header lines
        } else if let Some(hunk) = current_hunk.as_mut() {
            if let Some(content) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine::Added(content.to_string()));
                if let Some(file) = current_file.as_mut() {
                    file.added += 1;
                }
            } else if let Some(content) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine::Removed(content.to_string()));
                if let Some(file) = current_file.as_mut() {
                    file.removed += 1;
                }
            } else if let Some(content) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine::Context(content.to_string()));
            }
            // we ignore \ No newline at end of file and similar lines
        }
    }

    if let Some(mut file) = current_file.take() {
        if let Some(hunk) = current_hunk.take() {
            file.hunks.push(hunk);
        }
        files.push(file);
    }

    files
}

/// The project root that owns a field path — `<root>/.hadron/field.jsonl` → `<root>`.
/// A field sitting outside a `.hadron/` directory is taken to be in the root already.
pub fn repo_root_of(field_path: &Path) -> &Path {
    let Some(parent) = field_path.parent() else {
        return field_path;
    };
    let root = if parent.file_name() == Some(std::ffi::OsStr::new(".hadron")) {
        parent.parent().unwrap_or(parent)
    } else {
        parent
    };
    if root.as_os_str().is_empty() {
        Path::new(".")
    } else {
        root
    }
}

/// Strip Windows verbatim UNC prefix (`\\?\` or `\\?\UNC\`).
pub fn strip_unc_prefix(path_str: &str) -> String {
    if let Some(stripped) = path_str.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{stripped}")
    } else if let Some(stripped) = path_str.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        path_str.to_string()
    }
}

/// Formats the repository working directory for display in the UI (e.g. `~/dev/hadron/`).
pub fn format_working_dir(field_path: &Path) -> String {
    let root = repo_root_of(field_path);
    let abs_path = root
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| root.to_path_buf()));

    let abs_str = strip_unc_prefix(&abs_path.to_string_lossy());
    let home_var = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"));

    let path_str = if let Ok(home) = home_var {
        let home_clean = strip_unc_prefix(&home);
        let home_path = Path::new(&home_clean);
        let abs_p = Path::new(&abs_str);
        if let Ok(rel) = abs_p.strip_prefix(home_path) {
            if rel.as_os_str().is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", rel.display().to_string().replace('\\', "/"))
            }
        } else {
            abs_str
        }
    } else {
        abs_str
    };

    if path_str.ends_with('/') || path_str.ends_with('\\') {
        path_str
    } else {
        format!("{path_str}/")
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitStatus {
    Modified,
    Added,
    Deleted,
}

/// A local branch and whether it has landed in the target branch (`main`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    pub name: String,
    pub head: String,
    pub is_current: bool,
    pub merged: bool,
}

/// Parse `git for-each-ref --format='%(refname:short) %(objectname:short)' refs/heads/`
/// output against a precomputed set of names already merged into the target branch
/// (one `git branch --merged` call covers every branch, instead of a
/// `merge-base --is-ancestor` subprocess per branch).
pub fn parse_branches(
    raw: &str,
    current: &str,
    merged: &std::collections::HashSet<String>,
) -> Vec<BranchInfo> {
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(2, ' ');
            let name = parts.next()?.to_string();
            if name.is_empty() {
                return None;
            }
            let head = parts.next().unwrap_or("").trim().to_string();
            Some(BranchInfo {
                is_current: name == current,
                merged: merged.contains(&name),
                name,
                head,
            })
        })
        .collect()
}

/// Raw ref fingerprint for branch heads to detect changes cheaply.
pub fn branch_fingerprint(repo_root: &Path) -> String {
    run_git(
        repo_root,
        &["for-each-ref", "--format=%(refname:short) %(objectname:short)", "refs/heads/"],
    )
}

/// Every local branch, with `merged` set against `target` (e.g. `"main"`).
pub fn list_branches(repo_root: &Path, target: &str) -> Vec<BranchInfo> {
    let current = run_git(
        repo_root,
        &["rev-parse", "--abbrev-ref", "HEAD"],
    )
    .trim()
    .to_string();
    let refs = run_git(
        repo_root,
        &["for-each-ref", "--format=%(refname:short) %(objectname:short)", "refs/heads/"],
    );
    let merged_raw = run_git(
        repo_root,
        &["branch", "--merged", target, "--format=%(refname:short)"],
    );
    let merged: std::collections::HashSet<String> =
        merged_raw.lines().map(|s| s.trim().to_string()).collect();
    parse_branches(&refs, &current, &merged)
}

/// One `git worktree list --porcelain` entry — a checkout of this repo living
/// somewhere on disk, e.g. a quark's isolated `.hadron/trees/<id>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeInfo {
    pub path: String,
    pub head: String,
    /// `None` for a detached-HEAD worktree.
    pub branch: Option<String>,
}

/// Parse `git worktree list --porcelain` output — blank-line-separated blocks of
/// `worktree <path>` / `HEAD <sha>` / `branch refs/heads/<name>` (or `detached`).
pub fn parse_worktrees(raw: &str) -> Vec<WorktreeInfo> {
    let mut out = Vec::new();
    let mut current: Option<WorktreeInfo> = None;

    for line in raw.lines() {
        if line.is_empty() {
            out.extend(current.take());
            continue;
        }
        if let Some(p) = line.strip_prefix("worktree ") {
            out.extend(current.take());
            current = Some(WorktreeInfo { path: p.to_string(), head: String::new(), branch: None });
        } else if let Some(h) = line.strip_prefix("HEAD ") {
            if let Some(entry) = current.as_mut() {
                entry.head = h.chars().take(8).collect();
            }
        } else if let Some(b) = line.strip_prefix("branch refs/heads/") {
            if let Some(entry) = current.as_mut() {
                entry.branch = Some(b.to_string());
            }
        }
        // "detached" / "bare" / "locked" lines carry no data we render — skipped.
    }
    out.extend(current.take());
    out
}

/// Every worktree of this repo (the human's checkout plus every quark's).
pub fn list_worktrees(repo_root: &Path) -> Vec<WorktreeInfo> {
    let raw = run_git(repo_root, &["worktree", "list", "--porcelain"]);
    parse_worktrees(&raw)
}

/// Which branch `/abandon @quark` should act on, and where to look for it when
/// the worktree isn't sitting on a branch at all.
///
/// The obvious source is `hadron_gluon::worktree::current_branch(wt_path)` — but
/// `abandon_branch` detaches the worktree as its FIRST step, on every call,
/// including the one that only tags-and-refuses an unmerged branch. So a human
/// who types `/abandon @quark` (refused, unmerged) then `/abandon @quark confirm`
/// hits a worktree that is already detached on the second call, and
/// `current_branch` would report `None` — reading as "nothing to abandon" for the
/// exact branch the first call just tagged. Falling back to the one surviving
/// `quark/<id>/*` ref (there is normally at most one — old ones are pruned on
/// land) closes that gap; more than one is a real ambiguity this reports rather
/// than guesses at.
pub fn quark_branch_to_abandon(repo_root: &Path, wt_path: &Path, quark_id: &str) -> Result<String, String> {
    if let Some(b) = hadron_gluon::worktree::current_branch(wt_path) {
        return Ok(b);
    }
    let raw = run_git(
        repo_root,
        &["for-each-ref", "--format=%(refname:short)", &format!("refs/heads/quark/{quark_id}/")],
    );
    let mut candidates: Vec<String> =
        raw.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    match candidates.len() {
        0 => Err(format!(
            "`{quark_id}`'s worktree is detached and no `quark/{quark_id}/*` branch remains — nothing to abandon."
        )),
        1 => Ok(candidates.remove(0)),
        _ => Err(format!(
            "`{quark_id}` has {} pending branches ({}) and its worktree is detached — ambiguous; \
             delete the right one with `git branch -D <name>` directly.",
            candidates.len(),
            candidates.join(", ")
        )),
    }
}

/// A short kebab-case-ish tag name for an archived branch: `quark/acp-claude/01K`
/// → `archive/acp-claude-01K`. Not `text::slugify` — that trims prose to five
/// words and lowercases it, which would mangle a ULID; this only strips the
/// `quark/` prefix every branch carries and joins what's left with `-` so the
/// result is a single valid ref path segment.
fn archive_tag_name(branch: &str) -> String {
    format!("archive/{}", branch.trim_start_matches("quark/").replace('/', "-"))
}

/// Discard `branch`, currently checked out (or previously checked out — see
/// [`quark_branch_to_abandon`]) in a quark's worktree at `wt_path`.
///
/// Three steps, always in this order so an interrupted run never loses work:
/// 1. **Archive-tag it first** (`archive/<slug>` at its current HEAD) — idempotent,
///    so calling this twice for the same branch (the confirm re-invocation) does
///    not fail on "tag already exists".
/// 2. **Detach the worktree.** `git branch -d/-D` refuses a branch checked out in
///    ANY worktree, and this is the one it's checked out in; detaching to the
///    same commit changes no files, so it is safe regardless of a dirty tree.
/// 3. **`-d`.** Only when `force` (the human's explicit `/abandon @quark confirm`,
///    the in-chat authorisation the `Branch Deletion Uses -d` invariant asks for)
///    does a refused `-d` retry as `-D` — never on the first, unconfirmed call.
pub fn abandon_branch(repo_root: &Path, wt_path: &Path, branch: &str, force: bool) -> String {
    let sha = run_git(wt_path, &["rev-parse", "HEAD"]).trim().to_string();
    if sha.is_empty() {
        return format!("`{branch}` — could not resolve its HEAD commit; nothing touched.");
    }

    let tag = archive_tag_name(branch);
    let existing = run_git(repo_root, &["rev-parse", "--verify", "-q", &tag]).trim().to_string();
    if existing.is_empty() {
        let ok = Command::new("git")
            .current_dir(repo_root)
            .args(["tag", &tag, &sha])
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok {
            return format!("`{branch}` — could not create archive tag `{tag}`; nothing deleted.");
        }
    } else if existing != sha {
        return format!(
            "`{branch}` — archive tag `{tag}` already exists but points elsewhere ({existing}); \
             refusing to overwrite it. Resolve manually with `git tag -d {tag}` if that's stale."
        );
    }

    let detached = Command::new("git")
        .current_dir(wt_path)
        .args(["checkout", "--detach", "-q"])
        .output()
        .is_ok_and(|o| o.status.success());
    if !detached {
        return format!(
            "`{branch}` — tagged `{tag}` ({sha}), but could not detach its worktree; branch left in place."
        );
    }

    let deleted_d = Command::new("git")
        .current_dir(repo_root)
        .args(["branch", "-d", branch])
        .output()
        .is_ok_and(|o| o.status.success());
    if deleted_d {
        return format!("`{branch}` abandoned — tagged `{tag}` ({sha}), then deleted (`-d`, already merged).");
    }

    if !force {
        let reattach_note = reattach(wt_path, branch);
        return format!(
            "`{branch}` has unmerged commits — tagged `{tag}` ({sha}) but NOT deleted. Re-run \
             `/abandon @<quark> confirm` to force it (`-D`); restore any time with \
             `git branch {branch} {tag}`.{reattach_note}"
        );
    }

    let deleted_big_d = Command::new("git")
        .current_dir(repo_root)
        .args(["branch", "-D", branch])
        .output()
        .is_ok_and(|o| o.status.success());
    if deleted_big_d {
        format!(
            "`{branch}` force-abandoned — tagged `{tag}` ({sha}), then deleted (`-D`, confirmed). \
             Restore any time with `git branch {branch} {tag}`."
        )
    } else {
        let reattach_note = reattach(wt_path, branch);
        format!("`{branch}` — tagged `{tag}` ({sha}), but `-D` still failed; branch left in place.{reattach_note}")
    }
}

/// Put the worktree back on `branch` after a refused delete — every path that
/// leaves the branch alive must also leave the worktree exactly where it found
/// it, or the daemon refuses to dispatch that quark at all (a detached HEAD is
/// treated as an unusable worktree — see `worktree::assert_not_default_branch`
/// in `hadron-gluon`). This was the actual cause of a live "all quarks stuck"
/// incident: `abandon_branch` detached first, `-d` correctly refused the
/// unmerged branch, and nothing ever re-attached it.
///
/// Returns a trailing note ONLY on failure (empty string on success), so callers
/// can append it to their message without adding noise to the common case.
fn reattach(wt_path: &Path, branch: &str) -> String {
    let ok = Command::new("git")
        .current_dir(wt_path)
        .args(["checkout", branch, "-q"])
        .output()
        .is_ok_and(|o| o.status.success());
    if ok {
        String::new()
    } else {
        format!(
            " Its worktree could not be re-attached to `{branch}` — it is left on a detached \
             HEAD; re-attach manually with `git -C {} checkout {branch}`.",
            wt_path.display()
        )
    }
}

/// A short ASCII commit graph (`git log --graph --oneline --decorate`) — rendered
/// verbatim in a monospace font rather than parsed, since git already draws the
/// graph characters and decorations (branch/tag labels) correctly.
/// A short ASCII commit graph — rendered verbatim or parsed for UI representation.
///
/// **No `-n` cap**: the Graph tab walks every commit on every ref. That is only
/// affordable because the tab parses this string *once* (on load / on the snapshot
/// toggle) and renders the rows through a virtualized `gpui::list`, so the cost is
/// one subprocess plus one parse — not one element per commit. Re-introducing a
/// limit here would silently hide history again; cap what is *rendered*, never what
/// is walked.
pub fn commit_graph(repo_root: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args([
            "log",
            "--graph",
            "--all",
            "--pretty=format:%h|%H|%p|%an|%ar|%D|%s",
        ])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneSeg {
    pub from_col: usize,
    pub to_col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefKind {
    Head,
    LocalBranch,
    RemoteBranch,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefDecoration {
    pub name: String,
    pub kind: RefKind,
}

/// One rendered line of `git log --graph`. Git already computes the lane layout and
/// draws the ASCII rails; we only re-style it. A *commit* row carries commit metadata
/// (`hash`, `full_hash`, `parents`, `author`, `relative_date`, `decorations`, `subject`);
/// a *connector* row (`|/`, `| |`) carries only `rail` and joins commits across lanes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphRow {
    /// The leading graph-character prefix, up to and including this row's `*` marker
    /// on a commit row (e.g. `"* "`, `"| * "`), or the whole run on a connector row.
    pub rail: String,
    /// 7-char abbreviated sha — `None` on a connector row.
    pub hash: Option<String>,
    pub full_hash: Option<String>,
    pub parents: Vec<String>,
    pub author: Option<String>,
    pub relative_date: Option<String>,
    pub subject: String,
    pub decorations: Vec<RefDecoration>,
    pub lanes: Vec<LaneSeg>,
    pub node_col: Option<usize>,
}

/// Parse a rail string into lane segment connections and commit node column.
pub fn parse_rail_lanes(rail: &str) -> (Vec<LaneSeg>, Option<usize>) {
    let mut lanes = Vec::new();
    let mut node_col = None;
    for (i, ch) in rail.chars().enumerate() {
        match ch {
            '*' | 'o' => {
                let col = i / 2;
                node_col = Some(col);
                lanes.push(LaneSeg { from_col: col, to_col: col });
            }
            '|' => {
                let col = i / 2;
                lanes.push(LaneSeg { from_col: col, to_col: col });
            }
            '/' => {
                let from_col = (i + 1) / 2;
                let to_col = from_col.saturating_sub(1);
                lanes.push(LaneSeg { from_col, to_col });
            }
            '\\' => {
                let from_col = i / 2;
                let to_col = from_col + 1;
                lanes.push(LaneSeg { from_col, to_col });
            }
            '_' | '-' => {
                let from_col = i / 2;
                let to_col = from_col;
                lanes.push(LaneSeg { from_col, to_col });
            }
            _ => {}
        }
    }
    (lanes, node_col)
}

/// Parse `git log --graph --pretty=format:%h|%H|%p|%an|%ar|%D|%s` output into structured
/// rows so the chamber can style the rails, dots, hashes, metadata and ref decorations.
pub fn parse_graph(raw: &str) -> Vec<GraphRow> {
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let Some(star) = line.find('*') else {
                let rail = line.trim_end().to_string();
                let (lanes, node_col) = parse_rail_lanes(&rail);
                return GraphRow {
                    rail,
                    lanes,
                    node_col,
                    ..Default::default()
                };
            };
            // The rail runs past the `*` whenever other lanes continue to its right
            // (`* | | abc1234 …`); cutting at the `*` would read the next `|` as the sha.
            let end = line[star..]
                .find(|c: char| !matches!(c, '*' | '|' | '/' | '\\' | '_' | ' '))
                .map_or(line.len(), |off| star + off);
            let rail = line[..end].trim_end().to_string();
            let (lanes, node_col) = parse_rail_lanes(&rail);
            let rest = line[end..].trim_start();

            let parts: Vec<&str> = rest.splitn(7, '|').collect();
            let hash = parts.first().copied().filter(|s| !s.is_empty()).map(|s| s.to_string());
            let full_hash = parts.get(1).copied().filter(|s| !s.is_empty()).map(|s| s.to_string());
            let parents = parts
                .get(2)
                .map_or(Vec::new(), |s| s.split_whitespace().map(|p| p.to_string()).collect());
            let author = parts.get(3).copied().filter(|s| !s.is_empty()).map(|s| s.to_string());
            let relative_date = parts.get(4).copied().filter(|s| !s.is_empty()).map(|s| s.to_string());
            let raw_decorations = parts.get(5).copied().unwrap_or("");
            let subject = parts.get(6).copied().unwrap_or("").to_string();

            let mut decorations = Vec::new();
            if !raw_decorations.trim().is_empty() {
                for item in raw_decorations.split(", ") {
                    let item = item.trim();
                    if item.is_empty() {
                        continue;
                    }
                    let (kind, name) = if let Some(target) = item.strip_prefix("HEAD -> ") {
                        (RefKind::Head, target.to_string())
                    } else if item == "HEAD" {
                        (RefKind::Head, "HEAD".to_string())
                    } else if let Some(tag_name) = item.strip_prefix("tag: ") {
                        (RefKind::Tag, tag_name.to_string())
                    } else if item.starts_with("origin/") || item.starts_with("upstream/") {
                        (RefKind::RemoteBranch, item.to_string())
                    } else {
                        (RefKind::LocalBranch, item.to_string())
                    };
                    decorations.push(RefDecoration { name, kind });
                }
            }

            GraphRow {
                rail,
                hash,
                full_hash,
                parents,
                author,
                relative_date,
                subject,
                decorations,
                lanes,
                node_col,
            }
        })
        .collect()
}

/// Run a git subcommand in `repo_root`, returning stdout (empty on any failure —
/// callers treat "nothing" the same as "git couldn't answer", never an error to
/// surface, matching [`get_git_statuses`]'s existing best-effort convention).
fn run_git(repo_root: &Path, args: &[&str]) -> String {
    Command::new("git")
        .current_dir(repo_root)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default()
}

pub fn get_git_statuses(repo_root: &Path) -> std::collections::HashMap<String, GitStatus> {
    let mut statuses = std::collections::HashMap::new();
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["status", "--porcelain"])
        .output();
    if let Ok(output) = out {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if line.len() < 4 {
                    continue;
                }
                let code = &line[0..2];
                let path_part = &line[3..];
                let path = if code.starts_with('R') {
                    if let Some(pos) = path_part.find(" -> ") {
                        &path_part[pos + 4..]
                    } else {
                        path_part
                    }
                } else {
                    path_part
                };
                let path = path.trim_matches('"').to_string();

                let status = if code.contains('D') {
                    GitStatus::Deleted
                } else if code.contains('A') || code.contains('?') {
                    GitStatus::Added
                } else if code.contains('M') || code.contains('R') || code.contains('T') {
                    GitStatus::Modified
                } else {
                    continue;
                };
                statuses.insert(path, status);
            }
        }
    }
    statuses
}

/// Preview or perform safe cleanup of merged quark branches.
pub fn prune_merged_worktrees_and_branches(repo_root: &Path, confirm: bool) -> String {
    let base = hadron_gluon::worktree::default_branch(repo_root);
    let branches = list_branches(repo_root, &base);
    let merged_branches: Vec<&BranchInfo> = branches
        .iter()
        .filter(|b| b.merged && b.name.starts_with("quark/"))
        .collect();

    if merged_branches.is_empty() {
        return "No merged `quark/*` branches found to prune.".to_string();
    }

    if !confirm {
        let mut msg = format!("**`/prune` Preview ({} merged branches found)**\n\n", merged_branches.len());
        for b in &merged_branches {
            msg.push_str(&format!("- `{}` (merged into `{base}`)\n", b.name));
        }
        msg.push_str("\nRun `/prune confirm` to create archive tags and delete these branches (`git branch -d`).");
        return msg;
    }

    let mut pruned_count = 0;
    let mut details = Vec::new();
    for b in &merged_branches {
        let tag = archive_tag_name(&b.name);
        let sha = run_git(repo_root, &["rev-parse", "--verify", "-q", &b.name]).trim().to_string();
        if !sha.is_empty() {
            let _ = Command::new("git").current_dir(repo_root).args(["tag", &tag, &sha]).output();
        }
        let del = Command::new("git").current_dir(repo_root).args(["branch", "-d", &b.name]).output();
        if del.is_ok_and(|o| o.status.success()) {
            pruned_count += 1;
            details.push(format!("- `{}` -> archived as `{tag}` & deleted", b.name));
        } else {
            details.push(format!("- `{}` -> skipped/could not delete", b.name));
        }
    }

    format!(
        "**Prune Complete ({}/{} branches cleaned)**\n\n{}",
        pruned_count,
        merged_branches.len(),
        details.join("\n")
    )
}

/// Reverts the latest landed commit on HEAD via git revert.
pub fn revert_last_landed_commit(repo_root: &Path) -> String {
    let last_sha = run_git(repo_root, &["log", "-1", "--format=%h", "HEAD"]).trim().to_string();
    if last_sha.is_empty() {
        return "No commit found on HEAD to revert.".to_string();
    }
    let revert = Command::new("git")
        .current_dir(repo_root)
        .args(["revert", "--no-edit", &last_sha])
        .output();
    if revert.is_ok_and(|o| o.status.success()) {
        format!("Successfully created revert commit for `{last_sha}`.")
    } else {
        format!("`git revert` failed for `{last_sha}` — resolve merge conflicts manually.")
    }
}

/// Restores an archived branch from its archive/<slug> tag.
pub fn unabandon_branch(repo_root: &Path, slug: &str) -> String {
    let slug = slug.trim().trim_start_matches("archive/");
    if slug.is_empty() {
        return "`/unabandon` needs a branch or tag name (e.g. `/unabandon quark-slug`).".to_string();
    }
    let tag = format!("archive/{slug}");
    let tag_sha = run_git(repo_root, &["rev-parse", "--verify", "-q", &tag]).trim().to_string();
    if tag_sha.is_empty() {
        return format!("No archive tag found matching `{tag}`.");
    }
    let branch_name = if slug.starts_with("quark/") { slug.to_string() } else { format!("quark/{slug}") };
    let res = Command::new("git")
        .current_dir(repo_root)
        .args(["branch", &branch_name, &tag])
        .output();
    if res.is_ok_and(|o| o.status.success()) {
        format!("Restored branch `{branch_name}` at `{tag}` ({tag_sha}).")
    } else {
        format!("Could not create branch `{branch_name}` from `{tag}` — branch may already exist.")
    }
}

/// True if the given directory is inside a valid git working tree.
pub fn is_git_repo(repo_root: &Path) -> bool {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output();
    match out {
        Ok(o) => o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "true",
        Err(_) => false,
    }
}

/// True if HEAD points to a valid commit (repository has at least one commit).
pub fn has_commits(repo_root: &Path) -> bool {
    let out = Command::new("git")
        .current_dir(repo_root)
        .args(["rev-parse", "--verify", "HEAD"])
        .output();
    match out {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Initialize a git repository in `repo_root` if not already initialized, seed a `.gitignore`,
/// and ensure an initial commit exists on `main`.
pub fn init_repository(repo_root: &Path) -> anyhow::Result<String> {
    if !repo_root.exists() {
        std::fs::create_dir_all(repo_root)?;
    }

    let is_repo = is_git_repo(repo_root);
    if !is_repo {
        let init_main = Command::new("git")
            .current_dir(repo_root)
            .args(["init", "-b", "main"])
            .output();
        let init_ok = match init_main {
            Ok(ref o) if o.status.success() => true,
            _ => {
                let init_plain = Command::new("git")
                    .current_dir(repo_root)
                    .arg("init")
                    .output();
                init_plain.is_ok_and(|o| o.status.success())
            }
        };
        if !init_ok {
            anyhow::bail!("`git init` failed in {}", repo_root.display());
        }
    }

    // Ensure .gitignore exists with .hadron/trees/ and .hadron/gluon.lock
    let gitignore_path = repo_root.join(".gitignore");
    if gitignore_path.exists() {
        let existing = std::fs::read_to_string(&gitignore_path).unwrap_or_default();
        let mut to_append = Vec::new();
        if !existing.contains(".hadron/trees") {
            to_append.push(".hadron/trees/");
        }
        if !existing.contains(".hadron/gluon.lock") {
            to_append.push(".hadron/gluon.lock");
        }
        if !to_append.is_empty() {
            let mut updated = existing;
            if !updated.ends_with('\n') && !updated.is_empty() {
                updated.push('\n');
            }
            for item in to_append {
                updated.push_str(item);
                updated.push('\n');
            }
            std::fs::write(&gitignore_path, updated)?;
        }
    } else {
        let content = "# Hadron worktrees and daemon runtime lock\n.hadron/trees/\n.hadron/gluon.lock\n";
        std::fs::write(&gitignore_path, content)?;
    }

    if !has_commits(repo_root) {
        let _ = Command::new("git")
            .current_dir(repo_root)
            .args(["checkout", "-B", "main"])
            .output();

        let _ = Command::new("git")
            .current_dir(repo_root)
            .args(["add", ".gitignore"])
            .output();

        let commit_res = Command::new("git")
            .current_dir(repo_root)
            .env("GIT_AUTHOR_NAME", "hadron")
            .env("GIT_AUTHOR_EMAIL", "hadron@localhost")
            .env("GIT_COMMITTER_NAME", "hadron")
            .env("GIT_COMMITTER_EMAIL", "hadron@localhost")
            .args(["commit", "-m", "chore: initialize repository for Hadron"])
            .output();

        if let Ok(out) = commit_res {
            if !out.status.success() {
                let _ = Command::new("git")
                    .current_dir(repo_root)
                    .env("GIT_AUTHOR_NAME", "hadron")
                    .env("GIT_AUTHOR_EMAIL", "hadron@localhost")
                    .env("GIT_COMMITTER_NAME", "hadron")
                    .env("GIT_COMMITTER_EMAIL", "hadron@localhost")
                    .args(["commit", "--allow-empty", "-m", "chore: initialize repository for Hadron"])
                    .output();
            }
        }
        Ok("Initialized Git repository on branch `main` with `.gitignore` and initial commit.".to_string())
    } else if !is_repo {
        Ok("Initialized Git repository on branch `main` with `.gitignore`.".to_string())
    } else {
        Ok("Git repository is already initialized.".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_repo_root_is_the_parent_of_the_hadron_dir() {
        let field = PathBuf::from("/home/jake/dev/hadron/.hadron/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/home/jake/dev/hadron"));
    }

    #[test]
    fn a_field_outside_a_hadron_dir_is_already_in_the_root() {
        let field = PathBuf::from("/tmp/scratch/field.jsonl");
        assert_eq!(repo_root_of(&field), Path::new("/tmp/scratch"));
    }

    #[test]
    fn format_working_dir_formats_with_tilde_and_trailing_slash() {
        let field = PathBuf::from(".hadron/field.jsonl");
        let formatted = format_working_dir(&field);
        assert!(formatted.ends_with('/'));
        if let Ok(home) = std::env::var("HOME") {
            let current = std::env::current_dir().unwrap();
            if current.starts_with(&home) {
                assert!(formatted.starts_with("~/"));
            }
        }
    }


    #[test]
    fn parse_branches_flags_current_and_merged() {
        let raw = "\
main abc1234
quark/acp-claude-2/01K feed00d
quark/acp-agy/01K dead000";
        let merged: std::collections::HashSet<String> =
            ["main".to_string(), "quark/acp-agy/01K".to_string()].into_iter().collect();
        let branches = parse_branches(raw, "quark/acp-claude-2/01K", &merged);

        assert_eq!(branches.len(), 3);
        assert_eq!(branches[0], BranchInfo {
            name: "main".into(), head: "abc1234".into(), is_current: false, merged: true,
        });
        assert_eq!(branches[1], BranchInfo {
            name: "quark/acp-claude-2/01K".into(), head: "feed00d".into(), is_current: true, merged: false,
        });
        assert_eq!(branches[2], BranchInfo {
            name: "quark/acp-agy/01K".into(), head: "dead000".into(), is_current: false, merged: true,
        });
    }

    #[test]
    fn parse_worktrees_splits_blank_line_separated_blocks() {
        let raw = "\
worktree /home/jake/dev/hadron
HEAD f33de6e1234567890
branch refs/heads/main

worktree /home/jake/dev/hadron/.hadron/trees/acp-claude-2
HEAD abcdef0123456789
branch refs/heads/quark/acp-claude-2/01K

worktree /home/jake/dev/hadron/.hadron/trees/detached-scratch
HEAD 0011223344556677
detached
";
        let worktrees = parse_worktrees(raw);
        assert_eq!(worktrees.len(), 3);
        assert_eq!(worktrees[0], WorktreeInfo {
            path: "/home/jake/dev/hadron".into(), head: "f33de6e1".into(), branch: Some("main".into()),
        });
        assert_eq!(worktrees[1], WorktreeInfo {
            path: "/home/jake/dev/hadron/.hadron/trees/acp-claude-2".into(),
            head: "abcdef01".into(),
            branch: Some("quark/acp-claude-2/01K".into()),
        });
        assert_eq!(worktrees[2], WorktreeInfo {
            path: "/home/jake/dev/hadron/.hadron/trees/detached-scratch".into(),
            head: "00112233".into(),
            branch: None,
        });
    }

    #[test]
    fn parse_worktrees_of_empty_input_is_empty() {
        assert_eq!(parse_worktrees(""), Vec::new());
    }

    /// `/abandon`'s git plumbing, run against a REAL repo — not just parsed
    /// fixtures — because it drives three destructive-adjacent git subcommands
    /// (`checkout --detach`, `tag`, `branch -d`/`-D`) and rule 1 asks for a caller
    /// that actually executes, not just a parser that compiles.
    mod abandon {
        use super::*;
        use std::process::Command;

        fn git_id(dir: &Path, args: &[&str]) -> std::process::Output {
            Command::new("git")
                .current_dir(dir)
                .env("GIT_AUTHOR_NAME", "hadron-test")
                .env("GIT_AUTHOR_EMAIL", "hadron-test@localhost")
                .env("GIT_COMMITTER_NAME", "hadron-test")
                .env("GIT_COMMITTER_EMAIL", "hadron-test@localhost")
                .args(args)
                .output()
                .expect("git must run")
        }

        /// A repo with one commit on `main` and a quark worktree one unmerged
        /// commit ahead of it on `quark/testq/01ABC` — the exact shape `/abandon`
        /// is meant to act on.
        fn repo_with_unmerged_quark_branch() -> (tempfile::TempDir, PathBuf, PathBuf, String) {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            assert!(git_id(&root, &["init", "-q", "-b", "main"]).status.success());
            std::fs::write(root.join("f.txt"), "x\n").unwrap();
            assert!(git_id(&root, &["add", "."]).status.success());
            assert!(git_id(&root, &["commit", "-q", "-m", "init"]).status.success());

            let branch = "quark/testq/01ABC".to_string();
            let wt = root.join(".hadron").join("trees").join("testq");
            assert!(git_id(&root, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()])
                .status
                .success());
            assert!(git_id(&wt, &["checkout", "-q", "-b", &branch]).status.success());
            std::fs::write(wt.join("g.txt"), "y\n").unwrap();
            assert!(git_id(&wt, &["add", "."]).status.success());
            assert!(git_id(&wt, &["commit", "-q", "-m", "unmerged work"]).status.success());

            (dir, root, wt, branch)
        }

        #[test]
        fn an_unconfirmed_abandon_tags_and_refuses_to_delete() {
            let (_dir, root, wt, branch) = repo_with_unmerged_quark_branch();
            let msg = abandon_branch(&root, &wt, &branch, false);
            assert!(msg.contains("unmerged commits"), "{msg}");
            assert!(msg.contains("tagged"), "{msg}");

            let tags = run_git(&root, &["tag", "-l", "archive/testq-01ABC"]);
            assert!(tags.contains("archive/testq-01ABC"), "tag not created: {tags:?}");
            let branches = run_git(&root, &["branch", "--list", &branch]);
            assert!(branches.contains("testq"), "branch was deleted without confirm: {branches:?}");
        }

        /// The live incident this guards: `abandon_branch` used to detach the
        /// worktree and never re-attach it when `-d` was refused, so the daemon
        /// permanently refused to dispatch that quark — "all quarks stuck" from a
        /// single unconfirmed `/abandon` on an unmerged branch.
        #[test]
        fn a_refused_delete_reattaches_the_worktree_to_the_branch() {
            let (_dir, root, wt, branch) = repo_with_unmerged_quark_branch();
            let msg = abandon_branch(&root, &wt, &branch, false);
            assert!(
                !msg.contains("could not be re-attached"),
                "reattach itself should not fail here: {msg}"
            );
            assert_eq!(
                hadron_gluon::worktree::current_branch(&wt),
                Some(branch),
                "worktree must end back on the branch it started on, not detached"
            );
        }

        #[test]
        fn a_confirmed_abandon_force_deletes_and_the_tag_survives() {
            let (_dir, root, wt, branch) = repo_with_unmerged_quark_branch();
            let first = abandon_branch(&root, &wt, &branch, false);
            assert!(first.contains("unmerged"), "{first}");

            let second = abandon_branch(&root, &wt, &branch, true);
            assert!(second.contains("force-abandoned"), "{second}");

            let branches = run_git(&root, &["branch", "--list", &branch]);
            assert!(branches.trim().is_empty(), "branch should be gone: {branches:?}");
            let tags = run_git(&root, &["tag", "-l", "archive/testq-01ABC"]);
            assert!(tags.contains("archive/testq-01ABC"), "archive tag must survive -D");
        }

        #[test]
        fn an_already_merged_branch_deletes_on_the_first_call() {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().to_path_buf();
            assert!(git_id(&root, &["init", "-q", "-b", "main"]).status.success());
            std::fs::write(root.join("f.txt"), "x\n").unwrap();
            assert!(git_id(&root, &["add", "."]).status.success());
            assert!(git_id(&root, &["commit", "-q", "-m", "init"]).status.success());

            let branch = "quark/testq/01MERGED".to_string();
            let wt = root.join(".hadron").join("trees").join("testq");
            assert!(git_id(&root, &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()])
                .status
                .success());
            // Branch cut but never advanced past `main` — trivially "merged".
            assert!(git_id(&wt, &["checkout", "-q", "-b", &branch]).status.success());

            let msg = abandon_branch(&root, &wt, &branch, false);
            assert!(msg.contains("already merged"), "{msg}");
            let branches = run_git(&root, &["branch", "--list", &branch]);
            assert!(branches.trim().is_empty(), "a merged branch should go on the first call: {branches:?}");
        }

        #[test]
        fn resolution_survives_the_worktree_going_detached_between_calls() {
            let (_dir, root, wt, branch) = repo_with_unmerged_quark_branch();
            assert_eq!(quark_branch_to_abandon(&root, &wt, "testq"), Ok(branch.clone()));

            // Simulate the unconfirmed call's detach step directly, without going
            // through `abandon_branch`, to isolate the resolution fallback.
            assert!(git_id(&wt, &["checkout", "--detach", "-q"]).status.success());
            assert_eq!(
                quark_branch_to_abandon(&root, &wt, "testq"),
                Ok(branch),
                "must fall back to the surviving quark/testq/* ref once detached"
            );
        }
    }

    #[test]
    fn parse_rail_lanes_extracts_segments_and_nodes() {
        let (lanes, node_col) = parse_rail_lanes("* | |");
        assert_eq!(node_col, Some(0));
        assert_eq!(lanes, vec![LaneSeg { from_col: 0, to_col: 0 }, LaneSeg { from_col: 1, to_col: 1 }, LaneSeg { from_col: 2, to_col: 2 }]);

        let (lanes_conn, node_col_conn) = parse_rail_lanes("|/");
        assert_eq!(node_col_conn, None);
        assert_eq!(lanes_conn, vec![LaneSeg { from_col: 0, to_col: 0 }, LaneSeg { from_col: 1, to_col: 0 }]);
    }

    #[test]
    fn parse_graph_reads_commit_and_connector_rows() {
        let raw = "\
* 3aee5bb|3aee5bb1234567890abcdef1234567890abcdef|p1|Jake|2 hours ago|HEAD -> main, origin/main|fix the gate
| * abc1234|abc1234567890abcdef1234567890abcdef1234|p2|Alice|1 hour ago|quark/acp-claude/01K|wip on branch
|/
* def5678|def5678567890abcdef1234567890abcdef1234|p3|Bob|3 hours ago||earlier commit";
        let rows = parse_graph(raw);
        assert_eq!(rows.len(), 4);

        assert_eq!(rows[0], GraphRow {
            rail: "*".into(),
            hash: Some("3aee5bb".into()),
            full_hash: Some("3aee5bb1234567890abcdef1234567890abcdef".into()),
            parents: vec!["p1".into()],
            author: Some("Jake".into()),
            relative_date: Some("2 hours ago".into()),
            decorations: vec![
                RefDecoration { name: "main".into(), kind: RefKind::Head },
                RefDecoration { name: "origin/main".into(), kind: RefKind::RemoteBranch },
            ],
            subject: "fix the gate".into(),
            lanes: vec![LaneSeg { from_col: 0, to_col: 0 }],
            node_col: Some(0),
        });
        // Commit in a non-first lane: rail keeps the leading connectors up to the `*`.
        assert_eq!(rows[1], GraphRow {
            rail: "| *".into(),
            hash: Some("abc1234".into()),
            full_hash: Some("abc1234567890abcdef1234567890abcdef1234".into()),
            parents: vec!["p2".into()],
            author: Some("Alice".into()),
            relative_date: Some("1 hour ago".into()),
            decorations: vec![RefDecoration { name: "quark/acp-claude/01K".into(), kind: RefKind::LocalBranch }],
            subject: "wip on branch".into(),
            lanes: vec![LaneSeg { from_col: 0, to_col: 0 }, LaneSeg { from_col: 1, to_col: 1 }],
            node_col: Some(1),
        });
        // Pure connector row — rail only, no commit.
        assert_eq!(rows[2], GraphRow {
            rail: "|/".into(),
            lanes: vec![LaneSeg { from_col: 0, to_col: 0 }, LaneSeg { from_col: 1, to_col: 0 }],
            node_col: None,
            ..Default::default()
        });
        assert_eq!(rows[3].hash.as_deref(), Some("def5678"));
        assert!(rows[3].decorations.is_empty());
        assert_eq!(rows[3].subject, "earlier commit");
        assert_eq!(rows[3].node_col, Some(0));
    }

    #[test]
    fn parse_graph_keeps_the_lanes_that_continue_past_the_commit() {
        // `* | |` — cutting the rail at the `*` would read the next `|` as the sha.
        let rows = parse_graph("* | | 1234abc|1234abc1234567890abcdef1234567890abcdef|p1 p2|Jake|1 hour ago||a merge point");
        assert_eq!(rows[0], GraphRow {
            rail: "* | |".into(),
            hash: Some("1234abc".into()),
            full_hash: Some("1234abc1234567890abcdef1234567890abcdef".into()),
            parents: vec!["p1".into(), "p2".into()],
            author: Some("Jake".into()),
            relative_date: Some("1 hour ago".into()),
            decorations: vec![],
            subject: "a merge point".into(),
            lanes: vec![LaneSeg { from_col: 0, to_col: 0 }, LaneSeg { from_col: 1, to_col: 1 }, LaneSeg { from_col: 2, to_col: 2 }],
            node_col: Some(0),
        });
    }

    #[test]
    fn parse_graph_rich_metadata() {
        let raw = "\
* 3aee5bb|3aee5bb1234567890abcdef1234567890abcdef|abc1234 def5678|Jane Doe|2 hours ago|HEAD -> main, tag: v1.0.0, origin/main, quark/cli-agy/01KY8CCE|feat: rich graph
| * c1d2e3f|c1d2e3f4567890abcdef1234567890abcdef12|parent1|John Smith|3 days ago||fix: bug (Task 3)
|/";
        let rows = parse_graph(raw);
        assert_eq!(rows.len(), 3);

        let r0 = &rows[0];
        assert_eq!(r0.rail, "*");
        assert_eq!(r0.hash.as_deref(), Some("3aee5bb"));
        assert_eq!(
            r0.full_hash.as_deref(),
            Some("3aee5bb1234567890abcdef1234567890abcdef")
        );
        assert_eq!(r0.parents, vec!["abc1234", "def5678"]);
        assert_eq!(r0.author.as_deref(), Some("Jane Doe"));
        assert_eq!(r0.relative_date.as_deref(), Some("2 hours ago"));
        assert_eq!(r0.subject, "feat: rich graph");
        assert_eq!(
            r0.decorations,
            vec![
                RefDecoration {
                    name: "main".into(),
                    kind: RefKind::Head,
                },
                RefDecoration {
                    name: "v1.0.0".into(),
                    kind: RefKind::Tag,
                },
                RefDecoration {
                    name: "origin/main".into(),
                    kind: RefKind::RemoteBranch,
                },
                RefDecoration {
                    name: "quark/cli-agy/01KY8CCE".into(),
                    kind: RefKind::LocalBranch,
                },
            ]
        );

        let r1 = &rows[1];
        assert_eq!(r1.rail, "| *");
        assert_eq!(r1.hash.as_deref(), Some("c1d2e3f"));
        assert_eq!(
            r1.full_hash.as_deref(),
            Some("c1d2e3f4567890abcdef1234567890abcdef12")
        );
        assert_eq!(r1.parents, vec!["parent1"]);
        assert_eq!(r1.author.as_deref(), Some("John Smith"));
        assert_eq!(r1.relative_date.as_deref(), Some("3 days ago"));
        assert_eq!(r1.subject, "fix: bug (Task 3)");
        assert!(r1.decorations.is_empty());

        let r2 = &rows[2];
        assert_eq!(r2.rail, "|/");
        assert!(r2.hash.is_none());
        assert!(r2.full_hash.is_none());
        assert!(r2.parents.is_empty());
        assert!(r2.author.is_none());
        assert!(r2.relative_date.is_none());
        assert_eq!(r2.subject, "");
        assert!(r2.decorations.is_empty());
    }

    #[test]
    fn test_parse_diff() {
        let raw = "\
diff --git a/crates/ui/src/table/table.rs b/crates/ui/src/table/table.rs
index a1b2c3d..e4f5g6h 100644
--- a/crates/ui/src/table/table.rs
+++ b/crates/ui/src/table/table.rs
@@ -10,3 +10,4 @@
 fn foo() {
-    println!(\"old\");
+    println!(\"new\");
+    println!(\"added\");
 }";
        let files = parse_diff(raw);
        assert_eq!(files.len(), 1);
        let file = &files[0];
        assert_eq!(file.path, "crates/ui/src/table/table.rs");
        assert_eq!(file.added, 2);
        assert_eq!(file.removed, 1);
        assert_eq!(file.hunks.len(), 1);

        let hunk = &file.hunks[0];
        assert_eq!(hunk.header, "@@ -10,3 +10,4 @@");
        assert_eq!(hunk.lines.len(), 5);
        assert_eq!(hunk.lines[0], DiffLine::Context("fn foo() {".to_string()));
        assert_eq!(
            hunk.lines[1],
            DiffLine::Removed("    println!(\"old\");".to_string())
        );
        assert_eq!(
            hunk.lines[2],
            DiffLine::Added("    println!(\"new\");".to_string())
        );
        assert_eq!(
            hunk.lines[3],
            DiffLine::Added("    println!(\"added\");".to_string())
        );
    }

    #[test]
    fn commit_diff_parses_patch() {
        let raw_patch = "\
commit 3aee5bb1234567890abcdef1234567890abcdef
Author: Jake <jake@orch.dev>
Date:   Thu Jul 23 22:00:00 2026 -0400

    feat: example commit

diff --git a/src/lib.rs b/src/lib.rs
index a1b2c3d..e4f5g6h 100644
--- a/src/lib.rs
+++ b/src/lib.rs
@@ -1,2 +1,3 @@
 context
-old
+new
";
        let parsed = parse_diff(raw_patch);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].path, "src/lib.rs");

        let diffs = commit_diff(Path::new("."), "HEAD");
        assert!(diffs.is_some());
    }

    #[test]
    fn test_revert_and_unabandon() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let run_git_cmd = |args: &[&str]| {
            Command::new("git")
                .current_dir(root)
                .env("GIT_AUTHOR_NAME", "Test")
                .env("GIT_AUTHOR_EMAIL", "test@example.com")
                .env("GIT_COMMITTER_NAME", "Test")
                .env("GIT_COMMITTER_EMAIL", "test@example.com")
                .args(args)
                .output()
                .unwrap()
        };
        assert!(run_git_cmd(&["init", "-q", "-b", "main"]).status.success());
        assert!(run_git_cmd(&["config", "user.name", "Test"]).status.success());
        assert!(run_git_cmd(&["config", "user.email", "test@example.com"]).status.success());
        std::fs::write(root.join("file1.txt"), "1\n").unwrap();
        assert!(run_git_cmd(&["add", "."]).status.success());
        assert!(run_git_cmd(&["commit", "-q", "-m", "init"]).status.success());
        std::fs::write(root.join("file2.txt"), "2\n").unwrap();
        assert!(run_git_cmd(&["add", "."]).status.success());
        assert!(run_git_cmd(&["commit", "-q", "-m", "second"]).status.success());

        let res_revert = revert_last_landed_commit(root);
        assert!(res_revert.contains("Successfully created revert commit"), "res_revert: {res_revert}");

        let res_unabandon = unabandon_branch(root, "nonexistent-slug");
        assert!(res_unabandon.contains("No archive tag found"));
    }

    #[test]
    fn test_commit_message() {
        let msg = commit_message(Path::new("."), "HEAD");
        assert!(msg.is_some());
        assert!(!msg.unwrap().is_empty());
    }

    #[test]
    fn test_is_git_repo_and_init_repository() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Non-git directory initially
        assert!(!is_git_repo(root));
        assert!(!has_commits(root));

        // Initialize repository
        let res = init_repository(root).unwrap();
        assert!(res.contains("Initialized Git repository"), "res: {res}");
        assert!(is_git_repo(root));
        assert!(has_commits(root));

        // .gitignore created and contains .hadron/trees/
        let gitignore = std::fs::read_to_string(root.join(".gitignore")).unwrap();
        assert!(gitignore.contains(".hadron/trees/"));
        assert!(gitignore.contains(".hadron/gluon.lock"));

        // Idempotent re-initialization
        let res2 = init_repository(root).unwrap();
        assert!(res2.contains("already initialized"), "res2: {res2}");
    }
}


