//! Workflow CRUD operations.
//!
//! Phase 1 / F-2 ships the mutating + read surface: `list`, `get`,
//! `create`, `update`, `delete`, `enable`, `disable`. F-8 will add the
//! run-row CRUD (`insert_run`, `mark_run_terminal`, `list_runs`,
//! `get_run`, `count_runs`).
//!
//! Each mutating op publishes the matching `DomainEvent::Workflow*`
//! event on the bus so F-3's subscriber (health recompute on connection
//! events), F-7's scheduler (cron registration), and any future
//! observer can react without polling.

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::config::Config;
use crate::openhuman::connections::aggregator;
use crate::openhuman::workflows::health::{self, ConnectionsSnapshot};
use crate::openhuman::workflows::scheduler;
use crate::openhuman::workflows::store::{self, Pagination};
use crate::openhuman::workflows::templates;
use crate::openhuman::workflows::types::{
    CreateWorkflowRequest, ListFilter, Run, RunId, RunNowError, RunStep, StarterTemplate,
    StarterTemplateView, Trigger, UpdateWorkflowRequest, Workflow, WorkflowHealth, WorkflowId,
    WorkflowOrigin,
};
use crate::rpc::RpcOutcome;
use anyhow::{anyhow, Result};
use chrono::Utc;
use std::collections::HashSet;
use uuid::Uuid;

/// Phase the workflows engine reports as "current". F2-16 bumps to 2
/// now that every Phase 2 trigger (webhook / composio_event /
/// channel_message) + node kind (tool_call / http_request /
/// channel_message / condition / delay) + the active_hours gate ships.
/// The drafter validator (`workflow_propose_create` /
/// `_update`) reads this constant so chat-driven proposals can now
/// emit multi-node Phase 2 chains.
const CURRENT_PHASE: u32 = 2;

/// Wrap [`scheduler::register`] so a scheduler hiccup doesn't fail
/// the surrounding RPC. The scheduler is a derived view of the
/// workflows table — if registration fails (e.g. a corrupt cron
/// expression that slipped past the validator), the persisted row is
/// still correct and `reconcile_at_startup` will retry on the next
/// core boot.
fn scheduler_register_best_effort(workflow: &Workflow) {
    if let Err(err) = scheduler::register(workflow) {
        tracing::warn!(
            target: "workflows-rpc",
            "[workflows-rpc] scheduler::register failed for wf={}: {err:#}; persisted row is unchanged",
            workflow.id
        );
    }
}

/// F2-9: best-effort tunnel registration for `Webhook` triggers.
///
/// Mirrors `scheduler_register_best_effort` — failures (no socket
/// manager bound yet, router lock contention) only log; the persisted
/// row is the source of truth and the next `reconcile_at_startup`
/// will retry. No-op for non-Webhook triggers AND for disabled
/// workflows (an inactive workflow shouldn't be accepting inbound
/// POSTs).
fn webhook_register_best_effort(workflow: &Workflow) {
    let crate::openhuman::workflows::types::Trigger::Webhook { tunnel_uuid, .. } =
        &workflow.trigger
    else {
        return;
    };
    if !workflow.enabled {
        return;
    }
    let Some(mgr) = crate::openhuman::socket::global_socket_manager() else {
        tracing::debug!(
            target: "workflows-rpc",
            "[workflows-rpc] webhook register: no socket manager — skipping (reconcile_at_startup will retry)"
        );
        return;
    };
    let Some(router) = mgr.webhook_router() else {
        tracing::debug!(
            target: "workflows-rpc",
            "[workflows-rpc] webhook register: socket manager has no router yet — skipping"
        );
        return;
    };
    let uuid_str = tunnel_uuid.to_string();
    if let Err(err) = router.register_workflow(
        &uuid_str,
        &workflow.id,
        Some(format!("workflow:{}", workflow.id)),
        None,
    ) {
        tracing::warn!(
            target: "workflows-rpc",
            "[workflows-rpc] webhook register_workflow failed for wf={} tunnel={}: {err}; \
             persisted row is unchanged",
            workflow.id, uuid_str
        );
    }
}

