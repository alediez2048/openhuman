//! F4-9 — approval queue SQLite operations.
//!
//! Thin CRUD around `approval_queue`. Each fn opens its own
//! short-lived connection via `with_connection` and assumes the
//! row schema from migration 010.

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use rusqlite::params;
use uuid::Uuid;

use super::types::{ApprovalEntry, ApprovalId, ApprovalStatus, EnqueueApprovalRequest};
use crate::openhuman::config::Config;
use crate::openhuman::workflows::store::with_connection;

/// Persist a new pending entry; returns the freshly-minted id.
pub fn enqueue(config: &Config, req: EnqueueApprovalRequest) -> Result<ApprovalId> {
    let id = Uuid::new_v4().to_string();
    let now = Utc::now();
    let payload_json =
        serde_json::to_string(&req.payload).context("approval::enqueue payload serialise")?;
    let context_json = match req.context.as_ref() {
        Some(c) => Some(serde_json::to_string(c).context("approval::enqueue context serialise")?),
        None => None,
    };
    let id_for_db = id.clone();
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO approval_queue \
             (id, campaign_id, workflow_id, run_id, node_id, action_kind, target, \
              payload_json, context_json, status, created_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', ?10)",
            params![
                id_for_db,
                req.campaign_id,
                req.workflow_id,
                req.run_id,
                req.node_id,
                req.action_kind,
                req.target,
                payload_json,
                context_json,
                now.to_rfc3339(),
            ],
        )?;
        Ok(())
    })?;
    Ok(id)
}

/// Fetch one entry by id. `Ok(None)` when unknown.
pub fn get(config: &Config, id: &ApprovalId) -> Result<Option<ApprovalEntry>> {
    let id_for_db = id.clone();
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(
            "SELECT id, campaign_id, workflow_id, run_id, node_id, action_kind, target, \
             payload_json, context_json, status, created_at, decided_at, decided_by, error \
             FROM approval_queue WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id_for_db])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row_to_entry(row)?))
        } else {
            Ok(None)
        }
    })
}

/// List entries, optionally filtered by campaign + status. `None`
/// for either argument means "no filter on this axis". Newest-first.
pub fn list(
    config: &Config,
    campaign_id: Option<&str>,
    status: Option<ApprovalStatus>,
) -> Result<Vec<ApprovalEntry>> {
    let mut sql = String::from(
        "SELECT id, campaign_id, workflow_id, run_id, node_id, action_kind, target, \
         payload_json, context_json, status, created_at, decided_at, decided_by, error \
         FROM approval_queue WHERE 1=1",
    );
    let mut binds: Vec<String> = Vec::new();
    if let Some(cid) = campaign_id {
        sql.push_str(" AND campaign_id = ?");
        binds.push(cid.to_string());
    }
    if let Some(s) = status {
        sql.push_str(" AND status = ?");
        binds.push(s.as_str().to_string());
    }
    sql.push_str(" ORDER BY created_at DESC");
    with_connection(config, |conn| {
        let mut stmt = conn.prepare(&sql)?;
        let mut rows = stmt.query(rusqlite::params_from_iter(binds.iter()))?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            out.push(row_to_entry(row)?);
        }
        Ok(out)
    })
}

/// Record a Pending → (Approved | Rejected) transition. Returns
/// the post-transition entry. Errors when the row isn't in
/// `Pending` (the UI shouldn't show approve/reject on non-pending
/// rows, but enforce server-side so a double-tap or stale tab can't
/// drive a second decision).
pub fn record_decision(
    config: &Config,
    id: &ApprovalId,
    new_status: ApprovalStatus,
    decided_by: &str,
    edited_payload: Option<serde_json::Value>,
) -> Result<ApprovalEntry> {
    if !matches!(
        new_status,
        ApprovalStatus::Approved | ApprovalStatus::Rejected
    ) {
        return Err(anyhow!(
            "approval::record_decision: new_status must be approved or rejected"
        ));
    }
    let now = Utc::now();
    let id_for_db = id.clone();
    let by_for_db = decided_by.to_string();
    let payload_swap = match edited_payload.as_ref() {
        Some(p) => {
            Some(serde_json::to_string(p).context("approval::record_decision payload serialise")?)
        }
        None => None,
    };
    with_connection(config, |conn| {
        // Pre-flight check that the row is Pending.
        let current: Option<String> = conn
            .query_row(
                "SELECT status FROM approval_queue WHERE id = ?1",
                params![id_for_db],
                |row| row.get(0),
            )
            .ok();
        let current = current.ok_or_else(|| anyhow!("approval not found: {id_for_db}"))?;
        if current != "pending" {
            return Err(anyhow!(
                "approval::record_decision: id={id_for_db} not pending (current={current})"
            ));
        }
        if let Some(payload) = payload_swap {
            conn.execute(
                "UPDATE approval_queue SET status = ?2, decided_at = ?3, decided_by = ?4, \
                 payload_json = ?5 WHERE id = ?1",
                params![
                    id_for_db,
                    new_status.as_str(),
                    now.to_rfc3339(),
                    by_for_db,
                    payload,
                ],
            )?;
        } else {
            conn.execute(
                "UPDATE approval_queue SET status = ?2, decided_at = ?3, decided_by = ?4 \
                 WHERE id = ?1",
                params![id_for_db, new_status.as_str(), now.to_rfc3339(), by_for_db,],
            )?;
        }
        Ok(())
    })?;
    get(config, id)?.ok_or_else(|| anyhow!("approval row vanished after decision"))
}

/// Mark a previously-approved row as `Sent`. Called by the re-issue
/// path after the externally-visible action committed.
pub fn mark_sent(config: &Config, id: &ApprovalId) -> Result<()> {
    let id_for_db = id.clone();
    with_connection(config, |conn| {
        conn.execute(
            "UPDATE approval_queue SET status = 'sent' WHERE id = ?1 AND status = 'approved'",
            params![id_for_db],
        )?;
        Ok(())
    })
}

/// Mark a previously-approved row as `Failed` with an attached
/// error message.
pub fn mark_failed(config: &Config, id: &ApprovalId, error: &str) -> Result<()> {
    let id_for_db = id.clone();
    let err_for_db = error.to_string();
    with_connection(config, |conn| {
        conn.execute(
            "UPDATE approval_queue SET status = 'failed', error = ?2 \
             WHERE id = ?1 AND status = 'approved'",
            params![id_for_db, err_for_db],
        )?;
        Ok(())
    })
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<ApprovalEntry> {
    let payload_json: String = row.get(7)?;
    let context_json: Option<String> = row.get(8)?;
    let status_str: String = row.get(9)?;
    let created_at_str: String = row.get(10)?;
    let decided_at_str: Option<String> = row.get(11)?;
    Ok(ApprovalEntry {
        id: row.get(0)?,
        campaign_id: row.get(1)?,
        workflow_id: row.get(2)?,
        run_id: row.get(3)?,
        node_id: row.get(4)?,
        action_kind: row.get(5)?,
        target: row.get(6)?,
        payload: serde_json::from_str(&payload_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, Box::new(e))
        })?,
        context: match context_json {
            Some(s) => Some(serde_json::from_str(&s).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?),
            None => None,
        },
        status: ApprovalStatus::parse(&status_str).unwrap_or(ApprovalStatus::Pending),
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at_str)
            .map(|d| d.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?,
        decided_at: match decided_at_str {
            Some(s) => Some(
                chrono::DateTime::parse_from_rfc3339(&s)
                    .map(|d| d.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(e),
                        )
                    })?,
            ),
            None => None,
        },
        decided_by: row.get(12)?,
        error: row.get(13)?,
    })
}
