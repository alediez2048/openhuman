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
use rusqlite::OptionalExtension;
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

// ── F-8 run-row CRUD ────────────────────────────────────────────────────

use crate::openhuman::workflows::types::{
    Run, RunId, RunStatus, RunStep, RunStepId, TriggerSource,
};

/// Maximum byte length for `workflow_run_steps.output_json` per
/// NFR-2.3.5. F-8 truncates output to this size on the UTF-8 boundary
/// before persisting; the truncation marker (`"\n…[truncated]"`)
/// counts toward the cap.
pub const RUN_STEP_OUTPUT_MAX_BYTES: usize = 64 * 1024;
const RUN_STEP_TRUNCATION_MARKER: &str = "\n…[truncated]";

/// UTF-8-safe truncation: returns at most `max_bytes` bytes of `s`,
/// appending a `…[truncated]` marker iff truncation actually
/// happened. The byte boundary is moved backwards if needed so no
/// multibyte character is split (SQLite would reject invalid UTF-8).
///
/// Public so the executor + future propose-tools can share the same
/// rule.
pub fn truncate_output_to_64kib(s: String) -> String {
    if s.len() <= RUN_STEP_OUTPUT_MAX_BYTES {
        return s;
    }
    // Leave headroom for the marker so the final string still fits
    // under the cap.
    let mut cut = RUN_STEP_OUTPUT_MAX_BYTES.saturating_sub(RUN_STEP_TRUNCATION_MARKER.len());
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    let mut out = s[..cut].to_string();
    out.push_str(RUN_STEP_TRUNCATION_MARKER);
    out
}

fn run_status_str(status: &RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "pending",
        RunStatus::Running => "running",
        RunStatus::Succeeded => "succeeded",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
        RunStatus::TimedOut => "timed_out",
    }
}

fn parse_run_status(raw: &str) -> Result<RunStatus> {
    match raw {
        "pending" => Ok(RunStatus::Pending),
        "running" => Ok(RunStatus::Running),
        "succeeded" => Ok(RunStatus::Succeeded),
        "failed" => Ok(RunStatus::Failed),
        "cancelled" => Ok(RunStatus::Cancelled),
        "timed_out" => Ok(RunStatus::TimedOut),
        other => anyhow::bail!("unknown run status `{other}`"),
    }
}

/// Insert a `workflow_runs` row. Called by the executor (F-8)
/// before spawning the execute_inner task.
pub fn insert_run(config: &Config, run: &Run) -> Result<()> {
    let trigger_source =
        serde_json::to_string(&run.trigger_source).context("encode trigger_source")?;
    with_connection(config, |db| {
        db.execute(
            "INSERT INTO workflow_runs \
             (id, workflow_id, trigger_source, status, started_at, completed_at, error, cancelled) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                run.id,
                run.workflow_id,
                trigger_source,
                run_status_str(&run.status),
                run.started_at.to_rfc3339(),
                run.completed_at.map(|t| t.to_rfc3339()),
                run.error,
                run.cancelled as i64,
            ],
        )
        .context("Failed to insert workflow_runs row")?;
        Ok(())
    })
}

/// Mark a run as terminal (Succeeded / Failed / Cancelled / TimedOut).
/// Returns `false` when the id was unknown (e.g. the workflow was
/// deleted mid-run and the cascade swept the row).
pub fn mark_run_terminal(
    config: &Config,
    run_id: &RunId,
    status: RunStatus,
    completed_at: DateTime<Utc>,
    error: Option<String>,
) -> Result<bool> {
    with_connection(config, |db| {
        let rows = db
            .execute(
                "UPDATE workflow_runs SET status = ?2, completed_at = ?3, error = ?4 WHERE id = ?1",
                rusqlite::params![
                    run_id,
                    run_status_str(&status),
                    completed_at.to_rfc3339(),
                    error,
                ],
            )
            .context("Failed to mark run terminal")?;
        Ok(rows > 0)
    })
}

/// Mark a run as cancelled (the soft-cancel flag from ADR-014). F-9's
/// `executor::cancel_run` calls this and then leaves the executor's
/// node loop to observe the bit via [`is_cancelled`] between nodes.
pub fn set_cancelled_flag(config: &Config, run_id: &RunId) -> Result<bool> {
    with_connection(config, |db| {
        let rows = db
            .execute(
                "UPDATE workflow_runs SET cancelled = 1 WHERE id = ?1",
                rusqlite::params![run_id],
            )
            .context("Failed to set workflow_runs.cancelled")?;
        Ok(rows > 0)
    })
}