/// F2-9: best-effort tunnel deregistration. Called on
/// disable/delete/update-with-trigger-change. Same fallback semantics
/// as register — failures log; the persisted row is the source of
/// truth.
fn webhook_deregister_best_effort(workflow_id: &WorkflowId, trigger: &Trigger) {
    let crate::openhuman::workflows::types::Trigger::Webhook { tunnel_uuid, .. } = trigger else {
        return;
    };
    let Some(mgr) = crate::openhuman::socket::global_socket_manager() else {
        return;
    };
    let Some(router) = mgr.webhook_router() else {
        return;
    };
    let uuid_str = tunnel_uuid.to_string();
    if let Err(err) = router.unregister(&uuid_str, "workflow") {
        tracing::debug!(
            target: "workflows-rpc",
            "[workflows-rpc] webhook unregister for wf={} tunnel={} returned: {err}",
            workflow_id, uuid_str
        );
    }
}

/// Build a `ConnectionsSnapshot` from the live aggregator output. On
/// aggregator failure (network blip during a Composio fan-out, etc.)
/// we fall back to an empty snapshot — the workflow is then marked
/// `NeedsConnections { missing: refs }`. F-3's subscriber will fix it
/// up on the next `ConnectionAdded` event.
async fn current_snapshot(config: &Config) -> ConnectionsSnapshot {
    match aggregator::list_all(config).await {
        Ok(views) => ConnectionsSnapshot::new(views),
        Err(err) => {
            tracing::warn!(
                target: "workflows",
                "[workflows-rpc] aggregator::list_all failed during health recompute: {err:#}; falling back to empty snapshot"
            );
            ConnectionsSnapshot::empty()
        }
    }
}

/// `workflows_list` — paginated/filtered list view.
pub async fn list(config: &Config, filter: ListFilter) -> Result<RpcOutcome<Vec<Workflow>>> {
    let rows = store::list_workflows(config, &filter)?;
    let total = rows.len();
    tracing::debug!(
        target: "workflows",
        "[workflows-rpc] list count={total} filter={filter:?}"
    );
    Ok(RpcOutcome::single_log(
        rows,
        format!("workflows_list returned {total} rows"),
    ))
}

/// `workflows_get` — single-row fetch. Returns `Ok(None)` when the id is
/// unknown so the list-view can detect deleted-mid-edit without an
/// error path.
pub async fn get(config: &Config, id: WorkflowId) -> Result<RpcOutcome<Option<Workflow>>> {
    let wf = store::get_workflow(config, &id)?;
    tracing::debug!(
        target: "workflows",
        "[workflows-rpc] get id={id} hit={}",
        wf.is_some()
    );
    Ok(RpcOutcome::single_log(wf, format!("workflows_get id={id}")))
}

