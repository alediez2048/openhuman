//! Agent-callable workflow tools (`workflow_*`).
//!
//! Two flavours, intentionally split across F-10 and F-12 so each PR
//! has a single security focus:
//!
//! - F-10 — **read-only tools**: `workflow_list`, `workflow_get`,
//!   `workflows_list_runs`, `workflows_get_run`. Plus the NFR-2.3.7
//!   allowlist enforcement test that asserts zero mutating tools.
//! - F-12 — **propose-only tools**: `workflow_propose_create`,
//!   `_update`, `_delete`, `_enable`, `_disable`, `_run_now`. All
//!   return preview payloads; none mutate (ADR-012).
//!
//! Every mutation happens via the `workflows_*` RPC surface (which the
//! UI calls on [Save] / [Enable] / etc.); the agent never has a path
//! to the mutating RPC.
//!
//! ## F-10 wiring
//!
//! The tool implementations live under
//! `src/openhuman/tools/impl/workflows/` (matching the
//! `tools/impl/<domain>/` convention used by every other tool). This
//! module re-exports the **stable tool names** so callers don't depend
//! on the implementation crate's internal layout:
//!
//! - [`TOOL_WORKFLOW_LIST`] / [`TOOL_WORKFLOW_GET`] /
//!   [`TOOL_WORKFLOWS_LIST_RUNS`] / [`TOOL_WORKFLOWS_GET_RUN`].
//! - [`READ_ONLY_TOOL_NAMES`] — slice carrying all four, kept in
//!   sync with [`crate::openhuman::workflows::executor::READ_ONLY_WORKFLOW_TOOL_NAMES`]
//!   by the F-10 enforcement test.
//!
//! Mutation names the agent must NOT have (for the negative-allowlist
//! assertion) are listed in [`FORBIDDEN_MUTATING_TOOL_NAMES`].

pub use crate::openhuman::tools::implementations::workflows::{
    PROPOSE_TOOL_NAMES, READ_ONLY_TOOL_NAMES, TOOL_WORKFLOWS_GET_RUN, TOOL_WORKFLOWS_LIST_RUNS,
    TOOL_WORKFLOW_GET, TOOL_WORKFLOW_LIST, TOOL_WORKFLOW_PROPOSE_CREATE,
    TOOL_WORKFLOW_PROPOSE_DELETE, TOOL_WORKFLOW_PROPOSE_DISABLE, TOOL_WORKFLOW_PROPOSE_ENABLE,
    TOOL_WORKFLOW_PROPOSE_RUN_NOW, TOOL_WORKFLOW_PROPOSE_UPDATE,
};

/// Names the agent surface must NEVER expose. Every entry corresponds
/// to a mutating workflows RPC; the agent reaches mutations only
/// through `workflow_propose_*` (F-12), which surfaces a preview the
/// user clicks-to-confirm.
///
/// The negative allowlist assertion in
/// `workflows::agent_tools_tests` walks every registered tool name and
/// rejects any match against this list — a regression catches the
/// moment someone accidentally exposes a write surface.
pub const FORBIDDEN_MUTATING_TOOL_NAMES: &[&str] = &[
    "workflows_create",
    "workflows_update",
    "workflows_delete",
    "workflows_enable",
    "workflows_disable",
    "workflows_run_now",
    "workflows_cancel_run",
    "workflow_create_from_proposal",
];
