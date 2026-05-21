//! Run lifecycle: dispatch, scheduler-gate, per-node execution.
//!
//! F-8 fills `dispatch_run` with the real logic (workflow row insert,
//! tokio task spawn, scheduler_gate, agent_prompt sub-agent
//! invocation, run-step persistence). F-9 fills `cancel_run` with the
//! single-flight + soft-cancel path.
//!
//! F-7 ships stub bodies so the scheduler (cron tick + manual
//! `workflows_run_now`) can call into a stable surface without
//! waiting for F-8. The stub returns a fresh `RunId` without
//! persisting anything; F-8's real `dispatch_run` keeps the same
//! signature.

use crate::openhuman::config::Config;
use crate::openhuman::workflows::types::{RunId, TriggerSource, WorkflowId};
use anyhow::Result;
use thiserror::Error;
use uuid::Uuid;

/// Failure modes for [`cancel_run`]. F-9 fills the real variants;
/// F-7 only ever returns `NotImplemented` so RPC callers see a
/// recognizable code while the executor side is still under
/// construction.
#[derive(Debug, Clone, Error)]
pub enum CancelError {
    #[error("cancel_run not implemented yet (F-9 will land it)")]
    NotImplemented,
    #[error("run id `{0}` not found")]
    NotFound(RunId),
}

impl CancelError {
    /// Stable error-code string for the RPC layer.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotImplemented => "not_implemented",
            Self::NotFound(_) => "not_found",
        }
    }
}

/// F-7 stub. Returns a fresh `RunId` without persisting a run row.
///
/// F-8 replaces the body with:
///   - persist `workflow_runs` row with `status = Running`
///   - publish `WorkflowRunStarted`
///   - spawn `execute_inner` on a tokio task (wrapping the
///     `scheduler_gate::wait_ready` + per-node `execute_agent_prompt`
///     work)
///   - return the `RunId` immediately
///
/// The signature is locked here so F-7's scheduler + RPC layer don't
/// shift when F-8 lands.
pub async fn dispatch_run(
    _config: &Config,
    workflow_id: WorkflowId,
    trigger_source: TriggerSource,
) -> Result<RunId> {
    let run_id = Uuid::new_v4().to_string();
    tracing::info!(
        target: "workflows-executor",
        "[workflows-executor] dispatch_run (F-7 stub) wf={workflow_id} run={run_id} source={trigger_source:?}"
    );
    Ok(run_id)
}

/// F-7 stub. Always returns [`CancelError::NotImplemented`]. F-9
/// lands the real soft-cancel path.
pub async fn cancel_run(_config: &Config, run_id: RunId) -> Result<(), CancelError> {
    tracing::debug!(
        target: "workflows-executor",
        "[workflows-executor] cancel_run called for run={run_id} — NotImplemented stub (F-9)"
    );
    Err(CancelError::NotImplemented)
}
