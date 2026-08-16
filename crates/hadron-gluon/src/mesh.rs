use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Specification for offloading build, test, or quark turns to an isolated container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerOffloadSpec {
    pub container_image: String,
    pub working_dir: String,
    pub command: Vec<String>,
    pub environment_variables: HashMap<String, String>,
    pub secrets_to_mount: Vec<String>,
    pub timeout_seconds: u64,
    pub memory_limit_mb: Option<u64>,
    pub cpu_cores: Option<u32>,
}

/// Remote worker transport descriptor for offloading swarm quark turns over HTTP/WebSocket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteWorkerEndpoint {
    pub worker_id: String,
    pub endpoint_url: String,
    pub auth_token_secret_name: Option<String>,
    pub supported_quarks: Vec<String>,
    pub max_concurrency: usize,
    pub is_healthy: bool,
}

/// Mesh coordinator for routing and offloading tasks across local and remote workers.
#[derive(Debug, Clone, Default)]
pub struct SwarmMeshCoordinator {
    workers: HashMap<String, RemoteWorkerEndpoint>,
    active_offloads: HashMap<String, ContainerOffloadSpec>,
}

impl SwarmMeshCoordinator {
    pub fn new() -> Self {
        Self {
            workers: HashMap::new(),
            active_offloads: HashMap::new(),
        }
    }

    pub fn register_worker(&mut self, worker: RemoteWorkerEndpoint) {
        self.workers.insert(worker.worker_id.clone(), worker);
    }

    pub fn unregister_worker(&mut self, worker_id: &str) -> Option<RemoteWorkerEndpoint> {
        self.workers.remove(worker_id)
    }

    pub fn list_workers(&self) -> Vec<RemoteWorkerEndpoint> {
        self.workers.values().cloned().collect()
    }

    pub fn find_available_worker_for_quark(&self, quark_id: &str) -> Option<&RemoteWorkerEndpoint> {
        self.workers
            .values()
            .find(|w| w.is_healthy && w.supported_quarks.iter().any(|q| q == quark_id || q == "*"))
    }

    pub fn schedule_offload(&mut self, job_id: String, spec: ContainerOffloadSpec) {
        self.active_offloads.insert(job_id, spec);
    }

    pub fn get_offload(&self, job_id: &str) -> Option<&ContainerOffloadSpec> {
        self.active_offloads.get(job_id)
    }

    pub fn complete_offload(&mut self, job_id: &str) -> Option<ContainerOffloadSpec> {
        self.active_offloads.remove(job_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_swarm_mesh_worker_routing() {
        let mut mesh = SwarmMeshCoordinator::new();
        let worker1 = RemoteWorkerEndpoint {
            worker_id: "worker-gpu-1".into(),
            endpoint_url: "https://cloud.hadron.internal/worker-1".into(),
            auth_token_secret_name: Some("REMOTE_WORKER_KEY".into()),
            supported_quarks: vec!["heavy-build".into(), "mutation-runner".into()],
            max_concurrency: 4,
            is_healthy: true,
        };

        mesh.register_worker(worker1);

        let found = mesh.find_available_worker_for_quark("mutation-runner");
        assert!(found.is_some());
        assert_eq!(found.unwrap().worker_id, "worker-gpu-1");

        let not_found = mesh.find_available_worker_for_quark("unknown-quark");
        assert!(not_found.is_none());
    }
}
