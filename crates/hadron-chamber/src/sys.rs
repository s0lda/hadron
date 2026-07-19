use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{ChildStdin, Command, Stdio};

pub struct Terminal {
    stdin: ChildStdin,
    stdout_reader: BufReader<std::process::ChildStdout>,
    pub cwd: String,
}

impl Terminal {
    pub fn new(repo_root: &Path) -> Result<Self, String> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "sh".to_string());

        let mut child = Command::new("sh")
            .arg("-c")
            .arg(format!("exec {} 2>&1", shell))
            .current_dir(repo_root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn shell: {}", e))?;

        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        let mut term = Self {
            stdin,
            stdout_reader: BufReader::new(stdout),
            cwd: String::new(),
        };

        if let Ok(_) = term.execute("true") {
            // cwd is initialized during execute
        } else {
            term.cwd = repo_root.to_string_lossy().into_owned();
        }

        Ok(term)
    }

    pub fn execute(&mut self, cmd: &str) -> Result<String, String> {
        let marker = "___HADRON_CMD_DONE_98765___";
        let script = format!("{}; echo \"{}\"; pwd; echo \"{}\"\n", cmd, marker, marker);

        if let Err(e) = self.stdin.write_all(script.as_bytes()) {
            return Err(format!("Failed to write to terminal: {}", e));
        }
        if let Err(e) = self.stdin.flush() {
            return Err(format!("Failed to flush terminal: {}", e));
        }

        let mut output = String::new();
        let mut cwd = String::new();
        let mut phase = 0;
        loop {
            let mut line = String::new();
            match self.stdout_reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if line.contains(marker) {
                        phase += 1;
                        if phase == 2 {
                            break;
                        }
                        continue;
                    }
                    if phase == 0 {
                        output.push_str(&line);
                    } else if phase == 1 {
                        cwd.push_str(&line);
                    }
                }
                Err(e) => return Err(format!("Read error: {}", e)),
            }
        }

        self.cwd = cwd.trim().to_string();
        Ok(output.trim().to_string())
    }
}

/// Lists the files in the workspace *as they are on disk*, honouring `.gitignore`.
///
/// `git ls-files` alone lists the **index**, which is not what a file tree means:
/// a file deleted from the working tree is still in the index (so it would keep
/// showing), and a new untracked file is not in it at all (so it would never
/// appear). `--cached --others --exclude-standard` unions tracked and untracked,
/// and the `exists()` filter drops anything that is only in the index.
/// Each returned entry is `(path, is_ignored)`. Non-ignored (tracked or untracked)
/// files are listed individually; gitignored entries are unioned in with `--directory`
/// so a **wholly-ignored directory collapses to one entry** (e.g. `target/`) instead of
/// every file inside it. That collapse is not cosmetic: in this workspace the raw ignored
/// listing is ~100k files (all of `target/`, the vendored `gpui-component/`, venvs), which
/// would swamp both the tree and the `@`-mention index. A collapsed directory keeps its
/// trailing `/` so the tree can render it as an (empty, muted) folder rather than a file.
pub fn list_workspace_files(repo_root: &Path) -> Vec<(String, bool)> {
    let mut files: Vec<(String, bool)> = Vec::new();

    // Tracked ∪ untracked, minus ignored — the real, editable files.
    if let Ok(output) = Command::new("git")
        .args([
            "ls-files",
            "--cached",
            "--others",
            "--exclude-standard",
            "--deduplicate",
        ])
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                if repo_root.join(line).exists() {
                    files.push((line.to_string(), false));
                }
            }
        }
    }

    // Ignored entries, with wholly-ignored directories collapsed to a single entry.
    if let Ok(output) = Command::new("git")
        .args([
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
            "--deduplicate",
        ])
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                // `--directory` never emits `.git/`, but guard anyway; `exists()` drops a
                // stale entry. A collapsed dir keeps its trailing `/` (checked verbatim).
                let bare = line.trim_end_matches('/');
                if !bare.is_empty() && !line.starts_with(".git/") && repo_root.join(bare).exists() {
                    files.push((line.to_string(), true));
                }
            }
        }
    }

    files.sort_by(|a, b| a.0.cmp(&b.0));
    files.dedup_by(|a, b| a.0 == b.0);
    files
}

/// One node of the File Tree the right-rail renders. Built by folding the flat
/// `(path, is_ignored)` list from [`list_workspace_files`] into a nested tree via
/// [`FileTreeNode::insert`]. Pure data (no gpui) so the tree-building logic is unit
/// testable; the chamber's `render_node` walks it to paint rows.
#[derive(Default, Debug)]
pub struct FileTreeNode {
    pub children: std::collections::BTreeMap<String, FileTreeNode>,
    pub is_file: bool,
    pub is_ignored: bool,
    /// The node's path from the tree root (e.g. `"src/app/render.rs"`). Set on
    /// EVERY node, interior folders included — an empty `full_path` on a folder
    /// made every folder row share the gpui id `tree-row-`, colliding so the
    /// expand click mis-routed (folders "wouldn't open") and folder context menus
    /// pointed at an empty path.
    pub full_path: String,
}

