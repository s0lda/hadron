//! Preons — a named voice a human can address by `@name` instead of by role or
//! quark id.
//!
//! A preon is authored as a `.md` file with front-matter (`name:`, optional
//! `preferred_role:`) plus a body, loaded from `~/.hadron/preons` and
//! `<repo>/.hadron/preons` — the same shape, and the same loader pattern, as
//! [`crate::skills::load_skills`]. This module only loads and merges the files;
//! routing `@preon-name` to a seat via its `preferred_role` is a separate
//! concern (the router), not this one.

use std::fs;
use std::path::{Path, PathBuf};

use hadron_lattice::term::{self, Source};

use crate::skills::{front_matter_value, split_front_matter};

pub mod synth;
pub use synth::*;

/// One loaded preon: a name a human can address, an optional role it prefers to
/// route through, and the body a quark speaking as this preon is handed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preon {
    /// The merge key and the `@name` a human types. Required — a file with no
    /// `name:` front-matter has nothing to key on and is skipped (see
    /// [`load_dir`]).
    pub name: String,
    /// The role this preon prefers to route through (spec §4). Optional: a
    /// preon with no preferred role still loads, it just has nothing for the
    /// router to resolve against yet.
    pub preferred_role: Option<String>,
    /// The text after the front-matter block, trimmed.
    pub body: String,
}

/// Load preons from disk and merge them by name.
///
/// Walks `global_dir` then `repo_dir` (each `None` is simply skipped — a missing
/// directory is not an error), reading every `*.md` file in each (sorted by
/// filename within a directory, for determinism). Each file is upserted into the
/// corpus **by name**: a later source replaces an earlier one with the same name,
/// so the precedence is `global < repo` — a repo preon wins over a global
/// preon with the same name.
///
/// With `global_dir: None, repo_dir: None` this returns `[]` — no directory is
/// walked, so behaviour with no preons installed is unchanged (back-compat with
/// today's routing).
pub fn load_preons(global_dir: Option<&Path>, repo_dir: Option<&Path>) -> Vec<Preon> {
    let mut preons: Vec<Preon> = Vec::new();

    for dir in [global_dir, repo_dir].into_iter().flatten() {
        for loaded in load_dir(dir) {
            upsert(&mut preons, loaded);
        }
    }

    preons
}

/// Load roles from disk and merge them by name.
pub fn load_roles(global_dir: Option<&Path>, repo_dir: Option<&Path>) -> Vec<Preon> {
    let mut roles: Vec<Preon> = Vec::new();

    for dir in [global_dir, repo_dir].into_iter().flatten() {
        for loaded in load_dir(dir) {
            upsert(&mut roles, loaded);
        }
    }

    roles
}

/// Insert `preon`, replacing any existing entry with the same `name` in place
/// (so later sources keep their position relative to unrelated preons — only
/// the overridden name's content changes, not the corpus order).
fn upsert(preons: &mut Vec<Preon>, preon: Preon) {
    if let Some(existing) = preons.iter_mut().find(|p| p.name == preon.name) {
        *existing = preon;
    } else {
        preons.push(preon);
    }
}

/// Read every `*.md` file directly under `dir` (non-recursive) as a candidate
/// preon. A missing or unreadable directory yields no preons, silently — the
/// caller passes `None` for "not configured" and an absent `~/.hadron/preons` on a
/// machine that has never used preons is the same case, not an error.
fn load_dir(dir: &Path) -> Vec<Preon> {
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
                // mistake should be visible, not a preon that never loads with no
                // clue.
                Err(e) => {
                    term::warn(Source::Gluon, &format!("skipping {} — could not read it: {e}", path.display()));
                    return None;
                }
            };
            match parse_preon_file(&text) {
                Some(preon) => Some(preon),
                None => {
                    // A preon file with no `name:` front-matter has no merge
                    // key — rather than guess one from the filename (which would
                    // make the name depend on how the file happens to be named,
                    // not on what its author declared), it is skipped. Silent
                    // would be worse: a typo'd or missing `name:` should be
                    // visible, not a preon that silently never loads.
                    term::warn(
                        Source::Gluon,
                        &format!("skipping preon file {} — missing required `name:` front-matter", path.display()),
                    );
                    None
                }
            }
        })
        .collect()
}

