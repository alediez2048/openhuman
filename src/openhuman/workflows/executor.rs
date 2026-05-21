//! Run lifecycle: dispatch, scheduler-gate, per-node execution.
//!
//! F-8 fills this module: `dispatch_run` creates a `workflow_runs` row
//! and spawns `execute_inner` on a tokio task. `execute_inner` awaits
//! `scheduler_gate::wait_ready`, applies the per-workflow timeout,
//! walks the (Phase-1: single-node) graph, and dispatches to
//! `execute_agent_prompt`. `build_node_agent_definition` constructs the
//! `agent_prompt` sub-agent's allowlist (NFR-2.3.7).
//!
//! F-9 fills the single-flight `in_flight` map + soft-cancel +
//! boot-time orphan recovery.
