//! Read-only workflow tools registered on the agent surface (F-10).
//!
//! Four tools wrap the workflows domain's existing read RPCs so the
//! `agent_prompt` sub-agent (F-8) can introspect the user's workflows
//! without ever reaching a mutating surface. F-12 will land the
//! `workflow_propose_*` family alongside these; together they form
//! the complete agent-callable surface defined by ADR-016 / NFR-2.3.7.
//!
//! Naming follows the `workflow_<verb>` singular / `workflows_<verb>`
//! plural convention from NFR-2.5.2:
//!
//! - `workflow_list` — caller's workflows.
//! - `workflow_get` — single workflow by id.
//! - `workflows_list_runs` — paginated run history for a workflow.
//! - `workflows_get_run` — a single run + its steps.
//!
//! The name constants here are the canonical source consumed by
//! [`crate::openhuman::workflows::executor::READ_ONLY_WORKFLOW_TOOL_NAMES`]
//! and [`crate::openhuman::workflows::executor::build_node_agent_definition`].

mod get;
mod get_run;
mod list;
mod list_runs;

#[cfg(test)]
mod tests;

pub use get::WorkflowGetTool;
pub use get_run::WorkflowsGetRunTool;
pub use list::WorkflowListTool;
pub use list_runs::WorkflowsListRunsTool;

/// Stable tool name for [`WorkflowListTool`]. F-8's allowlist references
/// this verbatim — keep in sync with
/// `executor::READ_ONLY_WORKFLOW_TOOL_NAMES[0]`.
pub const TOOL_WORKFLOW_LIST: &str = "workflow_list";
/// Stable tool name for [`WorkflowGetTool`].
pub const TOOL_WORKFLOW_GET: &str = "workflow_get";
/// Stable tool name for [`WorkflowsListRunsTool`].
pub const TOOL_WORKFLOWS_LIST_RUNS: &str = "workflows_list_runs";
/// Stable tool name for [`WorkflowsGetRunTool`].
pub const TOOL_WORKFLOWS_GET_RUN: &str = "workflows_get_run";

/// Canonical list of the four read-only workflow tool names. Used by
/// the allowlist-enforcement test in
/// `workflows::agent_tools_tests` to assert F-8's
/// `READ_ONLY_WORKFLOW_TOOL_NAMES` constant matches every tool that
/// actually registers — a runtime "tool not found" inside an
/// `agent_prompt` node is the failure mode this catches.
pub const READ_ONLY_TOOL_NAMES: &[&str] = &[
    TOOL_WORKFLOW_LIST,
    TOOL_WORKFLOW_GET,
    TOOL_WORKFLOWS_LIST_RUNS,
    TOOL_WORKFLOWS_GET_RUN,
];