impl FileTreeNode {
    /// `is_dir_leaf` marks a path that is itself a directory (a collapsed
    /// gitignored dir, kept with a trailing `/` by [`list_workspace_files`]) — its
    /// last component is a folder, not a file. Interior directories start
    /// un-ignored; [`FileTreeNode::resolve_ignores`] computes their flag from their
    /// children afterwards.
    pub fn insert(&mut self, path: &str, is_ignored: bool, is_dir_leaf: bool) {
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() {
            return;
        }
        let mut current = self;
        let mut running = String::new();
        for (i, part) in parts.iter().enumerate() {
            let last = i == parts.len() - 1;
            let is_file = last && !is_dir_leaf;
            if running.is_empty() {
                running = part.to_string();
            } else {
                running = format!("{running}/{part}");
            }
            current = current.children.entry(part.to_string()).or_default();
            // EVERY node gets its running path — not just the leaf (see `full_path`).
            if current.full_path.is_empty() {
                current.full_path = running.clone();
            }
            if last {
                current.is_file = is_file;
                current.is_ignored = is_ignored;
            }
        }
    }

    /// Bottom-up: a file/collapsed-dir keeps its own flag; a directory with
    /// children is ignored only when **every** child is. Returns this node's
    /// resolved ignored state so the parent can fold it in.
    pub fn resolve_ignores(&mut self) -> bool {
        if self.is_file || self.children.is_empty() {
            return self.is_ignored;
        }
        let mut all_ignored = true;
        for child in self.children.values_mut() {
            if !child.resolve_ignores() {
                all_ignored = false;
            }
        }
        self.is_ignored = all_ignored;
        all_ignored
    }
}

/// Children of a node, folders first then files, each group name-sorted — the
/// order the File Tree paints rows in.
pub fn sorted_children(node: &FileTreeNode) -> Vec<(&String, &FileTreeNode)> {
    let mut children: Vec<(&String, &FileTreeNode)> = node.children.iter().collect();
    children.sort_by(|(a_name, a), (b_name, b)| match (a.is_file, b.is_file) {
        (false, true) => std::cmp::Ordering::Less,
        (true, false) => std::cmp::Ordering::Greater,
        _ => a_name.cmp(b_name),
    });
    children
}

