use hadron_lattice::artifacts::*;
use tempfile::tempdir;

#[test]
fn test_artifact_bus_lifecycle() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // 1. Publish markdown plan artifact
    let plan_payload = ArtifactPayload::Markdown("# Phase 5 Plan\nTask 1...".into());
    let path = publish_artifact(root, "phase-5-plan", "@agy", plan_payload).unwrap();
    assert!(path.exists());

    // 2. Publish openapi json artifact
    let openapi_payload = ArtifactPayload::OpenApiJson(serde_json::json!({
        "openapi": "3.0.0",
        "info": { "title": "Hadron Swarm API", "version": "1.0.0" }
    }));
    publish_artifact(root, "swarm-api-spec", "@architect", openapi_payload).unwrap();

    // 3. List artifacts
    let list = list_artifacts(root).unwrap();
    assert_eq!(list.len(), 2);

    // 4. Read specific artifact
    let read_back = read_artifact(root, "swarm-api-spec").unwrap();
    match read_back {
        ArtifactPayload::OpenApiJson(v) => {
            assert_eq!(v["info"]["title"], "Hadron Swarm API");
        }
        _ => panic!("Expected OpenApiJson variant"),
    }
}
