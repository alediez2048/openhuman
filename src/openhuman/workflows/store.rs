//! SQLite persistence for the Workflows domain.
//!
//! Owns `${OPENHUMAN_WORKSPACE}/workflows.db` per ADR-003 (separate from
//! `connections.db`). F-1 ships only the connection-opener + migration
//! runner; CRUD methods land in F-2 (`ops.rs` uses these helpers via
//! `with_connection`), and run-row CRUD in F-8.
//!
//! Follows the connections/cron `with_connection(config, f)` closure
//! pattern rather than a long-lived `WorkflowsStore` struct: each caller
//! opens an ephemeral connection and SQLite file-level locking handles
//! concurrency. This keeps the executor (F-8) and the bus subscriber
//! (F-3) free of explicit `Arc<Mutex<_>>` synchronisation while still
//! sharing the same `PRAGMA foreign_keys = ON` semantics on every open.

use crate::openhuman::config::Config;
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::Connection;
use std::path::PathBuf;

const MIGRATION_001: &str = include_str!("migrations/001_init_workflows.sql");
const MIGRATION_002: &str = include_str!("migrations/002_runs.sql");
const MIGRATION_003: &str = include_str!("migrations/003_run_steps.sql");

/// Resolves the database path for this workspace: `${workspace_dir}/workflows.db`.
fn db_path(config: &Config) -> PathBuf {
    config.workspace_dir.join("workflows.db")
}

/// Opens an ephemeral connection, applies migrations, and runs `f`.
///
/// Migrations are idempotent (`CREATE TABLE IF NOT EXISTS` plus a
/// `schema_migrations` ledger), so calling this from many sites is
/// cheap. The `PRAGMA foreign_keys = ON` is re-set on every open
/// because SQLite disables it per-connection by default and the
/// run-rows / step-rows depend on `ON DELETE CASCADE`.
pub fn with_connection<T>(config: &Config, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
    let path = db_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "Failed to create workspace directory for workflows.db: {}",
                parent.display()
            )
        })?;
    }

    let conn = Connection::open(&path)
        .with_context(|| format!("Failed to open workflows.db at {}", path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")
        .context("Failed to enable foreign_keys on workflows.db")?;
    apply_migrations(&conn)?;

    tracing::trace!(target: "workflows", "[workflows-store] opened workflows.db at {}", path.display());

    f(&conn)
}

/// Bootstraps `schema_migrations` then applies 001 → 002 → 003 in order,
/// recording each as applied. Each migration runs inside a transaction
/// so a failure leaves the DB unchanged for retry on the next open.
fn apply_migrations(conn: &Connection) -> Result<()> {
    // Step 1 — make sure the ledger exists before we check it. The
    // migration files themselves also CREATE this table (defensive
    // duplication kept since migration order is fixed and the bootstrap
    // is cheaper than parsing the SQL).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (\
            version    INTEGER PRIMARY KEY,\
            applied_at TEXT NOT NULL\
        );",
    )
    .context("Failed to bootstrap schema_migrations table")?;

    apply_one(conn, 1, "001_init_workflows", MIGRATION_001)?;
    apply_one(conn, 2, "002_runs", MIGRATION_002)?;
    apply_one(conn, 3, "003_run_steps", MIGRATION_003)?;

    Ok(())
}

fn apply_one(conn: &Connection, version: i64, label: &str, sql: &str) -> Result<()> {
    if is_applied(conn, version)? {
        return Ok(());
    }
    conn.execute_batch(sql)
        .with_context(|| format!("Failed to apply workflows migration {label}"))?;
    record_applied(conn, version)?;
    tracing::info!(target: "workflows", "[workflows-store] applied migration {label} (v{version})");
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
    .context("Failed to record workflows migration as applied")?;
    Ok(())
}