/// Read the soft-cancel bit. Returns `Ok(false)` for unknown ids — the
/// executor calls this between nodes and a missing row means the
/// cascade-delete already removed the workflow, so "not cancelled"
/// matches "abort gracefully on the next node" semantics.
pub fn is_cancelled(config: &Config, run_id: &RunId) -> Result<bool> {
    with_connection(config, |db| {
        let value: Option<i64> = db
            .query_row(
                "SELECT cancelled FROM workflow_runs WHERE id = ?1",
                rusqlite::params![run_id],
                |row| row.get(0),
            )
            .optional()
            .context("Failed to read workflow_runs.cancelled")?;
        Ok(value.unwrap_or(0) != 0)
    })
}

/// Mark every `status = 'running'` row as `Failed { error = "CoreCrashed" }`
/// in a single SQL UPDATE, returning the (workflow_id, run_id) pairs of
/// the touched rows so the caller can publish
/// `WorkflowRunCompleted { status: Failed }` events. Used by the F-9
/// boot-time orphan-recovery sweep — see [`executor::orphan_recovery_sweep`].
///
/// Idempotent: a second call against a sweep-cleaned DB returns
/// `Ok(vec![])` because every previously-Running row is now Failed.
pub fn orphan_running_runs(
    config: &Config,
    completed_at: DateTime<Utc>,
) -> Result<Vec<(WorkflowId, RunId)>> {
    with_connection(config, |db| {
        let mut stmt = db
            .prepare("SELECT id, workflow_id FROM workflow_runs WHERE status = 'running'")
            .context("Failed to prepare orphan_running_runs select")?;
        let pairs: Vec<(RunId, WorkflowId)> = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        db.execute(
            "UPDATE workflow_runs \
             SET status = 'failed', \
                 error = 'CoreCrashed', \
                 completed_at = ?1 \
             WHERE status = 'running'",
            rusqlite::params![completed_at.to_rfc3339()],
        )
        .context("Failed to orphan-sweep workflow_runs")?;
        // Return in (workflow_id, run_id) order for the caller's
        // ergonomic match against `WorkflowRunCompleted` event fields.
        Ok(pairs
            .into_iter()
            .map(|(run_id, workflow_id)| (workflow_id, run_id))
            .collect())
    })
}

/// Insert a `workflow_run_steps` row with status = Running. The
/// executor calls this right before invoking the agent.
pub fn insert_run_step(config: &Config, step: &RunStep) -> Result<()> {
    with_connection(config, |db| {
        db.execute(
            "INSERT INTO workflow_run_steps \
             (id, run_id, node_id, status, started_at, completed_at, output_json, error) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                step.id,
                step.run_id,
                step.node_id,
                run_status_str(&step.status),
                step.started_at.to_rfc3339(),
                step.completed_at.map(|t| t.to_rfc3339()),
                step.output_json,
                step.error,
            ],
        )
        .context("Failed to insert workflow_run_steps row")?;
        Ok(())
    })
}

/// Update a step row's terminal fields. `output_json` is truncated to
/// 64 KiB by the caller (executor) before this is called.
pub fn update_run_step_terminal(
    config: &Config,
    step_id: &RunStepId,
    status: RunStatus,
    completed_at: DateTime<Utc>,
    output_json: Option<String>,
    error: Option<String>,
) -> Result<bool> {
    with_connection(config, |db| {
        let rows = db
            .execute(
                "UPDATE workflow_run_steps SET \
                 status = ?2, completed_at = ?3, output_json = ?4, error = ?5 \
                 WHERE id = ?1",
                rusqlite::params![
                    step_id,
                    run_status_str(&status),
                    completed_at.to_rfc3339(),
                    output_json,
                    error,
                ],
            )
            .context("Failed to update workflow_run_steps row")?;
        Ok(rows > 0)
    })
}

#[derive(Debug, Clone, Copy)]
pub struct Pagination {
    pub limit: u32,
    pub offset: u32,
}

impl Default for Pagination {
    fn default() -> Self {
        Self {
            limit: 50,
            offset: 0,
        }
    }
}

impl Pagination {
    /// Clamp `limit` to [1, 100] so agent tools / RPC clients can't
    /// request a runaway page size (NFR-2.5.6).
    pub fn clamp(self) -> Self {
        Self {
            limit: self.limit.clamp(1, 100),
            offset: self.offset,
        }
    }
}

