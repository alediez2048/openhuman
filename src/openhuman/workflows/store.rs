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

// ── CRUD helpers (F-2) ──────────────────────────────────────────────────
//
// Each helper opens its own connection via `with_connection`. Callers
// pass a `&Config`; SQLite file-level locking handles concurrency.
//
// Encoding: `origin`, `health`, `trigger`, `nodes`, `edges`, `settings`
// are JSON-encoded into the matching `*_json` (or `health` / `origin`)
// TEXT columns. Phase 1 does not normalise sub-rows.

use crate::openhuman::workflows::types::{
    HealthFilter, ListFilter, Workflow, WorkflowId, WorkflowOrigin,
};
use chrono::DateTime;
use rusqlite::Row;

/// Inserts a new `workflows` row. Caller is responsible for setting
/// `id`, `created_at`, `updated_at`, and computing the initial `health`.
pub fn insert_workflow(config: &Config, wf: &Workflow) -> Result<()> {
    with_connection(config, |db| {
        let (origin, health, trigger, nodes, edges, settings) = encode_blobs(wf)?;
        db.execute(
            "INSERT INTO workflows \
             (id, schema_version, name, description, enabled, origin, health, \
              trigger_json, nodes_json, edges_json, settings_json, \
              created_at, updated_at, last_run_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            rusqlite::params![
                wf.id,
                wf.schema_version,
                wf.name,
                wf.description,
                wf.enabled as i64,
                origin,
                health,
                trigger,
                nodes,
                edges,
                settings,
                wf.created_at.to_rfc3339(),
                wf.updated_at.to_rfc3339(),
                wf.last_run_at.map(|t| t.to_rfc3339()),
            ],
        )
        .context("Failed to insert workflows row")?;
        Ok(())
    })
}

/// Fetches one workflow by id; `Ok(None)` when the id is unknown.
pub fn get_workflow(config: &Config, id: &WorkflowId) -> Result<Option<Workflow>> {
    with_connection(config, |db| {
        let mut stmt = db
            .prepare(
                "SELECT id, schema_version, name, description, enabled, origin, health, \
                 trigger_json, nodes_json, edges_json, settings_json, \
                 created_at, updated_at, last_run_at \
                 FROM workflows WHERE id = ?1",
            )
            .context("Failed to prepare get_workflow statement")?;
        let mut rows = stmt
            .query(rusqlite::params![id])
            .context("Failed to query get_workflow")?;
        if let Some(row) = rows.next().context("Failed to read get_workflow row")? {
            Ok(Some(row_to_workflow(row)?))
        } else {
            Ok(None)
        }
    })
}

/// Lists workflows matching `filter`, sorted by `updated_at DESC` so
/// the list view shows the most recently changed first.
pub fn list_workflows(config: &Config, filter: &ListFilter) -> Result<Vec<Workflow>> {
    // Filtering happens in code rather than SQL because the JSON-encoded
    // `health` column makes WHERE clauses fragile and the table is
    // bounded in size (Phase 1 expects O(10) workflows per user). When
    // workflow counts grow, we can add discriminator columns + indexes
    // — for now, scan + filter is correct and small.
    with_connection(config, |db| {
        let mut stmt = db
            .prepare(
                "SELECT id, schema_version, name, description, enabled, origin, health, \
                 trigger_json, nodes_json, edges_json, settings_json, \
                 created_at, updated_at, last_run_at \
                 FROM workflows ORDER BY updated_at DESC",
            )
            .context("Failed to prepare list_workflows statement")?;
        let workflows = stmt
            .query_map([], |row| Ok(row_to_workflow(row)))
            .context("Failed to query list_workflows")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to materialise list_workflows row")?
            .into_iter()
            .collect::<Result<Vec<Workflow>>>()?;

        let search_lc = filter.search.as_deref().map(str::to_lowercase);
        let filtered = workflows
            .into_iter()
            .filter(|wf| match filter.enabled {
                Some(want) => wf.enabled == want,
                None => true,
            })
            .filter(|wf| match filter.health_state {
                Some(want) => matches_health_filter(&wf.health, want),
                None => true,
            })
            .filter(|wf| match &search_lc {
                Some(needle) => wf.name.to_lowercase().contains(needle.as_str()),
                None => true,
            })
            .collect();
        Ok(filtered)
    })
}

/// Replaces the mutable fields on an existing row in place. Origin and
/// identity columns (id, schema_version, created_at) are not touched.
/// Returns `false` when no row matched the id.
pub fn update_workflow(config: &Config, wf: &Workflow) -> Result<bool> {
    with_connection(config, |db| {
        let (_origin, health, trigger, nodes, edges, settings) = encode_blobs(wf)?;
        let rows = db
            .execute(
                "UPDATE workflows SET \
                 name = ?2, description = ?3, enabled = ?4, health = ?5, \
                 trigger_json = ?6, nodes_json = ?7, edges_json = ?8, settings_json = ?9, \
                 updated_at = ?10, last_run_at = ?11 \
                 WHERE id = ?1",
                rusqlite::params![
                    wf.id,
                    wf.name,
                    wf.description,
                    wf.enabled as i64,
                    health,
                    trigger,
                    nodes,
                    edges,
                    settings,
                    wf.updated_at.to_rfc3339(),
                    wf.last_run_at.map(|t| t.to_rfc3339()),
                ],
            )
            .context("Failed to update workflows row")?;
        Ok(rows > 0)
    })
}