/// `workflows_create` — assemble + persist + publish `WorkflowDefined`.
///
/// Validation in F-2 is shallow on purpose: required scalars
/// (`name` non-empty, `nodes` non-empty), and a hard reject on
/// `origin = Imported` (no importer ships in Phase 1 — accepting it
/// here would let an accidental client forge provenance). F-11 lands
/// the deeper semantic validation against the connections snapshot.
pub async fn create(config: &Config, req: CreateWorkflowRequest) -> Result<RpcOutcome<Workflow>> {
    if req.name.trim().is_empty() {
        return Err(anyhow!("workflows_create: `name` is required"));
    }
    if req.nodes.is_empty() {
        return Err(anyhow!("workflows_create: `nodes` must not be empty"));
    }
    if matches!(req.origin, WorkflowOrigin::Imported) {
        // Phase 1 has no import path. Accepting this would let an
        // accidental client forge provenance against the F-5 catalog
        // dedup query.
        return Err(anyhow!(
            "workflows_create: `origin = Imported` is not allowed in Phase 1"
        ));
    }

    let now = Utc::now();
    // UUIDv4 matches the established codebase convention (cron, etc.).
    // The F-1 ticket spec called for UUIDv7 but the workspace `uuid`
    // crate only enables the `v4` feature, and at Phase 1 scale
    // (O(10s) of workflows per user) the index-locality benefit of v7
    // doesn't matter. Documented in DEVLOG.
    let id = Uuid::new_v4().to_string();
    let workflow_seed = Workflow {
        id: id.clone(),
        schema_version: 1,
        name: req.name,
        description: req.description,
        enabled: false,
        origin: req.origin.clone(),
        health: WorkflowHealth::Ready, // overwritten below
        trigger: req.trigger,
        nodes: req.nodes,
        edges: req.edges,
        settings: req.settings.unwrap_or_default(),
        created_at: now,
        updated_at: now,
        last_run_at: None,
    };

    let mut workflow = workflow_seed;
    let snapshot = current_snapshot(config).await;
    workflow.health = health::recompute(&workflow, &snapshot);

    store::insert_workflow(config, &workflow)?;

    // F-7: schedule the cron trigger if the workflow ships enabled.
    // ops::create defaults `enabled = false`, so this is normally a
    // no-op; the F-6 catalog's [Add & Enable] flow follows up with
    // `workflows_enable` which calls register() then.
    scheduler_register_best_effort(&workflow);
    // F2-9: register the inbound webhook tunnel if the trigger is
    // Webhook and the workflow ships enabled. Same enabled-gate as
    // the scheduler — disabled workflows don't accept inbound POSTs.
    webhook_register_best_effort(&workflow);

    publish_global(DomainEvent::WorkflowDefined {
        workflow_id: workflow.id.clone(),
        origin_json: serde_json::to_value(&workflow.origin).unwrap_or(serde_json::Value::Null),
    });
    tracing::info!(
        target: "workflows",
        "[workflows-rpc] create id={} origin={:?}",
        workflow.id,
        workflow.origin
    );

    let log = format!("workflows_create id={}", workflow.id);
    Ok(RpcOutcome::single_log(workflow, log))
}

/// `workflows_update` — partial update via [`WorkflowPatch`]. Applies
/// only the `Some(_)` fields, bumps `updated_at`, recomputes health,
/// publishes `WorkflowUpdated`.
pub async fn update(config: &Config, req: UpdateWorkflowRequest) -> Result<RpcOutcome<Workflow>> {
    let mut workflow = store::get_workflow(config, &req.id)?
        .ok_or_else(|| anyhow!("workflows_update: id `{}` not found", req.id))?;

    let p = req.patches;
    if let Some(name) = p.name {
        if name.trim().is_empty() {
            return Err(anyhow!("workflows_update: `name` cannot be empty"));
        }
        workflow.name = name;
    }
    if let Some(description) = p.description {
        workflow.description = description;
    }
    if let Some(trigger) = p.trigger {
        workflow.trigger = trigger;
    }
    if let Some(nodes) = p.nodes {
        if nodes.is_empty() {
            return Err(anyhow!("workflows_update: `nodes` must not be empty"));
        }
        workflow.nodes = nodes;
    }
    if let Some(edges) = p.edges {
        workflow.edges = edges;
    }
    if let Some(settings) = p.settings {
        workflow.settings = settings;
    }

    workflow.updated_at = Utc::now();
    let snapshot = current_snapshot(config).await;
    workflow.health = health::recompute(&workflow, &snapshot);

    let updated = store::update_workflow(config, &workflow)?;
    if !updated {
        // Row was deleted between the load and the update — surface as
        // not-found rather than silently no-op'ing.
        return Err(anyhow!("workflows_update: id `{}` not found", req.id));
    }

    // F-7: re-register the cron trigger. The deregister-then-register
    // pair handles every shape change (cron expr edit, enabled bit
    // flipped via a patch, trigger type changed Manual ↔ Cron).
    scheduler::deregister(&workflow.id);
    scheduler_register_best_effort(&workflow);
    // F2-9: do the same for webhook tunnels. Trigger-shape changes
    // (Manual → Webhook, or Webhook → Cron) flow through this same
    // deregister-then-register pair.
    webhook_deregister_best_effort(&workflow.id, &workflow.trigger);
    webhook_register_best_effort(&workflow);

    publish_global(DomainEvent::WorkflowUpdated {
        workflow_id: workflow.id.clone(),
    });
    tracing::info!(
        target: "workflows",
        "[workflows-rpc] update id={}",
        workflow.id
    );

    let log = format!("workflows_update id={}", workflow.id);
    Ok(RpcOutcome::single_log(workflow, log))
}

