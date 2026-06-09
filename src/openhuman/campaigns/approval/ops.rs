//! F4-9 — approval queue RPC ops.
//!
//! Thin async wrappers around `store::*`. Publish `DomainEvent::Approval*`
//! variants on transitions so the UI + push-notification surface can
//! pick them up without polling.

use anyhow::Result;

use super::store;
use super::types::{
    ApprovalDecision, ApprovalEntry, ApprovalId, ApprovalStatus, EnqueueApprovalRequest,
};
use crate::core::event_bus::publish_global;
use crate::core::event_bus::DomainEvent;
use crate::openhuman::config::Config;

/// List pending entries, optionally filtered by campaign.
pub async fn list_pending(
    config: &Config,
    campaign_id: Option<String>,
) -> Result<Vec<ApprovalEntry>> {
    store::list(
        config,
        campaign_id.as_deref(),
        Some(ApprovalStatus::Pending),
    )
}

/// List entries in any status (for history view).
pub async fn list_all(config: &Config, campaign_id: Option<String>) -> Result<Vec<ApprovalEntry>> {
    store::list(config, campaign_id.as_deref(), None)
}

pub async fn get(config: &Config, id: ApprovalId) -> Result<Option<ApprovalEntry>> {
    store::get(config, &id)
}

/// Enqueue a draft. Called by the executor when a campaign with
/// `DraftAndApprove` would have fired an outbound action.
pub fn enqueue(config: &Config, req: EnqueueApprovalRequest) -> Result<ApprovalId> {
    let id = store::enqueue(config, req.clone())?;
    publish_global(DomainEvent::CampaignApprovalEnqueued {
        approval_id: id.clone(),
        campaign_id: req.campaign_id.clone(),
        workflow_id: req.workflow_id.clone(),
        action_kind: req.action_kind.clone(),
        target: req.target.clone(),
    });
    Ok(id)
}

pub async fn approve(
    config: &Config,
    id: ApprovalId,
    edited_payload: Option<serde_json::Value>,
    decided_by: String,
) -> Result<ApprovalEntry> {
    let entry = store::record_decision(
        config,
        &id,
        ApprovalStatus::Approved,
        &decided_by,
        edited_payload,
    )?;
    publish_global(DomainEvent::CampaignApprovalDecided {
        approval_id: id,
        campaign_id: entry.campaign_id.clone(),
        status_json: serde_json::to_value(entry.status).unwrap_or(serde_json::Value::Null),
    });
    Ok(entry)
}

pub async fn reject(
    config: &Config,
    id: ApprovalId,
    _reason: Option<String>,
    decided_by: String,
) -> Result<ApprovalEntry> {
    let entry = store::record_decision(config, &id, ApprovalStatus::Rejected, &decided_by, None)?;
    publish_global(DomainEvent::CampaignApprovalDecided {
        approval_id: id,
        campaign_id: entry.campaign_id.clone(),
        status_json: serde_json::to_value(entry.status).unwrap_or(serde_json::Value::Null),
    });
    Ok(entry)
}

pub async fn batch_approve(
    config: &Config,
    ids: Vec<ApprovalId>,
    decided_by: String,
) -> Result<Vec<ApprovalEntry>> {
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        // Per-id approve so a single bad id doesn't poison the batch
        // — failures are skipped + logged but don't short-circuit.
        match approve(config, id.clone(), None, decided_by.clone()).await {
            Ok(entry) => out.push(entry),
            Err(e) => {
                tracing::warn!(
                    target: "approvals",
                    "[approvals] batch_approve: id={id} failed: {e:#}"
                );
            }
        }
    }
    Ok(out)
}

#[allow(dead_code)]
pub(crate) fn _decision_anchor(_: ApprovalDecision) {
    // Anchor so the unused-import lint doesn't trip when the agent
    // tool surface that consumes ApprovalDecision lands in F4-11+.
}