/// List runs for a workflow, newest-first. Used by F-8's
/// `workflows_list_runs` RPC and the F-12 propose-delete preview.
pub fn list_runs(
    config: &Config,
    workflow_id: &WorkflowId,
    pagination: Pagination,
) -> Result<Vec<Run>> {
    let pagination = pagination.clamp();
    with_connection(config, |db| {
        let mut stmt = db
            .prepare(
                "SELECT id, workflow_id, trigger_source, status, started_at, completed_at, error, cancelled \
                 FROM workflow_runs \
                 WHERE workflow_id = ?1 \
                 ORDER BY started_at DESC \
                 LIMIT ?2 OFFSET ?3",
            )
            .context("Failed to prepare list_runs")?;
        let rows = stmt
            .query_map(
                rusqlite::params![workflow_id, pagination.limit, pagination.offset],
                |row| Ok(row_to_run(row)),
            )?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Result<Vec<Run>>>()?;
        Ok(rows)
    })
}

/// Lightweight count of how many runs a workflow has. Used by the
/// F-12 `workflow_propose_delete` preview ("X runs will be deleted").
pub fn count_runs(config: &Config, workflow_id: &WorkflowId) -> Result<u32> {
    with_connection(config, |db| {
        let count: i64 = db.query_row(
            "SELECT COUNT(*) FROM workflow_runs WHERE workflow_id = ?1",
            rusqlite::params![workflow_id],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u32)
    })
}

/// Fetch one run + its steps. Returns `Ok(None)` when the run id is
/// unknown — the list view should treat that as "deleted mid-poll".
pub fn get_run(config: &Config, run_id: &RunId) -> Result<Option<(Run, Vec<RunStep>)>> {
    with_connection(config, |db| {
        let run: Option<Run> = {
            let mut stmt = db.prepare(
                "SELECT id, workflow_id, trigger_source, status, started_at, completed_at, error, cancelled \
                 FROM workflow_runs WHERE id = ?1",
            )?;
            let mut rows = stmt.query(rusqlite::params![run_id])?;
            if let Some(row) = rows.next()? {
                Some(row_to_run(row)?)
            } else {
                None
            }
        };
        let Some(run) = run else {
            return Ok(None);
        };
        let mut stmt = db.prepare(
            "SELECT id, run_id, node_id, status, started_at, completed_at, output_json, error \
             FROM workflow_run_steps WHERE run_id = ?1 ORDER BY started_at ASC",
        )?;
        let raw_rows = stmt
            .query_map(rusqlite::params![run_id], |row| Ok(row_to_run_step(row)))?
            .collect::<Result<Vec<_>, _>>()?;
        let steps = raw_rows.into_iter().collect::<Result<Vec<RunStep>>>()?;
        Ok(Some((run, steps)))
    })
}

fn row_to_run(row: &Row<'_>) -> Result<Run> {
    let id: String = row.get(0).context("read run.id")?;
    let workflow_id: String = row.get(1).context("read run.workflow_id")?;
    let trigger_source_raw: String = row.get(2).context("read run.trigger_source")?;
    let status_raw: String = row.get(3).context("read run.status")?;
    let started_at_raw: String = row.get(4).context("read run.started_at")?;
    let completed_at_raw: Option<String> = row.get(5).context("read run.completed_at")?;
    let error: Option<String> = row.get(6).context("read run.error")?;
    let cancelled: i64 = row.get(7).context("read run.cancelled")?;
    Ok(Run {
        id,
        workflow_id,
        trigger_source: serde_json::from_str::<TriggerSource>(&trigger_source_raw)
            .context("decode trigger_source")?,
        status: parse_run_status(&status_raw)?,
        started_at: DateTime::parse_from_rfc3339(&started_at_raw)
            .context("parse started_at")?
            .with_timezone(&Utc),
        completed_at: completed_at_raw
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|t| t.with_timezone(&Utc)))
            .transpose()
            .context("parse completed_at")?,
        error,
        cancelled: cancelled != 0,
    })
}

fn row_to_run_step(row: &Row<'_>) -> Result<RunStep> {
    let id: String = row.get(0).context("read step.id")?;
    let run_id: String = row.get(1).context("read step.run_id")?;
    let node_id: String = row.get(2).context("read step.node_id")?;
    let status_raw: String = row.get(3).context("read step.status")?;
    let started_at_raw: String = row.get(4).context("read step.started_at")?;
    let completed_at_raw: Option<String> = row.get(5).context("read step.completed_at")?;
    let output_json: Option<String> = row.get(6).context("read step.output_json")?;
    let error: Option<String> = row.get(7).context("read step.error")?;
    Ok(RunStep {
        id,
        run_id,
        node_id,
        status: parse_run_status(&status_raw)?,
        started_at: DateTime::parse_from_rfc3339(&started_at_raw)
            .context("parse step.started_at")?
            .with_timezone(&Utc),
        completed_at: completed_at_raw
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|t| t.with_timezone(&Utc)))
            .transpose()
            .context("parse step.completed_at")?,
        output_json,
        error,
    })
}