/// `workflows_delete` — F2-14 soft-delete. Sets `workflows.deleted_at`
/// and unregisters cron / webhook hooks so the row stops firing
/// immediately. The actual row removal happens 30 days later via
/// [`retention::run_purge_sweep`]; in the meantime the user can
/// `workflows_restore` to bring the workflow back with its run
/// history intact.
pub async fn delete(config: &Config, id: WorkflowId) -> Result<RpcOutcome<bool>> {
    // F2-9: peek at the trigger BEFORE the soft-delete so we can
    // deregister the webhook tunnel (if any) cleanly. Cheap read;
    // none-result short-circuits below.
    let pre_delete_trigger: Option<Trigger> = store::get_workflow(config, &id)?.map(|w| w.trigger);
    // F-7: deregister BEFORE the soft-delete so a cron tick can't
    // race the state change and dispatch a run against a workflow
    // we've already marked as deleted.
    scheduler::deregister(&id);
    if let Some(trigger) = &pre_delete_trigger {
        webhook_deregister_best_effort(&id, trigger);
    }
    let removed = store::delete_workflow(config, &id)?;
    if removed {
        publish_global(DomainEvent::WorkflowDeleted {
            workflow_id: id.clone(),
        });
        tracing::info!(target: "workflows", "[workflows-rpc] soft-delete id={id}");
    } else {
        tracing::debug!(
            target: "workflows",
            "[workflows-rpc] delete id={id} no-op (already absent or already soft-deleted)"
        );
    }
    let log = format!("workflows_delete id={id} removed={removed}");
    Ok(RpcOutcome::single_log(removed, log))
}

/// `workflows_restore` — F2-14 undo for `workflows_delete`. Clears
/// `deleted_at`, re-registers cron / webhook hooks, and returns the
/// restored row. No-op when the id doesn't exist or the row is
/// already live.
pub async fn restore(config: &Config, id: WorkflowId) -> Result<RpcOutcome<Option<Workflow>>> {
    let restored = store::restore_workflow(config, &id)?;
    if !restored {
        tracing::debug!(
            target: "workflows",
            "[workflows-rpc] restore id={id} no-op (already live or unknown)"
        );
        let log = format!("workflows_restore id={id} restored=false");
        return Ok(RpcOutcome::single_log(None, log));
    }

    let workflow = store::get_workflow(config, &id)?
        .ok_or_else(|| anyhow!("workflows_restore: id `{id}` vanished between restore and read"))?;

    // Re-register the trigger hooks the soft-delete tore down.
    scheduler_register_best_effort(&workflow);
    webhook_register_best_effort(&workflow);

    publish_global(DomainEvent::WorkflowDefined {
        workflow_id: workflow.id.clone(),
        origin_json: serde_json::to_value(&workflow.origin).unwrap_or(serde_json::Value::Null),
    });
    tracing::info!(target: "workflows", "[workflows-rpc] restore id={id}");

    let log = format!("workflows_restore id={id} restored=true");
    Ok(RpcOutcome::single_log(Some(workflow), log))
}

/// `workflows_enable` — flip `enabled = true` and publish
/// `WorkflowEnabled`. No-op (no event) when the workflow is already
/// enabled, to avoid event-storm.
pub async fn enable(config: &Config, id: WorkflowId) -> Result<RpcOutcome<Workflow>> {
    set_enabled_to(config, id, true).await
}

/// `workflows_disable` — flip `enabled = false`.
pub async fn disable(config: &Config, id: WorkflowId) -> Result<RpcOutcome<Workflow>> {
    set_enabled_to(config, id, false).await
}

