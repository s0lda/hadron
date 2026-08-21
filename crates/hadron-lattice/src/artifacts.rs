use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArtifactPayload {
    Markdown(String),
    DiagramMermaid(String),
    OpenApiJson(serde_json::Value),
    UnifiedDiff(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactMeta {
    pub id: String,
    pub author: String,
    pub kind: String,
    pub created_at: String,
    pub file_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct StoredArtifact {
    pub meta: ArtifactMeta,
    pub payload: ArtifactPayload,
}

pub fn artifacts_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".hadron").join("artifacts")
}

pub fn publish_artifact(
    repo_root: &Path,
    id: &str,
    author: &str,
    payload: ArtifactPayload,
) -> Result<PathBuf> {
    let dir = artifacts_dir(repo_root);
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create artifacts directory at {:?}", dir))?;

    let kind = match &payload {
        ArtifactPayload::Markdown(_) => "markdown",
        ArtifactPayload::DiagramMermaid(_) => "mermaid",
        ArtifactPayload::OpenApiJson(_) => "openapi",
        ArtifactPayload::UnifiedDiff(_) => "diff",
    };

    let file_name = format!("{id}.json");
    let target_path = dir.join(&file_name);

    let meta = ArtifactMeta {
        id: id.to_string(),
        author: author.to_string(),
        kind: kind.to_string(),
        created_at: Utc::now().to_rfc3339(),
        file_name,
    };

    let stored = StoredArtifact { meta, payload };
    let json = serde_json::to_string_pretty(&stored)?;
    fs::write(&target_path, json)?;

    Ok(target_path)
}

pub fn list_artifacts(repo_root: &Path) -> Result<Vec<ArtifactMeta>> {
    let dir = artifacts_dir(repo_root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(stored) = serde_json::from_str::<StoredArtifact>(&content) {
                    out.push(stored.meta);
                }
            }
        }
    }

    out.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    Ok(out)
}

pub fn read_artifact(repo_root: &Path, id: &str) -> Result<ArtifactPayload> {
    let dir = artifacts_dir(repo_root);
    let target_path = dir.join(format!("{id}.json"));
    let content = fs::read_to_string(&target_path)
        .with_context(|| format!("Artifact {id} not found at {:?}", target_path))?;
    let stored: StoredArtifact = serde_json::from_str(&content)?;
    Ok(stored.payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_artifact_bus_publish_list_and_read() {
        let dir = tempdir().unwrap();
        let root = dir.path();

        let payload = ArtifactPayload::DiagramMermaid("graph TD; A-->B;".to_string());
        let path = publish_artifact(root, "architecture-diag", "@orchestrator", payload.clone()).unwrap();
        assert!(path.exists());

        let list = list_artifacts(root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "architecture-diag");
        assert_eq!(list[0].kind, "mermaid");
        assert_eq!(list[0].author, "@orchestrator");

        let read = read_artifact(root, "architecture-diag").unwrap();
        assert_eq!(read, payload);
    }
}
