//! JSON-RPC handlers for the workflows domain.
//!
//! Phase 1 / F-2 ships the mutating + read surface: `workflows_list`,
//! `workflows_get`, `workflows_create`, `workflows_update`,
//! `workflows_delete`, `workflows_enable`, `workflows_disable`. F-7
//! adds `workflows_run_now` + `workflows_cancel_run`. F-8 adds
//! `workflows_list_runs` + `workflows_get_run`. F-5 adds
//! `workflows_list_starter_templates`.
//!
//! All handlers return `RpcOutcome<T>` per `AGENTS.md`.

use crate::openhuman::config::Config;
use crate::openhuman::workflows::ops::{self, RunWithSteps};
use crate::openhuman::workflows::store::Pagination;
use crate::openhuman::workflows::types::{
    CreateWorkflowRequest, ListFilter, ListStarterTemplatesRequest, ManualInitiator, Run, RunId,
    StarterTemplateView, UpdateWorkflowRequest, Workflow, WorkflowId,
};
use crate::rpc::RpcOutcome;

/// `openhuman.workflows_list` — workflows matching the filter, sorted by
/// `updated_at DESC`.
pub async fn workflows_list(
    config: &Config,
    filter: ListFilter,
) -> Result<RpcOutcome<Vec<Workflow>>, String> {
    ops::list(config, filter).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_get` — fetch a single workflow by id; null when
/// the id is unknown.
pub async fn workflows_get(
    config: &Config,
    id: WorkflowId,
) -> Result<RpcOutcome<Option<Workflow>>, String> {
    ops::get(config, id).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_create` — persist a new workflow row and emit
/// `WorkflowDefined`. Rejects `origin = Imported` (no importer in
/// Phase 1).
pub async fn workflows_create(
    config: &Config,
    req: CreateWorkflowRequest,
) -> Result<RpcOutcome<Workflow>, String> {
    ops::create(config, req).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_update` — partial update via `WorkflowPatch`.
/// Recomputes health on every update.
pub async fn workflows_update(
    config: &Config,
    req: UpdateWorkflowRequest,
) -> Result<RpcOutcome<Workflow>, String> {
    ops::update(config, req).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_delete` — F2-14 soft-delete. Sets
/// `workflows.deleted_at` and unregisters cron / webhook hooks; the
/// row is hard-deleted by the retention sweep after 30 days.
/// `removed = false` when the id was unknown or already soft-deleted.
pub async fn workflows_delete(config: &Config, id: WorkflowId) -> Result<RpcOutcome<bool>, String> {
    ops::delete(config, id).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_restore` — F2-14 undo for `workflows_delete`.
/// Clears `deleted_at`, re-registers cron / webhook hooks, returns
/// the restored row. Returns `None` when the id was unknown or the
/// row was already live.
pub async fn workflows_restore(
    config: &Config,
    id: WorkflowId,
) -> Result<RpcOutcome<Option<Workflow>>, String> {
    ops::restore(config, id).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_enable` — flip `enabled = true`, emit
/// `WorkflowEnabled`.
pub async fn workflows_enable(
    config: &Config,
    id: WorkflowId,
) -> Result<RpcOutcome<Workflow>, String> {
    ops::enable(config, id).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_disable` — flip `enabled = false`, emit
/// `WorkflowDisabled`.
pub async fn workflows_disable(
    config: &Config,
    id: WorkflowId,
) -> Result<RpcOutcome<Workflow>, String> {
    ops::disable(config, id).await.map_err(|e| e.to_string())
}

/// `openhuman.workflows_list_starter_templates` — read-only catalog
/// query (F-5 / ADR-008). Returns the bundled RU-* templates the user
/// hasn't already seeded, with `missing_connections` computed against
/// the live aggregator snapshot.
pub async fn workflows_list_starter_templates(
    config: &Config,
    req: ListStarterTemplatesRequest,
) -> Result<RpcOutcome<Vec<StarterTemplateView>>, String> {
    ops::list_starter_templates(config, req.phase)
        .await
        .map_err(|e| e.to_string())
}

/// `openhuman.workflows_run_now` — fire a manual dispatch (F-7).
///
/// Returns the new `RunId` on success. Maps every `RunNowError`
/// variant to a structured string that includes the stable error
/// code so the UI / CLI can branch:
///   - `not_found` — workflow id is unknown.
///   - `health_blocked` — `health != Ready`. UI surfaces the
///     missing-connection list from the badge.
///   - `dispatch_failed` — store / executor error. Treat as transient.
pub async fn workflows_run_now(
    config: &Config,
    workflow_id: WorkflowId,
    initiator: ManualInitiator,
) -> Result<RpcOutcome<RunId>, String> {
    ops::run_now(config, workflow_id, initiator)
        .await
        .map_err(|e| {
            format!(
                "{code}: {body}",
                code = e.code(),
                body = serde_json::to_string(&e).unwrap_or_default()
            )
        })
}

/// `openhuman.workflows_list_runs` — paginated runs view, newest-first.
///
/// Limit is clamped to [1, 100] server-side; offset is unbounded.
pub async fn workflows_list_runs(
    config: &Config,
    workflow_id: WorkflowId,
    pagination: Pagination,
) -> Result<RpcOutcome<Vec<Run>>, String> {
    ops::list_runs(config, workflow_id, pagination)
        .await
        .map_err(|e| e.to_string())
}

/// `openhuman.workflows_get_run` — fetch a single run + its persisted
/// step rows. Returns `None` when the id is unknown.
pub async fn workflows_get_run(
    config: &Config,
    run_id: RunId,
) -> Result<RpcOutcome<Option<RunWithSteps>>, String> {
    ops::get_run(config, run_id)
        .await
        .map_err(|e| e.to_string())
}

/// `openhuman.workflows_cancel_run` — soft-cancel a running workflow
/// (F-9 fills the executor side; F-7 surfaces the RPC so F-14's UI
/// can already wire to it). Returns `not_implemented` until F-9
/// lands.
pub async fn workflows_cancel_run(
    config: &Config,
    run_id: RunId,
) -> Result<RpcOutcome<bool>, String> {
    ops::cancel_run(config, run_id)
        .await
        .map_err(|e| format!("{code}: {e}", code = e.code()))
}
