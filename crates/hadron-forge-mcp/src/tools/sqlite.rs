//! The **sqlite** family: local SQLite database queries, schema inspection, and migration.
//!
//! Provides direct SQLite query execution, schema introspection, transactional migration
//! verification, and structured data export strictly within the worktree jail.

use super::{ForgeMcpServer, ToolResponse};
use hadron_forge::sqlite::SqliteEngine;
use rmcp::handler::server::wrapper::{Json, Parameters};
use rmcp::schemars::JsonSchema;
use rmcp::{tool, tool_router};
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SqliteQueryArgs {
    /// The relative path to the SQLite database file (e.g. `data/app.db`).
    pub db_path: String,
    /// The SQL query or statement to execute.
    pub sql: String,
    /// Whether to open the database in read-only mode (default: false).
    #[serde(default)]
    pub read_only: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SqliteSchemaArgs {
    /// The relative path to the SQLite database file (e.g. `data/app.db`).
    pub db_path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SqliteMigrateArgs {
    /// The relative path to the SQLite database file (e.g. `data/app.db`).
    pub db_path: String,
    /// Multi-statement SQL migration script to execute transactionally.
    pub sql: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SqliteExportArgs {
    /// The relative path to the SQLite database file (e.g. `data/app.db`).
    pub db_path: String,
    /// The SQL SELECT query to execute and export.
    pub sql: String,
    /// Output export format: `markdown` (default), `json`, or `csv`.
    #[serde(default)]
    pub format: Option<String>,
}

#[tool_router(router = sqlite_router, vis = "pub(super)")]
impl ForgeMcpServer {
    /// Execute a SQL query or statement on a local SQLite database.
    #[tool(
        name = "hadron_forge_sqlite_query",
        description = "Execute a SQL query (SELECT, INSERT, UPDATE, DELETE, CREATE) on a local SQLite database and return structured rows or affected count."
    )]
    pub async fn sqlite_query(
        &self,
        Parameters(args): Parameters<SqliteQueryArgs>,
    ) -> Json<ToolResponse> {
        let engine = SqliteEngine::new(self.root.clone());
        let read_only = args.read_only.unwrap_or(false);

        match engine.query(&args.db_path, &args.sql, read_only) {
            Ok(res) => match serde_json::to_string_pretty(&res) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Introspect full database schema (tables, columns, types, indexes).
    #[tool(
        name = "hadron_forge_sqlite_schema",
        description = "Inspect the full schema of a local SQLite database, including tables, columns, data types, nullability, defaults, primary keys, and indexes."
    )]
    pub async fn sqlite_schema(
        &self,
        Parameters(args): Parameters<SqliteSchemaArgs>,
    ) -> Json<ToolResponse> {
        let engine = SqliteEngine::new(self.root.clone());

        match engine.schema(&args.db_path) {
            Ok(schema) => match serde_json::to_string_pretty(&schema) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Execute a multi-statement SQL migration script transactionally with rollback on error.
    #[tool(
        name = "hadron_forge_sqlite_migrate",
        description = "Execute a multi-statement SQL migration script transactionally with automatic rollback on error."
    )]
    pub async fn sqlite_migrate(
        &self,
        Parameters(args): Parameters<SqliteMigrateArgs>,
    ) -> Json<ToolResponse> {
        let engine = SqliteEngine::new(self.root.clone());

        match engine.migrate(&args.db_path, &args.sql) {
            Ok(res) => match serde_json::to_string_pretty(&res) {
                Ok(json) => Json(ToolResponse::success(Some(json))),
                Err(e) => Json(ToolResponse::error(format!("serialization error: {e}"))),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }

    /// Export query results to formatted Markdown table, JSON, or CSV.
    #[tool(
        name = "hadron_forge_sqlite_export",
        description = "Execute a SQL query and export formatted results as Markdown table, JSON, or CSV."
    )]
    pub async fn sqlite_export(
        &self,
        Parameters(args): Parameters<SqliteExportArgs>,
    ) -> Json<ToolResponse> {
        let engine = SqliteEngine::new(self.root.clone());
        let fmt = args.format.unwrap_or_else(|| "markdown".to_string());

        match engine.query(&args.db_path, &args.sql, true) {
            Ok(res) => match engine.export(&res, &fmt) {
                Ok(formatted) => Json(ToolResponse::success(Some(formatted))),
                Err(e) => Json(ToolResponse::error(e.to_string())),
            },
            Err(e) => Json(ToolResponse::error(e.to_string())),
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn sqlite_router_query_and_schema() {
        let temp = tempdir().unwrap();
        let server = ForgeMcpServer::new(temp.path().to_path_buf());
        let db = "test.db";

        // Migrate
        let mig_res = server
            .sqlite_migrate(Parameters(SqliteMigrateArgs {
                db_path: db.into(),
                sql: "CREATE TABLE items (id INTEGER PRIMARY KEY, name TEXT); INSERT INTO items (name) VALUES ('widget');".into(),
            }))
            .await;
        assert!(mig_res.0.ok);

        // Schema
        let schema_res = server
            .sqlite_schema(Parameters(SqliteSchemaArgs {
                db_path: db.into(),
            }))
            .await;
        assert!(schema_res.0.ok);
        assert!(schema_res.0.blocks.unwrap().contains("items"));

        // Query
        let query_res = server
            .sqlite_query(Parameters(SqliteQueryArgs {
                db_path: db.into(),
                sql: "SELECT * FROM items;".into(),
                read_only: Some(true),
            }))
            .await;
        assert!(query_res.0.ok);
        assert!(query_res.0.blocks.unwrap().contains("widget"));

        // Export Markdown
        let export_res = server
            .sqlite_export(Parameters(SqliteExportArgs {
                db_path: db.into(),
                sql: "SELECT * FROM items;".into(),
                format: Some("markdown".into()),
            }))
            .await;
        assert!(export_res.0.ok);
        assert!(export_res.0.blocks.unwrap().contains("| id | name |"));
    }
}
