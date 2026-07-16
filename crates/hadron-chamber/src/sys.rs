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

/// Reads the contents of a workspace file.
pub fn read_workspace_file(repo_root: &Path, file_path: &str) -> Option<String> {
    let full_path = repo_root.join(file_path);
    std::fs::read_to_string(full_path).ok()
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
}