/// Reads the contents of a workspace file.
pub fn read_workspace_file(repo_root: &Path, file_path: &str) -> Option<String> {
    let full_path = repo_root.join(file_path);
    if let (Ok(canon_root), Ok(canon_full)) = (repo_root.canonicalize(), full_path.canonicalize()) {
        if canon_full.starts_with(canon_root) {
            std::fs::read_to_string(canon_full).ok()
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn terminal_execution_is_stateful() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let mut term = Terminal::new(root).unwrap();
        let _ = term.execute("mkdir test_dir");
        let _ = term.execute("cd test_dir");
        let pwd_res = term.execute("pwd");
        assert!(pwd_res.is_ok());
        assert!(pwd_res.unwrap().contains("test_dir"));
    }

    #[test]
    fn file_tree_listing_and_opening_work() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .unwrap();

        let test_file = "test.txt";
        fs::write(root.join(test_file), "hello world").unwrap();

        Command::new("git")
            .args(["add", test_file])
            .current_dir(root)
            .output()
            .unwrap();

        let files = list_workspace_files(root);
        assert_eq!(files, vec![("test.txt".to_string(), false)]);

        let content = read_workspace_file(root, "test.txt");
        assert_eq!(content, Some("hello world".to_string()));
    }

    /// The file tree is a view of the **disk**, not of git's index. Jake deleted
    /// two screenshots and added five; the tree kept showing the deleted ones and
    /// never showed the new ones, because `git ls-files` reports the index.
    ///
    /// Gitignored files are now surfaced too (rendered muted), flagged `is_ignored = true`,
    /// so this also asserts they appear with the right flag rather than being dropped.
    #[test]
    fn the_file_tree_shows_the_disk_not_the_index() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        Command::new("git")
            .arg("init")
            .current_dir(root)
            .output()
            .unwrap();

        // Tracked and staged, then deleted from the working tree: still in the
        // index, gone from disk — it must NOT be listed.
        fs::write(root.join("deleted.png"), "old").unwrap();
        Command::new("git")
            .args(["add", "deleted.png"])
            .current_dir(root)
            .output()
            .unwrap();
        fs::remove_file(root.join("deleted.png")).unwrap();

        // Never added to git: it must be listed anyway.
        fs::write(root.join("brand-new.png"), "new").unwrap();

        // Ignored: it is now surfaced (muted), flagged is_ignored = true.
        fs::write(root.join(".gitignore"), "ignored.tmp\n").unwrap();
        fs::write(root.join("ignored.tmp"), "noise").unwrap();

        let files = list_workspace_files(root);
        assert!(
            files.contains(&("brand-new.png".to_string(), false)),
            "an untracked file on disk must appear in the tree (not ignored), got {files:?}"
        );
        assert!(
            !files.iter().any(|(p, _)| p == "deleted.png"),
            "a file deleted from disk must not linger because it is still in the index, got {files:?}"
        );
        assert!(
            files.contains(&("ignored.tmp".to_string(), true)),
            "a gitignored file must now appear flagged as ignored, got {files:?}"
        );
    }

    #[test]
    fn insert_builds_folder_nodes_with_children_and_full_paths() {
        // The user's case: an untracked file arrives as its full path (git lists
        // untracked files individually, not collapsed). The folder node MUST exist
        // and MUST carry the child — otherwise expanding it shows nothing.
        let mut root = FileTreeNode::default();
        root.insert("newdir/newfile.txt", false, false);

        let dir = root
            .children
            .get("newdir")
            .expect("intermediate folder node must be created");
        assert!(!dir.is_file, "a folder node is not a file");
        assert_eq!(dir.full_path, "newdir", "folder full_path is its running path");

        let file = dir
            .children
            .get("newfile.txt")
            .expect("the folder must carry its child — this is what expand renders");
        assert!(file.is_file);
        assert_eq!(file.full_path, "newdir/newfile.txt");
    }

    #[test]
    fn every_folder_node_gets_a_distinct_nonempty_full_path() {
        // Regression guard for the id-collision bug: a leaf-only full_path left
        // every folder row sharing the gpui id `tree-row-`.
        let mut root = FileTreeNode::default();
        root.insert("src/app/render.rs", false, false);
        root.insert("crates/lib.rs", false, false);

        let src = &root.children["src"];
        let app = &src.children["app"];
        let crates = &root.children["crates"];
        assert_eq!(src.full_path, "src");
        assert_eq!(app.full_path, "src/app");
        assert_eq!(crates.full_path, "crates");
        // Distinct, non-empty ids for every folder — the collision is impossible now.
        for p in [&src.full_path, &app.full_path, &crates.full_path] {
            assert!(!p.is_empty());
        }
        assert_ne!(src.full_path, crates.full_path);
    }

    #[test]
    fn collapsed_ignored_dir_is_a_childless_leaf() {
        // A wholly-ignored dir arrives collapsed with a trailing slash (is_dir_leaf).
        // It is a folder with no children — correctly nothing to expand.
        let mut root = FileTreeNode::default();
        root.insert("target/", true, true);
        let node = &root.children["target"];
        assert!(!node.is_file);
        assert!(node.is_ignored);
        assert!(node.children.is_empty(), "collapsed dir has no children");
        assert_eq!(node.full_path, "target");
    }

    #[test]
    fn resolve_ignores_marks_a_folder_ignored_only_when_all_children_are() {
        let mut root = FileTreeNode::default();
        root.insert("mixed/tracked.rs", false, false);
        root.insert("mixed/ignored.tmp", true, false);
        root.insert("allignored/a.tmp", true, false);
        root.resolve_ignores();

        assert!(
            !root.children["mixed"].is_ignored,
            "a folder with one tracked child is not ignored"
        );
        assert!(
            root.children["allignored"].is_ignored,
            "a folder whose every child is ignored folds to ignored"
        );
    }

    #[test]
    fn sorted_children_puts_folders_before_files_then_sorts_by_name() {
        let mut root = FileTreeNode::default();
        root.insert("zeta.rs", false, false); // file
        root.insert("alpha/a.rs", false, false); // folder
        root.insert("beta.rs", false, false); // file
        let order: Vec<&str> = sorted_children(&root)
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(order, vec!["alpha", "beta.rs", "zeta.rs"]);
    }

    #[test]
    fn read_workspace_file_prevents_directory_traversal() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let parent = root.parent().unwrap();
        let sibling = parent.join("sibling_dir");
        fs::create_dir_all(&sibling).unwrap();
        fs::write(sibling.join("secret.txt"), "secret data").unwrap();

        // Try reading a file that is outside repo_root using traversal
        let content = read_workspace_file(root, "../sibling_dir/secret.txt");
        assert_eq!(content, None);
    }
}
