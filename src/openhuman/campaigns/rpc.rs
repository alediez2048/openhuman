//! JSON-RPC handlers for the campaigns domain (F4-3).
//!
//! Thin wrappers around `ops`. Each handler:
//! - Maps `CampaignOpError` to a `{code}: {body}` string per the
//!   established pattern (`workflows_run_now`, etc.) so the frontend
//!   can branch on the stable code prefix.
//! - Returns `RpcOutcome<T>` per `AGENTS.md`.

use crate::openhuman::campaigns::ops;
use crate::openhuman::campaigns::store::ListCampaignsFilter;
use crate::openhuman::campaigns::types::{
    Campaign, CampaignId, CreateCampaignRequest, UpdateCampaignRequest,
};
use crate::openhuman::config::Config;
use crate::openhuman::workflows::types::WorkflowId;
use crate::rpc::RpcOutcome;

fn map_err(e: crate::openhuman::campaigns::types::CampaignOpError) -> String {
    format!(
        "{code}: {body}",
        code = e.code(),
        body = serde_json::to_string(&e).unwrap_or_default()
    )
}

pub async fn campaigns_list(
    config: &Config,
    filter: ListCampaignsFilter,
) -> Result<RpcOutcome<Vec<Campaign>>, String> {
    ops::list(config, filter).await.map_err(map_err)
}

pub async fn campaigns_get(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Option<Campaign>>, String> {
    ops::get(config, id).await.map_err(map_err)
}

pub async fn campaigns_create(
    config: &Config,
    req: CreateCampaignRequest,
) -> Result<RpcOutcome<Campaign>, String> {
    ops::create(config, req).await.map_err(map_err)
}

pub async fn campaigns_update(
    config: &Config,
    req: UpdateCampaignRequest,
) -> Result<RpcOutcome<Campaign>, String> {
    ops::update(config, req).await.map_err(map_err)
}

pub async fn campaigns_delete(config: &Config, id: CampaignId) -> Result<RpcOutcome<bool>, String> {
    ops::delete(config, id).await.map_err(map_err)
}

pub async fn campaigns_restore(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Option<Campaign>>, String> {
    ops::restore(config, id).await.map_err(map_err)
}

pub async fn campaigns_pause(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, String> {
    ops::pause(config, id).await.map_err(map_err)
}

pub async fn campaigns_resume(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, String> {
    ops::resume(config, id).await.map_err(map_err)
}

pub async fn campaigns_wind_down(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, String> {
    ops::wind_down(config, id).await.map_err(map_err)
}

pub async fn campaigns_archive(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, String> {
    ops::archive(config, id).await.map_err(map_err)
}

pub async fn campaigns_add_workflow(
    config: &Config,
    campaign_id: CampaignId,
    workflow_id: WorkflowId,
) -> Result<RpcOutcome<bool>, String> {
    ops::add_workflow(config, campaign_id, workflow_id)
        .await
        .map_err(map_err)
}

pub async fn campaigns_remove_workflow(
    config: &Config,
    campaign_id: CampaignId,
    workflow_id: WorkflowId,
) -> Result<RpcOutcome<bool>, String> {
    ops::remove_workflow(config, campaign_id, workflow_id)
        .await
        .map_err(map_err)
}

// ── F4-9 approval queue RPC ──────────────────────────────────────

pub async fn approvals_list_pending(
    config: &Config,
    campaign_id: Option<String>,
) -> Result<RpcOutcome<Vec<crate::openhuman::campaigns::approval::ApprovalEntry>>, String> {
    let entries = crate::openhuman::campaigns::approval::ops::list_pending(config, campaign_id)
        .await
        .map_err(|e| format!("approvals_list_failed: {e:#}"))?;
    Ok(RpcOutcome::single_log(
        entries,
        "approvals_list_pending".to_string(),
    ))
}

pub async fn approvals_get(
    config: &Config,
    id: String,
) -> Result<RpcOutcome<Option<crate::openhuman::campaigns::approval::ApprovalEntry>>, String> {
    let entry = crate::openhuman::campaigns::approval::ops::get(config, id.clone())
        .await
        .map_err(|e| format!("approvals_get_failed: {e:#}"))?;
    Ok(RpcOutcome::single_log(
        entry,
        format!("approvals_get id={id}"),
    ))
}

pub async fn approvals_approve(
    config: &Config,
    id: String,
    edited_payload: Option<serde_json::Value>,
    decided_by: Option<String>,
) -> Result<RpcOutcome<crate::openhuman::campaigns::approval::ApprovalEntry>, String> {
    let decided_by = decided_by.unwrap_or_else(|| "user".to_string());
    let entry =
        crate::openhuman::campaigns::approval::ops::approve(config, id.clone(), edited_payload, decided_by)
            .await
            .map_err(|e| format!("approvals_approve_failed: {e:#}"))?;
    Ok(RpcOutcome::single_log(
        entry,
        format!("approvals_approve id={id}"),
    ))
}

pub async fn approvals_reject(
    config: &Config,
    id: String,
    reason: Option<String>,
    decided_by: Option<String>,
) -> Result<RpcOutcome<crate::openhuman::campaigns::approval::ApprovalEntry>, String> {
    let decided_by = decided_by.unwrap_or_else(|| "user".to_string());
    let entry =
        crate::openhuman::campaigns::approval::ops::reject(config, id.clone(), reason, decided_by)
            .await
            .map_err(|e| format!("approvals_reject_failed: {e:#}"))?;
    Ok(RpcOutcome::single_log(
        entry,
        format!("approvals_reject id={id}"),
    ))
}

pub async fn approvals_batch_approve(
    config: &Config,
    ids: Vec<String>,
    decided_by: Option<String>,
) -> Result<RpcOutcome<Vec<crate::openhuman::campaigns::approval::ApprovalEntry>>, String> {
    let decided_by = decided_by.unwrap_or_else(|| "user".to_string());
    let entries = crate::openhuman::campaigns::approval::ops::batch_approve(config, ids.clone(), decided_by)
        .await
        .map_err(|e| format!("approvals_batch_approve_failed: {e:#}"))?;
    Ok(RpcOutcome::single_log(
        entries,
        format!("approvals_batch_approve count={}", ids.len()),
    ))
}

/// F4-8: read-only throttle status for the UI ("X / Y used today").
/// Returns `Ok(None)` when the campaign has no throttle configured;
/// otherwise the current window's consumption + next-window time.
pub async fn campaigns_throttle_status(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Option<crate::openhuman::campaigns::throttle::ThrottleSnapshot>>, String> {
    let campaign = match ops::get(config, id.clone()).await.map_err(map_err)? {
        crate::rpc::RpcOutcome { value: Some(c), .. } => c,
        _ => {
            return Ok(RpcOutcome::single_log(
                None,
                format!("campaigns_throttle_status not_found id={id}"),
            ))
        }
    };
    let snap = crate::openhuman::campaigns::throttle::ThrottleGate::current(
        config,
        &id,
        campaign.throttle.as_ref(),
    )
    .map_err(|e| format!("throttle_read_failed: {e:#}"))?;
    Ok(RpcOutcome::single_log(
        snap,
        format!("campaigns_throttle_status id={id}"),
    ))
}
