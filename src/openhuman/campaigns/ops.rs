//! Campaign operations (F4-3).
//!
//! Layer between the RPC surface and the SQLite store. Enforces the
//! lifecycle state machine (`CampaignStatus::can_transition_to`),
//! cascades pause/resume/archive to child workflows, and publishes
//! the matching `DomainEvent` so subscribers (UI, future proactive
//! surfacer) can react without polling.
//!
//! Mirrors the `workflows::ops` shape closely so callers, tests, and
//! the agent allowlist surface treat campaigns + workflows
//! symmetrically.

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::campaigns::store::{self, ListCampaignsFilter};
use crate::openhuman::campaigns::types::{
    Campaign, CampaignId, CampaignOpError, CampaignPatch, CampaignStatus, CreateCampaignRequest,
    UpdateCampaignRequest,
};
use crate::openhuman::config::Config;
use crate::openhuman::workflows;
use crate::rpc::RpcOutcome;
use chrono::Utc;
use uuid::Uuid;

// ── Helpers ────────────────────────────────────────────────────────────

fn now_id() -> String {
    Uuid::new_v4().to_string()
}

fn map_internal(e: anyhow::Error) -> CampaignOpError {
    CampaignOpError::Internal {
        detail: format!("{e:#}"),
    }
}

fn fetch_or_not_found(config: &Config, id: &CampaignId) -> Result<Campaign, CampaignOpError> {
    store::get_campaign(config, id)
        .map_err(map_internal)?
        .ok_or_else(|| CampaignOpError::NotFound { id: id.clone() })
}

/// Transition `campaign.status` to `to`, validate against the state
/// machine, and persist + publish the matching event. Returns the
/// freshly-read campaign so callers can return it to the user.
fn transition_status(
    config: &Config,
    mut campaign: Campaign,
    to: CampaignStatus,
    event: DomainEvent,
) -> Result<Campaign, CampaignOpError> {
    if !campaign.status.can_transition_to(to) {
        return Err(CampaignOpError::InvalidTransition {
            id: campaign.id.clone(),
            from: campaign.status,
            to,
        });
    }
    if campaign.status == to {
        // Idempotent no-op; skip the bus publish so subscribers don't
        // see redundant transitions.
        return Ok(campaign);
    }
    campaign.status = to;
    campaign.updated_at = Utc::now();
    let touched = store::update_campaign(config, &campaign).map_err(map_internal)?;
    if !touched {
        return Err(CampaignOpError::NotFound {
            id: campaign.id.clone(),
        });
    }
    publish_global(event);
    Ok(campaign)
}

/// Apply `f` to every child workflow of `campaign_id`. Best-effort —
/// individual workflow failures are logged but do not abort the
/// cascade. Returns the count that succeeded.
async fn cascade_to_children<F, Fut>(
    config: &Config,
    campaign_id: &CampaignId,
    op_label: &str,
    f: F,
) -> u32
where
    F: Fn(workflows::types::WorkflowId) -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<RpcOutcome<workflows::types::Workflow>>>,
{
    let ids = match store::list_workflow_ids_for_campaign(config, campaign_id) {
        Ok(v) => v,
        Err(err) => {
            tracing::warn!(
                target: "campaigns-ops",
                campaign_id = %campaign_id,
                "[campaigns-ops] cascade {op_label}: list_workflow_ids failed: {err:#}"
            );
            return 0;
        }
    };
    let mut ok: u32 = 0;
    for id in ids {
        let id_clone = id.clone();
        match f(id).await {
            Ok(_) => ok += 1,
            Err(err) => {
                tracing::warn!(
                    target: "campaigns-ops",
                    campaign_id = %campaign_id,
                    workflow_id = %id_clone,
                    "[campaigns-ops] cascade {op_label}: workflow op failed: {err:#}"
                );
            }
        }
    }
    ok
}

// ── CRUD ───────────────────────────────────────────────────────────────

/// Create a new campaign. Stamps `id`, `created_at`, `updated_at`,
/// `schema_version`; sets initial status to `Draft`. Publishes
/// `CampaignDefined` on success.
pub async fn create(
    config: &Config,
    req: CreateCampaignRequest,
) -> Result<RpcOutcome<Campaign>, CampaignOpError> {
    let id = now_id();
    let now = Utc::now();
    let campaign = Campaign {
        id: id.clone(),
        schema_version: 1,
        name: req.name,
        description: req.description,
        status: CampaignStatus::Draft,
        entity_binding: req.entity_binding,
        throttle: req.throttle,
        approval_policy: req.approval_policy,
        target_outcome: req.target_outcome,
        created_at: now,
        updated_at: now,
        last_run_at: None,
    };
    store::insert_campaign(config, &campaign).map_err(map_internal)?;
    publish_global(DomainEvent::CampaignDefined {
        campaign_id: id.clone(),
    });
    let log = format!("campaigns_create id={id} status=draft");
    Ok(RpcOutcome::single_log(campaign, log))
}