/// Flips the `enabled` bit and bumps `updated_at`. Returns `false`
/// when no row matched (caller maps to `not_found`).
pub fn set_enabled(
    config: &Config,
    id: &WorkflowId,
    enabled: bool,
    updated_at: DateTime<Utc>,
) -> Result<bool> {
    with_connection(config, |db| {
        let rows = db
            .execute(
                "UPDATE workflows SET enabled = ?2, updated_at = ?3 WHERE id = ?1",
                rusqlite::params![id, enabled as i64, updated_at.to_rfc3339()],
            )
            .context("Failed to update workflows.enabled")?;
        Ok(rows > 0)
    })
}

/// Hard-deletes a workflow row. The FK cascades drop `workflow_runs`
/// and `workflow_run_steps`. Returns `false` when no row matched.
///
/// TODO(F-15): replace with a 30-day soft-delete sweep per FR-1.3.4.
pub fn delete_workflow(config: &Config, id: &WorkflowId) -> Result<bool> {
    with_connection(config, |db| {
        let rows = db
            .execute("DELETE FROM workflows WHERE id = ?1", rusqlite::params![id])
            .context("Failed to delete workflows row")?;
        Ok(rows > 0)
    })
}

/// Returns every `template_id` referenced by a workflow whose origin is
/// `Seed { template_id: ... }`. F-5's catalog dedup query consumes this
/// to hide already-seeded templates.
pub fn list_seed_origins(config: &Config) -> Result<Vec<String>> {
    with_connection(config, |db| {
        let mut stmt = db
            .prepare("SELECT origin FROM workflows")
            .context("Failed to prepare list_seed_origins statement")?;
        let rows: Vec<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .filter_map(|r| r.ok())
            .collect();
        let mut out = Vec::new();
        for raw in rows {
            if let Ok(WorkflowOrigin::Seed { template_id }) = serde_json::from_str(&raw) {
                out.push(template_id);
            }
        }
        Ok(out)
    })
}

// ── encoding helpers ────────────────────────────────────────────────────

/// JSON-encodes the six structured columns of a workflow row. Returns
/// `(origin, health, trigger, nodes, edges, settings)`.
fn encode_blobs(wf: &Workflow) -> Result<(String, String, String, String, String, String)> {
    let origin = serde_json::to_string(&wf.origin).context("encode origin")?;
    let health = serde_json::to_string(&wf.health).context("encode health")?;
    let trigger = serde_json::to_string(&wf.trigger).context("encode trigger")?;
    let nodes = serde_json::to_string(&wf.nodes).context("encode nodes")?;
    let edges = serde_json::to_string(&wf.edges).context("encode edges")?;
    let settings = serde_json::to_string(&wf.settings).context("encode settings")?;
    Ok((origin, health, trigger, nodes, edges, settings))
}

fn row_to_workflow(row: &Row<'_>) -> Result<Workflow> {
    let id: String = row.get(0).context("read id")?;
    let schema_version: u32 = row.get::<_, i64>(1).context("read schema_version")? as u32;
    let name: String = row.get(2).context("read name")?;
    let description: Option<String> = row.get(3).context("read description")?;
    let enabled: i64 = row.get(4).context("read enabled")?;
    let origin_raw: String = row.get(5).context("read origin")?;
    let health_raw: String = row.get(6).context("read health")?;
    let trigger_raw: String = row.get(7).context("read trigger_json")?;
    let nodes_raw: String = row.get(8).context("read nodes_json")?;
    let edges_raw: String = row.get(9).context("read edges_json")?;
    let settings_raw: String = row.get(10).context("read settings_json")?;
    let created_at_raw: String = row.get(11).context("read created_at")?;
    let updated_at_raw: String = row.get(12).context("read updated_at")?;
    let last_run_at_raw: Option<String> = row.get(13).context("read last_run_at")?;

    Ok(Workflow {
        id,
        schema_version,
        name,
        description,
        enabled: enabled != 0,
        origin: serde_json::from_str(&origin_raw).context("decode origin")?,
        health: serde_json::from_str(&health_raw).context("decode health")?,
        trigger: serde_json::from_str(&trigger_raw).context("decode trigger")?,
        nodes: serde_json::from_str(&nodes_raw).context("decode nodes")?,
        edges: serde_json::from_str(&edges_raw).context("decode edges")?,
        settings: serde_json::from_str(&settings_raw).context("decode settings")?,
        created_at: DateTime::parse_from_rfc3339(&created_at_raw)
            .context("parse created_at")?
            .with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_at_raw)
            .context("parse updated_at")?
            .with_timezone(&Utc),
        last_run_at: last_run_at_raw
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|t| t.with_timezone(&Utc)))
            .transpose()
            .context("parse last_run_at")?,
    })
}

fn matches_health_filter(
    health: &crate::openhuman::workflows::types::WorkflowHealth,
    want: HealthFilter,
) -> bool {
    use crate::openhuman::workflows::types::WorkflowHealth as H;
    matches!(
        (health, want),
        (H::Ready, HealthFilter::Ready)
            | (H::NeedsConnections { .. }, HealthFilter::NeedsConnections)
            | (H::LastRunFailed { .. }, HealthFilter::LastRunFailed)
            | (H::SessionExpired { .. }, HealthFilter::SessionExpired)
    )
}
