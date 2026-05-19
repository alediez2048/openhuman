//! SQLite persistence for the Connections domain.
//!
//! Owns `${OPENHUMAN_WORKSPACE}/connections.db`. Phase 0 ships only the
//! `generic_http_connections` table — CRUD lands in P0-3. This file provides
//! the connection opener and migration runner; module-level CRUD functions
//! are intentionally absent in P0-1.
//!
//! See `Automations/ADRs/ADR-003-separate-sqlite-databases.md`.

use crate::openhuman::config::Config;
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;

const MIGRATION_001: &str = include_str!("migrations/001_init_generic_http.sql");

/// Resolves the database path for this workspace: `${workspace_dir}/connections.db`.
fn db_path(config: &Config) -> std::path::PathBuf {
    config.workspace_dir.join("connections.db")
}

/// Opens the connection, applying migrations on first touch.
///
/// Migrations are idempotent (`CREATE TABLE IF NOT EXISTS`) and recorded in the
/// `schema_migrations` table so repeated calls are cheap.
pub(crate) fn with_connection<T>(
    config: &Config,
    f: impl FnOnce(&Connection) -> Result<T>,
) -> Result<T> {
    let path = db_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create workspace directory for connections.db: {}",
                parent.display()
            )
        })?;
    }

    let conn = Connection::open(&path)
        .with_context(|| format!("Failed to open connections.db at {}", path.display()))?;

    apply_migrations(&conn)?;
    f(&conn)
}

fn apply_migrations(conn: &Connection) -> Result<()> {
    // Bootstrap the migrations table on every open. Idempotent.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version    INTEGER PRIMARY KEY,
            applied_at TEXT NOT NULL
        );",
    )
    .context("Failed to bootstrap schema_migrations table")?;

    if !is_applied(conn, 1)? {
        conn.execute_batch(MIGRATION_001)
            .context("Failed to apply migration 001_init_generic_http")?;
        record_applied(conn, 1)?;
    }

    Ok(())
}

fn is_applied(conn: &Connection, version: i64) -> Result<bool> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version = ?1",
            [version],
            |row| row.get(0),
        )
        .context("Failed to check schema_migrations")?;
    Ok(count > 0)
}

fn record_applied(conn: &Connection, version: i64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
        rusqlite::params![version, Utc::now().to_rfc3339()],
    )
    .context("Failed to record migration as applied")?;
    Ok(())
}

/// Lists every `GenericHttpConnection` row in `connections.db`, newest-first.
///
/// Read-only path used by the aggregator (P0-2). CRUD lands in P0-3.
pub(crate) fn list_generic_http(
    config: &Config,
) -> Result<Vec<crate::openhuman::connections::types::GenericHttpConnection>> {
    use crate::openhuman::connections::types::{AuthKind, GenericHttpConnection, SecretRef};
    use chrono::DateTime;
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, base_url, auth_kind, secret_ref, default_headers, created_at, updated_at
             FROM generic_http_connections
             ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let name: String = row.get(1)?;
            let base_url: String = row.get(2)?;
            let auth_kind_json: String = row.get(3)?;
            let secret_ref_json: Option<String> = row.get(4)?;
            let default_headers_json: String = row.get(5)?;
            let created_at_raw: String = row.get(6)?;
            let updated_at_raw: String = row.get(7)?;
            let auth_kind: AuthKind = serde_json::from_str(&auth_kind_json)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let secret_ref: Option<SecretRef> = match secret_ref_json {
                Some(raw) => Some(
                    serde_json::from_str(&raw)
                        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
                ),
                None => None,
            };
            let default_headers: Vec<(String, String)> =
                serde_json::from_str(&default_headers_json)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_raw)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                .with_timezone(&chrono::Utc);
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_raw)
                .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?
                .with_timezone(&chrono::Utc);
            Ok(GenericHttpConnection {
                id,
                name,
                base_url,
                auth_kind,
                secret_ref,
                default_headers,
                created_at,
                updated_at,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