/// Parse one preon `.md` file's front-matter + body into a [`Preon`]. `None`
/// when there is no front-matter block at all, or the block has no `name:` —
/// both cases the caller reports as "skipped: missing `name:`".
fn parse_preon_file(text: &str) -> Option<Preon> {
    let (front, body) = split_front_matter(text);
    let front = front?;

    let name = front_matter_value(front, "name")?.to_string();
    let preferred_role = front_matter_value(front, "preferred_role").map(str::to_string);

    Some(Preon {
        name,
        preferred_role,
        body: body.trim().to_string(),
    })
}

/// Manager providing runtime preon hot-swapping and dynamic prompt compilation
/// for active quark seats without process restarts.
#[derive(Debug, Clone)]
pub struct PreonHotSwapManager {
    registry: hadron_lattice::PreonRegistry,
    global_dir: Option<PathBuf>,
    repo_dir: Option<PathBuf>,
}

impl PreonHotSwapManager {
    pub fn new(global_dir: Option<&Path>, repo_dir: Option<&Path>) -> Self {
        let mut manager = Self {
            registry: hadron_lattice::PreonRegistry::new(),
            global_dir: global_dir.map(Path::to_path_buf),
            repo_dir: repo_dir.map(Path::to_path_buf),
        };
        manager.reload_from_disk();
        manager
    }

    /// Hot-reload preons from disk without daemon restart.
    pub fn reload_from_disk(&mut self) -> usize {
        let loaded = load_preons(
            self.global_dir.as_deref(),
            self.repo_dir.as_deref(),
        );
        let count = loaded.len();
        for preon in loaded {
            self.registry.register_preon(
                &preon.name,
                &preon.body,
                preon.preferred_role.as_deref(),
            );
        }
        count
    }

    /// Attach a registered preon to a specific quark seat.
    pub fn attach_preon(&mut self, quark_id: &str, preon_name: &str) -> Result<(), String> {
        self.registry.attach_to_seat(quark_id, preon_name)
    }

    /// Detach a preon from a specific quark seat.
    pub fn detach_preon(&mut self, quark_id: &str, preon_name: &str) -> bool {
        self.registry.detach_from_seat(quark_id, preon_name)
    }

    /// Inject an ephemeral specialist preon directly into a quark seat.
    pub fn inject_ephemeral(&mut self, quark_id: &str, role: &str, task: &str) {
        let preon = synth::synthesize_ephemeral_preon(role, task);
        self.registry.register_preon(
            &preon.name,
            &preon.body,
            preon.preferred_role.as_deref(),
        );
        let _ = self.registry.attach_to_seat(quark_id, &preon.name);
    }

    /// Compile runtime prompt for a turn with all attached preons.
    pub fn compile_prompt(&self, quark_id: &str, base_prompt: &str) -> String {
        self.registry.render_seat_prompt(quark_id, base_prompt)
    }

    pub fn registry(&self) -> &hadron_lattice::PreonRegistry {
        &self.registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_preon(dir: &Path, filename: &str, contents: &str) {
        std::fs::write(dir.join(filename), contents).unwrap();
    }

    #[test]
    fn load_preons_none_is_empty() {
        assert_eq!(load_preons(None, None), Vec::new());
    }

    #[test]
    fn preon_parses_name_and_preferred_role() {
        let repo = tempfile::tempdir().unwrap();
        write_preon(
            repo.path(),
            "security-reviewer.md",
            "---\nname: security-reviewer\npreferred_role: security\n---\n\nYou review for security issues.\n",
        );

        let loaded = load_preons(None, Some(repo.path()));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "security-reviewer");
        assert_eq!(loaded[0].preferred_role.as_deref(), Some("security"));
    }

