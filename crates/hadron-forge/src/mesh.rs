use serde::{Deserialize, Serialize};

/// Mesh offload configuration in hadron-forge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerRunCommand {
    pub runner: String,
    pub image: String,
    pub args: Vec<String>,
}

pub struct MeshForge;

impl MeshForge {
    pub fn build_docker_run_command(
        image: &str,
        workdir: &str,
        command: &[String],
    ) -> ContainerRunCommand {
        let mut args = vec![
            "run".to_string(),
            "--rm".to_string(),
            "-w".to_string(),
            workdir.to_string(),
            image.to_string(),
        ];
        args.extend_from_slice(command);

        ContainerRunCommand {
            runner: "docker".to_string(),
            image: image.to_string(),
            args,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mesh_forge_docker_command() {
        let cmd = MeshForge::build_docker_run_command("rust:latest", "/app", &["cargo".into(), "test".into()]);
        assert_eq!(cmd.runner, "docker");
        assert_eq!(cmd.image, "rust:latest");
        assert!(cmd.args.contains(&"cargo".to_string()));
    }
}
