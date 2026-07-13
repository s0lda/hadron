use std::path::Path;
use std::process::Command;
use hadron_lattice::Mode;

/// Executes a terminal command.
///
/// Under Bypass mode, executes `sh -c <cmd>`.
/// Under Auto mode, execution is also permitted.
/// Under Ask or Write modes, execution is rejected to enforce permission boundaries.
pub fn execute_terminal_command(repo_root: &Path, cmd: &str, mode: Mode) -> Result<String, String> {
    if matches!(mode, Mode::Ask | Mode::Write) {
        return Err("Execution denied: Terminal requires Bypass or Auto mode.".to_string());
    }

    match Command::new("sh").arg("-c").arg(cmd).current_dir(repo_root).output() {
        Ok(cmd_out) => {
            let mut output = String::from_utf8_lossy(&cmd_out.stdout).into_owned();
            let err = String::from_utf8_lossy(&cmd_out.stderr);
            if !err.is_empty() {
                if !output.is_empty() {
                    output.push('\n');
                }
                output.push_str(&err);
            }
            Ok(output.trim_end().to_string())
        }
        Err(e) => Err(format!("Failed to execute command: {}", e)),
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
    fn terminal_execution_is_gated_by_mode() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        
        let ask_res = execute_terminal_command(root, "echo 'hello'", Mode::Ask);
        assert!(ask_res.is_err());
        assert_eq!(ask_res.unwrap_err(), "Execution denied: Terminal requires Bypass or Auto mode.");

        let write_res = execute_terminal_command(root, "echo 'hello'", Mode::Write);
        assert!(write_res.is_err());
        assert_eq!(write_res.unwrap_err(), "Execution denied: Terminal requires Bypass or Auto mode.");

        let bypass_res = execute_terminal_command(root, "echo 'hello'", Mode::Bypass);
        assert!(bypass_res.is_ok());
        assert_eq!(bypass_res.unwrap(), "hello");
    }

    #[test]
    fn file_tree_listing_and_opening_work() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        // Initialize a dummy git repo so `git ls-files` works
        Command::new("git").arg("init").current_dir(root).output().unwrap();
        
        let test_file = "test.txt";
        fs::write(root.join(test_file), "hello world").unwrap();
        
        Command::new("git").args(["add", test_file]).current_dir(root).output().unwrap();

        let files = list_workspace_files(root);
        assert_eq!(files, vec!["test.txt"]);

        let content = read_workspace_file(root, "test.txt");
        assert_eq!(content, Some("hello world".to_string()));
    }
}