async fn set_enabled_to(
    config: &Config,
    id: WorkflowId,
    target: bool,
) -> Result<RpcOutcome<Workflow>> {
    let mut workflow = store::get_workflow(config, &id)?
        .ok_or_else(|| anyhow!("workflows_enable/disable: id `{id}` not found"))?;

    if workflow.enabled == target {
        // Idempotent no-op; skip the bus publish so subscribers don't
        // see redundant transitions.
        let action = if target { "enable" } else { "disable" };
        let log = format!("workflows_{action} id={id} (already {target})");
        return Ok(RpcOutcome::single_log(workflow, log));
    }

    let now = Utc::now();
    let updated = store::set_enabled(config, &id, target, now)?;
    if !updated {
        return Err(anyhow!("workflows_enable/disable: id `{id}` not found"));
    }
    workflow.enabled = target;
    workflow.updated_at = now;

    // F-7: keep the cron-trigger registration in sync with the
    // `enabled` bit. enable → register; disable → deregister.
    // F2-9: webhook tunnels follow the same pattern.
    if target {
        scheduler_register_best_effort(&workflow);
        webhook_register_best_effort(&workflow);
    } else {
        scheduler::deregister(&id);
        webhook_deregister_best_effort(&id, &workflow.trigger);
    }

    if target {
        publish_global(DomainEvent::WorkflowEnabled {
            workflow_id: id.clone(),
        });
    } else {
        publish_global(DomainEvent::WorkflowDisabled {
            workflow_id: id.clone(),
        });
    }
    let action = if target { "enable" } else { "disable" };
    tracing::info!(target: "workflows", "[workflows-rpc] {action} id={id}");

    let log = format!("workflows_{action} id={id}");
    Ok(RpcOutcome::single_log(workflow, log))
}

/// `workflows_run_now` — fire a manual dispatch. Returns the new
/// run id on success; surfaces `RunNowError` (NotFound / HealthBlocked /
/// Dispatch) verbatim so the RPC layer can map to the right
/// `RpcOutcome::Err { code }`.
pub async fn run_now(
    config: &Config,
    workflow_id: WorkflowId,
    initiator: crate::openhuman::workflows::types::ManualInitiator,
) -> Result<RpcOutcome<RunId>, RunNowError> {
    let run_id = scheduler::handle_run_now(config, workflow_id.clone(), initiator).await?;
    let log = format!("workflows_run_now wf={workflow_id} run={run_id}");
    Ok(RpcOutcome::single_log(run_id, log))
}

/// `workflows_cancel_run` — request a soft cancel. F-9 wires the real
/// executor side; F-7 surfaces the RPC so F-14's UI can already
/// bind to it.
pub async fn cancel_run(
    config: &Config,
    run_id: RunId,
) -> Result<RpcOutcome<bool>, crate::openhuman::workflows::executor::CancelError> {
    crate::openhuman::workflows::executor::cancel_run(config, run_id.clone()).await?;
    let log = format!("workflows_cancel_run run={run_id} cancelled=true");
    Ok(RpcOutcome::single_log(true, log))
}

/// Count of `workflow_runs` rows for a workflow. F-12's
/// `workflow_propose_delete` surfaces this through
/// [`WorkflowDeletePreview::run_count`] so the user sees how much
/// history will be dropped on the cascade delete (FR-1.3.4).
pub async fn count_runs(config: &Config, workflow_id: &WorkflowId) -> Result<RpcOutcome<u32>> {
    let n = store::count_runs(config, workflow_id)?;
    let log = format!("workflows_count_runs wf={workflow_id} count={n}");
    Ok(RpcOutcome::single_log(n, log))
}

/// `workflows_list_runs` — paginated runs view, newest-first.
///
/// Phase 1 caps `limit` to 100 (NFR-2.5.6) via [`Pagination::clamp`]
/// so an agent tool / runaway client can't request a million-row page.
pub async fn list_runs(
    config: &Config,
    workflow_id: WorkflowId,
    pagination: Pagination,
) -> Result<RpcOutcome<Vec<Run>>> {
    let pagination = pagination.clamp();
    let rows = store::list_runs(config, &workflow_id, pagination.clone())?;
    let count = rows.len();
    let log = format!(
        "workflows_list_runs wf={workflow_id} limit={l} offset={o} count={count}",
        l = pagination.limit,
        o = pagination.offset
    );
    Ok(RpcOutcome::single_log(rows, log))
}

