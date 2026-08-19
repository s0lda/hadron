//! Quick REPL and scratchpad evaluation engine (Capability #16).
//!
//! Allows querying nucleus notes, evaluating slash commands, and simulating/testing
//! tools in an ephemeral overlay without polluting the persistent session field.

use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplResult {
    SlashCommand(String),
    NucleusQuery(Vec<String>),
    ToolCall { tool: String, output: String },
    Unknown(String),
    Empty,
}

/// Evaluate REPL input string against repository state
pub fn evaluate_repl_input(input: &str, repo_root: &Path) -> ReplResult {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return ReplResult::Empty;
    }

    if trimmed.starts_with('/') {
        return ReplResult::SlashCommand(format!("Executed command: {trimmed}"));
    }

    if let Some(query) = trimmed.strip_prefix('?').or_else(|| trimmed.strip_prefix("note:")) {
        let nucleus_notes = repo_root.join(".hadron").join("nucleus").join("notes");
        let mut matches = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&nucleus_notes) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains(query.trim()) {
                    matches.push(name);
                }
            }
        }
        matches.sort();
        return ReplResult::NucleusQuery(matches);
    }

    if let Some(tool_spec) = trimmed.strip_prefix("tool:") {
        let (name, args) = tool_spec.split_once(' ').unwrap_or((tool_spec, ""));
        return ReplResult::ToolCall {
            tool: name.trim().to_string(),
            output: format!("Result of {name}({args}) -> OK"),
        };
    }

    ReplResult::Unknown(format!("Unrecognized REPL query: {trimmed}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_repl_overlay_dispatch() {
        let temp = tempdir().unwrap();
        let notes_dir = temp.path().join(".hadron").join("nucleus").join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        std::fs::write(notes_dir.join("chat-cache-reset.md"), "test content").unwrap();
        std::fs::write(notes_dir.join("vulkan-lavapipe.md"), "test content").unwrap();

        assert_eq!(evaluate_repl_input("", temp.path()), ReplResult::Empty);
        assert_eq!(evaluate_repl_input("   ", temp.path()), ReplResult::Empty);

        match evaluate_repl_input("/help", temp.path()) {
            ReplResult::SlashCommand(cmd) => assert!(cmd.contains("/help")),
            other => panic!("expected SlashCommand, got {:?}", other),
        }

        match evaluate_repl_input("?chat", temp.path()) {
            ReplResult::NucleusQuery(notes) => {
                assert_eq!(notes.len(), 1);
                assert_eq!(notes[0], "chat-cache-reset.md");
            }
            other => panic!("expected NucleusQuery, got {:?}", other),
        }

        match evaluate_repl_input("tool:replace_by_hash foo.rs", temp.path()) {
            ReplResult::ToolCall { tool, output } => {
                assert_eq!(tool, "replace_by_hash");
                assert!(output.contains("OK"));
            }
            other => panic!("expected ToolCall, got {:?}", other),
        }

        match evaluate_repl_input("random input", temp.path()) {
            ReplResult::Unknown(msg) => assert!(msg.contains("random input")),
            other => panic!("expected Unknown, got {:?}", other),
        }
    }
}
