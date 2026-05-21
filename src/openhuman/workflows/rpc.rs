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
use crate::openhuman::workflows::ops;
use crate::openhuman::workflows::types::{
    CreateWorkflowRequest, ListFilter, ListStarterTemplatesRequest, ManualInitiator, RunId,
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

/// `openhuman.workflows_delete` — hard-delete with FK cascade. Returns
/// `removed = false` when the id was unknown so the call is idempotent.
pub async fn workflows_delete(config: &Config, id: WorkflowId) -> Result<RpcOutcome<bool>, String> {
    ops::delete(config, id).await.map_err(|e| e.to_string())
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
