//! SQLite persistence for the Campaigns domain (F4-2).
//!
//! Lives in the same `workflows.db` SQLite file as the workflows
//! domain. Per ADR-003 (separate-DB rule), domains usually get their
//! own files — campaigns are the exception because cross-domain
//! queries ("every workflow under campaign X") would otherwise
//! require an `ATTACH DATABASE` join, which SQLite supports but
//! complicates migrations + transaction scopes. Sharing a file lets
//! workflow soft-delete + campaign soft-delete + cascade rules live
//! inside a single FK graph.
//!
//! Persistence shape mirrors `workflows/store.rs`:
//! - `Campaign` round-trips with JSON-blob columns for the structured
//!   fields (`entity_binding`, `throttle`, `approval_policy`,
//!   `target_outcome`).
//! - Soft-delete via `deleted_at`, parallel to F2-14's workflows
//!   soft-delete pattern.
//! - Each helper opens an ephemeral SQLite connection via
//!   `workflows::store::with_connection` so it shares the migration
//!   runner and the `PRAGMA foreign_keys = ON` semantics.

use crate::openhuman::config::Config;
use crate::openhuman::workflows::store::with_connection;
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row};

use super::types::{
    ApprovalPolicy, Campaign, CampaignId, CampaignStatus, EntityRef, OutcomeSpec, Throttle,
};

// ── List filter ─────────────────────────────────────────────────────────

/// Optional filter for [`list_campaigns`]. Defaults exclude
/// soft-deleted rows and don't filter by status.
#[derive(Debug, Default, Clone)]
pub struct ListCampaignsFilter {
    /// Filter to one specific status (`Draft` / `Active` / `Paused` /
    /// `WoundDown` / `Archived`). `None` = no status filter.
    pub status: Option<CampaignStatus>,
    /// When `true`, soft-deleted rows are included. Default `false`
    /// — the user-facing list view never shows deleted campaigns
    /// unless the user explicitly asks (the future "Trash" view).
    pub include_deleted: bool,
}

// ── CRUD ────────────────────────────────────────────────────────────────

/// Insert a new `campaigns` row. Caller is responsible for setting
/// `id`, `created_at`, `updated_at`, and choosing the initial
/// `status` (typically `Draft` on first create).
pub fn insert_campaign(config: &Config, campaign: &Campaign) -> Result<()> {
    let (status, entity_binding, throttle, approval_policy, target_outcome) =
        encode_blobs(campaign)?;
    with_connection(config, |db| {
        db.execute(
            "INSERT INTO campaigns \
             (id, schema_version, name, description, status, \
              entity_binding, throttle, approval_policy, target_outcome, \
              created_at, updated_at, last_run_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                campaign.id,
                campaign.schema_version,
                campaign.name,
                campaign.description,
                status,
                entity_binding,
                throttle,
                approval_policy,
                target_outcome,
                campaign.created_at.to_rfc3339(),
                campaign.updated_at.to_rfc3339(),
                campaign.last_run_at.map(|t| t.to_rfc3339()),
            ],
        )
        .context("Failed to insert campaigns row")?;
        Ok(())
    })
}

/// Fetch one campaign by id. Returns `Ok(None)` for unknown id OR
/// soft-deleted rows. Use [`get_campaign_including_deleted`] for the
/// restore path that needs to see deleted rows.
pub fn get_campaign(config: &Config, id: &CampaignId) -> Result<Option<Campaign>> {
    with_connection(config, |db| {
        let mut stmt = db.prepare(
            "SELECT id, schema_version, name, description, status, \
             entity_binding, throttle, approval_policy, target_outcome, \
             created_at, updated_at, last_run_at \
             FROM campaigns WHERE id = ?1 AND deleted_at IS NULL",
        )?;
        let row = stmt
            .query_row(rusqlite::params![id], |r| Ok(row_to_campaign(r)))
            .optional()?
            .transpose()?;
        Ok(row)
    })
}

/// Same as [`get_campaign`] but includes soft-deleted rows. Used by
/// [`restore_campaign`] to surface a row the user is restoring.
pub fn get_campaign_including_deleted(
    config: &Config,
    id: &CampaignId,
) -> Result<Option<Campaign>> {
    with_connection(config, |db| {
        let mut stmt = db.prepare(
            "SELECT id, schema_version, name, description, status, \
             entity_binding, throttle, approval_policy, target_outcome, \
             created_at, updated_at, last_run_at \
             FROM campaigns WHERE id = ?1",
        )?;
        let row = stmt
            .query_row(rusqlite::params![id], |r| Ok(row_to_campaign(r)))
            .optional()?
            .transpose()?;
        Ok(row)
    })
}

