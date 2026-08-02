//! The tool surface an HTTP-backed quark gets.
//!
//! `LocalQuark` speaks plain chat completions, so its tools are the OpenAI
//! `tools`/`tool_calls` shape — the one dialect OpenRouter, Ollama and LM
//! Studio all accept. Execution is an in-process call into `hadron-forge`,
//! jailed to the quark's own worktree; nothing here spawns a subprocess of
//! its own or talks to `hadron-forge-mcp`.
//!
//! Editing is **hash-based**, the same mechanism the ACP quarks get over MCP
//! (`hadron_forge_read_blocks` / `hadron_forge_edit`): a model must read a
//! file's blocks to learn a block's hash before it can replace that block. A
//! naive "replace this exact text" tool would duplicate `hadron-forge`'s own
//! edit logic outside the crate that owns it (and outside its safety net) —
//! `apply_block_edit` already exists and already has tests.

use hadron_forge::exec::{Program, EXEC_DEADLINE};
use hadron_forge::file::Root;
use hadron_lattice::Mode;
use serde_json::{json, Value};

use crate::adapter::acp::model::{permission_choice, RequestClass};

/// Transport-neutral answer to whether the current mode permits a named tool —
/// keeps `agent_client_protocol`'s wire-level `PermissionOptionKind` out of what
/// this module otherwise treats as plain HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Verdict {
    Allow,
    Reject,
}

/// `exec` runs an unmediated shell-adjacent program; every other declared tool
/// is forge-backed (jailed, hash-checked). Same split ACP's `RequestClass`
/// already makes, so this reuses the same ladder rather than a second one.
fn classify(name: &str) -> RequestClass {
    if name == "exec" {
        RequestClass::Other
    } else {
        RequestClass::Forge
    }
}

fn verdict(mode: Mode, name: &str) -> Verdict {
    use agent_client_protocol::schema::v1::PermissionOptionKind::AllowOnce;
    if permission_choice(mode, classify(name)) == AllowOnce {
        Verdict::Allow
    } else {
        Verdict::Reject
    }
}

/// The `tools` array to declare for a turn at the given mode — `None` when the
/// mode permits none at all (`Ask`: "talk, don't act"). Filtering happens here
/// rather than declare-then-refuse: a refused call still burns a round
/// (`MAX_TOOL_ROUNDS`), and unlike an ACP agent an HTTP quark has no fallback
/// native tools to retry with, so declaring an unreachable tool is a wasted
/// round waiting to happen.
pub fn declarations_for_mode(mode: Mode) -> Option<Value> {
    let allowed: Vec<Value> = declarations()
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|t| {
            let name = t["function"]["name"].as_str().unwrap_or_default();
            verdict(mode, name) == Verdict::Allow
        })
        .collect();
    if allowed.is_empty() {
        None
    } else {
        Some(Value::Array(allowed))
    }
}

/// The `tools` array sent with every chat request.
pub fn declarations() -> Value {
    json!([
        tool(
            "read_file",
            "Read a text file from the project. Returns its contents with 1-indexed line numbers noted.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path relative to the project root" },
                    "offset": { "type": "integer", "description": "1-indexed line to start at (default 1)" },
                    "limit": { "type": "integer", "description": "Maximum number of lines to return" }
                },
                "required": ["path"]
            }),
        ),
        tool(
            "list_dir",
            "List the entries of a directory in the project.",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Directory relative to the project root; \".\" for the root" } },
                "required": ["path"]
            }),
        ),
        tool(
            "grep",
            "Search the project for a literal substring. Returns matching lines with their paths and line numbers.",
            json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" },
                    "path": { "type": "string", "description": "Optional subdirectory to limit the search to" }
                },
                "required": ["pattern"]
            }),
        ),
        tool(
            "read_blocks",
            "List every top-level function/struct/enum/impl/trait in a file, each with its edit hash. \
             Call this BEFORE edit_block — the hash it reports is what edit_block needs, and it goes \
             stale the moment the block's text changes.",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string" } },
                "required": ["path"]
            }),
        ),
        tool(
            "edit_block",
            "Replace one whole top-level block (a function, struct, enum, impl, or trait) with new text. \
             `target_hash` must be the CURRENT hash of that block from read_blocks — a stale or wrong hash \
             is refused rather than applied to the wrong place. Only works on languages with block parsing \
             (Rust, Python, TypeScript, Go); use create_file for anything else or for a brand new file.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "target_hash": { "type": "string", "description": "The block's current hash, from read_blocks" },
                    "new_text": { "type": "string", "description": "The full replacement text for that block" }
                },
                "required": ["path", "target_hash", "new_text"]
            }),
        ),
        tool(
            "create_file",
            "Create a brand new file. Refused if the file already exists — use edit_block to change one.",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string" }, "contents": { "type": "string" } },
                "required": ["path", "contents"]
            }),
        ),
        tool(
            "git_diff",
            "Show the uncommitted changes in the project.",
            json!({
                "type": "object",
                "properties": { "path": { "type": "string", "description": "Optional path to limit the diff to" } }
            }),
        ),
        tool(
            "exec",
            "Run an allowlisted program (cargo or git) inside the project, bounded and output-capped. \
             There is no shell: no `;`, `|`, `>`, or globbing, and no argument may leave the worktree.",
            json!({
                "type": "object",
                "properties": {
                    "program": { "type": "string", "enum": ["cargo", "git"] },
                    "args": { "type": "array", "items": { "type": "string" }, "description": "e.g. [\"test\", \"-p\", \"hadron-gluon\"]" }
                },
                "required": ["program", "args"]
            }),
        ),
    ])
}