pub async fn get(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Option<Campaign>>, CampaignOpError> {
    let row = store::get_campaign(config, &id).map_err(map_internal)?;
    let log = format!("campaigns_get id={id} found={}", row.as_ref().is_some());
    Ok(RpcOutcome::single_log(row, log))
}

pub async fn list(
    config: &Config,
    filter: ListCampaignsFilter,
) -> Result<RpcOutcome<Vec<Campaign>>, CampaignOpError> {
    let status_label = filter
        .status
        .map(|s| format!("{s:?}"))
        .unwrap_or_else(|| "any".into());
    let rows = store::list_campaigns(config, filter).map_err(map_internal)?;
    let log = format!(
        "campaigns_list count={} status_filter={status_label}",
        rows.len()
    );
    Ok(RpcOutcome::single_log(rows, log))
}

/// Apply a `CampaignPatch` to an existing row. Status updates are NOT
/// routed through this op — use `pause` / `resume` / `archive` so the
/// lifecycle invariants stay enforced. Publishes `CampaignUpdated`.
pub async fn update(
    config: &Config,
    req: UpdateCampaignRequest,
) -> Result<RpcOutcome<Campaign>, CampaignOpError> {
    let mut current = fetch_or_not_found(config, &req.id)?;
    let patch = req.patch;
    if let Some(name) = patch.name {
        current.name = name;
    }
    if let Some(desc) = patch.description {
        current.description = Some(desc);
    }
    if let Some(binding) = patch.entity_binding {
        current.entity_binding = binding;
    }
    if let Some(throttle) = patch.throttle {
        current.throttle = throttle;
    }
    if let Some(policy) = patch.approval_policy {
        current.approval_policy = policy;
    }
    if let Some(outcome) = patch.target_outcome {
        current.target_outcome = outcome;
    }
    current.updated_at = Utc::now();
    let touched = store::update_campaign(config, &current).map_err(map_internal)?;
    if !touched {
        return Err(CampaignOpError::NotFound { id: req.id });
    }
    publish_global(DomainEvent::CampaignUpdated {
        campaign_id: current.id.clone(),
    });
    let log = format!("campaigns_update id={}", current.id);
    Ok(RpcOutcome::single_log(current, log))
}

pub async fn delete(config: &Config, id: CampaignId) -> Result<RpcOutcome<bool>, CampaignOpError> {
    let touched = store::delete_campaign(config, &id).map_err(map_internal)?;
    if touched {
        publish_global(DomainEvent::CampaignDeleted {
            campaign_id: id.clone(),
        });
    }
    let log = format!("campaigns_delete id={id} touched={touched}");
    Ok(RpcOutcome::single_log(touched, log))
}

pub async fn restore(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Option<Campaign>>, CampaignOpError> {
    let touched = store::restore_campaign(config, &id).map_err(map_internal)?;
    let row = if touched {
        store::get_campaign(config, &id).map_err(map_internal)?
    } else {
        None
    };
    let log = format!("campaigns_restore id={id} touched={touched}");
    Ok(RpcOutcome::single_log(row, log))
}

// ── Lifecycle transitions ──────────────────────────────────────────────

/// `Draft → Active` or `Paused → Active`. Re-enables every child
/// workflow (best-effort; failures logged, not propagated).
pub async fn resume(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, CampaignOpError> {
    let current = fetch_or_not_found(config, &id)?;
    let transitioned = transition_status(
        config,
        current,
        CampaignStatus::Active,
        DomainEvent::CampaignResumed {
            campaign_id: id.clone(),
        },
    )?;
    let n = cascade_to_children(config, &id, "resume", |wid| {
        let cfg = config.clone();
        async move { workflows::ops::enable(&cfg, wid).await }
    })
    .await;
    let log = format!("campaigns_resume id={id} children_enabled={n}");
    Ok(RpcOutcome::single_log(transitioned, log))
}

/// `Active → Paused`. Disables every child workflow.
pub async fn pause(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, CampaignOpError> {
    let current = fetch_or_not_found(config, &id)?;
    let transitioned = transition_status(
        config,
        current,
        CampaignStatus::Paused,
        DomainEvent::CampaignPaused {
            campaign_id: id.clone(),
        },
    )?;
    let n = cascade_to_children(config, &id, "pause", |wid| {
        let cfg = config.clone();
        async move { workflows::ops::disable(&cfg, wid).await }
    })
    .await;
    let log = format!("campaigns_pause id={id} children_disabled={n}");
    Ok(RpcOutcome::single_log(transitioned, log))
}

/// `Active | Paused | Draft → WoundDown`. Stops accepting new
/// records. Child workflows stay enabled for now — F4 design
/// expects the outbound workflows to short-circuit on
/// `campaign.status == WoundDown` checks (Phase 4 follow-up F4-8b).
pub async fn wind_down(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, CampaignOpError> {
    let current = fetch_or_not_found(config, &id)?;
    let transitioned = transition_status(
        config,
        current,
        CampaignStatus::WoundDown,
        DomainEvent::CampaignWoundDown {
            campaign_id: id.clone(),
        },
    )?;
    let log = format!("campaigns_wind_down id={id}");
    Ok(RpcOutcome::single_log(transitioned, log))
}

/// `WoundDown → Archived` (terminal). Disables every child workflow
/// regardless of its current state — archive means "stop everything,
/// preserved for audit only."
pub async fn archive(
    config: &Config,
    id: CampaignId,
) -> Result<RpcOutcome<Campaign>, CampaignOpError> {
    let current = fetch_or_not_found(config, &id)?;
    let transitioned = transition_status(
        config,
        current,
        CampaignStatus::Archived,
        DomainEvent::CampaignArchived {
            campaign_id: id.clone(),
        },
    )?;
    let n = cascade_to_children(config, &id, "archive", |wid| {
        let cfg = config.clone();
        async move { workflows::ops::disable(&cfg, wid).await }
    })
    .await;
    let log = format!("campaigns_archive id={id} children_disabled={n}");
    Ok(RpcOutcome::single_log(transitioned, log))
}

// ── Workflow link / unlink ─────────────────────────────────────────────

/// Link an existing standalone workflow to this campaign. Loads the
/// workflow, sets its `campaign_id`, persists. Returns `true` on
/// success / `false` when either id is unknown.
pub async fn add_workflow(
    config: &Config,
    campaign_id: CampaignId,
    workflow_id: workflows::types::WorkflowId,
) -> Result<RpcOutcome<bool>, CampaignOpError> {
    // Ensure the campaign exists (and is not soft-deleted).
    let _ = fetch_or_not_found(config, &campaign_id)?;
    let mut workflow =
        match workflows::store::get_workflow(config, &workflow_id).map_err(map_internal)? {
            Some(w) => w,
            None => {
                return Ok(RpcOutcome::single_log(
                    false,
                    format!("campaigns_add_workflow workflow_id={workflow_id} not_found"),
                ))
            }
        };
    workflow.campaign_id = Some(campaign_id.clone());
    workflow.updated_at = Utc::now();
    let touched = workflows::store::update_workflow(config, &workflow).map_err(map_internal)?;
    let log = format!(
        "campaigns_add_workflow campaign_id={campaign_id} workflow_id={workflow_id} touched={touched}"
    );
    Ok(RpcOutcome::single_log(touched, log))
}

/// Unlink a workflow from this campaign (sets `campaign_id = NULL`).
/// The workflow itself isn't deleted. Returns `true` if the workflow
/// was linked AND the unlink succeeded.
pub async fn remove_workflow(
    config: &Config,
    campaign_id: CampaignId,
    workflow_id: workflows::types::WorkflowId,
) -> Result<RpcOutcome<bool>, CampaignOpError> {
    let mut workflow =
        match workflows::store::get_workflow(config, &workflow_id).map_err(map_internal)? {
            Some(w) => w,
            None => {
                return Ok(RpcOutcome::single_log(
                    false,
                    format!("campaigns_remove_workflow workflow_id={workflow_id} not_found"),
                ))
            }
        };
    if workflow.campaign_id.as_deref() != Some(campaign_id.as_str()) {
        return Ok(RpcOutcome::single_log(
            false,
            format!(
                "campaigns_remove_workflow workflow_id={workflow_id} not linked to campaign={campaign_id}"
            ),
        ));
    }
    workflow.campaign_id = None;
    workflow.updated_at = Utc::now();
    let touched = workflows::store::update_workflow(config, &workflow).map_err(map_internal)?;
    let log = format!(
        "campaigns_remove_workflow campaign_id={campaign_id} workflow_id={workflow_id} touched={touched}"
    );
    Ok(RpcOutcome::single_log(touched, log))
}