/// List campaigns matching `filter`, newest-first by `updated_at`.
pub fn list_campaigns(config: &Config, filter: ListCampaignsFilter) -> Result<Vec<Campaign>> {
    with_connection(config, |db| {
        let mut sql = String::from(
            "SELECT id, schema_version, name, description, status, \
             entity_binding, throttle, approval_policy, target_outcome, \
             created_at, updated_at, last_run_at \
             FROM campaigns",
        );
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if !filter.include_deleted {
            clauses.push("deleted_at IS NULL".to_string());
        }
        if let Some(status) = filter.status {
            clauses.push(format!("status = ?{}", params.len() + 1));
            params.push(Box::new(campaign_status_str(status).to_string()));
        }
        if !clauses.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&clauses.join(" AND "));
        }
        sql.push_str(" ORDER BY updated_at DESC");
        let mut stmt = db.prepare(&sql)?;
        let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt
            .query_map(rusqlite::params_from_iter(params_refs), |r| {
                Ok(row_to_campaign(r))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .collect::<Result<Vec<Campaign>>>()?;
        Ok(rows)
    })
}

/// Apply an update — overwrites every mutable field on the row.
/// Caller is responsible for bumping `updated_at`. Returns `false`
/// when no row matched (typically: id is unknown or soft-deleted).
pub fn update_campaign(config: &Config, campaign: &Campaign) -> Result<bool> {
    let (status, entity_binding, throttle, approval_policy, target_outcome) =
        encode_blobs(campaign)?;
    with_connection(config, |db| {
        let rows = db
            .execute(
                "UPDATE campaigns SET \
                 name = ?2, description = ?3, status = ?4, \
                 entity_binding = ?5, throttle = ?6, approval_policy = ?7, \
                 target_outcome = ?8, updated_at = ?9, last_run_at = ?10 \
                 WHERE id = ?1 AND deleted_at IS NULL",
                rusqlite::params![
                    campaign.id,
                    campaign.name,
                    campaign.description,
                    status,
                    entity_binding,
                    throttle,
                    approval_policy,
                    target_outcome,
                    campaign.updated_at.to_rfc3339(),
                    campaign.last_run_at.map(|t| t.to_rfc3339()),
                ],
            )
            .context("Failed to update campaigns row")?;
        Ok(rows > 0)
    })
}

/// Soft-delete: sets `deleted_at` to now. Does NOT cascade-delete
/// child workflows — instead, the `ON DELETE SET NULL` FK on
/// `workflows.campaign_id` orphans them when the row is eventually
/// hard-deleted by the retention sweep. Until then, child workflows
/// see this campaign in `get_campaign_including_deleted` but not in
/// `get_campaign`.
pub fn delete_campaign(config: &Config, id: &CampaignId) -> Result<bool> {
    with_connection(config, |db| {
        let now = Utc::now().to_rfc3339();
        let rows = db
            .execute(
                "UPDATE campaigns SET deleted_at = ?2, updated_at = ?2 \
                 WHERE id = ?1 AND deleted_at IS NULL",
                rusqlite::params![id, now],
            )
            .context("Failed to soft-delete campaigns row")?;
        Ok(rows > 0)
    })
}

/// Inverse of [`delete_campaign`]: clears `deleted_at`. Returns
/// `false` when the row isn't soft-deleted (or doesn't exist).
pub fn restore_campaign(config: &Config, id: &CampaignId) -> Result<bool> {
    with_connection(config, |db| {
        let now = Utc::now().to_rfc3339();
        let rows = db
            .execute(
                "UPDATE campaigns SET deleted_at = NULL, updated_at = ?2 \
                 WHERE id = ?1 AND deleted_at IS NOT NULL",
                rusqlite::params![id, now],
            )
            .context("Failed to restore campaigns row")?;
        Ok(rows > 0)
    })
}

