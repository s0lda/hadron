//! The **inspect** family: read a slice of a file, list a directory, search
//! the tree — everything a quark needs to look around without a shell.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::inspect::{self, DirEntryInfo, FileSlice, GrepMatch};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadFileArgs {
    pub path: String,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListDirArgs {
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GrepArgs {
    pub pattern: String,
    pub path: Option<String>,
}

fn format_file_slice(slice: FileSlice) -> String {
    format!(
        "lines {}-{} of {}\n{}",
        slice.start_line, slice.end_line, slice.total_lines, slice.text
    )
}

fn format_dir_entries(entries: Vec<DirEntryInfo>) -> String {
    entries
        .into_iter()
        .map(|e| if e.is_dir { format!("{}/", e.name) } else { e.name })
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_grep_matches(matches: Vec<GrepMatch>) -> String {
    if matches.is_empty() {
        return "no matches".to_string();
    }
    matches
        .into_iter()
        .map(|m| format!("{}:{}: {}", m.path, m.line, m.text))
        .collect::<Vec<_>>()
        .join("\n")
}

impl ForgeMcpServer {
    /// The note slug when `path` (as read under the worktree root) really is a file in
    /// this server's nucleus `notes/` dir — not merely any `notes/*.md` in the tree.
    fn nucleus_note_slug(&self, path: &str) -> Option<String> {
        let slug = hadron_lattice::nucleus_usage::extract_note_slug_from_path(path)?;
        let read = std::fs::canonicalize(self.root.path().join(path)).ok()?;
        let notes = std::fs::canonicalize(self.nucleus_root.path().join("notes")).ok()?;
        (read.parent() == Some(notes.as_path())).then_some(slug)
    }
}

#[tool_router(router = inspect_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_read_file",
        description = "Read a slice of a file's text by 1-indexed line offset and limit, jailed to the worktree"
    )]
    pub async fn read_file(&self, Parameters(args): Parameters<ReadFileArgs>) -> Json<ToolResponse> {
        match inspect::read_file(&self.root, &args.path, args.offset, args.limit) {
            Ok(slice) => {
                if let Some(slug) = self.nucleus_note_slug(&args.path) {
                    super::nucleus::record_note_opens(self.nucleus_root.path(), [slug.as_str()]);
                }
                Json(ToolResponse::success(Some(format_file_slice(slice))))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_list_dir",
        description = "List a directory's entries (name, dir/file), jailed to the worktree"
    )]
    pub async fn list_dir(&self, Parameters(args): Parameters<ListDirArgs>) -> Json<ToolResponse> {
        match inspect::list_dir(&self.root, args.path.as_deref().unwrap_or("")) {
            Ok(entries) => Json(ToolResponse::success(Some(format_dir_entries(entries)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_grep",
        description = "Search file contents for a literal substring, recursively from a path (default: worktree root)"
    )]
    pub async fn grep(&self, Parameters(args): Parameters<GrepArgs>) -> Json<ToolResponse> {
        match inspect::grep(&self.root, &args.pattern, args.path.as_deref()) {
            Ok(matches) => Json(ToolResponse::success(Some(format_grep_matches(matches)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn tool_handlers_operate_on_jailed_root() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        std::fs::write(dir.path().join("f.txt"), "one\ntwo\nneedle\n").unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();

        let res = server
            .read_file(Parameters(ReadFileArgs { path: "f.txt".into(), offset: None, limit: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("needle"));

        let res = server.list_dir(Parameters(ListDirArgs { path: None })).await;
        assert!(res.0.ok);
        let listing = res.0.blocks.unwrap();
        assert!(listing.contains("f.txt"));
        assert!(listing.contains("sub/"));

        let res = server
            .grep(Parameters(GrepArgs { pattern: "needle".into(), path: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("f.txt:3:"));
    }

    #[tokio::test]
    async fn reading_a_nucleus_note_records_usage_but_other_notes_dirs_do_not() {
        let dir = tempfile::tempdir().unwrap();
        let nucleus = dir.path().join(".hadron").join("nucleus");
        std::fs::create_dir_all(nucleus.join("notes")).unwrap();
        std::fs::create_dir_all(dir.path().join("docs/notes")).unwrap();
        std::fs::write(nucleus.join("notes/real.md"), "fact\n").unwrap();
        std::fs::write(dir.path().join("docs/notes/decoy.md"), "not a lesson\n").unwrap();
        let server = ForgeMcpServer::with_nucleus(dir.path(), nucleus.as_path());

        for path in [".hadron/nucleus/notes/real.md", "docs/notes/decoy.md"] {
            let res = server
                .read_file(Parameters(ReadFileArgs { path: path.into(), offset: None, limit: None }))
                .await;
            assert!(res.0.ok, "{path}: {:?}", res.0.reason);
        }
        let ledger = hadron_lattice::nucleus_usage::load_usage_ledger(&nucleus).unwrap();
        assert_eq!(ledger.notes.get("real").map(|e| e.opens), Some(1));
        assert!(!ledger.notes.contains_key("decoy"));
    }

    #[tokio::test]
    async fn tool_handlers_refuse_a_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());

        let res = server
            .read_file(Parameters(ReadFileArgs { path: "../evil.txt".into(), offset: None, limit: None }))
            .await;
        assert!(!res.0.ok);
        assert!(res.0.reason.unwrap().contains("escapes root"));
    }
}