fn tool(name: &str, description: &str, parameters: Value) -> Value {
    json!({ "type": "function", "function": { "name": name, "description": description, "parameters": parameters } })
}

/// Run one tool call and return its result as the string that goes back into
/// the conversation as a `role: "tool"` message. An error is a RESULT, not a
/// failure: the model must see what went wrong so it can correct itself.
///
/// `mode` is checked again here even though `declarations_for_mode` already
/// keeps a disallowed tool out of the request: a model can emit a call it was
/// never offered (Nemotron demonstrably narrates tool envelopes as plain
/// text), so this is a deliberate second layer, not a redundant one.
pub fn execute(root: &Root, mode: Mode, name: &str, args: &Value) -> String {
    if verdict(mode, name) == Verdict::Reject {
        return format!("ERROR: `{name}` is not permitted in `{mode:?}` mode this turn");
    }
    let s = |k: &str| args.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let opt_s = |k: &str| args.get(k).and_then(Value::as_str);
    let opt_usize = |k: &str| args.get(k).and_then(Value::as_u64).map(|n| n as usize);
    // Every path argument, and only path arguments — `pattern`, `new_text` and
    // `contents` are content and must never be touched.
    let p = |k: &str| relativise(root, &s(k));
    let opt_p = |k: &str| opt_s(k).map(|v| relativise(root, v));

    match name {
        "read_file" => match hadron_forge::inspect::read_file(root, &p("path"), opt_usize("offset"), opt_usize("limit")) {
            Ok(slice) => format!(
                "lines {}-{} of {}:\n{}",
                slice.start_line, slice.end_line, slice.total_lines, slice.text
            ),
            Err(e) => format!("ERROR reading {}: {e}", s("path")),
        },
        "list_dir" => match hadron_forge::inspect::list_dir(root, &p("path")) {
            Ok(entries) => entries
                .iter()
                .map(|e| if e.is_dir { format!("{}/", e.name) } else { e.name.clone() })
                .collect::<Vec<_>>()
                .join("\n"),
            Err(e) => format!("ERROR listing {}: {e}", s("path")),
        },
        "grep" => match hadron_forge::inspect::grep(root, &s("pattern"), opt_p("path").as_deref()) {
            Ok(hits) if hits.is_empty() => "(no matches)".to_string(),
            Ok(hits) => hits.iter().map(|h| format!("{}:{}: {}", h.path, h.line, h.text)).collect::<Vec<_>>().join("\n"),
            Err(e) => format!("ERROR grepping: {e}"),
        },
        "read_blocks" => match hadron_forge::file::read_blocks(root, &p("path")) {
            Ok(report) if report.blocks.is_empty() => {
                "(no hashable top-level blocks in this file — use create_file to replace it whole)".to_string()
            }
            Ok(report) => report.blocks,
            Err(e) => format!("ERROR reading blocks of {}: {e}", s("path")),
        },
        "edit_block" => match hadron_forge::file::apply_block_edit(root, &p("path"), &s("target_hash"), &s("new_text")) {
            Ok(report) => format!("edited {}. New blocks:\n{}", s("path"), report.blocks),
            Err(e) => format!("ERROR editing {}: {e}", s("path")),
        },
        "create_file" => match hadron_forge::file::create_file(root, &p("path"), &s("contents")) {
            Ok(_) => format!("created {}", s("path")),
            Err(e) => format!("ERROR creating {}: {e}", s("path")),
        },
        "git_diff" => match hadron_forge::git::git_diff(root, opt_p("path").as_deref()) {
            Ok(d) if d.trim().is_empty() => "(no uncommitted changes)".to_string(),
            Ok(d) => d,
            Err(e) => format!("ERROR: {e}"),
        },
        "exec" => execute_exec(root, args),
        other => format!("unknown tool `{other}`"),
    }
}

