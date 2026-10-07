//! Cross-worktree peer inspector MCP tool.

use super::{ForgeMcpServer, ToolResponse};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PeerInspectArgs {
    /// Specific peer id to inspect (e.g. "acp-claude" or "cli-agy"). If omitted, lists all sibling worktrees.
    pub peer_id: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PeerAcquireLeaseArgs {
    /// Quark or seat identifier (e.g. "cli-agy" or "acp-claude")
    pub quark_id: String,
    /// Target file paths to lock exclusively for editing
    pub paths: Vec<String>,
    /// Lease duration in seconds (defaults to 1800 / 30m)
    pub ttl_secs: Option<u64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PeerReleaseLeaseArgs {
    /// Quark or seat identifier holding the lease
    pub quark_id: String,
    /// Ulid string of the lease to release
    pub lease_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PeerConflictsArgs {}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PeerListLeasesArgs {}

fn lock_file_path(root: &std::path::Path) -> std::path::PathBuf {
    if let Ok(trees_dir) = hadron_forge::peers::derive_trees_dir(root) {
        if let Some(hadron_dir) = trees_dir.parent() {
            return hadron_dir.join("locks").join("intent_locks.json");
        }
    }
    root.join(".hadron").join("locks").join("intent_locks.json")
}

#[tool_router(router = peers_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_peer_inspect",
        description = "Inspect sibling quark worktrees across .hadron/trees/* (branch, latest commit, dirty files, commits ahead)"
    )]
    pub async fn peer_inspect(&self, Parameters(args): Parameters<PeerInspectArgs>) -> Json<ToolResponse> {
        let root = self.root.clone();
        let peer_id = args.peer_id;

        let res = tokio::task::spawn_blocking(move || {
            if let Some(id) = peer_id {
                hadron_forge::peers::inspect_peer_worktree(&root, &id)
                    .map(|info| serde_json::to_string_pretty(&info).unwrap_or_default())
            } else {
                hadron_forge::peers::list_peer_worktrees(&root)
                    .map(|list| serde_json::to_string_pretty(&list).unwrap_or_default())
            }
        })
        .await;

        match res {
            Ok(Ok(json)) => Json(ToolResponse::success(Some(json))),
            Ok(Err(e)) => Json(ToolResponse::error(e.to_string())),
            Err(e) => Json(ToolResponse::error(format!("Peer inspection task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_peers_detect_conflicts",
        description = "Proactively scan sibling quark worktrees in .hadron/trees/* for overlapping file edits and merge collision risks"
    )]
    pub async fn peers_detect_conflicts(&self, Parameters(_args): Parameters<PeerConflictsArgs>) -> Json<ToolResponse> {
        let root = self.root.clone();

        let res = tokio::task::spawn_blocking(move || {
            hadron_forge::peers::detect_cross_worktree_conflicts(&root)
        })
        .await;

        match res {
            Ok(Ok(report)) => {
                let json = serde_json::to_string_pretty(&report).unwrap_or_else(|_| report.summary.clone());
                if report.conflicts_detected == 0 {
                    Json(ToolResponse::success(Some(json)))
                } else {
                    let mut resp = ToolResponse::error(report.summary);
                    resp.blocks = Some(json);
                    Json(resp)
                }
            }
            Ok(Err(e)) => Json(ToolResponse::error(e.to_string())),
            Err(e) => Json(ToolResponse::error(format!("Cross-worktree conflict detection task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_peers_acquire_lease",
        description = "Acquire an advisory exclusive write lease for target file paths across worker quarks"
    )]
    pub async fn peers_acquire_lease(
        &self,
        Parameters(args): Parameters<PeerAcquireLeaseArgs>,
    ) -> Json<ToolResponse> {
        let lock_file = lock_file_path(self.root.path());
        let quark = hadron_lattice::QuarkId::new(&args.quark_id);
        let paths: Vec<std::path::PathBuf> = args.paths.into_iter().map(std::path::PathBuf::from).collect();
        let ttl = std::time::Duration::from_secs(args.ttl_secs.unwrap_or(1800));

        let res = tokio::task::spawn_blocking(move || {
            let mut table = hadron_lattice::locks::IntentLockTable::load_from_file(&lock_file);
            match table.try_acquire(quark, &paths, ttl) {
                Ok(lease) => {
                    let _ = table.save_to_file(&lock_file);
                    Ok(serde_json::to_string_pretty(&lease).unwrap_or_default())
                }
                Err(conflicts) => {
                    let conflict_strs: Vec<String> = conflicts.iter().map(|p| p.to_string_lossy().to_string()).collect();
                    Err(format!("Conflict: paths already leased by sibling quarks: {:?}", conflict_strs))
                }
            }
        })
        .await;

        match res {
            Ok(Ok(json)) => Json(ToolResponse::success(Some(json))),
            Ok(Err(e)) => Json(ToolResponse::error(e)),
            Err(e) => Json(ToolResponse::error(format!("Acquire lease task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_peers_release_lease",
        description = "Release an advisory write lease by its lease ID"
    )]
    pub async fn peers_release_lease(
        &self,
        Parameters(args): Parameters<PeerReleaseLeaseArgs>,
    ) -> Json<ToolResponse> {
        let lock_file = lock_file_path(self.root.path());
        let quark = hadron_lattice::QuarkId::new(&args.quark_id);
        let lease_id = match hadron_lattice::Ulid::from_string(&args.lease_id) {
            Ok(id) => id,
            Err(e) => return Json(ToolResponse::error(format!("Invalid lease ULID: {e}"))),
        };

        let res = tokio::task::spawn_blocking(move || {
            let mut table = hadron_lattice::locks::IntentLockTable::load_from_file(&lock_file);
            let released = table.release_by_id(&quark, lease_id);
            let _ = table.save_to_file(&lock_file);
            released
        })
        .await;

        match res {
            Ok(true) => Json(ToolResponse::success(Some(format!("Lease {} released", args.lease_id)))),
            Ok(false) => Json(ToolResponse::error(format!("No active lease matching ID {} for quark {}", args.lease_id, args.quark_id))),
            Err(e) => Json(ToolResponse::error(format!("Release lease task failed: {e}"))),
        }
    }

    #[tool(
        name = "hadron_forge_peers_list_leases",
        description = "List all active cross-quark advisory file write leases"
    )]
    pub async fn peers_list_leases(
        &self,
        Parameters(_args): Parameters<PeerListLeasesArgs>,
    ) -> Json<ToolResponse> {
        let lock_file = lock_file_path(self.root.path());
        let res = tokio::task::spawn_blocking(move || {
            let table = hadron_lattice::locks::IntentLockTable::load_from_file(&lock_file);
            let leases = table.active_leases();
            serde_json::to_string_pretty(&leases).unwrap_or_else(|_| "[]".to_string())
        })
        .await;

        match res {
            Ok(json) => Json(ToolResponse::success(Some(json))),
            Err(e) => Json(ToolResponse::error(format!("List leases task failed: {e}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn fixture_multitree_repo() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let main = tmp.path().join("main-repo");
        std::fs::create_dir_all(&main).unwrap();

        let run = |args: &[&str], cwd: &std::path::Path| {
            let status = Command::new("git")
                .args(args)
                .current_dir(cwd)
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?} failed in {}", cwd.display());
        };

        run(&["init", "-q"], &main);
        run(&["config", "user.email", "test@test.com"], &main);
        run(&["config", "user.name", "Tester"], &main);
        std::fs::write(main.join("file.txt"), "hello\n").unwrap();
        run(&["add", "file.txt"], &main);
        run(&["commit", "-q", "-m", "initial commit"], &main);

        let trees = main.join(".hadron").join("trees");
        std::fs::create_dir_all(&trees).unwrap();

        let peer_a = trees.join("peer-alpha");
        run(&["worktree", "add", "-q", "-b", "quark/peer-alpha/feat1", peer_a.to_str().unwrap()], &main);

        (tmp, main, peer_a)
    }

    #[tokio::test]
    async fn peer_inspect_tool_executes() {
        let (_tmp, _main, peer_a) = fixture_multitree_repo();
        let server = ForgeMcpServer::new(&peer_a);

        let res = server
            .peer_inspect(Parameters(PeerInspectArgs { peer_id: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("peer-alpha"));
    }

    #[tokio::test]
    async fn peers_detect_conflicts_tool_executes() {
        let (_tmp, _main, peer_a) = fixture_multitree_repo();
        let server = ForgeMcpServer::new(&peer_a);

        let res = server
            .peers_detect_conflicts(Parameters(PeerConflictsArgs {}))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("Cross-worktree check CLEAN"));
    }

    #[tokio::test]
    async fn peers_lease_lifecycle_tools_execute() {
        let (_tmp, _main, peer_a) = fixture_multitree_repo();
        let server = ForgeMcpServer::new(&peer_a);

        // 1. Acquire lease
        let acq_res = server
            .peers_acquire_lease(Parameters(PeerAcquireLeaseArgs {
                quark_id: "worker-alpha".to_string(),
                paths: vec!["crates/lib.rs".to_string()],
                ttl_secs: Some(60),
            }))
            .await;
        assert!(acq_res.0.ok);
        let val: serde_json::Value = serde_json::from_str(acq_res.0.blocks.as_ref().unwrap()).unwrap();
        let lease_id = val["id"].as_str().unwrap().to_string();

        // 2. Conflicting acquire by another worker should fail
        let conflict_res = server
            .peers_acquire_lease(Parameters(PeerAcquireLeaseArgs {
                quark_id: "worker-beta".to_string(),
                paths: vec!["crates/lib.rs".to_string()],
                ttl_secs: Some(60),
            }))
            .await;
        assert!(!conflict_res.0.ok);

        // 3. List leases
        let list_res = server.peers_list_leases(Parameters(PeerListLeasesArgs {})).await;
        assert!(list_res.0.ok);
        assert!(list_res.0.blocks.unwrap().contains(&lease_id));

        // 4. Release lease
        let rel_res = server
            .peers_release_lease(Parameters(PeerReleaseLeaseArgs {
                quark_id: "worker-alpha".to_string(),
                lease_id: lease_id.clone(),
            }))
            .await;
        assert!(rel_res.0.ok);

        // 5. Worker beta can now acquire
        let acq_beta = server
            .peers_acquire_lease(Parameters(PeerAcquireLeaseArgs {
                quark_id: "worker-beta".to_string(),
                paths: vec!["crates/lib.rs".to_string()],
                ttl_secs: Some(60),
            }))
            .await;
        assert!(acq_beta.0.ok);
    }
}

