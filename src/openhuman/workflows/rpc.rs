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
    CreateWorkflowRequest, ListFilter, UpdateWorkflowRequest, Workflow, WorkflowId,
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
