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
