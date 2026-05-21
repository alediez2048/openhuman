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