/// Rewrite an absolute path that points INSIDE the worktree as the relative
/// one the forge expects; leave everything else exactly as the model sent it.
///
/// The prompt tells a quark "You are working in `<absolute worktree path>`", so
/// a model quotes that path straight back into its tool arguments — and
/// `hadron_forge::file::resolve_jailed_path` refuses every absolute path when
/// no external root is granted. The result is not one bad call but a whole
/// wasted turn: the model retries, gets the same refusal, and burns
/// `MAX_TOOL_ROUNDS` without an answer.
///
/// This is a spelling fix, not a hole in the jail: a path that does not already
/// live under the root is returned untouched and still reaches
/// `resolve_jailed_path` absolute, where it is refused exactly as before.
fn relativise(root: &Root, path: &str) -> String {
    let path = std::path::Path::new(path);
    if !path.is_absolute() {
        return path.display().to_string();
    }
    // Both spellings: the canonical root (what the forge compares against) and
    // the root as configured, which may still carry a symlink the model copied
    // out of its own prompt.
    let canonical = root.path().canonicalize();
    let bases = [canonical.as_deref().unwrap_or_else(|_| root.path()), root.path()];
    for base in bases {
        if let Ok(rest) = path.strip_prefix(base) {
            return if rest.as_os_str().is_empty() { ".".to_string() } else { rest.display().to_string() };
        }
    }
    path.display().to_string()
}

