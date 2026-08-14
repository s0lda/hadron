//! Local SQLite database query and schema engine for jailed agents.
//!
//! Provides direct SQLite query execution, schema introspection, transactional migration
//! verification, and structured data export strictly within the worktree jail.
//!
//! **Invariants:**
//! 1. Strict Jail: Database file path must resolve within the workspace `Root` or granted external roots.
//! 2. Transactional safety: Migration execution is wrapped in explicit transactions with automatic rollback on error.
//! 3. Structured output: Returns typed JSON values, column names, execution latency, and formatted tables.

use std::time::Instant;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};

use crate::file::{resolve_jailed_path, ForgeError, Root};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QueryResult {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub affected_rows: usize,
    pub execution_ms: u64,
    pub is_query: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ColumnInfo {
    pub cid: i64,
    pub name: String,
    pub type_name: String,
    pub not_null: bool,
    pub default_value: Option<String>,
    pub primary_key: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TableInfo {
    pub name: String,
    pub sql: Option<String>,
    pub columns: Vec<ColumnInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IndexInfo {
    pub name: String,
    pub table: String,
    pub unique: bool,
    pub sql: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DatabaseSchema {
    pub tables: Vec<TableInfo>,
    pub indexes: Vec<IndexInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationResult {
    pub success: bool,
    pub statements_executed: usize,
    pub execution_ms: u64,
    pub message: String,
}

#[derive(Clone)]
pub struct SqliteEngine {
    root: Root,
}

impl SqliteEngine {
    pub fn new(root: Root) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Root {
        &self.root
    }

    fn resolve_db_path(&self, db_path: &str) -> Result<std::path::PathBuf, ForgeError> {
        let resolved = resolve_jailed_path(&self.root, db_path)?;
        Ok(resolved)
    }

    /// Execute a raw SQL query or DDL/DML statement on a local SQLite database.
    pub fn query(
        &self,
        db_path: &str,
        sql: &str,
        read_only: bool,
    ) -> Result<QueryResult, ForgeError> {
        let path = self.resolve_db_path(db_path)?;
        let start = Instant::now();

        let flags = if read_only {
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI
        } else {
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_URI
        };

        let conn = Connection::open_with_flags(&path, flags)
            .map_err(|e| ForgeError::Io(format!("failed to open sqlite database {path:?}: {e}")))?;

        let mut stmt = conn
            .prepare(sql)
            .map_err(|e| ForgeError::Rejected(format!("invalid SQL statement: {e}")))?;

        let column_count = stmt.column_count();
        let column_names: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let is_query = column_count > 0;

        if is_query {
            let mut rows_out = Vec::new();
            let mut rows_iter = stmt
                .query([])
                .map_err(|e| ForgeError::Rejected(format!("SQL query failed: {e}")))?;

            while let Ok(Some(row)) = rows_iter.next() {
                let mut row_vals = Vec::with_capacity(column_count);
                for i in 0..column_count {
                    let val_ref = row.get_ref(i).unwrap_or(ValueRef::Null);
                    let val = match val_ref {
                        ValueRef::Null => serde_json::Value::Null,
                        ValueRef::Integer(i) => serde_json::Value::Number(i.into()),
                        ValueRef::Real(f) => serde_json::Number::from_f64(f)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        ValueRef::Text(t) => {
                            let s = String::from_utf8_lossy(t).into_owned();
                            serde_json::Value::String(s)
                        }
                        ValueRef::Blob(b) => {
                            serde_json::Value::String(format!("<blob {} bytes>", b.len()))
                        }
                    };
                    row_vals.push(val);
                }
                rows_out.push(row_vals);
            }

            let elapsed = start.elapsed().as_millis() as u64;
            let count = rows_out.len();
            Ok(QueryResult {
                columns: column_names,
                rows: rows_out,
                affected_rows: count,
                execution_ms: elapsed,
                is_query: true,
            })
        } else {
            let affected = stmt
                .execute([])
                .map_err(|e| ForgeError::Rejected(format!("SQL execution failed: {e}")))?;

            let elapsed = start.elapsed().as_millis() as u64;
            Ok(QueryResult {
                columns: Vec::new(),
                rows: Vec::new(),
                affected_rows: affected,
                execution_ms: elapsed,
                is_query: false,
            })
        }
    }

    /// Introspect full database schema (tables, columns, types, indexes).
    pub fn schema(&self, db_path: &str) -> Result<DatabaseSchema, ForgeError> {
        let path = self.resolve_db_path(db_path)?;

        let conn = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .map_err(|e| ForgeError::Io(format!("failed to open sqlite database {path:?}: {e}")))?;

        // Query sqlite_master for tables
        let mut stmt = conn
            .prepare("SELECT name, sql FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| ForgeError::Rejected(format!("failed to query tables: {e}")))?;

        let mut tables = Vec::new();
        let table_rows = stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let sql: Option<String> = row.get(1)?;
                Ok((name, sql))
            })
            .map_err(|e| ForgeError::Rejected(format!("failed to read table schema: {e}")))?;

        for table_res in table_rows.flatten() {
            let (name, sql) = table_res;

            // Query columns via pragma table_info
            let pragma_sql = format!("PRAGMA table_info(\"{}\")", name.replace('"', "\"\""));
            let mut pragma_stmt = conn
                .prepare(&pragma_sql)
                .map_err(|e| ForgeError::Rejected(format!("failed to query pragma for {name}: {e}")))?;

            let cols = pragma_stmt
                .query_map([], |r| {
                    Ok(ColumnInfo {
                        cid: r.get(0)?,
                        name: r.get(1)?,
                        type_name: r.get(2)?,
                        not_null: r.get::<_, i64>(3)? != 0,
                        default_value: r.get(4)?,
                        primary_key: r.get::<_, i64>(5)? != 0,
                    })
                })
                .map_err(|e| ForgeError::Rejected(format!("failed to read column metadata for {name}: {e}")))?
                .flatten()
                .collect();

            tables.push(TableInfo {
                name,
                sql,
                columns: cols,
            });
        }

        // Query indexes
        let mut idx_stmt = conn
            .prepare("SELECT name, tbl_name, sql FROM sqlite_master WHERE type='index' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .map_err(|e| ForgeError::Rejected(format!("failed to query indexes: {e}")))?;

        let indexes = idx_stmt
            .query_map([], |row| {
                let name: String = row.get(0)?;
                let tbl_name: String = row.get(1)?;
                let sql: Option<String> = row.get(2)?;
                let unique = sql
                    .as_deref()
                    .map(|s| s.to_uppercase().contains("UNIQUE"))
                    .unwrap_or(false);
                Ok(IndexInfo {
                    name,
                    table: tbl_name,
                    unique,
                    sql,
                })
            })
            .map_err(|e| ForgeError::Rejected(format!("failed to read index metadata: {e}")))?
            .flatten()
            .collect();

        Ok(DatabaseSchema { tables, indexes })
    }

    /// Execute a multi-statement SQL migration script transactionally with rollback on error.
    pub fn migrate(&self, db_path: &str, migration_sql: &str) -> Result<MigrationResult, ForgeError> {
        let path = self.resolve_db_path(db_path)?;
        let start = Instant::now();

        let mut conn = Connection::open(&path)
            .map_err(|e| ForgeError::Io(format!("failed to open sqlite database {path:?}: {e}")))?;

        let tx = conn
            .transaction()
            .map_err(|e| ForgeError::Io(format!("failed to start transaction: {e}")))?;

        let statements: Vec<&str> = migration_sql
            .split(';')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect();

        let count = statements.len();

        for (idx, sql) in statements.iter().enumerate() {
            if let Err(e) = tx.execute(sql, []) {
                let _ = tx.rollback();
                return Ok(MigrationResult {
                    success: false,
                    statements_executed: idx,
                    execution_ms: start.elapsed().as_millis() as u64,
                    message: format!("Migration failed at statement #{idx} ({sql}): {e}"),
                });
            }
        }

        tx.commit()
            .map_err(|e| ForgeError::Io(format!("failed to commit migration transaction: {e}")))?;

        Ok(MigrationResult {
            success: true,
            statements_executed: count,
            execution_ms: start.elapsed().as_millis() as u64,
            message: format!("Successfully applied {count} migration statement(s)"),
        })
    }

    /// Format query result as JSON, CSV, or Markdown table.
    pub fn export(&self, result: &QueryResult, format: &str) -> Result<String, ForgeError> {
        match format.to_lowercase().as_str() {
            "json" => {
                let mut list = Vec::new();
                for row in &result.rows {
                    let mut map = serde_json::Map::new();
                    for (col, val) in result.columns.iter().zip(row.iter()) {
                        map.insert(col.clone(), val.clone());
                    }
                    list.push(serde_json::Value::Object(map));
                }
                serde_json::to_string_pretty(&list)
                    .map_err(|e| ForgeError::Rejected(format!("JSON export error: {e}")))
            }
            "csv" => {
                let mut out = String::new();
                out.push_str(&result.columns.join(","));
                out.push('\n');
                for row in &result.rows {
                    let row_strs: Vec<String> = row
                        .iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => format!("\"{}\"", s.replace('"', "\"\"")),
                            other => other.to_string(),
                        })
                        .collect();
                    out.push_str(&row_strs.join(","));
                    out.push('\n');
                }
                Ok(out)
            }
            "markdown" | "md" => {
                if result.columns.is_empty() {
                    return Ok(format!("_0 rows affected (execution: {}ms)_", result.execution_ms));
                }
                let mut out = String::new();
                out.push_str("| ");
                out.push_str(&result.columns.join(" | "));
                out.push_str(" |\n| ");
                out.push_str(
                    &result
                        .columns
                        .iter()
                        .map(|_| "---")
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
                out.push_str(" |\n");
                for row in &result.rows {
                    out.push_str("| ");
                    let row_strs: Vec<String> = row
                        .iter()
                        .map(|v| match v {
                            serde_json::Value::String(s) => s.replace('|', "\\|"),
                            serde_json::Value::Null => "NULL".to_string(),
                            other => other.to_string(),
                        })
                        .collect();
                    out.push_str(&row_strs.join(" | "));
                    out.push_str(" |\n");
                }
                Ok(out)
            }
            other => Err(ForgeError::Rejected(format!(
                "unsupported export format {other:?} (expected 'json', 'csv', or 'markdown')"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sqlite_query_schema_and_migrate() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path().to_path_buf());
        let engine = SqliteEngine::new(root);

        let db_rel = "app.db";

        // Migration test
        let migration_sql = "
            CREATE TABLE users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                email TEXT UNIQUE,
                age INTEGER DEFAULT 18
            );
            CREATE TABLE posts (
                id INTEGER PRIMARY KEY,
                user_id INTEGER,
                title TEXT NOT NULL,
                FOREIGN KEY(user_id) REFERENCES users(id)
            );
            CREATE INDEX idx_users_email ON users(email);
        ";

        let mig_res = engine.migrate(db_rel, migration_sql).unwrap();
        assert!(mig_res.success);
        assert_eq!(mig_res.statements_executed, 3);

        // Schema test
        let schema = engine.schema(db_rel).unwrap();
        assert_eq!(schema.tables.len(), 2);
        let users_table = schema.tables.iter().find(|t| t.name == "users").unwrap();
        assert_eq!(users_table.columns.len(), 4);
        assert!(users_table.columns.iter().any(|c| c.name == "email"));

        // Insert query test
        let insert_res = engine
            .query(
                db_rel,
                "INSERT INTO users (name, email, age) VALUES ('Alice', 'alice@hadron.io', 30);",
                false,
            )
            .unwrap();
        assert_eq!(insert_res.affected_rows, 1);

        // Select query test
        let select_res = engine
            .query(db_rel, "SELECT id, name, email, age FROM users;", true)
            .unwrap();
        assert_eq!(select_res.rows.len(), 1);
        assert_eq!(select_res.columns, vec!["id", "name", "email", "age"]);
        assert_eq!(select_res.rows[0][1], serde_json::Value::String("Alice".into()));

        // Export test
        let md = engine.export(&select_res, "markdown").unwrap();
        assert!(md.contains("| id | name | email | age |"));
        assert!(md.contains("Alice"));

        let csv = engine.export(&select_res, "csv").unwrap();
        assert!(csv.contains("id,name,email,age"));
        assert!(csv.contains("\"Alice\""));
    }

    #[test]
    fn sqlite_migration_rolls_back_on_error() {
        let temp = tempfile::tempdir().unwrap();
        let root = Root::new(temp.path().to_path_buf());
        let engine = SqliteEngine::new(root);

        let bad_migration = "
            CREATE TABLE items (id INTEGER PRIMARY KEY);
            THIS IS INVALID SQL STATEMENT;
        ";

        let res = engine.migrate("broken.db", bad_migration).unwrap();
        assert!(!res.success);
        assert!(res.message.contains("Migration failed"));
    }
}
