//! The **cargo_tree** family: workspace dependencies and feature flags.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::cargo_tree::{self, CargoPackageInfo};
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CargoTreeArgs {
    pub package: Option<String>,
}

fn format_cargo_tree(packages: Vec<CargoPackageInfo>) -> String {
    if packages.is_empty() {
        return "no matching workspace packages found".to_string();
    }
    packages
        .into_iter()
        .map(|p| {
            let mut out = format!("{} v{}", p.name, p.version);
            if p.is_workspace_member {
                out.push_str(" (workspace member)");
            }
            if !p.features.is_empty() {
                out.push_str(&format!("\n  features: {}", p.features.join(", ")));
            }
            if !p.dependencies.is_empty() {
                out.push_str("\n  dependencies:");
                for d in p.dependencies {
                    let opt_str = if d.optional { " (optional)" } else { "" };
                    let kind_str = match d.kind.as_deref() {
                        Some(k) if k != "normal" => format!(" [{}]", k),
                        _ => String::new(),
                    };
                    out.push_str(&format!("\n    - {} {}{}{}", d.name, d.req, kind_str, opt_str));
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[tool_router(router = cargo_tree_router, vis = "pub(super)")]
impl ForgeMcpServer {
    #[tool(
        name = "hadron_forge_cargo_tree",
        description = "View workspace packages, dependencies and feature flags via cargo metadata"
    )]
    pub async fn cargo_tree(
        &self,
        Parameters(args): Parameters<CargoTreeArgs>,
    ) -> Json<ToolResponse> {
        match cargo_tree::get_cargo_tree(&self.root, args.package.as_deref()) {
            Ok(packages) => Json(ToolResponse::success(Some(format_cargo_tree(packages)))),
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cargo_tree_tool_handler_returns_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname = \"dummy\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
        )
        .unwrap();
        std::fs::create_dir(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/lib.rs"), "").unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .cargo_tree(Parameters(CargoTreeArgs { package: None }))
            .await;
        assert!(res.0.ok);
        assert!(res.0.blocks.unwrap().contains("dummy v0.1.0"));
    }

    #[tokio::test]
    async fn cargo_tree_tool_handler_returns_error_for_non_cargo_dir() {
        let dir = tempfile::tempdir().unwrap();
        let server = ForgeMcpServer::new(dir.path());
        let res = server
            .cargo_tree(Parameters(CargoTreeArgs { package: None }))
            .await;
        assert!(!res.0.ok);
    }
}

