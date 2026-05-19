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

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
