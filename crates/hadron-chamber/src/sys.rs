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

/// Lists files in the workspace using `git ls-files`.
pub fn list_workspace_files(repo_root: &Path) -> Vec<String> {
    if let Ok(output) = Command::new("git")
        .arg("ls-files")
        .current_dir(repo_root)
        .output()
    {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(String::from)
                .collect();
        }
    }
    vec![]
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
        assert_eq!(files, vec!["test.txt"]);

        let content = read_workspace_file(root, "test.txt");
        assert_eq!(content, Some("hello world".to_string()));
    }
}