fn execute_exec(root: &Root, args: &Value) -> String {
    let program_name = args.get("program").and_then(Value::as_str).unwrap_or("");
    let Some(program) = Program::parse(program_name) else {
        return format!("ERROR: `{program_name}` is not an allowlisted program (only cargo and git may run here)");
    };
    let call_args: Vec<String> = args
        .get("args")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    match hadron_forge::exec::exec(root, program, &call_args, EXEC_DEADLINE) {
        Ok(out) => format!(
            "exit code: {}\nstdout:\n{}\nstderr:\n{}{}",
            out.code.map(|c| c.to_string()).unwrap_or_else(|| "none (killed)".to_string()),
            out.stdout,
            out.stderr,
            if out.timed_out { "\n(timed out)" } else { "" },
        ),
        Err(e) => format!("ERROR running {program_name} {call_args:?}: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_declared_tool_has_the_openai_function_shape() {
        let tools = declarations();
        let arr = tools.as_array().expect("tools must be an array");
        assert!(arr.len() >= 5, "expected at least 5 tools, got {}", arr.len());
        for t in arr {
            assert_eq!(t["type"], "function", "every entry must be type=function");
            let f = &t["function"];
            assert!(f["name"].is_string(), "missing function.name in {t}");
            assert!(f["description"].is_string(), "missing function.description in {t}");
            assert_eq!(f["parameters"]["type"], "object", "parameters must be a JSON-Schema object");
        }
    }

    #[test]
    fn the_declared_names_are_the_ones_execute_dispatches_on() {
        let tools = declarations();
        let root = Root::new(std::env::temp_dir());
        for t in tools.as_array().unwrap() {
            let name = t["function"]["name"].as_str().unwrap();
            let out = execute(&root, Mode::Bypass, name, &json!({}));
            assert!(
                !out.contains("unknown tool") && !out.contains("is not permitted"),
                "declared tool `{name}` is not dispatched by execute(): {out}"
            );
        }
    }

    #[test]
    fn read_file_and_list_dir_work_against_a_real_worktree() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
        let root = Root::new(dir.path());
        let out = execute(&root, Mode::Bypass, "read_file", &json!({ "path": "a.txt" }));
        assert!(out.contains("hello") && out.contains("world"), "{out}");
        let out = execute(&root, Mode::Bypass, "list_dir", &json!({ "path": "." }));
        assert!(out.contains("a.txt"), "{out}");
    }

    #[test]
    fn read_blocks_then_edit_block_round_trips_a_real_hash() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn alpha() -> i32 { 1 }\n").unwrap();
        let root = Root::new(dir.path());
        let blocks = execute(&root, Mode::Bypass, "read_blocks", &json!({ "path": "lib.rs" }));
        let hash = blocks
            .split("Hash: ")
            .nth(1)
            .and_then(|s| s.split(']').next())
            .expect("read_blocks must report a hash");
        let out = execute(
            &root,
            Mode::Bypass,
            "edit_block",
            &json!({ "path": "lib.rs", "target_hash": hash, "new_text": "fn alpha() -> i32 { 2 }" }),
        );
        assert!(out.starts_with("edited"), "{out}");
        assert_eq!(std::fs::read_to_string(dir.path().join("lib.rs")).unwrap().trim(), "fn alpha() -> i32 { 2 }");
    }

    #[test]
    fn edit_block_with_a_stale_hash_is_refused_not_applied() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn alpha() -> i32 { 1 }\n").unwrap();
        let root = Root::new(dir.path());
        let out = execute(
            &root,
            Mode::Bypass,
            "edit_block",
            &json!({ "path": "lib.rs", "target_hash": "deadbeef", "new_text": "fn alpha() -> i32 { 2 }" }),
        );
        assert!(out.starts_with("ERROR"), "{out}");
        assert_eq!(std::fs::read_to_string(dir.path().join("lib.rs")).unwrap(), "fn alpha() -> i32 { 1 }\n");
    }

    #[test]
    fn exec_refuses_a_program_off_the_allowlist() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let out = execute(&root, Mode::Bypass, "exec", &json!({ "program": "sh", "args": ["-c", "echo hi"] }));
        assert!(out.contains("not an allowlisted program"), "{out}");
    }

    #[test]
    fn exec_runs_a_real_allowlisted_command() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let out = execute(&root, Mode::Bypass, "exec", &json!({ "program": "git", "args": ["status"] }));
        assert!(out.contains("exit code:"), "{out}");
    }

    /// The backstop layer (step 3): even though `declarations_for_mode` keeps a
    /// disallowed tool out of the request, `execute` must refuse it anyway if a
    /// model emits the call unprompted — the model has no fallback native tool,
    /// so this is the last line of defence, not a redundant one.
    #[test]
    fn execute_refuses_a_tool_the_mode_never_offered() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let root = Root::new(dir.path());

        let out = execute(&root, Mode::Ask, "read_file", &json!({ "path": "a.txt" }));
        assert!(out.contains("is not permitted"), "Ask must refuse every tool: {out}");
        let out = execute(&root, Mode::Write, "exec", &json!({ "program": "git", "args": ["status"] }));
        assert!(out.contains("is not permitted"), "Write must refuse exec: {out}");

        // The forge tools are still reachable at Write — the backstop is not a
        // blanket refusal.
        let out = execute(&root, Mode::Write, "read_file", &json!({ "path": "a.txt" }));
        assert!(out.contains("hello"), "Write must still allow forge tools: {out}");
    }

    /// The prompt tells the quark "You are working in `/abs/path/to/worktree`",
    /// so a model reaches for that absolute path — and `resolve_jailed_path`
    /// refuses EVERY absolute path when no external root is granted. Live cost
    /// on 2026-08-02: `http-openai-compatible` burned all 24 tool rounds and
    /// answered nothing, narrating "I'm in a git worktree at
    /// /home/Jake/dev/hadron/.hadron/trees/http-openai-compatible" as it went.
    #[test]
    fn an_absolute_path_inside_the_worktree_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\n").unwrap();
        let root = Root::new(dir.path());

        let abs = dir.path().join("a.txt");
        let out = execute(&root, Mode::Bypass, "read_file", &json!({ "path": abs.to_str().unwrap() }));
        assert!(out.contains("hello"), "an absolute path inside the worktree must read: {out}");

        let out = execute(&root, Mode::Bypass, "list_dir", &json!({ "path": dir.path().to_str().unwrap() }));
        assert!(out.contains("a.txt"), "the worktree root itself must list: {out}");
    }

    /// The jail is unchanged: relativising only ever rewrites a path that is
    /// already inside the root, so everything else still reaches
    /// `resolve_jailed_path` absolute and is refused there.
    #[test]
    fn an_absolute_path_outside_the_worktree_is_still_refused() {
        let dir = tempfile::tempdir().unwrap();
        let root = Root::new(dir.path());
        let out = execute(&root, Mode::Bypass, "read_file", &json!({ "path": "/etc/passwd" }));
        assert!(out.starts_with("ERROR"), "a path outside the worktree must stay refused: {out}");
    }
}
