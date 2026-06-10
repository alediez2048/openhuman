//! F3-6 chunk 2 — browser-agent audit log.
//!
//! Every browser_observe / browser_act / browser_extract call inside
//! a `BrowserAction` workflow node writes one row here. The table is
//! independent of `workflow_run_steps` because a single node can fire
//! many tool calls; cramming them into `run_steps.output_json` would
//! break the run-detail UI's structured rendering.
//!
//! ## Write path
//!
//! Tools call [`write_entry`] with `(config, AuditLogEntry)` at the
//! end of each `execute()` — happy AND error path. Failures are
//! best-effort: a logged warn + swallowed error, so an audit-log
//! write failure cannot ever break the tool's actual response (the
//! tool's contract is "execute the CDP primitive", not "guarantee
//! observability").
//!
//! ## Read path
//!
//! The run-detail UI calls the new RPC `browser_agent_get_audit_log`
//! (added in this chunk) which delegates to [`list_for_run`].
//! Consumer lands when F3-5's preview surface ships.
//!
//! ## Retention
//!
//! Hard-deleted by the workflow retention sweep after N days
//! (default 30). Retention wiring is the chunk-3 follow-up alongside
//! the existing `retention.rs` workflows sweep.

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use std::path::Path;

use crate::openhuman::config::Config;
use crate::openhuman::workflows::store::{with_connection, with_connection_at};

/// One persisted audit-log row. Mirrors the
/// `browser_agent_audit_log` table 1:1.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuditLogEntry {
    pub id: String,
    pub run_id: String,
    pub step_id: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub tool_name: String,
    /// JSON-encoded subset of the tool args. Writers should strip
    /// sensitive fields (passwords, tokens) before passing them
    /// here — F3-6 chunk 3 ships the redaction policy that does
    /// this automatically, so today writers pass the args as-is.
    pub args_json: String,
    pub result_summary: String,
    /// Filesystem path under `{workspace}/browser_audit/<run_id>/`
    /// when a screenshot was captured. `None` until F3-6 chunk 3
    /// wires the screenshot capture pipeline.
    pub screenshot_path: Option<String>,
    /// Count of fields scrubbed by the redaction pass. `0` until
    /// F3-6 chunk 3 ships redaction.
    pub redacted_fields_count: u32,
}

impl AuditLogEntry {
    /// Construct a fresh entry stamped at `Utc::now()`. Allocates a
    /// UUIDv4 id. Tools call this in `execute()` then hand it to
    /// [`write_entry`].
    pub fn new(
        run_id: impl Into<String>,
        tool_name: impl Into<String>,
        args_json: impl Into<String>,
        result_summary: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            run_id: run_id.into(),
            step_id: None,
            timestamp: Utc::now(),
            tool_name: tool_name.into(),
            args_json: args_json.into(),
            result_summary: result_summary.into(),
            screenshot_path: None,
            redacted_fields_count: 0,
        }
    }
}

/// Persist one audit-log row. Best-effort — failure logs a `warn!`
/// and returns `Ok(())` so a write-path glitch can't break the tool's
/// actual response. Tools call this from their `execute()` after the
/// CDP primitive has fired (or short-circuited via dry-run).
pub fn write_entry(config: &Config, entry: AuditLogEntry) -> Result<()> {
    let result = with_connection(config, |conn| insert_entry(conn, &entry));
    if let Err(err) = result {
        tracing::warn!(
            target: "browser-agent-audit",
            run = %entry.run_id,
            tool = %entry.tool_name,
            "[audit] write_entry failed (swallowed): {err:#}"
        );
    }
    Ok(())
}

/// Workspace-keyed variant for callers that don't hold a `Config`.
/// The F3-3 browser tools dispatch through this — they receive the
/// workspace path via `SessionRegistry::RunMeta::workspace_dir`.
/// Same best-effort contract as [`write_entry`].
pub fn write_entry_at(workspace_dir: &Path, entry: AuditLogEntry) -> Result<()> {
    let result = with_connection_at(workspace_dir, |conn| insert_entry(conn, &entry));
    if let Err(err) = result {
        tracing::warn!(
            target: "browser-agent-audit",
            run = %entry.run_id,
            tool = %entry.tool_name,
            "[audit] write_entry_at failed (swallowed): {err:#}"
        );
    }
    Ok(())
}

/// Read every entry for `run_id`, ordered ascending by timestamp.
/// Used by the F3-5 run-detail UI to render the agent's trace.
pub fn list_for_run(config: &Config, run_id: &str) -> Result<Vec<AuditLogEntry>> {
    with_connection(config, |conn| list_entries(conn, run_id))
}

