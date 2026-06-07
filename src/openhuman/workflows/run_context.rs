//! T-1 (Phase 2.5 Trust UX) — task-local scope for the active workflow
//! run's session id.
//!
//! Lets write tools deep in the call stack (currently `composio_execute`,
//! later `channel_send` / `webview_account_send` / native sends) publish
//! [`crate::core::event_bus::DomainEvent::DeliveryReceiptObserved`]
//! events scoped to the run that issued them — without each tool having
//! to plumb session id through its arguments.
//!
//! The workflow executor enters the scope around the
//! `Agent::from_config(...).run_single(...)` call in `run_agent_prompt`;
//! the scope is unset for chat-driven Composio calls (orchestrator turns,
//! direct RPC), so tools see `None` and skip the receipt — preserving
//! today's behaviour outside workflows.

use tokio::task_local;

task_local! {
    /// The active workflow run's event-bus session id (always shaped
    /// `"workflow:<run_id>"`, matching the F-16 `ToolExecutionCompleted`
    /// scope key). Set by the workflow executor via
    /// [`scope_workflow_run`]; read by write tools via
    /// [`current_workflow_session_id`].
    static WORKFLOW_RUN_SESSION_ID: String;
}

/// Run `fut` with the workflow run's session id bound on the task-local
/// stack. Mirrors the `tokio::task_local!::scope` ergonomics used by
/// other scoped contexts in the agent harness (sandbox, spawn-depth,
/// fork).
pub async fn scope_workflow_run<F>(session_id: String, fut: F) -> F::Output
where
    F: std::future::Future,
{
    WORKFLOW_RUN_SESSION_ID.scope(session_id, fut).await
}

/// Returns the active workflow run's session id, or `None` when the
/// caller isn't inside a workflow run scope (e.g. an orchestrator chat
/// turn, a direct RPC, or any tool dispatch that doesn't originate
/// from the workflow executor).
///
/// Tools should treat a `None` return as "skip the receipt" — outside
/// a workflow run there's no subscriber to listen for the event.
pub fn current_workflow_session_id() -> Option<String> {
    WORKFLOW_RUN_SESSION_ID.try_with(|v| v.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn returns_none_outside_scope() {
        assert!(current_workflow_session_id().is_none());
    }

    #[tokio::test]
    async fn returns_set_value_inside_scope() {
        let observed = scope_workflow_run("workflow:abc".into(), async move {
            current_workflow_session_id()
        })
        .await;
        assert_eq!(observed.as_deref(), Some("workflow:abc"));
    }

    #[tokio::test]
    async fn nested_scopes_use_inner_value() {
        let observed = scope_workflow_run("workflow:outer".into(), async move {
            scope_workflow_run("workflow:inner".into(), async move {
                current_workflow_session_id()
            })
            .await
        })
        .await;
        assert_eq!(observed.as_deref(), Some("workflow:inner"));
    }
}