    #[test]
    fn repo_preon_overrides_global_by_name() {
        let global = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        write_preon(
            global.path(),
            "reviewer.md",
            "---\nname: reviewer\npreferred_role: global-role\n---\n\nGLOBAL BODY.\n",
        );
        write_preon(
            repo.path(),
            "reviewer.md",
            "---\nname: reviewer\npreferred_role: repo-role\n---\n\nREPO BODY.\n",
        );

        let loaded = load_preons(Some(global.path()), Some(repo.path()));
        assert_eq!(loaded.len(), 1, "override replaces in place, does not add");
        let p = &loaded[0];
        assert_eq!(p.preferred_role.as_deref(), Some("repo-role"), "repo must win over global");
        assert!(p.body.contains("REPO BODY."));
        assert!(!p.body.contains("GLOBAL BODY."));
    }

    #[test]
    fn preon_without_name_is_skipped() {
        let repo = tempfile::tempdir().unwrap();
        write_preon(repo.path(), "nameless.md", "---\npreferred_role: security\n---\n\nBODY.\n");
        write_preon(repo.path(), "no-front-matter.md", "# just a heading\n\nno front matter at all.\n");

        let loaded = load_preons(None, Some(repo.path()));
        assert_eq!(loaded, Vec::new(), "both bad files are skipped, not merged");
    }

    #[test]
    fn preon_body_is_the_text_after_front_matter() {
        let repo = tempfile::tempdir().unwrap();
        write_preon(
            repo.path(),
            "plain.md",
            "---\nname: plain\n---\n\nThis is the body.\nSecond line.\n",
        );

        let loaded = load_preons(None, Some(repo.path()));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].body, "This is the body.\nSecond line.");
    }

    #[test]
    fn preon_with_no_preferred_role_still_loads() {
        let repo = tempfile::tempdir().unwrap();
        write_preon(repo.path(), "roleless.md", "---\nname: roleless\n---\n\nBODY.\n");

        let loaded = load_preons(None, Some(repo.path()));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].preferred_role, None);
    }

    #[test]
    fn a_missing_directory_yields_no_preons() {
        let missing = Path::new("/nonexistent/hadron-preons-dir-that-does-not-exist");
        let loaded = load_preons(Some(missing), Some(missing));
        assert_eq!(loaded, Vec::new());
    }

    #[test]
    fn global_and_repo_distinct_names_both_load() {
        let global = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        write_preon(global.path(), "alpha.md", "---\nname: alpha\n---\n\nALPHA.\n");
        write_preon(repo.path(), "beta.md", "---\nname: beta\n---\n\nBETA.\n");

        let loaded = load_preons(Some(global.path()), Some(repo.path()));
        assert_eq!(loaded.len(), 2);
        assert!(loaded.iter().any(|p| p.name == "alpha"));
        assert!(loaded.iter().any(|p| p.name == "beta"));
    }

    #[test]
    fn load_roles_works() {
        let global = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        write_preon(global.path(), "architect.md", "---\nname: architect\n---\n\nARCHITECT GLOBAL.\n");
        write_preon(repo.path(), "architect.md", "---\nname: architect\n---\n\nARCHITECT REPO.\n");

        let loaded = load_roles(Some(global.path()), Some(repo.path()));
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "architect");
        assert!(loaded[0].body.contains("ARCHITECT REPO."));
    }

    #[test]
    fn test_preon_hot_swap_manager() {
        let repo = tempfile::tempdir().unwrap();
        write_preon(
            repo.path(),
            "fuzz-expert.md",
            "---\nname: fuzz-expert\npreferred_role: Fuzzer\n---\n\nRun cargo fuzz.\n",
        );

        let mut manager = PreonHotSwapManager::new(None, Some(repo.path()));
        manager.attach_preon("quark-worker", "fuzz-expert").unwrap();

        // Ephemeral injection
        manager.inject_ephemeral("quark-worker", "VulkanSpecialist", "Debug swapchain presentation");

        let prompt = manager.compile_prompt("quark-worker", "Base instructions.");
        assert!(prompt.contains("fuzz-expert"));
        assert!(prompt.contains("VulkanSpecialist"));
        assert!(prompt.contains("Lavapipe"));
    }
}
