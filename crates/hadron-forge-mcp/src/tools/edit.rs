//! The **edit** family: change a file, or ask what its addressable blocks are.
//!
//! Every one of these is hash-checked — an edit names the block it believes it
//! is replacing, and a stale hash is refused rather than silently overwriting
//! whatever landed there in the meantime.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::file::{
    apply_block_edit, create_file, delete_file_cas, read_blocks, write_file_cas,
};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

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

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BatchEditItem {
    pub path: String,
    pub expected_hash: Option<String>,
    pub new_content: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BatchEditArgs {
    pub operations: Vec<BatchEditItem>,
}

#[tool_router(router = edit_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_edit",
        description = "Replace a specific AST block in a source file by its 8-hex content hash"
    )]
    pub async fn edit(&self, Parameters(args): Parameters<EditArgs>) -> Json<ToolResponse> {
        let res = apply_block_edit(&self.root, &args.path, &args.target_hash, &args.new_text);
        if res.is_ok() {
            if let Ok(bus) = hadron_lattice::GossipBus::new(&self.root.path().join(".hadron")) {
                let msg = hadron_lattice::GossipMessage {
                    quark: "mcp-forge".to_string(),
                    timestamp: chrono::Utc::now(),
                    payload: hadron_lattice::GossipPayload::FileTouch {
                        path: args.path.clone(),
                        is_edit: true,
                    },
                };
                let _ = bus.publish(&msg);
            }
        }
        match res {
            Ok(rep) => Json(ToolResponse::success(Some(rep.blocks))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_write_file",
        description = "Write whole file content under optimistic concurrency (Compare-And-Swap on file hash)"
    )]
    pub async fn write_file(&self, Parameters(args): Parameters<WriteFileArgs>) -> Json<ToolResponse> {
        let res = write_file_cas(&self.root, &args.path, &args.content, args.expected_hash.as_deref());
        if res.is_ok() {
            if let Ok(bus) = hadron_lattice::GossipBus::new(&self.root.path().join(".hadron")) {
                let msg = hadron_lattice::GossipMessage {
                    quark: "mcp-forge".to_string(),
                    timestamp: chrono::Utc::now(),
                    payload: hadron_lattice::GossipPayload::FileTouch {
                        path: args.path.clone(),
                        is_edit: true,
                    },
                };
                let _ = bus.publish(&msg);
            }
        }
        match res {
            Ok(rep) => Json(ToolResponse::success(Some(rep.blocks))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    #[tool(
        name = "hadron_forge_batch_edit",
        description = "Execute an atomic multi-file batch edit transaction with rollback on failure"
    )]
    pub async fn batch_edit(&self, Parameters(args): Parameters<BatchEditArgs>) -> Json<ToolResponse> {
        let mut tx = hadron_forge::transaction::BatchEditTransaction::new();
        for op in args.operations {
            let full_path = self.root.path().join(&op.path);
            tx.add_edit(hadron_forge::transaction::FileEditOp {
                path: full_path,
                expected_hash: op.expected_hash,
                new_content: op.new_content,
            });
        }
        match tx.validate_and_apply() {
            Ok(report) => Json(ToolResponse::success(Some(format!(
                "Committed batch transaction: {} file(s) modified, {} bytes written",
                report.files_modified.len(),
                report.total_bytes_written
            )))),
            Err(e) => Json(ToolResponse::error(format!("Batch transaction rolled back: {e}"))),
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

        // Batch Edit
        let res = server.batch_edit(Parameters(BatchEditArgs {
            operations: vec![
                BatchEditItem {
                    path: "batch_a.rs".into(),
                    expected_hash: None,
                    new_content: "pub fn a() {}".into(),
                },
                BatchEditItem {
                    path: "batch_b.rs".into(),
                    expected_hash: None,
                    new_content: "pub fn b() {}".into(),
                },
            ],
        })).await;
        assert!(res.0.ok);
        assert!(dir.path().join("batch_a.rs").exists());
        assert!(dir.path().join("batch_b.rs").exists());
    }
}