/// Test-only: count rows for a run. Mirrors `list_for_run` shape but
/// avoids the per-row decode cost. `pub(crate)` so the workflow tests
/// can assert "audit wrote N rows" without dragging in the full Vec.
#[doc(hidden)]
pub fn count_for_run(config: &Config, run_id: &str) -> Result<usize> {
    with_connection(config, |conn| {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM browser_agent_audit_log WHERE run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )?;
        Ok(n as usize)
    })
}

fn insert_entry(conn: &Connection, entry: &AuditLogEntry) -> Result<()> {
    conn.execute(
        "INSERT INTO browser_agent_audit_log \
         (id, run_id, step_id, timestamp, tool_name, args_json, result_summary, screenshot_path, redacted_fields_count) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            entry.id,
            entry.run_id,
            entry.step_id,
            entry.timestamp.to_rfc3339(),
            entry.tool_name,
            entry.args_json,
            entry.result_summary,
            entry.screenshot_path,
            entry.redacted_fields_count,
        ],
    )
    .context("audit insert")?;
    Ok(())
}

fn list_entries(conn: &Connection, run_id: &str) -> Result<Vec<AuditLogEntry>> {
    let mut stmt = conn.prepare(
        "SELECT id, run_id, step_id, timestamp, tool_name, args_json, \
                result_summary, screenshot_path, redacted_fields_count \
         FROM browser_agent_audit_log \
         WHERE run_id = ?1 \
         ORDER BY timestamp ASC",
    )?;
    let rows = stmt
        .query_map(params![run_id], |row| {
            let ts: String = row.get(3)?;
            let timestamp = DateTime::parse_from_rfc3339(&ts)
                .map_err(|_| rusqlite::Error::InvalidQuery)?
                .with_timezone(&Utc);
            Ok(AuditLogEntry {
                id: row.get(0)?,
                run_id: row.get(1)?,
                step_id: row.get(2)?,
                timestamp,
                tool_name: row.get(4)?,
                args_json: row.get(5)?,
                result_summary: row.get(6)?,
                screenshot_path: row.get(7)?,
                redacted_fields_count: row.get::<_, i64>(8)? as u32,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::config::Config;
    use tempfile::TempDir;

    fn make_config() -> (TempDir, Config) {
        let dir = TempDir::new().unwrap();
        let mut config = Config::default();
        config.workspace_dir = dir.path().to_path_buf();
        (dir, config)
    }

    #[test]
    fn write_then_list_round_trips_entries_in_timestamp_order() {
        let (_dir, cfg) = make_config();
        let run_id = "run-1";

        let e1 = AuditLogEntry::new(run_id, "browser_observe", "{}", "ok");
        write_entry(&cfg, e1.clone()).unwrap();

        // Sleep a tick so the next entry has a strictly-later timestamp
        // (RFC3339 has microsecond resolution; same-millisecond writes
        // would tie and ASC ordering becomes unstable on the secondary
        // key).
        std::thread::sleep(std::time::Duration::from_millis(2));

        let e2 = AuditLogEntry::new(run_id, "browser_act", r#"{"verb":"click"}"#, "clicked [3]");
        write_entry(&cfg, e2.clone()).unwrap();

        let entries = list_for_run(&cfg, run_id).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].tool_name, "browser_observe");
        assert_eq!(entries[1].tool_name, "browser_act");
        assert_eq!(entries[1].args_json, r#"{"verb":"click"}"#);
    }

    #[test]
    fn list_isolates_rows_by_run_id() {
        let (_dir, cfg) = make_config();
        write_entry(
            &cfg,
            AuditLogEntry::new("run-a", "browser_observe", "{}", "ok"),
        )
        .unwrap();
        write_entry(
            &cfg,
            AuditLogEntry::new("run-b", "browser_observe", "{}", "ok"),
        )
        .unwrap();
        assert_eq!(count_for_run(&cfg, "run-a").unwrap(), 1);
        assert_eq!(count_for_run(&cfg, "run-b").unwrap(), 1);
        assert_eq!(count_for_run(&cfg, "run-c").unwrap(), 0);
    }

    #[test]
    fn write_entry_swallows_db_errors_silently() {
        // Pointing the workspace at a read-only path that doesn't
        // exist proves the contract: a failed write returns Ok(())
        // and only logs a warn. Tools can't have audit failures
        // crash their happy path.
        let mut cfg = Config::default();
        cfg.workspace_dir =
            std::path::PathBuf::from("/nonexistent/path/that/cant/be/created/F3-6-audit-test");
        let entry = AuditLogEntry::new("run-x", "browser_observe", "{}", "ok");
        let result = write_entry(&cfg, entry);
        assert!(result.is_ok(), "write_entry must not propagate errors");
    }
}
