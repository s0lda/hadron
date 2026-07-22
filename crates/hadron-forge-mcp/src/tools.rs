use hadron_forge::file::{
    apply_block_edit, create_file, delete_file_cas, read_blocks, write_file_cas, Root,
};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::{tool, tool_handler, tool_router, ServerHandler};
use rmcp::schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[tool_handler(router = self.tool_router)]
impl ServerHandler for ForgeMcpServer {}

#[derive(Clone)]
pub struct ForgeMcpServer {
    tool_router: ToolRouter<Self>,
    pub root: Root,
}

impl ForgeMcpServer {
    pub fn new(root_path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            root: Root::new(root_path),
        }
    }
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct ToolResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ToolResponse {
    pub fn success(blocks: Option<String>) -> Self {
        Self {
            ok: true,
            blocks,
            reason: None,
        }
    }
    pub fn error(reason: impl Into<String>) -> Self {
        Self {
            ok: false,
            blocks: None,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct EditArgs {
    pub path: String,
    pub target_hash: String,
    pub new_text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WriteFileArgs {
    pub path: String,
    pub content: String,
    pub expected_hash: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CreateFileArgs {
    pub path: String,
    pub content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeleteFileArgs {
    pub path: String,
    pub expected_hash: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReadBlocksArgs {
    pub path: String,
}

#[tool_router(router = tool_router)]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_edit",
        description = "Replace a specific AST block in a source file by its 8-hex content hash"
    )]
    pub async fn edit(&self, Parameters(args): Parameters<EditArgs>) -> Json<ToolResponse> {
        match apply_block_edit(&self.root, &args.path, &args.target_hash, &args.new_text) {
            Ok(rep) => Json(ToolResponse::success(Some(rep.blocks))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_write_file",
        description = "Write whole file content under optimistic concurrency (Compare-And-Swap on file hash)"
    )]
    pub async fn write_file(&self, Parameters(args): Parameters<WriteFileArgs>) -> Json<ToolResponse> {
        match write_file_cas(&self.root, &args.path, &args.content, args.expected_hash.as_deref()) {
            Ok(rep) => Json(ToolResponse::success(Some(rep.blocks))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_create_file",
        description = "Create a new file, failing if the file already exists"
    )]
    pub async fn create_file(&self, Parameters(args): Parameters<CreateFileArgs>) -> Json<ToolResponse> {
        match create_file(&self.root, &args.path, &args.content) {
            Ok(rep) => Json(ToolResponse::success(Some(rep.blocks))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_delete_file",
        description = "Delete a file safely, optionally verifying its content hash first"
    )]
    pub async fn delete_file(&self, Parameters(args): Parameters<DeleteFileArgs>) -> Json<ToolResponse> {
        match delete_file_cas(&self.root, &args.path, args.expected_hash.as_deref()) {
            Ok(()) => Json(ToolResponse::success(None)),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_read_blocks",
        description = "Inspect addressable AST block hashes and line ranges for a source file"
    )]
    pub async fn read_blocks(&self, Parameters(args): Parameters<ReadBlocksArgs>) -> Json<ToolResponse> {
        match read_blocks(&self.root, &args.path) {
            Ok(rep) => Json(ToolResponse::success(Some(rep.blocks))),
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

        // Create
        let res = server.create_file(Parameters(CreateFileArgs {
            path: "foo.rs".into(),
            content: "pub fn foo() {}\n".into(),
        })).await;
        assert_eq!(res.0.ok, true);
        assert!(res.0.blocks.as_ref().unwrap().contains("fn foo"));

        // Read blocks
        let res = server.read_blocks(Parameters(ReadBlocksArgs {
            path: "foo.rs".into(),
        })).await;
        assert_eq!(res.0.ok, true);
        let blocks = res.0.blocks.as_ref().unwrap();
        let hash = blocks.split("Hash: ").nth(1).unwrap().split(']').next().unwrap();

        // Edit
        let res = server.edit(Parameters(EditArgs {
            path: "foo.rs".into(),
            target_hash: hash.into(),
            new_text: "pub fn foo() { println!(\"hi\"); }\n".into(),
        })).await;
        assert_eq!(res.0.ok, true);
        assert!(std::fs::read_to_string(dir.path().join("foo.rs")).unwrap().contains("println!"));

        // Write CAS
        let cur_hash = hadron_forge::block::short_hash(&std::fs::read_to_string(dir.path().join("foo.rs")).unwrap());
        let res = server.write_file(Parameters(WriteFileArgs {
            path: "foo.rs".into(),
            content: "pub fn foo() { println!(\"bye\"); }\n".into(),
            expected_hash: Some(cur_hash),
        })).await;
        assert_eq!(res.0.ok, true);

        // Delete
        let cur_hash = hadron_forge::block::short_hash(&std::fs::read_to_string(dir.path().join("foo.rs")).unwrap());
        let res = server.delete_file(Parameters(DeleteFileArgs {
            path: "foo.rs".into(),
            expected_hash: Some(cur_hash),
        })).await;
        assert_eq!(res.0.ok, true);
        assert!(!dir.path().join("foo.rs").exists());
    }
}
