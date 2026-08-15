//! The **scaffold** family: automated project template generation, stack detection, and dependency resolution.
//!
//! Exposes `hadron_forge_scaffold` to initialize projects (Rust, React/Vite, Vue, Svelte, Python, Next.js),
//! detect framework and runtime stack, and safely mutate dependency manifests.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::scaffold::{
    scaffold_project, DependencySpec, ProjectTemplate, ScaffoldAction, ScaffoldInput,
};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DependencySpecParam {
    pub name: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub dev: Option<bool>,
    #[serde(default)]
    pub features: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ScaffoldArgs {
    /// Action: "init_project", "add_dependency", "audit_dependencies", "detect_stack".
    pub action: String,
    /// Template for init_project ("rust_binary", "rust_library", "vite_react_ts", "vite_vue_ts", "vite_svelte_ts", "vite_vanilla_ts", "python_uv", "next_js", "static_html").
    #[serde(default)]
    pub template: Option<String>,
    /// Target directory relative to project root (defaults to ".").
    #[serde(default)]
    pub target_dir: Option<String>,
    /// Project or package name (e.g. "my-service").
    #[serde(default)]
    pub name: Option<String>,
    /// Dependencies to add (for "add_dependency" action).
    #[serde(default)]
    pub dependencies: Option<Vec<DependencySpecParam>>,
}

#[tool_router(router = scaffold_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_scaffold",
        description = "Initialize project boilerplates (Rust, Vite React/Vue/Svelte, Python, Next.js), detect tech stacks, add dependencies to manifests, or audit supply-chain security."
    )]
    pub async fn scaffold(&self, Parameters(args): Parameters<ScaffoldArgs>) -> Json<ToolResponse> {
        let action = match args.action.as_str() {
            "init_project" => ScaffoldAction::InitProject,
            "add_dependency" => ScaffoldAction::AddDependency,
            "audit_dependencies" => ScaffoldAction::AuditDependencies,
            "detect_stack" => ScaffoldAction::DetectStack,
            other => {
                return Json(ToolResponse::error(format!(
                    "Unknown action '{other}'. Expected init_project, add_dependency, audit_dependencies, or detect_stack"
                )))
            }
        };

        let template = args.template.as_deref().and_then(|t| match t {
            "rust_binary" | "rust" => Some(ProjectTemplate::RustBinary),
            "rust_library" | "rust_lib" => Some(ProjectTemplate::RustLibrary),
            "vite_react_ts" | "react" => Some(ProjectTemplate::ViteReactTs),
            "vite_vue_ts" | "vue" => Some(ProjectTemplate::ViteVueTs),
            "vite_svelte_ts" | "svelte" => Some(ProjectTemplate::ViteSvelteTs),
            "vite_vanilla_ts" | "vanilla" => Some(ProjectTemplate::ViteVanillaTs),
            "python_uv" | "python" => Some(ProjectTemplate::PythonUv),
            "next_js" | "next" => Some(ProjectTemplate::NextJs),
            "static_html" | "html" => Some(ProjectTemplate::StaticHtml),
            _ => None,
        });

        let deps = args.dependencies.map(|ds| {
            ds.into_iter()
                .map(|d| DependencySpec {
                    name: d.name,
                    version: d.version,
                    dev: d.dev,
                    features: d.features,
                })
                .collect()
        });

        let input = ScaffoldInput {
            action,
            template,
            target_dir: args.target_dir,
            name: args.name,
            dependencies: deps,
        };

        match scaffold_project(&self.root, &input) {
            Ok(output) => {
                let json = serde_json::to_string_pretty(&output)
                    .unwrap_or_else(|_| output.message.clone());
                Json(ToolResponse::success(Some(json)))
            }
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scaffold_tool_inits_and_detects() {
        let temp = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path());

        let res = server
            .scaffold(Parameters(ScaffoldArgs {
                action: "init_project".into(),
                template: Some("rust_binary".into()),
                target_dir: Some("app".into()),
                name: Some("test-app".into()),
                dependencies: None,
            }))
            .await;

        assert!(res.0.ok);
        let blocks = res.0.blocks.unwrap();
        assert!(blocks.contains("Cargo.toml"));
        assert!(blocks.contains("rust"));
    }
}