// ── F-3 health-recompute helpers ────────────────────────────────────────

use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::workflows::types::WorkflowHealth;

/// Returns every workflow whose `nodes_json` column mentions a JSON
/// fragment derived from `r#ref`. Pre-filter only — the recompute pass
/// in `bus.rs` filters again through `health::referenced_connections`
/// (so the LIKE may legally over-select). The SQL fragment is escaped
/// to keep `_` / `%` / `\` literal.
///
/// The fragment shape is deliberately variant-specific (`toolkit_id`
/// for Composio, `connection_id` for GenericHttp, etc.) so the LIKE
/// pre-filter matches only ConnectionRef serializations — not any
/// other JSON value that happens to share the variant's primary id.
pub fn list_workflows_referencing(config: &Config, r#ref: &ConnectionRef) -> Result<Vec<Workflow>> {
    let Some(fragment) = json_fragment_for(r#ref) else {
        return Ok(Vec::new());
    };
    let escaped = escape_like(&fragment);
    let pattern = format!("%{escaped}%");

    with_connection(config, |db| {
        let mut stmt = db
            .prepare(
                "SELECT id, schema_version, name, description, enabled, origin, health, \
                 trigger_json, nodes_json, edges_json, settings_json, \
                 created_at, updated_at, last_run_at \
                 FROM workflows \
                 WHERE nodes_json LIKE ?1 ESCAPE '\\' \
                 ORDER BY updated_at DESC",
            )
            .context("Failed to prepare list_workflows_referencing")?;
        let rows = stmt
            .query_map(rusqlite::params![pattern], |row| Ok(row_to_workflow(row)))
            .context("Failed to query list_workflows_referencing")?
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to materialise list_workflows_referencing row")?
            .into_iter()
            .collect::<Result<Vec<Workflow>>>()?;
        Ok(rows)
    })
}

/// Replace ONLY the `health` column (plus bump `updated_at`). Used by
/// F-3's bus subscriber so the bounded UPDATE doesn't churn unrelated
/// fields. Returns `false` when no row matched.
pub fn set_health(
    config: &Config,
    id: &WorkflowId,
    health: &WorkflowHealth,
    updated_at: chrono::DateTime<Utc>,
) -> Result<bool> {
    let encoded = serde_json::to_string(health).context("encode health")?;
    with_connection(config, |db| {
        let rows = db
            .execute(
                "UPDATE workflows SET health = ?2, updated_at = ?3 WHERE id = ?1",
                rusqlite::params![id, encoded, updated_at.to_rfc3339()],
            )
            .context("Failed to set_health on workflows row")?;
        Ok(rows > 0)
    })
}

/// JSON fragment unique to a `ConnectionRef` serialization. The fragment
/// is exactly the `"key":"value"` substring the variant produces in its
/// canonical JSON form — see `connections/types.rs` for the wire shape.
/// Returns `None` when no stable fragment can be derived (e.g. variants
/// whose primary id is empty); callers treat `None` as "no workflows
/// could possibly reference this".
fn json_fragment_for(r#ref: &ConnectionRef) -> Option<String> {
    match r#ref {
        ConnectionRef::Composio { toolkit_id, .. } if !toolkit_id.is_empty() => {
            Some(format!(r#""toolkit_id":"{toolkit_id}""#))
        }
        ConnectionRef::Channel { channel_id, .. } if !channel_id.is_empty() => {
            Some(format!(r#""channel_id":"{channel_id}""#))
        }
        ConnectionRef::Webview { account_id, .. } if !account_id.is_empty() => {
            Some(format!(r#""account_id":"{account_id}""#))
        }
        ConnectionRef::Builtin { integration } if !integration.is_empty() => {
            Some(format!(r#""integration":"{integration}""#))
        }
        ConnectionRef::Mcp { server_id, .. } if !server_id.is_empty() => {
            Some(format!(r#""server_id":"{server_id}""#))
        }
        ConnectionRef::GenericHttp { connection_id } if !connection_id.is_empty() => {
            Some(format!(r#""connection_id":"{connection_id}""#))
        }
        _ => None,
    }
}

/// Escape SQL LIKE metacharacters so the fragment matches literally.
fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '%' | '_' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}