/// Return the ids of every workflow that references this campaign
/// via `workflows.campaign_id`. Skips soft-deleted workflows.
///
/// Returns ids rather than full `Workflow` rows so we don't tie the
/// campaigns store to the workflows store's row decoder (the decoder
/// reads 15 columns + json decodes; over-coupling for an FK lookup).
/// Callers that need full rows iterate the ids through
/// `workflows::store::get_workflow`.
pub fn list_workflow_ids_for_campaign(
    config: &Config,
    id: &CampaignId,
) -> Result<Vec<crate::openhuman::workflows::types::WorkflowId>> {
    with_connection(config, |db| {
        let mut stmt = db.prepare(
            "SELECT id FROM workflows \
             WHERE campaign_id = ?1 AND deleted_at IS NULL \
             ORDER BY created_at ASC",
        )?;
        let ids = stmt
            .query_map(rusqlite::params![id], |r| r.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(ids)
    })
}

// ── Encoding / decoding ─────────────────────────────────────────────────

fn encode_blobs(c: &Campaign) -> Result<(String, String, Option<String>, String, Option<String>)> {
    let status = campaign_status_str(c.status).to_string();
    let entity_binding =
        serde_json::to_string(&c.entity_binding).context("encode entity_binding")?;
    let throttle = c
        .throttle
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("encode throttle")?;
    let approval_policy =
        serde_json::to_string(&c.approval_policy).context("encode approval_policy")?;
    let target_outcome = c
        .target_outcome
        .as_ref()
        .map(serde_json::to_string)
        .transpose()
        .context("encode target_outcome")?;
    Ok((
        status,
        entity_binding,
        throttle,
        approval_policy,
        target_outcome,
    ))
}

fn row_to_campaign(row: &Row<'_>) -> Result<Campaign> {
    let id: String = row.get(0).context("read campaign.id")?;
    let schema_version: u32 = row.get(1).context("read campaign.schema_version")?;
    let name: String = row.get(2).context("read campaign.name")?;
    let description: Option<String> = row.get(3).context("read campaign.description")?;
    let status_raw: String = row.get(4).context("read campaign.status")?;
    let entity_binding_raw: String = row.get(5).context("read campaign.entity_binding")?;
    let throttle_raw: Option<String> = row.get(6).context("read campaign.throttle")?;
    let approval_policy_raw: String = row.get(7).context("read campaign.approval_policy")?;
    let target_outcome_raw: Option<String> = row.get(8).context("read campaign.target_outcome")?;
    let created_at_raw: String = row.get(9).context("read campaign.created_at")?;
    let updated_at_raw: String = row.get(10).context("read campaign.updated_at")?;
    let last_run_at_raw: Option<String> = row.get(11).context("read campaign.last_run_at")?;

    let status = parse_campaign_status(&status_raw)?;
    let entity_binding: EntityRef =
        serde_json::from_str(&entity_binding_raw).context("decode entity_binding")?;
    let throttle: Option<Throttle> = throttle_raw
        .map(|s| serde_json::from_str::<Throttle>(&s))
        .transpose()
        .context("decode throttle")?;
    let approval_policy: ApprovalPolicy =
        serde_json::from_str(&approval_policy_raw).context("decode approval_policy")?;
    let target_outcome: Option<OutcomeSpec> = target_outcome_raw
        .map(|s| serde_json::from_str::<OutcomeSpec>(&s))
        .transpose()
        .context("decode target_outcome")?;

    Ok(Campaign {
        id,
        schema_version,
        name,
        description,
        status,
        entity_binding,
        throttle,
        approval_policy,
        target_outcome,
        created_at: DateTime::parse_from_rfc3339(&created_at_raw)
            .context("parse campaign.created_at")?
            .with_timezone(&Utc),
        updated_at: DateTime::parse_from_rfc3339(&updated_at_raw)
            .context("parse campaign.updated_at")?
            .with_timezone(&Utc),
        last_run_at: last_run_at_raw
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|t| t.with_timezone(&Utc)))
            .transpose()
            .context("parse campaign.last_run_at")?,
    })
}

/// String form of [`CampaignStatus`] for the `status` TEXT column.
/// Kept in sync with `parse_campaign_status` — match exhaustively so
/// adding a new variant is a compile error.
fn campaign_status_str(s: CampaignStatus) -> &'static str {
    match s {
        CampaignStatus::Draft => "draft",
        CampaignStatus::Active => "active",
        CampaignStatus::Paused => "paused",
        CampaignStatus::WoundDown => "wound_down",
        CampaignStatus::Archived => "archived",
    }
}

fn parse_campaign_status(s: &str) -> Result<CampaignStatus> {
    Ok(match s {
        "draft" => CampaignStatus::Draft,
        "active" => CampaignStatus::Active,
        "paused" => CampaignStatus::Paused,
        "wound_down" => CampaignStatus::WoundDown,
        "archived" => CampaignStatus::Archived,
        other => anyhow::bail!("unknown campaign status `{other}`"),
    })
}