/// `workflows_get_run` — fetch a single run + its steps. Returns
/// `Ok(None)` when the run id is unknown so the polling UI can
/// distinguish "deleted mid-poll" from a transport error.
pub async fn get_run(config: &Config, run_id: RunId) -> Result<RpcOutcome<Option<RunWithSteps>>> {
    let row = store::get_run(config, &run_id)?;
    let hit = row.is_some();
    let payload = row.map(|(run, steps)| RunWithSteps { run, steps });
    let log = format!("workflows_get_run id={run_id} hit={hit}");
    Ok(RpcOutcome::single_log(payload, log))
}

/// Composite response for [`workflows_get_run`] — the run row plus
/// its persisted step rows in started_at ASC order.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RunWithSteps {
    pub run: Run,
    pub steps: Vec<RunStep>,
}

/// `workflows_list_starter_templates` — read-only catalog query.
///
/// Pipeline: parse the bundled templates → filter by `phase` (defaults
/// to [`CURRENT_PHASE`]) → dedup against the user's existing
/// `Seed { template_id }` workflows → compute `missing_connections`
/// against the live aggregator snapshot → return one
/// [`StarterTemplateView`] per surviving template.
///
/// Per ADR-008 the catalog is **read-only**: this op never persists
/// anything. F-6's [Add] button calls `workflows_create` with the
/// view's `raw_payload`.
pub async fn list_starter_templates(
    config: &Config,
    phase: Option<u32>,
) -> Result<RpcOutcome<Vec<StarterTemplateView>>> {
    let phase = phase.unwrap_or(CURRENT_PHASE);
    let bundled = templates::all_bundled();
    let user_seeded: HashSet<String> = store::list_seed_origins(config)?.into_iter().collect();
    let snapshot = current_snapshot(config).await;

    let views: Vec<StarterTemplateView> = bundled
        .into_iter()
        .filter(|t| t.min_phase <= phase)
        .filter(|t| !user_seeded.contains(&t.template_id))
        .map(|t| build_view(t, &snapshot))
        .collect();

    let log = format!(
        "workflows_list_starter_templates phase={phase} returned={count}",
        count = views.len()
    );
    Ok(RpcOutcome::single_log(views, log))
}

/// Assemble a [`StarterTemplateView`] from a parsed [`StarterTemplate`]
/// + the current connections snapshot. The `raw_payload` is the
/// serde_json representation of the original template body — F-6's
/// [Add] flow passes it straight into `workflows_create` without
/// reparsing.
fn build_view(template: StarterTemplate, snapshot: &ConnectionsSnapshot) -> StarterTemplateView {
    let trigger_summary = summarize_trigger_value(&template.trigger);
    let missing_connections: Vec<_> = template
        .required_connections
        .iter()
        .filter(|r| !snapshot.is_connected(r))
        .cloned()
        .collect();
    let raw_payload = serde_json::to_value(&template).unwrap_or(serde_json::Value::Null);

    StarterTemplateView {
        template_id: template.template_id,
        name: template.name,
        description: template.description,
        tags: template.tags,
        trigger_summary,
        required_connections: template.required_connections,
        missing_connections,
        rationale_at_seed: template.rationale_at_seed,
        raw_payload,
    }
}

/// Cheap, deterministic trigger summary. Phase 1 produces a stable
/// label per [`Trigger`] variant; F-14's cron-humanizer hook can land
/// the full natural-language string later without changing this
/// surface.
fn summarize_trigger_value(value: &serde_json::Value) -> String {
    // Best-effort: deserialize into the typed `Trigger` shape. If the
    // template carries a Phase-2 variant we don't model yet, fall back
    // to the raw `type` discriminator.
    match serde_json::from_value::<Trigger>(value.clone()) {
        Ok(Trigger::Cron { expr, tz, .. }) => match tz.as_deref() {
            Some(z) => format!("{expr} ({z})"),
            None => expr,
        },
        Ok(Trigger::Manual) => "Run on demand".into(),
        Ok(Trigger::Webhook { target_path, .. }) => format!("Webhook → {target_path}"),
        Ok(Trigger::ComposioEvent {
            toolkit,
            trigger_id,
        }) => {
            format!("{toolkit}: {trigger_id}")
        }
        Ok(Trigger::ChannelMessage { provider, .. }) => format!("{provider} message"),
        Err(_) => value
            .get("type")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| "Custom trigger".into()),
    }
}
