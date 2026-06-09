//! Run lifecycle: dispatch, scheduler-gate-shaped sequence, per-node
//! execution, run-row + step-row persistence.
//!
//! ## Phase 1 scope
//!
//! F-8 ships the structural pipeline end-to-end:
//!   - `dispatch_run` loads the workflow, validates Phase 1 invariants
//!     (single node, kind = AgentPrompt, health = Ready for cron
//!     ticks), persists `workflow_runs` with `status = Running`,
//!     publishes `WorkflowRunStarted`, spawns the run on a tokio task,
//!     and returns the new `RunId` immediately.
//!   - `execute_inner` walks the (Phase-1: single) node graph under a
//!     `tokio::time::timeout` derived from `workflow.settings.timeout_secs`
//!     (clamped to `[1, 3600]`s per FR-1.6.5). On timeout the run is
//!     marked `TimedOut`; on node failure it's marked `Failed` (per
//!     FR-1.6.4 Phase 1's `on_error = Halt` policy); otherwise
//!     `Succeeded`. Every transition publishes the matching
//!     `WorkflowRun*` event.
//!   - `execute_agent_prompt` persists a `workflow_run_steps` row,
//!     publishes `WorkflowRunStepStarted`, runs the node (see
//!     "agent-invocation placeholder" below), truncates output to
//!     64 KiB on a UTF-8 boundary, and publishes
//!     `WorkflowRunStepCompleted`.
//!   - `build_node_agent_definition(allowed_connections)` returns the
//!     allowlist NFR-2.3.7 specifies: baseline tools + the connection-
//!     resolved tools + the four read-only workflow tools (F-10
//!     registers those four; F-8 references them by stable name).
//!
//! ## Agent invocation (F-15 → F-16)
//!
//! [`run_agent_prompt`] uses
//! [`crate::openhuman::agent::Agent::from_config_for_agent_with_tool_override`]
//! to spawn a `workflow_node` archetype with the per-run
//! `NodeAgentDefinition.allowed_tools` allowlist. The TOML's empty
//! `[tools].named = []` is REPLACED with the dynamic list — so the
//! LLM sees only baseline + connection-resolved + read-only workflow
//! tools, and ADR-016's allowlist is enforced at runtime (not just
//! computed and discarded as it was before F-16).
//!
//! Event channel = `"workflow"`, session id = `"workflow:<run_id>"`
//! so downstream subscribers (token-usage accounting, telemetry,
//! Sentry, and F-16 D's tool-failure counter) can filter
//! workflow-driven turns from CLI / cron / chat.
//!
//! **Honest step status (F-16 D):** the executor subscribes to
//! [`DomainEvent::ToolExecutionCompleted`] events scoped to the
//! current run's session id before spawning the agent. Any tool
//! call the harness emitted with `success = false` (either denied
//! by `visible_tool_names` per `turn.rs:1035` or executed-with-error
//! per `turn.rs:1109`) increments the run's `tool_failure_count`.
//! If the count is > 0 when the agent finishes, the step is marked
//! `Failed` even if the agent itself returned text — closing the
//! pre-F-16 lie where a workflow's `Succeeded` status meant
//! "the agent emitted prose", not "all the actions actually fired".
//!
//! Tests inject a deterministic stub via
//! [`set_test_agent_prompt_override`] so the persistence pipeline
//! assertions don't depend on a configured LLM provider in the test
//! workspace. The override is `#[cfg(test)]`-gated; production code
//! never sees it.
//!
//! ## F-9 additions
//!
//! - Single-flight invariant (ADR-014): `dispatch_run` rejects a
//!   second overlapping dispatch with [`DispatchError::AlreadyRunning`]
//!   and publishes [`DomainEvent::WorkflowRunSkipped`]. Slot release
//!   is RAII via [`InFlightSlot`] so every exit path — including
//!   `panic!` inside `execute_inner` — frees the slot.
//! - Real [`cancel_run`]: looks up the run, returns `NotFound` /
//!   `NotRunning { current_status }` for the surface, otherwise
//!   flips the `workflow_runs.cancelled` bit. The current node's
//!   LLM call is **not** aborted (FR-1.6.9 cooperative cancel).
//!   `execute_inner` reads the bit between nodes via
//!   `cancellation_observed` and upgrades the terminal status to
//!   `Cancelled`.
//! - [`orphan_recovery_sweep`]: boot-time sweep that marks every
//!   `status = 'running'` row as `Failed { error = "CoreCrashed" }`.
//!   Wired into `src/core/jsonrpc.rs` BEFORE `reconcile_at_startup`
//!   so a re-registered cron tick can't bounce off a stale
//!   single-flight slot forever.

use crate::core::event_bus::{publish_global, DomainEvent};
use crate::openhuman::config::Config;
use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::workflows::memory as workflow_memory;
use crate::openhuman::workflows::run_context;
use crate::openhuman::workflows::store;
use crate::openhuman::workflows::types::{
    AgentPromptConfig, Node, NodeConfig, NodeKind, Run, RunId, RunStatus, RunStep, RunStepId,
    TriggerSource, Workflow, WorkflowId,
};
use anyhow::Result;
use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use thiserror::Error;
use uuid::Uuid;

// ── Tool allowlist ─────────────────────────────────────────────────────

/// Baseline tool names every `agent_prompt` sub-agent gets, regardless
/// of the workflow's `allowed_connections`. Exported so F-10 / F-12
/// can assert against this exact set in their allowlist-enforcement
/// tests.
///
/// Keeps memory + time + the unified `list_connections` (Phase 0)
/// always-available. Adding to this list requires updating F-10's
/// regression tests in lock-step.
pub const BASELINE_TOOL_NAMES: &[&str] = &[
    "memory_recall",
    "memory_store",
    "current_time",
    "list_connections",
    "web_search_tool",
    "web_fetch",
];

/// The four read-only workflow tools F-10 registers + that
/// [`build_node_agent_definition`] adds to every `agent_prompt`
/// sub-agent's allowlist. F-8 references these by name; F-10's
/// registration site is the source of truth for the tool bodies.
pub const READ_ONLY_WORKFLOW_TOOL_NAMES: &[&str] = &[
    "workflow_list",
    "workflow_get",
    "workflows_list_runs",
    "workflows_get_run",
];

/// Per-node tool surface the executor passes into the
/// `workflow_node` sub-agent at spawn time (F-16).
///
/// `allowed_tools` is the wire passed to
/// [`crate::openhuman::agent::Agent::from_config_for_agent_with_tool_override`]
/// — whatever names appear in this list are exactly what the LLM can
/// call from inside the workflow run; nothing else is reachable. This
/// is the runtime enforcement of ADR-016 (the F-15 placeholder swap
/// landed in F-16; the executor used to call `Agent::from_config`
/// without applying this list at all, which is how the orchestrator
/// identity leaked in and broke the Slack-send path).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeAgentDefinition {
    /// Exact `allowed_tools` set the sub-agent runs with. Order is
    /// stable: baseline → connection-resolved → read-only workflow
    /// tools. Tests assert on this list verbatim.
    pub allowed_tools: Vec<String>,
    /// Iteration cap from the node's [`AgentPromptConfig`]. Defaults
    /// to 12 if the template / proposal omitted it.
    pub iteration_cap: u32,
    /// Model tier from the node config; `None` lets the executor pick
    /// the project default. F-16 does not yet wire a per-tier override
    /// into the workflow_node agent definition (the TOML carries
    /// `model.hint = "agentic"` and the override builder doesn't
    /// touch the model field). When `model_tier` is Some, the
    /// executor logs the value at `info!` and proceeds with the
    /// definition's default model. Phase 2 follow-up.
    pub model_tier: Option<String>,
}

/// Build the allowlist for an `agent_prompt` node. Per ADR-016 the
/// shape is exactly:
///
///   `baseline + connection-resolved + read-only workflow tools`
///
/// — and nothing else (no `workflow_propose_*`, no mutating workflow
/// tools, no skill-creator surfaces).
///
/// **Composio discovery surface (F-16 follow-up).** When any
/// `ConnectionRef::Composio` is present in `allowed_connections`,
/// the connection-resolved block adds `composio_list_toolkits` and
/// `composio_list_tools` alongside `composio_execute`. Without
/// these, the LLM has no way to discover the real action slug to
/// pass as `composio_execute`'s `tool` parameter (which expects
/// e.g. `"GMAIL_SEND_EMAIL"`, not `"composio"` / `"gmail"` /
/// `"slack"`). Live testing on 2026-05-22 surfaced the agent
/// guessing `tool: "composio"` and the backend 400-ing with
/// `Toolkit "composio" is not enabled`. The discovery tools give
/// the agent a deterministic two-step path: list_tools → execute.
pub fn build_node_agent_definition(
    allowed_connections: &[ConnectionRef],
    iteration_cap: u32,
    model_tier: Option<String>,
) -> NodeAgentDefinition {
    let mut allowed_tools: Vec<String> =
        BASELINE_TOOL_NAMES.iter().map(|s| s.to_string()).collect();
    let has_composio = allowed_connections
        .iter()
        .any(|r| matches!(r, ConnectionRef::Composio { .. }));
    if has_composio {
        // Discovery tools land BEFORE the executor in the list so
        // the LLM sees the natural order: "find the action, then
        // run it". Both tools are read-only and cheap.
        allowed_tools.push("composio_list_toolkits".into());
        allowed_tools.push("composio_list_tools".into());
    }
    for r in allowed_connections {
        allowed_tools.push(connection_tool_name(r));
    }
    allowed_tools.extend(READ_ONLY_WORKFLOW_TOOL_NAMES.iter().map(|s| s.to_string()));
    // Dedup while preserving order — a sub-agent could plausibly list
    // `list_connections` in its connection set as a no-op (harmless).
    let mut seen = std::collections::HashSet::new();
    allowed_tools.retain(|t| seen.insert(t.clone()));
    NodeAgentDefinition {
        allowed_tools,
        iteration_cap,
        model_tier,
    }
}

/// Stable per-mechanism tool name the executor adds to the allowlist
/// for each `ConnectionRef` the node opts into. F-10's read-only
/// tools + F-12's propose-only tools are agnostic to these names;
/// the canonical resolution lives in the existing tool registry
/// (e.g. `composio_execute`, `channel_send`, etc.).
fn connection_tool_name(r: &ConnectionRef) -> String {
    match r {
        ConnectionRef::Composio { .. } => "composio_execute".into(),
        ConnectionRef::Channel { .. } => "channel_send".into(),
        ConnectionRef::Webview { .. } => "webview_account_send".into(),
        ConnectionRef::Builtin { integration } => format!("builtin_{integration}"),
        ConnectionRef::Mcp { .. } => "mcp_call_tool".into(),
        ConnectionRef::GenericHttp { .. } => "http_request".into(),
    }
}

// ── ExecutorState ──────────────────────────────────────────────────────

/// Process-global executor state — owns the single-flight invariant
/// from ADR-014. F-9 also moved the soft-cancel observer to the
/// persisted `workflow_runs.cancelled` column (read by
/// [`store::is_cancelled`]) instead of an in-memory map, so this
/// struct stays minimal.
pub struct ExecutorState {
    /// One in-flight `RunId` per `WorkflowId`. A `dispatch_run` call
    /// that lands on an occupied key publishes
    /// [`DomainEvent::WorkflowRunSkipped`] and returns
    /// [`DispatchError::AlreadyRunning`]. The slot is released by the
    /// [`InFlightSlot`] RAII guard moved into the spawned tokio task —
    /// so every exit path (success, failure, timeout, panic) releases.
    pub in_flight: Mutex<HashMap<WorkflowId, RunId>>,
}

impl ExecutorState {
    fn new() -> Self {
        Self {
            in_flight: Mutex::new(HashMap::new()),
        }
    }
}

fn state() -> &'static ExecutorState {
    static STATE: OnceLock<ExecutorState> = OnceLock::new();
    STATE.get_or_init(ExecutorState::new)
}

/// RAII guard that removes the workflow's `in_flight` entry on Drop.
/// Spawned into the run's tokio task by `dispatch_run` so every exit
/// path — success, error, timeout, panic — releases the slot.
struct InFlightSlot {
    workflow_id: WorkflowId,
    /// The `RunId` the slot was claimed for. Compared before removal
    /// so a stale guard (the workflow id was re-dispatched after a
    /// race we don't fully control) doesn't free another run's slot.
    run_id: RunId,
}

impl Drop for InFlightSlot {
    fn drop(&mut self) {
        let mut in_flight = state().in_flight.lock();
        if in_flight.get(&self.workflow_id) == Some(&self.run_id) {
            in_flight.remove(&self.workflow_id);
            tracing::debug!(
                target: "workflows-run",
                "[workflows-run] in_flight slot released wf={} run={}",
                self.workflow_id, self.run_id
            );
        } else {
            // Slot held a different RunId — leave it for that guard.
            tracing::warn!(
                target: "workflows-run",
                "[workflows-run] in_flight slot for wf={} held a different run when {} dropped; leaving as-is",
                self.workflow_id, self.run_id
            );
        }
    }
}

// ── Test-only state helpers (F-9) ──────────────────────────────────────

/// Manually claim the in-flight slot for a workflow. Used by F-9's
/// single-flight tests to set up the "previous run already
/// in-flight" precondition without spawning a tokio task that would
/// race the assertions.
#[cfg(test)]
pub fn state_in_flight_insert_for_test(workflow_id: WorkflowId, run_id: RunId) {
    state().in_flight.lock().insert(workflow_id, run_id);
}

/// Free a previously-claimed slot. Pair with
/// [`state_in_flight_insert_for_test`] so the test doesn't leak state
/// into sibling tests sharing the process-global executor singleton.
#[cfg(test)]
pub fn state_in_flight_remove_for_test(workflow_id: &str) {
    state().in_flight.lock().remove(workflow_id);
}

// ── Dispatch errors ────────────────────────────────────────────────────

#[derive(Debug, Clone, Error)]
pub enum DispatchError {
    #[error("workflow `{0}` not found")]
    NotFound(WorkflowId),
    #[error("workflow `{0}` has multiple nodes — Phase 1 supports exactly one agent_prompt node")]
    PhaseConstraint(WorkflowId),
    #[error("workflow `{0}`'s single node is `{1:?}` — Phase 1 supports only `agent_prompt`")]
    UnsupportedNodeKind(WorkflowId, NodeKind),
    /// Single-flight invariant (ADR-014) — a previous run for this
    /// workflow is still in-flight. The current `RunId` is surfaced so
    /// callers can deep-link to the existing run row.
    #[error("workflow `{workflow_id}` already running as run `{run_id}` (single-flight)")]
    AlreadyRunning {
        workflow_id: WorkflowId,
        run_id: RunId,
    },
    #[error("store error: {0}")]
    Store(String),
}

impl DispatchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::PhaseConstraint(_) => "phase_constraint",
            Self::UnsupportedNodeKind(..) => "unsupported_node_kind",
            Self::AlreadyRunning { .. } => "already_running",
            Self::Store(_) => "store_error",
        }
    }
}

/// Failure modes for [`cancel_run`]. F-9 fills both real cases — F-8's
/// `NotImplemented` placeholder is gone.
#[derive(Debug, Clone, Error)]
pub enum CancelError {
    #[error("run id `{0}` not found")]
    NotFound(RunId),
    /// The run reached a terminal status before the cancel arrived. The
    /// UI surfaces this as a transient "already complete" toast.
    #[error("run `{run_id}` is not running (current_status = {current_status:?})")]
    NotRunning {
        run_id: RunId,
        current_status: RunStatus,
    },
    #[error("store error: {0}")]
    Store(String),
}

impl CancelError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::NotRunning { .. } => "not_running",
            Self::Store(_) => "store_error",
        }
    }
}

// ── dispatch_run ───────────────────────────────────────────────────────

/// Dispatch a workflow run.
///
/// Enforces the single-flight invariant from ADR-014: while the
/// `ExecutorState.in_flight` slot is occupied for this `workflow_id`,
/// every additional `dispatch_run` call publishes
/// [`DomainEvent::WorkflowRunSkipped`] (reason = `AlreadyRunning`) and
/// returns [`DispatchError::AlreadyRunning`]. Slot release happens
/// inside the spawned task via the [`InFlightSlot`] guard — every
/// exit path (success, failure, timeout, panic) frees the slot.
///
/// Pipeline:
///   1. Load + validate the workflow (Phase 1 invariants).
///   2. Acquire the `in_flight` mutex. If occupied, publish
///      `WorkflowRunSkipped` and return `AlreadyRunning`.
///   3. Insert the slot, drop the mutex, persist the
///      `workflow_runs` row, publish `WorkflowRunStarted`.
///   4. Spawn `execute_inner` on a tokio task; the `InFlightSlot`
///      guard moves into the task so its `Drop` releases the slot
///      on any exit path.
///
/// Returns the new `RunId` immediately. Status updates flow through
/// the event bus + the `workflow_runs` table.
pub async fn dispatch_run(
    config: &Config,
    workflow_id: WorkflowId,
    trigger_source: TriggerSource,
) -> Result<RunId> {
    dispatch_run_with_payload(config, workflow_id, trigger_source, None).await
}

/// F2-9b — OQ-22: 256 KB cap on the trigger payload exposed via
/// `{{trigger.*}}`. Untrusted webhook / composio / channel bodies hit
/// this gate before they ever land in `NodeContext.trigger_payload`.
pub const TRIGGER_PAYLOAD_CAP_BYTES: usize = 256 * 1024;

/// Apply the trigger-payload cap. Values under the cap pass through
/// unchanged; oversize values are replaced with a structured marker
/// (`{ truncated: true, original_bytes: N, preview: "..." }`) so
/// downstream node bodies that template against `{{trigger.*}}` can
/// still see SOMETHING (and can branch on `{{trigger.truncated}}` if
/// they care).
///
/// Public so the F2-9b tests can drive the function directly without
/// going through `dispatch_run_with_payload`.
pub fn truncate_trigger_payload(payload: serde_json::Value) -> serde_json::Value {
    let serialised = match serde_json::to_string(&payload) {
        Ok(s) => s,
        Err(_) => return payload,
    };
    let bytes = serialised.len();
    if bytes <= TRIGGER_PAYLOAD_CAP_BYTES {
        return payload;
    }
    // Slice on a UTF-8 char boundary so the preview is a valid string.
    let preview_cap = TRIGGER_PAYLOAD_CAP_BYTES.min(serialised.len());
    let mut cut = preview_cap;
    while cut > 0 && !serialised.is_char_boundary(cut) {
        cut -= 1;
    }
    let preview = serialised[..cut].to_string();
    tracing::warn!(
        target: "workflows-run",
        "[workflows-run] trigger payload truncated: {bytes} bytes > cap {TRIGGER_PAYLOAD_CAP_BYTES}; preview kept"
    );
    serde_json::json!({
        "truncated": true,
        "original_bytes": bytes,
        "cap_bytes": TRIGGER_PAYLOAD_CAP_BYTES,
        "preview": preview,
    })
}

/// F2-9 extension: like [`dispatch_run`] but accepts an optional
/// trigger payload (webhook body / composio event / channel message).
/// The payload is plumbed into the run's `NodeContext.trigger_payload`
/// so per-kind bodies can template `{{trigger.*}}` against it.
///
/// In-memory only — survives the run's lifetime, NOT a core restart.
/// Persistent resume of triggered runs lives in F2-9b alongside the
/// boot reconcile work.
pub async fn dispatch_run_with_payload(
    config: &Config,
    workflow_id: WorkflowId,
    trigger_source: TriggerSource,
    trigger_payload: Option<serde_json::Value>,
) -> Result<RunId> {
    // F2-9b: OQ-22 256 KB trigger-payload cap. Applied at the single
    // entry point so every triggered run (webhook, composio_event,
    // channel_message) gets the same enforcement before any node
    // body templates against `{{trigger.*}}`.
    let trigger_payload = trigger_payload.map(truncate_trigger_payload);

    let workflow = match store::get_workflow(config, &workflow_id) {
        Ok(Some(w)) => w,
        Ok(None) => return Err(DispatchError::NotFound(workflow_id).into()),
        Err(err) => return Err(DispatchError::Store(format!("{err:#}")).into()),
    };

    validate_workflow_shape(&workflow)?;

    let run_id = Uuid::new_v4().to_string();

    // Single-flight gate. Hold the lock just long enough to claim
    // the slot — the row insert + event publish run outside the
    // critical section.
    {
        let mut in_flight = state().in_flight.lock();
        if let Some(existing) = in_flight.get(&workflow.id).cloned() {
            // Lock dropped before publish to keep the critical section
            // tight; the event bus is async-friendly.
            drop(in_flight);
            publish_global(DomainEvent::WorkflowRunSkipped {
                workflow_id: workflow.id.clone(),
                reason_json: serde_json::json!({ "kind": "already_running" }),
                attempted_trigger_source_json: serde_json::to_value(&trigger_source)
                    .unwrap_or(serde_json::Value::Null),
            });
            tracing::info!(
                target: "workflows-skip",
                "[workflows-skip] wf={} already running (existing run={})",
                workflow.id, existing
            );
            return Err(DispatchError::AlreadyRunning {
                workflow_id: workflow.id,
                run_id: existing,
            }
            .into());
        }
        in_flight.insert(workflow.id.clone(), run_id.clone());
    }

    let now = Utc::now();
    let run = Run {
        id: run_id.clone(),
        workflow_id: workflow.id.clone(),
        trigger_source: trigger_source.clone(),
        status: RunStatus::Running,
        started_at: now,
        completed_at: None,
        error: None,
        cancelled: false,
        failure_reason: None,
    };

    if let Err(err) = store::insert_run(config, &run) {
        // Release the slot we just claimed — the row never landed.
        state().in_flight.lock().remove(&workflow.id);
        return Err(DispatchError::Store(format!("{err:#}")).into());
    }

    publish_global(DomainEvent::WorkflowRunStarted {
        workflow_id: workflow.id.clone(),
        run_id: run.id.clone(),
    });
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] dispatch_run wf={} run={run_id} source={trigger_source:?}",
        workflow.id
    );

    let slot = InFlightSlot {
        workflow_id: workflow.id.clone(),
        run_id: run.id.clone(),
    };
    let config_owned = config.clone();
    let payload_owned = trigger_payload;
    tokio::spawn(async move {
        // Move the slot into the task so Drop fires on every exit
        // path — including a panic inside execute_inner.
        let _slot_guard = slot;
        execute_inner(config_owned, workflow, run, payload_owned).await;
    });
    Ok(run_id)
}

/// Request a soft cancel of an in-flight run (ADR-014).
///
/// The current node's LLM call is **not** aborted — aborting mid-stream
/// would corrupt the agent's memory writes. Instead the run's
/// `cancelled` flag flips to true; the executor's between-node loop
/// reads it via [`store::is_cancelled`] and exits as `Cancelled` once
/// the current node finishes.
///
/// Returns:
///   - `Ok(())` — flag flipped (idempotent — flipping it twice is
///     fine).
///   - `Err(NotFound)` — no `workflow_runs` row with this id.
///   - `Err(NotRunning { current_status })` — the run already reached
///     a terminal status before the cancel arrived.
pub async fn cancel_run(config: &Config, run_id: RunId) -> Result<(), CancelError> {
    let row =
        store::get_run(config, &run_id).map_err(|err| CancelError::Store(format!("{err:#}")))?;
    let (run, _steps) = match row {
        Some(pair) => pair,
        None => return Err(CancelError::NotFound(run_id)),
    };

    match run.status {
        RunStatus::Running | RunStatus::Pending => {}
        terminal => {
            return Err(CancelError::NotRunning {
                run_id,
                current_status: terminal,
            });
        }
    }

    store::set_cancelled_flag(config, &run_id)
        .map_err(|err| CancelError::Store(format!("{err:#}")))?;
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] cancel_run flag set wf={} run={run_id}",
        run.workflow_id
    );
    Ok(())
}

/// Sweep stale `Running` rows that lingered through a core crash.
///
/// Runs at boot **before** [`scheduler::reconcile_at_startup`] so a
/// just-restored cron tick can't dispatch into a workflow whose
/// previous run is still listed as `Running` (which would fail the
/// single-flight gate forever). For every row it touches, publishes
/// `WorkflowRunCompleted { status: Failed }` so subscribers (UI,
/// memory-of-run, etc.) observe the transition.
///
/// Returns the count of rows marked. Idempotent — a clean DB returns
/// `Ok(0)`.
pub async fn orphan_recovery_sweep(config: &Config) -> Result<usize> {
    let pairs = store::orphan_running_runs(config, Utc::now())?;
    let count = pairs.len();
    if count == 0 {
        tracing::debug!(
            target: "workflows-run",
            "[workflows-run] orphan_recovery_sweep no Running rows"
        );
        return Ok(0);
    }
    let status_json = serde_json::to_value(RunStatus::Failed).unwrap_or(serde_json::Value::Null);
    for (workflow_id, run_id) in &pairs {
        publish_global(DomainEvent::WorkflowRunCompleted {
            workflow_id: workflow_id.clone(),
            run_id: run_id.clone(),
            status_json: status_json.clone(),
        });
    }
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] orphan_recovery_sweep marked {count} runs as Failed{{CoreCrashed}}"
    );
    Ok(count)
}

/// F2-7b: boot-time recovery for runs that were sleeping inside a
/// `delay` node when the core died. Companion to
/// [`orphan_recovery_sweep`] (which skips these rows because their
/// `pending_resume_at` is non-NULL).
///
/// Strategy:
///   - Resume time already passed during downtime → flip the run to
///     `Failed { delay_interrupted_by_core_restart }` immediately +
///     publish `WorkflowRunCompleted`. We can't recover the in-process
///     execution state to actually resume the chain (full mid-
///     execution resume is a Phase 3 canvas-editor concern), but
///     failing loudly with a clear reason is honest.
///   - Resume time still in the future → spawn a tokio task that
///     sleeps until then, then performs the same flip. The run stays
///     visible as `Running` with a populated `pending_resume_at`
///     until the resume tick fires, which matches the UI semantic
///     "this workflow is waiting on a delay node".
///
/// Returns `(flipped_now, scheduled)` for ops visibility.
pub async fn resume_or_fail_delayed_runs(config: &Config) -> Result<(usize, usize)> {
    let runs = store::list_delayed_running_runs(config)?;
    if runs.is_empty() {
        tracing::debug!(
            target: "workflows-run",
            "[workflows-run] resume_or_fail_delayed_runs no delayed rows"
        );
        return Ok((0, 0));
    }

    let now = Utc::now();
    let (past, future): (Vec<_>, Vec<_>) = runs
        .into_iter()
        .partition(|(_, _, resume_at)| *resume_at <= now);

    let flipped_now = past.len();
    for (workflow_id, run_id, resume_at) in past {
        fail_delayed_run(
            config,
            &workflow_id,
            &run_id,
            resume_at,
            "resume_time_passed",
        );
    }

    let scheduled = future.len();
    for (workflow_id, run_id, resume_at) in future {
        let wait = (resume_at - now).to_std().unwrap_or_default();
        let cfg = config.clone();
        let wf = workflow_id.clone();
        let rid = run_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            fail_delayed_run(&cfg, &wf, &rid, resume_at, "resume_tick_fired_post_restart");
        });
    }

    tracing::info!(
        target: "workflows-run",
        "[workflows-run] resume_or_fail_delayed_runs: {flipped_now} flipped immediately, {scheduled} scheduled for later"
    );
    Ok((flipped_now, scheduled))
}

/// Helper for [`resume_or_fail_delayed_runs`]: mark a single delayed
/// run as Failed with a clear reason, clear its `pending_resume_at`
/// so a subsequent sweep doesn't re-pick it, and publish the
/// completion event so observers see the transition.
fn fail_delayed_run(
    config: &Config,
    workflow_id: &str,
    run_id: &str,
    resume_at: DateTime<Utc>,
    reason: &str,
) {
    let now = Utc::now();
    let _ = store::clear_pending_resume_at(config, &run_id.to_string());
    let error =
        format!("delay_interrupted_by_core_restart (reason={reason}, resume_at={resume_at})");
    // T-4: delay-interruption is a distinct failure mode that doesn't
    // fit any of the catalogued classifier rules, so it goes through
    // as Unknown — the raw_detail carries the informative string.
    let failure_reason = Some(crate::openhuman::workflows::types::FailureReason::Unknown {
        raw_detail: error.clone(),
    });
    if let Err(err) = store::mark_run_terminal(
        config,
        &run_id.to_string(),
        RunStatus::Failed,
        now,
        Some(error.clone()),
        failure_reason.as_ref(),
    ) {
        tracing::error!(
            target: "workflows-run",
            "[workflows-run] mark_run_terminal failed for delayed run={run_id}: {err:#}"
        );
        return;
    }
    publish_global(DomainEvent::WorkflowRunCompleted {
        workflow_id: workflow_id.to_string(),
        run_id: run_id.to_string(),
        status_json: serde_json::to_value(RunStatus::Failed).unwrap_or(serde_json::Value::Null),
    });
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] failed delayed run wf={workflow_id} run={run_id} reason={reason}"
    );
}

// ── execute_inner ──────────────────────────────────────────────────────

/// Dispatch-time workflow-shape guard. Pre-F2-2 this hard-coded the
/// Phase-1 single-node + AgentPrompt-only invariants. F2-2 swaps to
/// the Phase-2 set:
///   - At least one node.
///   - Every node's kind is in `allowed_node_kinds(CURRENT_PHASE)`.
///   - The edge set is a DAG (topological_sort below catches cycles).
///
/// The validator catches the same conditions at create time per F-11;
/// this is the runtime belts-and-suspenders for direct-RPC clients.
fn validate_workflow_shape(workflow: &Workflow) -> Result<(), DispatchError> {
    if workflow.nodes.is_empty() {
        return Err(DispatchError::PhaseConstraint(workflow.id.clone()));
    }
    let allowed = crate::openhuman::workflows::validator::allowed_node_kinds(CURRENT_PHASE);
    for node in &workflow.nodes {
        if !allowed.contains(&node.kind) {
            return Err(DispatchError::UnsupportedNodeKind(
                workflow.id.clone(),
                node.kind,
            ));
        }
    }
    Ok(())
}

/// Phase anchor for `validate_workflow_shape`. Bumped to `2` in F2-3
/// once `AgentPrompt` + `ToolCall` both have real executor bodies;
/// HttpRequest / ChannelMessage / Condition / Delay still route to
/// their `NodeDispatchError::NotImplementedYet` arm and the
/// dispatcher surfaces that as a clean `Failed` terminal status
/// (the F2-1 design contract — "reachable without behaviour").
/// F2-4..F2-7 replace each NotImplementedYet arm with a real body.
/// F4-7 bumped to `4` once `ForEach` shipped its executor body.
const CURRENT_PHASE: u32 = 4;

/// Order nodes by a topological sort over `edges`, returning the
/// execution order. Phase 2 chains are linear (single ancestor per
/// node), but the sort works for any DAG so Phase 3 (branching) can
/// reuse the same call path. Cycles return
/// [`DispatchError::PhaseConstraint`] so the run finalises as
/// `Failed` with a clear "workflow graph has a cycle" terminal error.
///
/// Nodes not referenced by any edge land at the end of the sort in
/// their declaration order — supports the common "one-node, no edges"
/// case that today's Phase-1 workflows ship with.
pub(crate) fn topological_sort(
    workflow_id: &WorkflowId,
    nodes: &[Node],
    edges: &[crate::openhuman::workflows::types::Edge],
) -> Result<Vec<crate::openhuman::workflows::types::NodeId>, DispatchError> {
    use crate::openhuman::workflows::types::NodeId;
    use std::collections::{HashMap, HashSet, VecDeque};

    // Build adjacency + in-degree maps.
    let mut in_degree: HashMap<NodeId, usize> = HashMap::new();
    let mut adjacency: HashMap<NodeId, Vec<NodeId>> = HashMap::new();
    for node in nodes {
        in_degree.insert(node.id.clone(), 0);
        adjacency.insert(node.id.clone(), Vec::new());
    }
    for edge in edges {
        // Edges referencing unknown ids are surfaced by the validator
        // at create time; here we tolerate them by skipping so a
        // malformed payload bypassing validate doesn't crash the
        // sort. The shape guard above already rejected empty nodes.
        if !in_degree.contains_key(&edge.from) || !in_degree.contains_key(&edge.to) {
            continue;
        }
        adjacency
            .get_mut(&edge.from)
            .expect("from-id is in nodes (guarded above)")
            .push(edge.to.clone());
        *in_degree
            .get_mut(&edge.to)
            .expect("to-id is in nodes (guarded above)") += 1;
    }

    // Kahn's algorithm. Stable order by node-declaration sequence:
    // tie-break by `nodes` index so the run-history view shows a
    // deterministic order across replays.
    let mut sorted: Vec<NodeId> = Vec::with_capacity(nodes.len());
    let mut visited: HashSet<NodeId> = HashSet::new();
    let mut queue: VecDeque<NodeId> = VecDeque::new();
    for node in nodes {
        if *in_degree
            .get(&node.id)
            .expect("in_degree initialised above")
            == 0
        {
            queue.push_back(node.id.clone());
        }
    }
    while let Some(id) = queue.pop_front() {
        if !visited.insert(id.clone()) {
            continue;
        }
        sorted.push(id.clone());
        let successors = adjacency.get(&id).cloned().unwrap_or_default();
        for next in successors {
            let entry = in_degree
                .get_mut(&next)
                .expect("successor is in nodes (guarded above)");
            *entry = entry.saturating_sub(1);
            if *entry == 0 {
                // Push in the order edges appear so a deterministic
                // declaration sequence drives the runtime order for
                // linear chains.
                queue.push_back(next);
            }
        }
    }

    if sorted.len() != nodes.len() {
        tracing::warn!(
            target: "workflows-run",
            workflow = %workflow_id,
            sorted = sorted.len(),
            total = nodes.len(),
            "[workflows-run] topological_sort detected a cycle"
        );
        return Err(DispatchError::PhaseConstraint(workflow_id.clone()));
    }
    Ok(sorted)
}

/// Transitive-closure reachability map: for each node id, the set of
/// node ids reachable from it via outbound edges (NOT including the
/// node itself). Built once at run start; consumed by
/// [`execute_inner`] when a `condition` node routes to a target so
/// the un-routed branch's nodes can be skipped without reordering
/// the topologically-sorted walk.
fn build_reachability(
    nodes: &[Node],
    edges: &[crate::openhuman::workflows::types::Edge],
) -> HashMap<String, std::collections::HashSet<String>> {
    // Adjacency list: from -> [to, to, ...]
    let mut adjacency: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes {
        adjacency.entry(node.id.as_str()).or_default();
    }
    for edge in edges {
        adjacency
            .entry(edge.from.as_str())
            .or_default()
            .push(edge.to.as_str());
    }
    // BFS from each node to compute the transitive set.
    let mut out: HashMap<String, std::collections::HashSet<String>> =
        HashMap::with_capacity(nodes.len());
    for node in nodes {
        let mut visited: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut queue: std::collections::VecDeque<&str> = std::collections::VecDeque::new();
        if let Some(neighbours) = adjacency.get(node.id.as_str()) {
            for n in neighbours {
                queue.push_back(*n);
            }
        }
        while let Some(id) = queue.pop_front() {
            if !visited.insert(id.to_string()) {
                continue;
            }
            if let Some(neighbours) = adjacency.get(id) {
                for n in neighbours {
                    if !visited.contains(*n) {
                        queue.push_back(*n);
                    }
                }
            }
        }
        out.insert(node.id.clone(), visited);
    }
    out
}

/// Drives the run to a terminal status. Spawned on a tokio task by
/// `dispatch_run`; doesn't return anything because every state
/// transition flows through the event bus + the `workflow_runs` table.
///
/// F2-2 rewrite — walks the topologically-sorted node list, builds a
/// [`crate::openhuman::workflows::templating::NodeContext`] per
/// iteration, calls `dispatch_node` per node, stuffs each output into
/// the context so downstream nodes can template against it.
///
/// Soft-cancel observation (ADR-014, FR-1.6.9): between nodes the
/// loop reads `workflow_runs.cancelled` via [`store::is_cancelled`].
/// Multi-node chains check the bit before each node; if cancel
/// landed mid-chain, the current node completes (cooperative cancel)
/// and the run terminates `Cancelled` before the next dispatch.
///
/// Error policy: F2-2 ships the `on_error = Halt` default — any
/// node failure terminates the run as `Failed` with the failing
/// node's error as `terminal_error`. F2-8 lands per-node `Continue`.
async fn execute_inner(
    config: Config,
    workflow: Workflow,
    run: Run,
    trigger_payload: Option<serde_json::Value>,
) {
    let timeout_secs = workflow.settings.timeout_secs.clamp(1, 3600);
    let workflow_id = workflow.id.clone();
    let run_id = run.id.clone();

    // Sort once; if the graph has a cycle, finalise as Failed.
    let order = match topological_sort(&workflow.id, &workflow.nodes, &workflow.edges) {
        Ok(order) => order,
        Err(err) => {
            finalize_run(
                &config,
                &workflow_id,
                &run_id,
                RunStatus::Failed,
                Some(format!("workflow graph rejected by dispatcher: {err}")),
            );
            return;
        }
    };
    // Index nodes by id for O(1) lookup during the walk.
    let nodes_by_id: HashMap<_, _> = workflow
        .nodes
        .iter()
        .map(|n| (n.id.clone(), n.clone()))
        .collect();

    // F4-7: collect every node id that is named as a `for_each` body.
    // These nodes are dispatched once per iteration by `execute_for_each`
    // — the outer walker MUST skip them, otherwise the body chain
    // would also run once at workflow scope without an iteration
    // record in context (and templating `{{record.*}}` would fail).
    let for_each_body_nodes: std::collections::HashSet<String> = workflow
        .nodes
        .iter()
        .filter_map(|n| match &n.config {
            NodeConfig::ForEach(cfg) => Some(cfg.body_nodes.iter().cloned()),
            _ => None,
        })
        .flatten()
        .collect();

    // F2-6: precompute the reachability closure so a `condition`
    // node's routing decision can restrict the remainder of the walk
    // to nodes downstream of the routed target. Without this, the
    // un-routed branch (e.g. `else_node` when we routed to
    // `then_node`) would still execute as the cursor advances
    // through the topological order. Phase 2 branches are typically
    // 1-node; Phase 3 canvas with joining branches uses the same
    // closure unchanged.
    let reachable = build_reachability(&workflow.nodes, &workflow.edges);
    // `branch_root` tracks the last condition's routing target. When
    // Some, the cursor only fires nodes that are `branch_root` itself
    // OR downstream of it.
    let mut branch_root: Option<crate::openhuman::workflows::types::NodeId> = None;

    // F2-2: NodeContext seeded with the trigger's payload. Today
    // Cron / Manual triggers carry no payload (`Value::Null`); F2-9
    // / F2-10 / F2-11 surface webhook / composio_event /
    // channel_message payloads via the same field.
    // F2-9: the run's trigger payload (webhook body / composio event /
    // channel message) lands on `NodeContext.trigger_payload` so node
    // bodies can template `{{trigger.*}}` against it. Cron / Manual
    // triggers pass `None`, which materialises as JSON `Null` —
    // matches the F2-2 default.
    let mut ctx = crate::openhuman::workflows::templating::NodeContext::new(
        trigger_payload.unwrap_or(serde_json::Value::Null),
    );

    // Pre-run cancel check — handles the case where cancel_run fired
    // between the dispatch and this task's first scheduling tick.
    if cancellation_observed(&config, &workflow_id, &run_id) {
        finalize_run(
            &config,
            &workflow_id,
            &run_id,
            RunStatus::Cancelled,
            Some("cancelled before first node".into()),
        );
        return;
    }

    let total_deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs as u64);
    let mut terminal_status = RunStatus::Succeeded;
    let mut terminal_error: Option<String> = None;

    // F2-6: walk by cursor (not for-loop) so a `condition` node can
    // jump the cursor to its `then_node_id` / `else_node_id` instead
    // of advancing one step. The cursor moves forward only — backward
    // jumps would form a cycle (rejected by `topological_sort`).
    let mut cursor: usize = 0;
    while cursor < order.len() {
        let node_id = &order[cursor];
        let node = nodes_by_id
            .get(node_id)
            .expect("topological_sort returns only node ids from `nodes`");

        // F4-7: skip nodes that exist only as `for_each` body members.
        // They get dispatched per-iteration by `execute_for_each` with
        // an `iteration_scope` set; running them at workflow scope
        // would mis-fire without a `{{record.*}}` source.
        if for_each_body_nodes.contains(node_id.as_str()) {
            tracing::debug!(
                target: "workflows-run",
                run = %run.id,
                skipped = %node_id,
                "[workflows-run] skipping node — runs only inside a for_each body"
            );
            cursor += 1;
            continue;
        }

        // F2-6: when a prior `condition` routed to a target, skip any
        // node not in that target's reachability closure. Lets the
        // unselected branch's nodes pass without execution while
        // still preserving the linear cursor walk.
        if let Some(root) = &branch_root {
            let on_branch = node_id == root
                || reachable
                    .get(root.as_str())
                    .map(|set| set.contains(node_id.as_str()))
                    .unwrap_or(false);
            if !on_branch {
                tracing::debug!(
                    target: "workflows-run",
                    run = %run.id,
                    skipped = %node_id,
                    branch_root = %root,
                    "[workflows-run] skipping node — not on routed branch"
                );
                cursor += 1;
                continue;
            }
        }

        // Between-nodes cancel check — cooperative cancellation per
        // FR-1.6.9. Triggers before dispatching the next node so the
        // user sees Cancelled rather than waiting for the next step.
        if cancellation_observed(&config, &workflow_id, &run_id) {
            terminal_status = RunStatus::Cancelled;
            terminal_error = Some("cancelled mid-run".into());
            break;
        }

        // Per-run wall-clock check against the remaining timeout
        // budget. Each node gets the REMAINING budget, not a fresh
        // one — total run time stays bounded by `settings.timeout_secs`.
        let remaining = total_deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            terminal_status = RunStatus::TimedOut;
            terminal_error = Some(format!("run exceeded {timeout_secs}s timeout"));
            break;
        }

        // F2-8: wrap dispatch in the per-node retry loop. The wrapper
        // honours `node.retry_policy` (None = single attempt) and
        // shares the per-run wall-clock budget.
        let outcome = tokio::time::timeout(
            remaining,
            dispatch_node_with_retry(&config, &run, node, &ctx),
        )
        .await;

        match outcome {
            Ok(Ok(body)) => {
                // F2-6 routing: condition nodes (and any future kind
                // with branching semantics) write a sentinel
                // `_workflow_route_to` key into their body.
                //   - missing key → advance cursor by 1 (default)
                //   - `null` value → halt the walk cleanly (Succeeded)
                //   - String value → find the target id in `order`
                //     forward of the current cursor and jump to it.
                //     Backward jumps OR an unknown id surface as a
                //     Failed terminal status.
                let routing = body
                    .as_object()
                    .and_then(|obj| obj.get(ROUTING_KEY))
                    .cloned();
                ctx.record_output(node_id.clone(), body);
                match routing {
                    None => {
                        cursor += 1;
                    }
                    Some(serde_json::Value::Null) => {
                        // Condition with `else_node_id = None`
                        // evaluated to false → halt-success.
                        break;
                    }
                    Some(serde_json::Value::String(target_id)) => {
                        match order
                            .iter()
                            .position(|id| id.as_str() == target_id.as_str())
                        {
                            Some(next_idx) if next_idx > cursor => {
                                cursor = next_idx;
                                // Restrict subsequent walk to nodes
                                // reachable from the routed target —
                                // skips the un-selected branch.
                                branch_root = Some(target_id);
                            }
                            Some(_) => {
                                // Backward jump — would form a cycle
                                // that topological_sort should have
                                // caught. Treat as a hard runtime
                                // failure for defence-in-depth.
                                terminal_status = RunStatus::Failed;
                                terminal_error = Some(format!(
                                    "node `{}` routing to `{}` would form a cycle (already executed)",
                                    node.id, target_id
                                ));
                                break;
                            }
                            None => {
                                terminal_status = RunStatus::Failed;
                                terminal_error = Some(format!(
                                    "node `{}` routes to `{}` which is not in the workflow",
                                    node.id, target_id
                                ));
                                break;
                            }
                        }
                    }
                    Some(other) => {
                        terminal_status = RunStatus::Failed;
                        terminal_error = Some(format!(
                            "node `{}` produced an invalid `{}` value: {}",
                            node.id, ROUTING_KEY, other
                        ));
                        break;
                    }
                }
            }
            Ok(Err(err)) => {
                // F2-8: honour the workflow-level `on_error` policy.
                //   - `Halt` (Phase-1 default): mark run Failed, stop.
                //   - `Continue`: log the per-step failure, advance
                //     to the next node, mark run Failed at the end
                //     (so the user sees the overall workflow as
                //     having had at least one failed step — chain
                //     completion AND failure are both honest).
                tracing::warn!(
                    target: "workflows-run",
                    run = %run.id,
                    node = %node.id,
                    "[workflows-run] node failed after retries: {err}"
                );
                match workflow.settings.on_error {
                    crate::openhuman::workflows::types::OnErrorPolicy::Halt => {
                        terminal_status = RunStatus::Failed;
                        terminal_error = Some(format!("node `{}` failed: {err}", node.id));
                        break;
                    }
                    crate::openhuman::workflows::types::OnErrorPolicy::Continue => {
                        // Per-step failure recorded already by the
                        // executor body. Mark the overall run Failed
                        // (preserve the first failure as the
                        // terminal_error) and continue walking.
                        if terminal_error.is_none() {
                            terminal_error = Some(format!("node `{}` failed: {err}", node.id));
                        }
                        terminal_status = RunStatus::Failed;
                        cursor += 1;
                        continue;
                    }
                }
            }
            Err(_elapsed) => {
                terminal_status = RunStatus::TimedOut;
                terminal_error = Some(format!("run exceeded {timeout_secs}s timeout"));
                break;
            }
        }
    }

    // Post-walk cancel check — same FR-1.6.9 cooperative pattern. A
    // cancel that landed during the final node's body upgrades a
    // successful return to Cancelled.
    if matches!(terminal_status, RunStatus::Succeeded)
        && cancellation_observed(&config, &workflow_id, &run_id)
    {
        terminal_status = RunStatus::Cancelled;
        terminal_error = Some("cancelled mid-run".into());
    }

    finalize_run(
        &config,
        &workflow_id,
        &run_id,
        terminal_status,
        terminal_error,
    );
    // InFlightSlot drop in the parent task releases the slot.
}

/// `is_cancelled` with safe fallback: a DB read error is logged and
/// treated as "not cancelled" so a transient SQLite hiccup doesn't
/// turn into a spurious `Cancelled` terminal status. The bit is
/// persistent — the next between-nodes check will catch it.
fn cancellation_observed(config: &Config, workflow_id: &str, run_id: &str) -> bool {
    match store::is_cancelled(config, &run_id.to_string()) {
        Ok(flag) => flag,
        Err(err) => {
            tracing::warn!(
                target: "workflows-run",
                "[workflows-run] is_cancelled lookup failed wf={workflow_id} run={run_id}: {err:#}; treating as not-cancelled"
            );
            false
        }
    }
}

/// Persist the terminal status, fire `WorkflowRunCompleted`, log the
/// transition. Shared between the pre-node-cancel path and the
/// post-node path so the event surface is identical.
fn finalize_run(
    config: &Config,
    workflow_id: &str,
    run_id: &str,
    terminal_status: RunStatus,
    terminal_error: Option<String>,
) {
    // T-4 (Phase 2.5 Trust UX): classify the failure reason at the
    // terminal-flip site so the per-run outcome card can render a
    // curated one-liner + fix-it action instead of forcing the user
    // to parse the raw error string. None for Succeeded / Cancelled.
    let failure_reason = if matches!(terminal_status, RunStatus::Failed) {
        terminal_error
            .as_deref()
            .map(|err| crate::openhuman::workflows::failure_classifier::classify_failure(err, 0))
    } else {
        None
    };
    if let Err(err) = store::mark_run_terminal(
        config,
        &run_id.to_string(),
        terminal_status,
        Utc::now(),
        terminal_error,
        failure_reason.as_ref(),
    ) {
        tracing::error!(
            target: "workflows-run",
            "[workflows-run] mark_run_terminal failed wf={workflow_id} run={run_id}: {err:#}"
        );
    }

    let status_json = serde_json::to_value(terminal_status).unwrap_or(serde_json::Value::Null);
    publish_global(DomainEvent::WorkflowRunCompleted {
        workflow_id: workflow_id.to_string(),
        run_id: run_id.to_string(),
        status_json,
    });
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] run terminal wf={workflow_id} run={run_id} status={terminal_status:?}"
    );
}

// ── dispatch_node ──────────────────────────────────────────────────────

/// Structured failure modes for [`dispatch_node`]. F2-1 introduces
/// `NotImplementedYet` so a workflow that includes a Phase 2 node kind
/// can save + start a run, but the run terminates with a clear error
/// rather than panicking. F2-3..F2-7 wire the real per-kind bodies and
/// remove the `NotImplementedYet` arms one at a time.
///
/// Surfaced via anyhow at the `execute_inner` call site so the existing
/// `RunStatus::Failed` + `terminal_error` plumbing carries the message
/// straight to the user without a new persistence shape.
#[derive(Debug, thiserror::Error)]
pub enum NodeDispatchError {
    /// The matching `NodeConfig::*` variant has no executor body yet.
    /// F2-3..F2-7 land the bodies; until then the run fails honestly.
    #[error(
        "node kind `{0:?}` is not yet implemented in this build — \
         lands in F2-3..F2-7 (Phase 2 execution depth)"
    )]
    NotImplementedYet(NodeKind),
}

/// F2-2 dispatcher: matches `node.config`, routes to the per-kind
/// executor, and returns the node's output body as a JSON `Value`
/// for the multi-node loop to stuff into [`NodeContext::outputs`].
///
/// Today only `AgentPrompt` has a real body. Every other Phase 2
/// variant returns `NodeDispatchError::NotImplementedYet` — the
/// upstream `execute_inner` translates that into a terminal
/// `RunStatus::Failed` so the user sees a clear "this node kind isn't
/// implemented yet" error in their run history.
///
/// Wiring contract for F2-3..F2-7: each ticket adds its arm by
/// replacing the matching `Err(NotImplementedYet(_))` with a real
/// `execute_<kind>(config, run, node, ctx).await` call.
///
/// `ctx` carries the trigger payload + every prior node's output body
/// for OQ-7 templating substitution. Per-kind bodies call
/// `templating::substitute` on their string fields before invoking
/// the underlying tool / HTTP / channel surface.
pub(crate) async fn dispatch_node(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    match &node.config {
        NodeConfig::AgentPrompt(_) => execute_agent_prompt(config, run, node, ctx).await,
        NodeConfig::ToolCall(_) => execute_tool_call(config, run, node, ctx).await,
        NodeConfig::HttpRequest(_) => execute_http_request(config, run, node, ctx).await,
        NodeConfig::ChannelMessage(_) => execute_channel_message(config, run, node, ctx).await,
        NodeConfig::Condition(_) => execute_condition(config, run, node, ctx).await,
        NodeConfig::Delay(_) => execute_delay(config, run, node, ctx).await,
        // F4-7: Box::pin breaks the recursive future-type the compiler
        // sees: dispatch_node → execute_for_each → dispatch_node_with_retry
        // → dispatch_node. Even though body nodes are validated to not
        // be for_each (no actual runtime recursion), the inferred
        // future type still chases itself without the indirection,
        // overflowing the stack as state-machine generation expands.
        NodeConfig::ForEach(_) => Box::pin(execute_for_each(config, run, node, ctx)).await,
    }
}

/// F2-8: wraps `dispatch_node` in a per-node retry loop respecting
/// `Node.retry_policy`. `None` policy → single attempt (no retry,
/// preserves F2-2 behaviour). On retry-eligible failure, sleeps the
/// exponential backoff and re-dispatches; on success at any attempt,
/// returns the body. On exhaustion, returns the last failure.
///
/// Publishes `DomainEvent::WorkflowRunStepRetried` per retry so
/// observability can chart retry rates.
///
/// Today every error is retry-eligible (matches `RetryableError::Any`).
/// Once `dispatch_node` surfaces structured error kinds, this loop
/// will respect `retry_policy.retry_on` filtering.
pub(crate) async fn dispatch_node_with_retry(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    use crate::openhuman::workflows::types::BackoffSpec;
    let (max_attempts, backoff): (u32, Option<BackoffSpec>) = match &node.retry_policy {
        Some(p) => (p.max_attempts.max(1), Some(p.backoff.clone())),
        None => (1, None),
    };
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 1..=max_attempts {
        // F4-7: Box::pin keeps the per-attempt state machine size
        // small. Without it, dispatch_node's state (which inlines
        // every per-kind body including the large execute_for_each)
        // is embedded directly into this retry loop's state machine
        // — compounded across retries + nested for_each iterations,
        // it overflows the 2 MiB default tokio task stack.
        match Box::pin(dispatch_node(config, run, node, ctx)).await {
            Ok(body) => {
                if attempt > 1 {
                    tracing::info!(
                        target: "workflows-run",
                        run = %run.id,
                        node = %node.id,
                        attempt,
                        "[workflows-run] node succeeded on retry"
                    );
                }
                return Ok(body);
            }
            Err(err) => {
                last_err = Some(err);
                if attempt < max_attempts {
                    // Compute backoff for the NEXT attempt's wait.
                    let wait_ms = if let Some(BackoffSpec::Exponential { initial_ms, max_ms }) =
                        backoff.as_ref()
                    {
                        // Current `attempt` (1-indexed) just failed.
                        // Wait BEFORE the next attempt (= attempt+1)
                        // is `initial_ms * 2^(attempt-1)`, capped at
                        // `max_ms`. So attempt 1 fails → wait
                        // `initial_ms * 1 = initial_ms`; attempt 2 →
                        // `initial_ms * 2`; attempt 3 → `initial_ms * 4`; …
                        let shift = (attempt as u32).saturating_sub(1);
                        let factor: u64 = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
                        let raw = (*initial_ms as u64).saturating_mul(factor);
                        raw.min(*max_ms as u64)
                    } else {
                        0
                    };
                    publish_global(DomainEvent::WorkflowRunStepRetried {
                        run_id: run.id.clone(),
                        node_id: node.id.clone(),
                        attempt: attempt + 1,
                        wait_ms,
                    });
                    tracing::warn!(
                        target: "workflows-run",
                        run = %run.id,
                        node = %node.id,
                        attempt,
                        next_attempt = attempt + 1,
                        wait_ms,
                        "[workflows-run] node failed; retrying after backoff"
                    );
                    if wait_ms > 0 {
                        if let Some(stub) = test_retry_backoff_override() {
                            stub(wait_ms).await;
                        } else {
                            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
                        }
                    }
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("retry loop exhausted without recording error")))
}

// ── Test-only backoff override (F2-8) ──────────────────────────────────

type RetryBackoffStubFn = Box<
    dyn Fn(u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;

static RETRY_BACKOFF_OVERRIDE: OnceLock<Mutex<Option<Arc<RetryBackoffStubFn>>>> = OnceLock::new();

/// Test-only hook to short-circuit the retry-loop backoff sleep so
/// tests don't wait real wall-clock time between attempts. Receives
/// the requested `wait_ms` so a test can assert on the schedule
/// while skipping the actual sleep.
#[cfg(any(test, feature = "e2e-test-support"))]
pub fn set_test_retry_backoff_override<F, Fut>(stub: F)
where
    F: Fn(u64) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let boxed: RetryBackoffStubFn = Box::new(move |ms| Box::pin(stub(ms)));
    let slot = RETRY_BACKOFF_OVERRIDE.get_or_init(|| Mutex::new(None));
    *slot.lock() = Some(Arc::new(boxed));
}

#[cfg(any(test, feature = "e2e-test-support"))]
pub fn clear_test_retry_backoff_override() {
    if let Some(slot) = RETRY_BACKOFF_OVERRIDE.get() {
        *slot.lock() = None;
    }
}

fn test_retry_backoff_override() -> Option<Arc<RetryBackoffStubFn>> {
    RETRY_BACKOFF_OVERRIDE
        .get()
        .and_then(|slot| slot.lock().clone())
}

// ── execute_tool_call (F2-3) ───────────────────────────────────────────

/// Build the workflow-runtime tool registry from a `&Config`.
///
/// Mirrors the pattern in `runtime_node::ops::build_runtime_tools` —
/// the same SecurityPolicy / Memory / NativeRuntime setup that the
/// JS runtime uses. Centralising the construction here (vs reusing
/// `runtime_node::ops`'s private helper) keeps the workflows domain
/// self-contained; a follow-up refactor can extract this to
/// `tools::ops::build_default_tool_set(config)` if a third caller
/// shows up.
///
/// Returns `Err(String)` on memory-construction failure so the F2-3
/// dispatcher can surface a clean `node_id` + `reason` error rather
/// than crashing the run.
fn build_tools_registry(
    config: &Config,
) -> Result<Vec<Box<dyn crate::openhuman::tools::Tool>>, String> {
    use crate::openhuman::agent::host_runtime::{NativeRuntime, RuntimeAdapter};
    use crate::openhuman::memory::Memory;
    use crate::openhuman::security::SecurityPolicy;
    let security = Arc::new(SecurityPolicy::from_config(
        &config.autonomy,
        &config.workspace_dir,
    ));
    let runtime: Arc<dyn RuntimeAdapter> = Arc::new(NativeRuntime::new());
    let local_embedding = config.workload_local_model("embeddings");
    let memory: Arc<dyn Memory> = Arc::from(
        crate::openhuman::memory_store::create_memory_with_local_ai(
            &config.memory,
            local_embedding.as_deref(),
            &config.embedding_routes,
            Some(&config.storage.provider.config),
            &config.workspace_dir,
        )
        .map_err(|e| format!("memory init failed: {e}"))?,
    );
    let audit = crate::openhuman::security::get_or_create_workspace_audit_logger(
        crate::openhuman::config::AuditConfig::default(),
        config.workspace_dir.clone(),
    )
    .map_err(|e| format!("audit logger init failed: {e}"))?;
    Ok(crate::openhuman::tools::ops::all_tools_with_runtime(
        Arc::new(config.clone()),
        &security,
        runtime,
        audit,
        memory,
        &config.browser,
        &config.http_request,
        &config.workspace_dir,
        &config.agents,
        config,
    ))
}

/// F2-3 node body for `NodeKind::ToolCall`.
///
/// Steps:
///   1. Look up `tool_name` in the workflow-runtime tool registry.
///      Unknown name → step Failed with a clear error (the dispatcher
///      surfaces it back to `execute_inner`'s terminal-error pipeline).
///   2. Run [`crate::openhuman::workflows::templating::substitute_json`]
///      on `arguments_template` against the live `NodeContext` so
///      upstream node outputs / trigger payload values land in the
///      resolved args.
///   3. Persist a `RunStep` row in `Running` state, publish
///      `WorkflowRunStepStarted`.
///   4. Call `tool.execute(args).await`. Map the `ToolResult` body to
///      the step's `output_json` AND to the `NodeOutput` body returned
///      to the dispatcher (so downstream `{{node.<id>.output...}}`
///      templates can index into it).
///   5. Persist terminal status + publish `WorkflowRunStepCompleted`.
///
/// Output body shape: `{ "text": String, "is_error": bool, "blocks":
/// [<ToolContent>...] }` — `text` is the LLM-facing rendering
/// (`output_for_llm`); `blocks` carries the raw content blocks so
/// downstream nodes can index structured fields when needed.
async fn execute_tool_call(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    let tool_cfg = match &node.config {
        NodeConfig::ToolCall(cfg) => cfg,
        other => anyhow::bail!(
            "execute_tool_call invoked on non-ToolCall node config: {:?}",
            std::mem::discriminant(other)
        ),
    };

    let step_id: RunStepId = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let step = RunStep {
        id: step_id.clone(),
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        output_json: None,
        error: None,
        delivery_receipts: Vec::new(),
    };
    if let Err(err) = store::insert_run_step(config, &step) {
        anyhow::bail!("insert_run_step failed: {err:#}");
    }
    publish_global(DomainEvent::WorkflowRunStepStarted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
    });

    let (terminal_status, output_json, error, body_value) =
        match dispatch_tool_call_inner(config, tool_cfg, ctx).await {
            Ok(body) => {
                let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
                (RunStatus::Succeeded, Some(payload), None, body)
            }
            Err(reason) => {
                tracing::warn!(
                    target: "workflows-run",
                    run = %run.id,
                    node = %node.id,
                    "[workflows-run] tool_call failed: {reason}"
                );
                (
                    RunStatus::Failed,
                    None,
                    Some(reason),
                    serde_json::Value::Null,
                )
            }
        };

    if let Err(err) = store::update_run_step_terminal(
        config,
        &step_id,
        terminal_status,
        Utc::now(),
        output_json,
        error.clone(),
        &[],
    ) {
        anyhow::bail!("update_run_step_terminal failed: {err:#}");
    }
    let status_json = serde_json::to_value(&terminal_status).unwrap_or(serde_json::Value::Null);
    publish_global(DomainEvent::WorkflowRunStepCompleted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status_json,
    });
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] tool_call step terminal run={} node={} tool={} status={terminal_status:?}",
        run.id,
        node.id,
        tool_cfg.tool_name,
    );

    if matches!(terminal_status, RunStatus::Failed) {
        if let Some(reason) = error {
            anyhow::bail!("tool_call step failed: {reason}");
        }
        anyhow::bail!("tool_call step failed");
    }
    Ok(body_value)
}

/// Inner runner — pulls the tool out of the registry, runs templating,
/// dispatches, maps to a JSON body. Returns `Err(reason)` for any
/// failure mode the dispatcher needs to surface as a Failed step.
async fn dispatch_tool_call_inner(
    config: &Config,
    tool_cfg: &crate::openhuman::workflows::types::ToolCallConfig,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value, String> {
    // Run templating BEFORE the stub-bypass branch so tests + the
    // real path both see the same resolved args. Otherwise the stub
    // path would expose the raw `{{...}}` tokens to the test code,
    // bypassing the OQ-7 substitution F2-3 is supposed to verify.
    let (resolved_args, unresolved) =
        crate::openhuman::workflows::templating::substitute_json(&tool_cfg.arguments_template, ctx);
    if !unresolved.is_empty() {
        tracing::warn!(
            target: "workflows-run",
            tool = %tool_cfg.tool_name,
            unresolved = unresolved.len(),
            "[workflows-run] tool_call args carry unresolved template refs; passing through"
        );
    }

    if let Some(stub) = test_tool_call_override() {
        let body = stub(&tool_cfg.tool_name, &resolved_args, ctx).await?;
        // Honor `is_error: true` even on the stub path so the F-16-
        // style "halt on tool error" contract is testable without
        // routing through the real registry.
        if body
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let text = body
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or("(no text)");
            return Err(format!(
                "tool `{}` returned is_error=true: {text}",
                tool_cfg.tool_name
            ));
        }
        return Ok(body);
    }

    let registry = build_tools_registry(config)?;
    let tool = registry
        .into_iter()
        .find(|t| t.name() == tool_cfg.tool_name)
        .ok_or_else(|| {
            format!(
                "tool not registered: `{}` — check the tool name + the runtime tool registry",
                tool_cfg.tool_name
            )
        })?;

    let started = std::time::Instant::now();
    let result = tool
        .execute(resolved_args)
        .await
        .map_err(|e| format!("tool `{}` execute failed: {e:#}", tool_cfg.tool_name))?;
    let elapsed_ms = started.elapsed().as_millis() as u64;

    publish_global(DomainEvent::ToolExecutionCompleted {
        tool_name: tool_cfg.tool_name.clone(),
        session_id: format!("workflow:{}", ctx_runtime_session_id_or_empty(ctx)),
        elapsed_ms,
        success: !result.is_error,
    });

    // Render the body for downstream templating. The `text` field is
    // the LLM-facing rendering; `blocks` carries the raw content
    // blocks so downstream nodes can index structured fields.
    let text = result
        .content
        .iter()
        .filter_map(|c| match c {
            crate::openhuman::tools::traits::ToolContent::Text { text } => Some(text.clone()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");
    let blocks =
        serde_json::to_value(&result.content).unwrap_or(serde_json::Value::Array(Vec::new()));
    let body = serde_json::json!({
        "text": text,
        "is_error": result.is_error,
        "blocks": blocks,
    });

    if result.is_error {
        return Err(format!(
            "tool `{}` returned is_error=true: {text}",
            tool_cfg.tool_name
        ));
    }
    Ok(body)
}

/// Best-effort accessor for a "session id" derived from the context.
/// Today `NodeContext` doesn't carry the run_id; the workflows-run
/// session id is `workflow:<run_id>` and the tool-execution event
/// uses it for filtering. F2-3 leaves this as an empty string —
/// telemetry attribution can be sharpened in a follow-up by passing
/// the run id through `NodeContext`.
fn ctx_runtime_session_id_or_empty(
    _ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> &'static str {
    ""
}

// ── Test-only tool-call override (F2-3) ────────────────────────────────

type ToolCallStubFn = Box<
    dyn Fn(
            &str,
            &serde_json::Value,
            &crate::openhuman::workflows::templating::NodeContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>,
        > + Send
        + Sync,
>;

static TOOL_CALL_OVERRIDE: OnceLock<Mutex<Option<Arc<ToolCallStubFn>>>> = OnceLock::new();

/// Test-only hook: replaces `dispatch_tool_call_inner`'s real registry
/// + execute path with a caller-supplied stub. The stub receives the
/// requested `tool_name`, the (pre-substituted) `arguments_template`,
/// and the live `NodeContext`, and returns the body JSON that gets
/// recorded as the step's `output_json` AND passed back to the
/// dispatcher for downstream templating.
///
/// Idempotent / last-writer-wins so tests can re-install with a
/// different behaviour between cases.
#[cfg(any(test, feature = "e2e-test-support"))]
pub fn set_test_tool_call_override<F, Fut>(stub: F)
where
    F: Fn(&str, &serde_json::Value, &crate::openhuman::workflows::templating::NodeContext) -> Fut
        + Send
        + Sync
        + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'static,
{
    let boxed: ToolCallStubFn = Box::new(move |name, args, ctx| Box::pin(stub(name, args, ctx)));
    let slot = TOOL_CALL_OVERRIDE.get_or_init(|| Mutex::new(None));
    *slot.lock() = Some(Arc::new(boxed));
}

#[cfg(any(test, feature = "e2e-test-support"))]
pub fn clear_test_tool_call_override() {
    if let Some(slot) = TOOL_CALL_OVERRIDE.get() {
        *slot.lock() = None;
    }
}

fn test_tool_call_override() -> Option<Arc<ToolCallStubFn>> {
    TOOL_CALL_OVERRIDE
        .get()
        .and_then(|slot| slot.lock().clone())
}

// ── execute_http_request (F2-4) ────────────────────────────────────────

/// F2-4 node body for `NodeKind::HttpRequest`.
///
/// Steps:
///   1. Resolve the `GenericHttpConnection` row + decrypted credential
///      via `connections::ops::resolve_generic_http_for_runtime`.
///   2. Run `substitute` on `path_template`, `body_template`, and
///      every header value against the live `NodeContext`. Object
///      keys are NOT templated (OQ-7 lock).
///   3. Build the request: `<base_url><resolved_path>`, attach
///      `default_headers` + templated headers + per-`AuthKind`
///      Authorization, with a 30s timeout (FR-1.6.5 default; the
///      run-level `settings.timeout_secs` already wraps this).
///   4. Send via reqwest. Read status + headers + body bytes.
///   5. Render the response body to UTF-8 (lossy on non-text).
///      Attempt JSON parse; `body_json: None` on parse failure.
///   6. Honour `response_capture`:
///      - `BodyAndStatus` → full `{status, headers, body_text, body_json?}`
///      - `StatusOnly` → `{status}`
///      - `JsonPath{path}` → `{status, captured: <path-walked value>}`
///   7. Map HTTP 4xx/5xx to a Failed step so the workflow-level Halt
///      policy fires; the response is still persisted so the run-history
///      view shows what came back.
///   8. NEVER log the decrypted credential or the resolved
///      Authorization header (NFR-2.4.4). Only field-key names + URL
///      base + status make it into `tracing::*` events.
async fn execute_http_request(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    let http_cfg = match &node.config {
        NodeConfig::HttpRequest(cfg) => cfg,
        other => anyhow::bail!(
            "execute_http_request invoked on non-HttpRequest node config: {:?}",
            std::mem::discriminant(other)
        ),
    };

    let step_id: RunStepId = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let step = RunStep {
        id: step_id.clone(),
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        output_json: None,
        error: None,
        delivery_receipts: Vec::new(),
    };
    if let Err(err) = store::insert_run_step(config, &step) {
        anyhow::bail!("insert_run_step failed: {err:#}");
    }
    publish_global(DomainEvent::WorkflowRunStepStarted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
    });

    let (terminal_status, output_json, error, body_value) =
        match dispatch_http_request_inner(config, http_cfg, ctx).await {
            Ok(body) => {
                let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
                (RunStatus::Succeeded, Some(payload), None, body)
            }
            Err((reason, partial_body)) => {
                tracing::warn!(
                    target: "workflows-run",
                    run = %run.id,
                    node = %node.id,
                    "[workflows-run] http_request failed: {reason}"
                );
                let payload = serde_json::to_string(&partial_body).unwrap_or_else(|_| "{}".into());
                (RunStatus::Failed, Some(payload), Some(reason), partial_body)
            }
        };

    if let Err(err) = store::update_run_step_terminal(
        config,
        &step_id,
        terminal_status,
        Utc::now(),
        output_json,
        error.clone(),
        &[],
    ) {
        anyhow::bail!("update_run_step_terminal failed: {err:#}");
    }
    let status_json = serde_json::to_value(&terminal_status).unwrap_or(serde_json::Value::Null);
    publish_global(DomainEvent::WorkflowRunStepCompleted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status_json,
    });

    if matches!(terminal_status, RunStatus::Failed) {
        if let Some(reason) = error {
            anyhow::bail!("http_request step failed: {reason}");
        }
        anyhow::bail!("http_request step failed");
    }
    Ok(body_value)
}

/// Inner runner — returns `Err((reason, partial_body))` so the caller
/// can persist the response body (status/headers/body_text) into the
/// step's `output_json` even when the HTTP call surfaced a 4xx/5xx.
/// This lets the run-history view show what came back from the
/// server alongside the terminal failure reason.
async fn dispatch_http_request_inner(
    config: &Config,
    http_cfg: &crate::openhuman::workflows::types::HttpRequestConfig,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value, (String, serde_json::Value)> {
    // Build a resolved view with templating applied. Both branches
    // (real send + test stub) consume the same shape so the stub
    // can assert on substituted values, not raw template tokens.
    use crate::openhuman::workflows::templating::substitute;
    let resolved_path = substitute(&http_cfg.path_template, ctx).resolved;
    let resolved_body = http_cfg
        .body_template
        .as_deref()
        .map(|t| substitute(t, ctx).resolved);
    let resolved_headers: std::collections::BTreeMap<String, String> = http_cfg
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), substitute(v, ctx).resolved))
        .collect();
    let resolved_cfg = crate::openhuman::workflows::types::HttpRequestConfig {
        connection_id: http_cfg.connection_id.clone(),
        method: http_cfg.method,
        path_template: resolved_path.clone(),
        headers: resolved_headers,
        body_template: resolved_body.clone(),
        response_capture: http_cfg.response_capture.clone(),
    };

    if let Some(stub) = test_http_request_override() {
        return stub(&resolved_cfg, ctx).await;
    }

    // Resolve the connection + decrypted credential.
    let (row, cleartext) =
        match crate::openhuman::connections::ops::resolve_generic_http_for_runtime(
            config,
            &http_cfg.connection_id,
        ) {
            Ok(Some(pair)) => pair,
            Ok(None) => {
                return Err((
                    format!(
                        "generic_http connection `{}` not found",
                        http_cfg.connection_id
                    ),
                    serde_json::Value::Null,
                ));
            }
            Err(e) => {
                return Err((
                    format!(
                        "generic_http resolve failed for `{}`: {e:#}",
                        http_cfg.connection_id
                    ),
                    serde_json::Value::Null,
                ));
            }
        };

    // Path / headers / body already substituted above; use the
    // `resolved_*` locals.
    let mut url = row.base_url.clone();
    if !resolved_path.is_empty() {
        if !resolved_path.starts_with('/') && !url.ends_with('/') {
            url.push('/');
        }
        url.push_str(&resolved_path);
    }

    // Query-param auth — must land in the URL before reqwest gets it.
    if let crate::openhuman::connections::types::AuthKind::QueryParam { name } = &row.auth_kind {
        if let Some(value) = cleartext.as_deref() {
            let sep = if url.contains('?') { '&' } else { '?' };
            url = format!(
                "{url}{sep}{n}={v}",
                n = urlencoding::encode(name),
                v = urlencoding::encode(value)
            );
        }
    }

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return Err((
                format!("reqwest build failed: {e:#}"),
                serde_json::Value::Null,
            ));
        }
    };

    let method = match http_cfg.method {
        crate::openhuman::workflows::types::HttpMethod::Get => reqwest::Method::GET,
        crate::openhuman::workflows::types::HttpMethod::Post => reqwest::Method::POST,
        crate::openhuman::workflows::types::HttpMethod::Put => reqwest::Method::PUT,
        crate::openhuman::workflows::types::HttpMethod::Delete => reqwest::Method::DELETE,
    };
    let mut req = client.request(method.clone(), &url);

    // default_headers first (overridable by node-config headers).
    for (k, v) in &row.default_headers {
        req = req.header(k, v);
    }
    // Per-node headers (already templated into `resolved_cfg.headers`
    // above, but we have them at-hand via the resolved_headers local).
    for (k, v) in &resolved_cfg.headers {
        req = req.header(k, v);
    }

    // Auth header — NEVER log the resolved value.
    match (&row.auth_kind, cleartext.as_deref()) {
        (crate::openhuman::connections::types::AuthKind::Bearer, Some(token)) => {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        (crate::openhuman::connections::types::AuthKind::Basic, Some(creds)) => {
            req = req.header("Authorization", format!("Basic {creds}"));
        }
        (crate::openhuman::connections::types::AuthKind::ApiKeyHeader { name }, Some(value)) => {
            req = req.header(name, value);
        }
        _ => {}
    }

    // Body — already templated above.
    if let Some(body) = resolved_body.as_ref() {
        req = req.body(body.clone());
    }

    // Cleartext credential is no longer needed once headers/URL are
    // built. Drop it explicitly so the rest of the function never has
    // it in scope.
    drop(cleartext);

    tracing::debug!(
        target: "workflows-run",
        url = %url,
        method = %method,
        "[workflows-run] http_request dispatching"
    );

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return Err((
                format!("http_request send failed: {e}"),
                serde_json::Value::Null,
            ));
        }
    };

    let status_u16 = resp.status().as_u16();
    let header_map: serde_json::Map<String, serde_json::Value> = resp
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                serde_json::Value::String(v.to_str().unwrap_or("").to_string()),
            )
        })
        .collect();
    let body_bytes = resp.bytes().await.unwrap_or_default();
    let body_text = String::from_utf8_lossy(&body_bytes).to_string();
    let body_json: Option<serde_json::Value> = serde_json::from_str(&body_text).ok();

    let full_response = serde_json::json!({
        "status": status_u16,
        "headers": serde_json::Value::Object(header_map),
        "body_text": body_text,
        "body_json": body_json,
    });

    // Shape the body_value per `response_capture`.
    let captured = match &http_cfg.response_capture {
        crate::openhuman::workflows::types::ResponseCapture::BodyAndStatus => full_response.clone(),
        crate::openhuman::workflows::types::ResponseCapture::StatusOnly => {
            serde_json::json!({ "status": status_u16 })
        }
        crate::openhuman::workflows::types::ResponseCapture::JsonPath { path } => {
            let walked = walk_dotted_path(
                full_response
                    .get("body_json")
                    .unwrap_or(&serde_json::Value::Null),
                path,
            );
            serde_json::json!({
                "status": status_u16,
                "captured": walked.unwrap_or(serde_json::Value::Null),
            })
        }
    };

    if !(200..400).contains(&status_u16) {
        return Err((
            format!("http_request returned status {status_u16}"),
            captured,
        ));
    }
    Ok(captured)
}

/// Walk `value` through a dotted path. Mirrors `templating::walk_path`
/// but is local to the executor since the templating module's helper
/// is private. Object key access only — array indexing via `[N]` is
/// deferred (same OQ-7 scope as the templating walker).
fn walk_dotted_path(value: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut cursor = value;
    for segment in path.split('.') {
        if segment.is_empty() {
            return None;
        }
        cursor = cursor.get(segment)?;
    }
    Some(cursor.clone())
}

// ── Test-only http_request override (F2-4) ─────────────────────────────

type HttpRequestStubFn = Box<
    dyn Fn(
            &crate::openhuman::workflows::types::HttpRequestConfig,
            &crate::openhuman::workflows::templating::NodeContext,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<
                        Output = Result<serde_json::Value, (String, serde_json::Value)>,
                    > + Send,
            >,
        > + Send
        + Sync,
>;

static HTTP_REQUEST_OVERRIDE: OnceLock<Mutex<Option<Arc<HttpRequestStubFn>>>> = OnceLock::new();

/// Test-only hook: replaces `dispatch_http_request_inner`'s real
/// connection-resolve + send path with a caller-supplied stub.
/// Same shape as `set_test_tool_call_override`.
#[cfg(any(test, feature = "e2e-test-support"))]
pub fn set_test_http_request_override<F, Fut>(stub: F)
where
    F: Fn(
            &crate::openhuman::workflows::types::HttpRequestConfig,
            &crate::openhuman::workflows::templating::NodeContext,
        ) -> Fut
        + Send
        + Sync
        + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value, (String, serde_json::Value)>>
        + Send
        + 'static,
{
    let boxed: HttpRequestStubFn = Box::new(move |cfg, ctx| Box::pin(stub(cfg, ctx)));
    let slot = HTTP_REQUEST_OVERRIDE.get_or_init(|| Mutex::new(None));
    *slot.lock() = Some(Arc::new(boxed));
}

#[cfg(any(test, feature = "e2e-test-support"))]
pub fn clear_test_http_request_override() {
    if let Some(slot) = HTTP_REQUEST_OVERRIDE.get() {
        *slot.lock() = None;
    }
}

fn test_http_request_override() -> Option<Arc<HttpRequestStubFn>> {
    HTTP_REQUEST_OVERRIDE
        .get()
        .and_then(|slot| slot.lock().clone())
}

// ── execute_channel_message (F2-5) ─────────────────────────────────────

/// F2-5 node body for `NodeKind::ChannelMessage`.
///
/// Sends a templated message to a connected chat channel. Reuses the
/// existing `channels::controllers::ops::channel_send_message` which
/// is the unified send path that backs every provider (Slack /
/// Discord / Telegram / WhatsApp / iMessage). The
/// `ConnectionMessageConfig` shape (F2-1) carries the channel slug as
/// `connection_id`; F2-5 maps that 1:1 to the `channel` arg of
/// `channel_send_message`.
///
/// Steps:
///   1. Substitute `body_template` against the live `NodeContext`.
///   2. Persist a `RunStep` row, publish `WorkflowRunStepStarted`.
///   3. Call `channel_send_message(config, connection_id,
///      json!({"text": resolved_body}))`.
///   4. Map result → step `output_json` + dispatcher body. On Err,
///      mark step Failed and bubble the reason up.
///   5. Output body shape: `{ "sent": bool, "channel": String,
///      "text": String, "response": Value }`. Downstream nodes can
///      template `{{node.<this>.output.sent}}` or
///      `{{node.<this>.output.response.message_id}}`.
async fn execute_channel_message(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    let chan_cfg = match &node.config {
        NodeConfig::ChannelMessage(cfg) => cfg,
        other => anyhow::bail!(
            "execute_channel_message invoked on non-ChannelMessage node config: {:?}",
            std::mem::discriminant(other)
        ),
    };

    let step_id: RunStepId = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let step = RunStep {
        id: step_id.clone(),
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        output_json: None,
        error: None,
        delivery_receipts: Vec::new(),
    };
    if let Err(err) = store::insert_run_step(config, &step) {
        anyhow::bail!("insert_run_step failed: {err:#}");
    }
    publish_global(DomainEvent::WorkflowRunStepStarted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
    });

    let (terminal_status, output_json, error, body_value) =
        match dispatch_channel_message_inner(config, chan_cfg, ctx).await {
            Ok(body) => {
                let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
                (RunStatus::Succeeded, Some(payload), None, body)
            }
            Err(reason) => {
                tracing::warn!(
                    target: "workflows-run",
                    run = %run.id,
                    node = %node.id,
                    "[workflows-run] channel_message failed: {reason}"
                );
                (
                    RunStatus::Failed,
                    None,
                    Some(reason),
                    serde_json::Value::Null,
                )
            }
        };

    if let Err(err) = store::update_run_step_terminal(
        config,
        &step_id,
        terminal_status,
        Utc::now(),
        output_json,
        error.clone(),
        &[],
    ) {
        anyhow::bail!("update_run_step_terminal failed: {err:#}");
    }
    let status_json = serde_json::to_value(&terminal_status).unwrap_or(serde_json::Value::Null);
    publish_global(DomainEvent::WorkflowRunStepCompleted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status_json,
    });

    if matches!(terminal_status, RunStatus::Failed) {
        if let Some(reason) = error {
            anyhow::bail!("channel_message step failed: {reason}");
        }
        anyhow::bail!("channel_message step failed");
    }
    Ok(body_value)
}

/// Inner runner — templates the body, dispatches via the unified
/// channel-send path, maps the response to a JSON body value. Returns
/// `Err(reason)` for any failure mode.
async fn dispatch_channel_message_inner(
    config: &Config,
    chan_cfg: &crate::openhuman::workflows::types::ChannelMessageConfig,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value, String> {
    use crate::openhuman::workflows::templating::substitute;
    let resolved_body = substitute(&chan_cfg.body_template, ctx).resolved;

    if let Some(stub) = test_channel_message_override() {
        return stub(chan_cfg, &resolved_body, ctx).await;
    }

    let message_payload = serde_json::json!({ "text": resolved_body });
    let response = crate::openhuman::channels::controllers::channel_send_message(
        config,
        &chan_cfg.connection_id,
        message_payload,
    )
    .await
    .map_err(|e| format!("channel_send_message failed: {e}"))?;

    Ok(serde_json::json!({
        "sent": true,
        "channel": chan_cfg.connection_id,
        "channel_id": chan_cfg.channel_id,
        "text": resolved_body,
        "response": response.value,
    }))
}

// ── Test-only channel_message override (F2-5) ──────────────────────────

type ChannelMessageStubFn = Box<
    dyn Fn(
            &crate::openhuman::workflows::types::ChannelMessageConfig,
            &str, // resolved body
            &crate::openhuman::workflows::templating::NodeContext,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<serde_json::Value, String>> + Send>,
        > + Send
        + Sync,
>;

static CHANNEL_MESSAGE_OVERRIDE: OnceLock<Mutex<Option<Arc<ChannelMessageStubFn>>>> =
    OnceLock::new();

/// Test-only hook: replaces the real `channel_send_message` dispatch
/// with a caller-supplied stub. Same shape as F2-3's
/// `set_test_tool_call_override` and F2-4's
/// `set_test_http_request_override`. The stub receives the
/// (pre-substituted) `ChannelMessageConfig`, the resolved body
/// string, and the live `NodeContext`. Returns the body JSON that
/// gets recorded as the step's `output_json` AND passed back to the
/// dispatcher for downstream templating.
#[cfg(any(test, feature = "e2e-test-support"))]
pub fn set_test_channel_message_override<F, Fut>(stub: F)
where
    F: Fn(
            &crate::openhuman::workflows::types::ChannelMessageConfig,
            &str,
            &crate::openhuman::workflows::templating::NodeContext,
        ) -> Fut
        + Send
        + Sync
        + 'static,
    Fut: std::future::Future<Output = Result<serde_json::Value, String>> + Send + 'static,
{
    let boxed: ChannelMessageStubFn =
        Box::new(move |cfg, body, ctx| Box::pin(stub(cfg, body, ctx)));
    let slot = CHANNEL_MESSAGE_OVERRIDE.get_or_init(|| Mutex::new(None));
    *slot.lock() = Some(Arc::new(boxed));
}

#[cfg(any(test, feature = "e2e-test-support"))]
pub fn clear_test_channel_message_override() {
    if let Some(slot) = CHANNEL_MESSAGE_OVERRIDE.get() {
        *slot.lock() = None;
    }
}

fn test_channel_message_override() -> Option<Arc<ChannelMessageStubFn>> {
    CHANNEL_MESSAGE_OVERRIDE
        .get()
        .and_then(|slot| slot.lock().clone())
}

// ── execute_condition (F2-6) ───────────────────────────────────────────

/// Reserved key in a node's output `Value` that signals the
/// executor's run loop to route to a specific downstream node id
/// instead of advancing topologically. Set by `execute_condition`;
/// read in `execute_inner` (the multi-node loop).
///
/// Other future node kinds with branching semantics (await_human_approval,
/// fan_out termination) can write this same key to take advantage of
/// the routing path without inventing a parallel surface.
pub(crate) const ROUTING_KEY: &str = "_workflow_route_to";

/// F2-6 node body for `NodeKind::Condition`.
///
/// Steps:
///   1. Substitute `left` + `right` against the live `NodeContext`.
///   2. Compile / evaluate the predicate per `op`. Regex compile
///      failures surface as a clean Failed step (no panic).
///   3. Emit a body Value with `matched: bool`, `left`, `right`,
///      `op`, AND the reserved `ROUTING_KEY` carrying the target
///      node id (then / else). When the predicate is false and
///      `else_node_id` is `None`, the body still records
///      `matched: false` but ROUTING_KEY is absent — `execute_inner`
///      reads that as "halt the walk; remaining nodes skipped".
async fn execute_condition(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    let cond_cfg = match &node.config {
        NodeConfig::Condition(cfg) => cfg,
        other => anyhow::bail!(
            "execute_condition invoked on non-Condition node config: {:?}",
            std::mem::discriminant(other)
        ),
    };

    let step_id: RunStepId = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let step = RunStep {
        id: step_id.clone(),
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        output_json: None,
        error: None,
        delivery_receipts: Vec::new(),
    };
    if let Err(err) = store::insert_run_step(config, &step) {
        anyhow::bail!("insert_run_step failed: {err:#}");
    }
    publish_global(DomainEvent::WorkflowRunStepStarted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
    });

    let (terminal_status, output_json, error, body_value) = match evaluate_condition(cond_cfg, ctx)
    {
        Ok(body) => {
            let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
            (RunStatus::Succeeded, Some(payload), None, body)
        }
        Err(reason) => (
            RunStatus::Failed,
            None,
            Some(reason),
            serde_json::Value::Null,
        ),
    };

    if let Err(err) = store::update_run_step_terminal(
        config,
        &step_id,
        terminal_status,
        Utc::now(),
        output_json,
        error.clone(),
        &[],
    ) {
        anyhow::bail!("update_run_step_terminal failed: {err:#}");
    }
    let status_json = serde_json::to_value(&terminal_status).unwrap_or(serde_json::Value::Null);
    publish_global(DomainEvent::WorkflowRunStepCompleted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status_json,
    });

    if matches!(terminal_status, RunStatus::Failed) {
        if let Some(reason) = error {
            anyhow::bail!("condition step failed: {reason}");
        }
        anyhow::bail!("condition step failed");
    }
    Ok(body_value)
}

/// Pure-Rust predicate evaluator. Returns the body Value (with the
/// optional `ROUTING_KEY`) or `Err(reason)` for shape errors that
/// should surface as a Failed step (today: bad regex).
fn evaluate_condition(
    cfg: &crate::openhuman::workflows::types::ConditionConfig,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value, String> {
    use crate::openhuman::workflows::templating::substitute;
    use crate::openhuman::workflows::types::CompareOp;
    let left = substitute(&cfg.left, ctx).resolved;
    let right = substitute(&cfg.right, ctx).resolved;

    let matched = match cfg.op {
        CompareOp::Eq => left == right,
        CompareOp::NotEq => left != right,
        CompareOp::Contains => left.contains(&right),
        CompareOp::Matches => {
            let re = regex::Regex::new(&right)
                .map_err(|e| format!("invalid regex in condition.right: {e}"))?;
            re.is_match(&left)
        }
    };

    // F2-6 routing convention (see `execute_inner`):
    //   - `String(target_id)` → jump to that node
    //   - `Null`              → halt the walk cleanly (Succeeded)
    //   - key missing         → default advance (non-routing nodes)
    // ALWAYS insert the key for condition nodes so the executor never
    // accidentally falls through to default-advance after a
    // halt-on-false branch.
    let route_value = if matched {
        serde_json::Value::String(cfg.then_node_id.clone())
    } else {
        match &cfg.else_node_id {
            Some(target) => serde_json::Value::String(target.clone()),
            None => serde_json::Value::Null,
        }
    };

    let mut body = serde_json::Map::new();
    body.insert("matched".into(), serde_json::Value::Bool(matched));
    body.insert("left".into(), serde_json::Value::String(left));
    body.insert("right".into(), serde_json::Value::String(right));
    body.insert(
        "op".into(),
        serde_json::to_value(&cfg.op).unwrap_or(serde_json::Value::Null),
    );
    body.insert(ROUTING_KEY.into(), route_value);
    Ok(serde_json::Value::Object(body))
}

// ── execute_delay (F2-7) ───────────────────────────────────────────────

/// F2-7 node body for `NodeKind::Delay`.
///
/// Pauses the run for `cfg.seconds` via `tokio::time::sleep`. The
/// validator already caps `seconds` at 86 400 (24 h) so a runaway
/// workflow can't sleep forever; the per-run `settings.timeout_secs`
/// wraps the whole walk via `execute_inner`'s shared deadline,
/// further bounding the total wait.
///
/// Persistent resume across core restarts is the bigger half of the
/// F2-7 spec and is **deferred to F2-7b**. Today an in-process
/// restart loses any delayed run; cron-triggered workflows are
/// re-scheduled on the next tick, manual runs need to be re-fired.
/// Acceptable for the Phase-2 MVP since the 24h cap bounds the
/// exposure window.
///
/// Steps:
///   1. Persist a `RunStep` row (Running), publish `WorkflowRunStepStarted`.
///   2. Sleep for `cfg.seconds` (test stub may bypass).
///   3. Re-check soft-cancel after the sleep — if set, mark step
///      Cancelled and bail.
///   4. Mark step Succeeded with `{ slept_seconds: u64 }` body.
async fn execute_delay(
    config: &Config,
    run: &Run,
    node: &Node,
    _ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    let delay_cfg = match &node.config {
        NodeConfig::Delay(cfg) => cfg,
        other => anyhow::bail!(
            "execute_delay invoked on non-Delay node config: {:?}",
            std::mem::discriminant(other)
        ),
    };

    let step_id: RunStepId = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let step = RunStep {
        id: step_id.clone(),
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        output_json: None,
        error: None,
        delivery_receipts: Vec::new(),
    };
    if let Err(err) = store::insert_run_step(config, &step) {
        anyhow::bail!("insert_run_step failed: {err:#}");
    }
    publish_global(DomainEvent::WorkflowRunStepStarted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
    });

    // Sleep — test override may short-circuit so tests don't wait.
    let seconds = delay_cfg.seconds;
    // F2-7b: persist the resume cursor BEFORE sleeping so a core
    // crash leaves enough state for `resume_or_fail_delayed_runs` to
    // recover the row correctly on next boot. Best-effort write — a
    // store error here downgrades to in-process-only delay (same
    // pre-F2-7b behaviour) rather than aborting the step.
    let resume_at = Utc::now() + chrono::Duration::seconds(seconds as i64);
    if let Err(err) = store::set_pending_resume_at(config, &run.id, resume_at) {
        tracing::warn!(
            target: "workflows-run",
            "[workflows-run] set_pending_resume_at failed run={}: {err:#}; delay state won't survive a crash",
            run.id
        );
    }
    if let Some(stub) = test_delay_override() {
        stub(seconds).await;
    } else {
        tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
    }
    // F2-7b: clear the cursor whether the sleep completed cleanly or
    // we're about to bail on cancel; the row is no longer "sleeping".
    if let Err(err) = store::clear_pending_resume_at(config, &run.id) {
        tracing::warn!(
            target: "workflows-run",
            "[workflows-run] clear_pending_resume_at failed run={}: {err:#}",
            run.id
        );
    }

    // Post-sleep cancel check — if a cancel landed during the
    // pause, upgrade the step to Cancelled rather than Succeeded
    // (matches F-9's between-nodes cooperative cancel pattern;
    // execute_inner will read this on the next iteration too).
    if cancellation_observed(config, &run.workflow_id, &run.id) {
        if let Err(err) = store::update_run_step_terminal(
            config,
            &step_id,
            RunStatus::Cancelled,
            Utc::now(),
            None,
            Some("cancelled during delay".into()),
            &[],
        ) {
            anyhow::bail!("update_run_step_terminal failed: {err:#}");
        }
        publish_global(DomainEvent::WorkflowRunStepCompleted {
            run_id: run.id.clone(),
            node_id: node.id.clone(),
            status_json: serde_json::to_value(RunStatus::Cancelled)
                .unwrap_or(serde_json::Value::Null),
        });
        anyhow::bail!("delay step cancelled");
    }

    let body = serde_json::json!({ "slept_seconds": seconds });
    let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    if let Err(err) = store::update_run_step_terminal(
        config,
        &step_id,
        RunStatus::Succeeded,
        Utc::now(),
        Some(payload),
        None,
        &[],
    ) {
        anyhow::bail!("update_run_step_terminal failed: {err:#}");
    }
    publish_global(DomainEvent::WorkflowRunStepCompleted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status_json: serde_json::to_value(RunStatus::Succeeded).unwrap_or(serde_json::Value::Null),
    });
    Ok(body)
}

// ── Test-only delay override (F2-7) ────────────────────────────────────

type DelayStubFn = Box<
    dyn Fn(u64) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Send + Sync,
>;

static DELAY_OVERRIDE: OnceLock<Mutex<Option<Arc<DelayStubFn>>>> = OnceLock::new();

/// Test-only hook: replaces the real `tokio::time::sleep` with a
/// caller-supplied async fn (typically `|_| async {}` for instant
/// resolution). Same shape as F2-3..F2-5 overrides — last-writer-
/// wins; serialize tests on `DELAY_TEST_LOCK` in `executor_tests`.
#[cfg(any(test, feature = "e2e-test-support"))]
pub fn set_test_delay_override<F, Fut>(stub: F)
where
    F: Fn(u64) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let boxed: DelayStubFn = Box::new(move |seconds| Box::pin(stub(seconds)));
    let slot = DELAY_OVERRIDE.get_or_init(|| Mutex::new(None));
    *slot.lock() = Some(Arc::new(boxed));
}

#[cfg(any(test, feature = "e2e-test-support"))]
pub fn clear_test_delay_override() {
    if let Some(slot) = DELAY_OVERRIDE.get() {
        *slot.lock() = None;
    }
}

fn test_delay_override() -> Option<Arc<DelayStubFn>> {
    DELAY_OVERRIDE.get().and_then(|slot| slot.lock().clone())
}

// ── execute_for_each (F4-7) ────────────────────────────────────────────

/// F4-7: iterate an [`crate::openhuman::campaigns::entity_store::EntityStore`]
/// query and run the configured `body_nodes` once per matching record.
///
/// Structure mirrors the other `execute_*` bodies — one parent step
/// row for the for_each node itself, plus each body-node iteration
/// dispatches through `dispatch_node_with_retry` which inserts its
/// own step rows. A 3-record × 1-body-node for_each produces 1 parent
/// row + 3 inner rows.
///
/// Per-record on-error policy: when an iteration's body chain fails,
/// the for_each respects the parent node's `on_error` (`Halt`
/// short-circuits the remaining iterations; `Continue` proceeds).
/// Aggregate output: `{ records_processed, records_succeeded, records_failed }`.
async fn execute_for_each(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    let cfg = match &node.config {
        NodeConfig::ForEach(cfg) => cfg.clone(),
        other => anyhow::bail!(
            "execute_for_each invoked on non-ForEach node config: {:?}",
            std::mem::discriminant(other)
        ),
    };

    let step_id: RunStepId = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let step = RunStep {
        id: step_id.clone(),
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        output_json: None,
        error: None,
        delivery_receipts: Vec::new(),
    };
    if let Err(err) = store::insert_run_step(config, &step) {
        anyhow::bail!("insert_run_step failed: {err:#}");
    }
    publish_global(DomainEvent::WorkflowRunStepStarted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
    });

    // Resolve entity_binding — explicit > inherit from parent campaign.
    // Always load the parent campaign (if any) so we have its throttle
    // for the F4-8 gate; entity_binding inheritance reads from the
    // same campaign load.
    let workflow = store::get_workflow(config, &run.workflow_id)
        .map_err(|e| anyhow::anyhow!("for_each: load workflow failed: {e:#}"))?
        .ok_or_else(|| {
            anyhow::anyhow!("for_each: workflow {} vanished mid-run", run.workflow_id)
        })?;
    let parent_campaign = if let Some(cid) = workflow.campaign_id.as_ref() {
        crate::openhuman::campaigns::store::get_campaign(config, cid)
            .map_err(|e| anyhow::anyhow!("for_each: load campaign {cid}: {e:#}"))?
    } else {
        None
    };
    let binding = match cfg.entity_binding.clone() {
        Some(b) => b,
        None => match parent_campaign.as_ref() {
            Some(campaign) => campaign.entity_binding.clone(),
            None => {
                let err = "for_each: entity_binding is None and workflow has no parent campaign \
                           — set entity_binding explicitly or assign the workflow to a campaign";
                finalize_for_each_step(
                    config,
                    &run.id,
                    &step_id,
                    &node.id,
                    false,
                    0,
                    0,
                    0,
                    Some(err),
                );
                anyhow::bail!(err);
            }
        },
    };

    // Open the EntityStore via the (test-overridable) factory.
    let store_for_records = match open_entity_store_for_executor(config, &binding) {
        Ok(s) => s,
        Err(err) => {
            let msg = format!("for_each: open entity_store: {err:#}");
            finalize_for_each_step(
                config,
                &run.id,
                &step_id,
                &node.id,
                false,
                0,
                0,
                0,
                Some(&msg),
            );
            anyhow::bail!(msg);
        }
    };

    let records = match store_for_records.list(cfg.query.clone()).await {
        Ok(r) => r,
        Err(err) => {
            let msg = format!("for_each: list records: {err:#}");
            finalize_for_each_step(
                config,
                &run.id,
                &step_id,
                &node.id,
                false,
                0,
                0,
                0,
                Some(&msg),
            );
            anyhow::bail!(msg);
        }
    };

    let cap = cfg.max_per_run as usize;
    let records: Vec<_> = records.into_iter().take(cap).collect();
    let total = records.len();

    // Index body nodes by id for per-iteration lookup. body_nodes are
    // ids referring to nodes already present in the workflow.
    let nodes_by_id: HashMap<_, _> = workflow
        .nodes
        .iter()
        .map(|n| (n.id.clone(), n.clone()))
        .collect();
    for body_id in &cfg.body_nodes {
        if !nodes_by_id.contains_key(body_id) {
            let msg = format!("for_each: body_node id `{body_id}` not in workflow");
            finalize_for_each_step(
                config,
                &run.id,
                &step_id,
                &node.id,
                false,
                total,
                0,
                0,
                Some(&msg),
            );
            anyhow::bail!(msg);
        }
    }

    let halt_on_error = matches!(
        workflow.settings.on_error,
        crate::openhuman::workflows::types::OnErrorPolicy::Halt
    );

    // F4-8: campaign throttle. `parent_campaign.throttle` is the
    // shared budget across every sub-workflow + every iteration.
    // Without a parent campaign (standalone for_each) there's no
    // gate to consult — pass `None` and the gate is a no-op.
    let campaign_id_for_throttle = parent_campaign.as_ref().map(|c| c.id.clone());
    let throttle = parent_campaign.as_ref().and_then(|c| c.throttle.clone());

    let mut succeeded: usize = 0;
    let mut failed: usize = 0;
    let mut skipped_for_throttle: usize = 0;
    'outer: for (idx, record) in records.into_iter().enumerate() {
        // F4-8: reserve a slot before doing any work for this
        // iteration. `Ok(0)` means the window is exhausted. For short
        // windows (PerMinute/PerHour) we sleep + retry; for PerDay
        // we surface the skip via DomainEvent::WorkflowRunSkipped
        // and break out of the loop (the next-window-at lands as
        // pending_resume_at so a future iteration resumes).
        if let (Some(cid), Some(t)) = (campaign_id_for_throttle.as_ref(), throttle.as_ref()) {
            loop {
                let granted = crate::openhuman::campaigns::throttle::ThrottleGate::reserve(
                    config,
                    cid,
                    Some(t),
                    1,
                )
                .unwrap_or(0);
                if granted >= 1 {
                    break;
                }
                // Exhausted — decide based on window length.
                let snap = crate::openhuman::campaigns::throttle::ThrottleGate::current(
                    config,
                    cid,
                    Some(t),
                )
                .ok()
                .flatten();
                match t.window {
                    crate::openhuman::campaigns::types::ThrottleWindow::PerMinute
                    | crate::openhuman::campaigns::types::ThrottleWindow::PerHour => {
                        // Short windows: sleep until the next bucket.
                        let dur = snap
                            .map(|s| {
                                (s.next_window_at - Utc::now()).max(chrono::Duration::seconds(1))
                            })
                            .unwrap_or(chrono::Duration::seconds(1));
                        let secs = dur.num_seconds().clamp(1, 3_600) as u64;
                        if let Some(stub) = test_delay_override() {
                            stub(secs).await;
                        } else {
                            tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
                        }
                        // Retry the reserve.
                    }
                    crate::openhuman::campaigns::types::ThrottleWindow::PerDay => {
                        // Surface as skipped, leave remaining records
                        // for the next-window resume path.
                        if let Some(s) = snap {
                            publish_global(DomainEvent::WorkflowRunSkipped {
                                workflow_id: run.workflow_id.clone(),
                                reason_json: serde_json::to_value(
                                    crate::openhuman::workflows::types::SkippedReason::ThrottleExhausted {
                                        campaign_id: cid.clone(),
                                        window_start: s.window_start.to_rfc3339(),
                                        next_window_at: s.next_window_at.to_rfc3339(),
                                    },
                                )
                                .unwrap_or(serde_json::Value::Null),
                                attempted_trigger_source_json: serde_json::to_value(
                                    &run.trigger_source,
                                )
                                .unwrap_or(serde_json::Value::Null),
                            });
                        }
                        skipped_for_throttle = total.saturating_sub(idx);
                        break 'outer;
                    }
                }
            }
        }

        let mut iter_ctx = ctx.clone();
        iter_ctx.set_iteration_scope(record.clone());

        let mut iteration_failed = false;
        for body_id in &cfg.body_nodes {
            let body_node = nodes_by_id
                .get(body_id)
                .expect("body_nodes pre-validated above");
            // Box::pin breaks the type-level recursion the compiler
            // sees (dispatch_node → execute_for_each →
            // dispatch_node_with_retry → dispatch_node). At runtime
            // we never recurse — body nodes are validated to not be
            // for_each — but the inferred future type still has to
            // close.
            match Box::pin(dispatch_node_with_retry(config, run, body_node, &iter_ctx)).await {
                Ok(body) => {
                    iter_ctx.record_output(body_id.clone(), body);
                }
                Err(err) => {
                    tracing::warn!(
                        target: "workflows-run",
                        run = %run.id,
                        record = %record.id.native,
                        body_node = %body_id,
                        "[for_each] iteration body node failed: {err:#}"
                    );
                    iteration_failed = true;
                    break;
                }
            }
        }

        if iteration_failed {
            failed += 1;
            // F4-8: return the reserved slot on iteration failure.
            // We can't distinguish "failed before externally-visible
            // action" from "failed after"; the simpler contract is
            // "any failure refunds the slot." Net effect: the budget
            // gates *successful* outbound actions, not attempts.
            if let (Some(cid), Some(t)) = (campaign_id_for_throttle.as_ref(), throttle.as_ref()) {
                let _ = crate::openhuman::campaigns::throttle::ThrottleGate::release(
                    config,
                    cid,
                    Some(t),
                    1,
                );
            }
            if halt_on_error {
                break 'outer;
            }
        } else {
            succeeded += 1;
        }

        if let Some(secs) = cfg.per_iteration_delay_secs {
            if secs > 0 {
                if let Some(stub) = test_delay_override() {
                    stub(secs as u64).await;
                } else {
                    tokio::time::sleep(std::time::Duration::from_secs(secs as u64)).await;
                }
            }
        }
    }

    let processed = succeeded + failed;
    let final_ok = failed == 0 || !halt_on_error && processed == total;
    let body = serde_json::json!({
        "records_processed": processed,
        "records_succeeded": succeeded,
        "records_failed": failed,
        "records_skipped_for_throttle": skipped_for_throttle,
    });
    let error_msg = if final_ok {
        None
    } else {
        Some(format!(
            "for_each: {failed}/{processed} iterations failed (on_error=halt)"
        ))
    };
    finalize_for_each_step(
        config,
        &run.id,
        &step_id,
        &node.id,
        final_ok,
        total,
        succeeded,
        failed,
        error_msg.as_deref(),
    );
    if !final_ok {
        anyhow::bail!("for_each terminated early: {failed} failures (on_error=halt)");
    }
    Ok(body)
}

/// Finalise the parent for_each step row + publish the matching event.
/// Centralised so the multiple error paths in `execute_for_each` don't
/// each have to reproduce the persistence + bus boilerplate.
fn finalize_for_each_step(
    config: &Config,
    run_id: &str,
    step_id: &RunStepId,
    node_id: &str,
    ok: bool,
    total: usize,
    succeeded: usize,
    failed: usize,
    error: Option<&str>,
) {
    let status = if ok {
        RunStatus::Succeeded
    } else {
        RunStatus::Failed
    };
    let body = serde_json::json!({
        "records_total": total,
        "records_succeeded": succeeded,
        "records_failed": failed,
    });
    let payload = serde_json::to_string(&body).unwrap_or_else(|_| "{}".into());
    if let Err(err) = store::update_run_step_terminal(
        config,
        step_id,
        status,
        Utc::now(),
        Some(payload),
        error.map(|e| e.to_string()),
        &[],
    ) {
        tracing::warn!(
            target: "workflows-run",
            "[for_each] update_run_step_terminal failed run={run_id} step={step_id}: {err:#}"
        );
    }
    publish_global(DomainEvent::WorkflowRunStepCompleted {
        run_id: run_id.to_string(),
        node_id: node_id.to_string(),
        status_json: serde_json::to_value(status).unwrap_or(serde_json::Value::Null),
    });
}

// ── EntityStore factory (test-overridable) ────────────────────────

type EntityStoreFactoryFn = Box<
    dyn Fn(
            &Config,
            &crate::openhuman::campaigns::types::EntityRef,
        ) -> Result<Box<dyn crate::openhuman::campaigns::entity_store::EntityStore>>
        + Send
        + Sync,
>;

static ENTITY_STORE_FACTORY_OVERRIDE: OnceLock<Mutex<Option<Arc<EntityStoreFactoryFn>>>> =
    OnceLock::new();

/// Open an `EntityStore` for the executor. Production path delegates
/// to `entity_store::open_entity_store`; tests inject a stub
/// returning a `MockEntityStore` via [`set_test_entity_store_factory`].
fn open_entity_store_for_executor(
    config: &Config,
    binding: &crate::openhuman::campaigns::types::EntityRef,
) -> Result<Box<dyn crate::openhuman::campaigns::entity_store::EntityStore>> {
    if let Some(slot) = ENTITY_STORE_FACTORY_OVERRIDE.get() {
        if let Some(f) = slot.lock().clone() {
            return (f)(config, binding);
        }
    }
    crate::openhuman::campaigns::entity_store::open_entity_store(config, binding)
}

#[cfg(any(test, feature = "e2e-test-support"))]
pub fn set_test_entity_store_factory<F>(stub: F)
where
    F: Fn(
            &Config,
            &crate::openhuman::campaigns::types::EntityRef,
        ) -> Result<Box<dyn crate::openhuman::campaigns::entity_store::EntityStore>>
        + Send
        + Sync
        + 'static,
{
    let boxed: EntityStoreFactoryFn = Box::new(stub);
    let slot = ENTITY_STORE_FACTORY_OVERRIDE.get_or_init(|| Mutex::new(None));
    *slot.lock() = Some(Arc::new(boxed));
}

#[cfg(any(test, feature = "e2e-test-support"))]
pub fn clear_test_entity_store_factory() {
    if let Some(slot) = ENTITY_STORE_FACTORY_OVERRIDE.get() {
        *slot.lock() = None;
    }
}

// ── execute_agent_prompt ───────────────────────────────────────────────

/// Phase 1 node body: persist a step row, fire `WorkflowRunStepStarted`,
/// run the agent (PLACEHOLDER per the module-doc), truncate + persist
/// output, fire `WorkflowRunStepCompleted`.
async fn execute_agent_prompt(
    config: &Config,
    run: &Run,
    node: &Node,
    ctx: &crate::openhuman::workflows::templating::NodeContext,
) -> Result<serde_json::Value> {
    let agent_prompt_config = match &node.config {
        NodeConfig::AgentPrompt(cfg) => cfg,
        other => {
            // Unreachable under `dispatch_node`'s routing, but guard
            // anyway so a future caller that bypasses the dispatcher
            // can't silently mis-dispatch.
            anyhow::bail!(
                "execute_agent_prompt invoked on non-AgentPrompt node config: {:?}",
                std::mem::discriminant(other)
            );
        }
    };
    // F2-2: OQ-7 templating substitution on the prompt string. The
    // substituted prompt + the original config are passed downstream;
    // `run_agent_prompt` reads its prompt from the config by reference,
    // so we build a thin override that swaps the resolved prompt in.
    let templated =
        crate::openhuman::workflows::templating::substitute(&agent_prompt_config.prompt, ctx);
    if !templated.unresolved.is_empty() {
        tracing::warn!(
            target: "workflows-run",
            run = %run.id,
            node = %node.id,
            unresolved = templated.unresolved.len(),
            "[workflows-run] agent_prompt has unresolved template refs; \
             passing through literally — the agent will see the `{{...}}` tokens"
        );
    }
    // The agent-config we hand to `run_agent_prompt` carries the
    // substituted prompt; `allowed_connections` / `iteration_cap` /
    // `model_tier` pass through unchanged. Cheap clone.
    let resolved_prompt_config = AgentPromptConfig {
        prompt: templated.resolved,
        allowed_connections: agent_prompt_config.allowed_connections.clone(),
        iteration_cap: agent_prompt_config.iteration_cap,
        model_tier: agent_prompt_config.model_tier.clone(),
    };
    let step_id: RunStepId = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let step = RunStep {
        id: step_id.clone(),
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status: RunStatus::Running,
        started_at,
        completed_at: None,
        output_json: None,
        error: None,
        delivery_receipts: Vec::new(),
    };
    if let Err(err) = store::insert_run_step(config, &step) {
        anyhow::bail!("insert_run_step failed: {err:#}");
    }

    publish_global(DomainEvent::WorkflowRunStepStarted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
    });
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] step started run={} node={} prompt_chars={}",
        run.id,
        node.id,
        resolved_prompt_config.prompt.chars().count()
    );

    let agent_def = build_node_agent_definition(
        &resolved_prompt_config.allowed_connections,
        resolved_prompt_config.iteration_cap,
        resolved_prompt_config.model_tier.clone(),
    );

    let (
        terminal_status,
        output_json,
        error,
        agent_narrative,
        observed_tool_calls,
        observed_receipts,
    ) = match run_agent_prompt(
        config,
        &run.workflow_id,
        &run.id,
        &resolved_prompt_config,
        &agent_def,
    )
    .await
    {
        Ok(output) => {
            let narrative = output.text.clone();
            let trace = output.tool_calls.clone();
            let receipts = output.delivery_receipts.clone();
            let truncated = store::truncate_output_to_64kib(output.text);
            let payload = serde_json::to_string(&serde_json::json!({ "text": truncated }))
                .unwrap_or_else(|_| "{}".into());
            let successful_tool_calls = output.tool_calls.iter().filter(|c| c.success).count();
            if output.tool_failure_count > 0 {
                // F-16 D: tool denials / executed-with-error count
                // overrides the "agent returned text" success
                // signal. The text payload is still persisted (so
                // the run-history view can show what the agent
                // tried to say), but the status reads honest.
                let summary = format!(
                    "agent run completed with {} tool call(s) reported as failed by the harness \
                     (denied by allowlist or returned is_error=true). \
                     Check workflows-run + agent_loop logs for details.",
                    output.tool_failure_count
                );
                (
                    RunStatus::Failed,
                    Some(payload),
                    Some(summary),
                    narrative,
                    trace,
                    receipts,
                )
            } else if successful_tool_calls == 0
                && !resolved_prompt_config.allowed_connections.is_empty()
            {
                // F-21 deferred follow-up — structural counterpart to
                // F-16. F-16 catches "tool failed → run Failed". This
                // catches "agent never called a tool → run still
                // showed Succeeded". Live repro: 2026-06-05 / 06-07
                // morning email digest where the model (reasoning-v1
                // after the Anthropic+OpenAI router shift) narrated
                // its next-action intent ("Let me try X") in plain
                // text without emitting any tool_use blocks, hit
                // iteration cap, and the run was marked Succeeded
                // because no tool failure was observed.
                //
                // Rule: workflow granted action connection(s) but the
                // agent made zero successful tool calls ⇒ Failed. The
                // conservative `allowed_connections.is_empty()` guard
                // means pure read-only / summarisation workflows that
                // legitimately produce only text are NOT touched.
                let summary = format!(
                    "agent narrated next-action intent in {} chars of text without emitting \
                     any tool_use blocks. Workflow granted {} action connection(s) but the \
                     agent made zero successful tool calls — the run accomplished nothing \
                     measurable. Common causes: model gets stuck in chain-of-thought (try \
                     pinning workflow_node to a stronger model via [teams.workflow_node] \
                     agent_model in config.toml), or the prompt asks for an action the \
                     allowlist doesn't grant.",
                    narrative.chars().count(),
                    resolved_prompt_config.allowed_connections.len()
                );
                tracing::warn!(
                    target: "workflows-run",
                    run_id = %run.id,
                    narrative_chars = narrative.chars().count(),
                    allowed_connections = resolved_prompt_config.allowed_connections.len(),
                    "[workflows-run] no successful tool calls — flipping run to Failed (F-21)"
                );
                (
                    RunStatus::Failed,
                    Some(payload),
                    Some(summary),
                    narrative,
                    trace,
                    receipts,
                )
            } else {
                (
                    RunStatus::Succeeded,
                    Some(payload),
                    None,
                    narrative,
                    trace,
                    receipts,
                )
            }
        }
        Err(err) => (
            RunStatus::Failed,
            None,
            Some(format!("{err:#}")),
            String::new(),
            Vec::new(),
            Vec::new(),
        ),
    };

    // F-17 deliverable C: persist the run as a structured chunk in the
    // Memory Tree, ground-truth-first. Best-effort — a failed store
    // does NOT roll back the run's terminal status.
    persist_run_memory(
        run,
        terminal_status,
        &agent_narrative,
        &observed_tool_calls,
        &resolved_prompt_config.allowed_connections,
        error.as_deref(),
    )
    .await;

    if let Err(err) = store::update_run_step_terminal(
        config,
        &step_id,
        terminal_status,
        Utc::now(),
        output_json.clone(),
        error.clone(),
        &observed_receipts,
    ) {
        anyhow::bail!("update_run_step_terminal failed: {err:#}");
    }

    let status_json = serde_json::to_value(&terminal_status).unwrap_or(serde_json::Value::Null);
    publish_global(DomainEvent::WorkflowRunStepCompleted {
        run_id: run.id.clone(),
        node_id: node.id.clone(),
        status_json,
    });
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] step terminal run={} node={} status={terminal_status:?}",
        run.id,
        node.id
    );

    if matches!(terminal_status, RunStatus::Failed) {
        if let Some(reason) = error {
            anyhow::bail!("agent_prompt step failed: {reason}");
        }
        anyhow::bail!("agent_prompt step failed");
    }
    // F2-2: bubble the body to the multi-node dispatcher so downstream
    // nodes can template `{{node.<this>.output...}}`. Parse the JSON
    // back to Value so the templating walker can index it; on parse
    // failure fall back to `Null` (defensive — the body we just built
    // came from `serde_json::to_string` and should always re-parse).
    let body_value = output_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .unwrap_or(serde_json::Value::Null);
    Ok(body_value)
}

/// One tool-call observation captured by the F-16 event-bus tap during
/// a workflow run. Carries the fields the
/// [`crate::core::event_bus::DomainEvent::ToolExecutionCompleted`]
/// event surfaces today; F-17 deliverable C extends the subscriber to
/// record these as a `Vec` (not just a counter) so the post-run memory
/// builder can render per-call detail.
///
/// `redacted_args` + `inner_status` are reserved for a future event
/// extension; the harness doesn't surface them on the current event
/// surface, so the post-run builder writes the
/// [`crate::openhuman::workflows::memory::ToolCallTrace`] with empty
/// args + `None` inner_status. F4-6 is expected to extend the event.
#[derive(Debug, Clone)]
pub struct ToolCallObservation {
    pub tool_name: String,
    pub success: bool,
    pub elapsed_ms: u64,
}

/// Node-execution output. Carries the agent's final text response
/// AND the per-tool-call observations from the F-16 event-bus tap.
///
/// F-16 D: the caller in [`execute_agent_prompt_node`] uses
/// `tool_failure_count > 0` to override the step status to `Failed`
/// even when the agent itself returned text — so a workflow that
/// "completed" by emitting an apology after every tool call got
/// denied no longer lies in run history.
///
/// F-17 deliverable C: `tool_calls` carries the full ordered trace
/// so the post-run memory builder can record per-call detail in
/// `ActualOutcome.tool_calls`.
#[derive(Debug, Clone)]
pub struct NodeOutput {
    pub text: String,
    /// Number of `ToolExecutionCompleted { success: false }` events
    /// observed during this run, scoped to `event_context =
    /// "workflow:<run_id>"`. Counts BOTH:
    ///   - tool calls blocked by `visible_tool_names` (turn.rs:1035)
    ///   - tool calls that executed and returned `is_error = true`
    /// Both are surfaced via the same `DomainEvent` with
    /// `success: false`, so the counter doesn't need to distinguish.
    pub tool_failure_count: u32,
    /// Chronological list of every tool call observed during the run.
    /// Used to build `ActualOutcome.tool_calls` in the post-run
    /// memory chunk.
    pub tool_calls: Vec<ToolCallObservation>,
    /// T-1 (Phase 2.5 Trust UX): every `DeliveryReceiptObserved`
    /// event captured during the run, in dispatch order. Populated by
    /// the same subscriber that records `tool_calls`; passed through
    /// to `update_run_step_terminal` so the receipts land on the
    /// persisted row and feed T-2's outcome card. Empty for read-only
    /// runs and runs where no write tool succeeded.
    pub delivery_receipts: Vec<crate::openhuman::workflows::types::DeliveryReceipt>,
}

/// Test-only override for [`run_agent_prompt`]. Production code
/// always takes the [`Agent::from_config`] path; tests inject a
/// deterministic stub via [`set_test_agent_prompt_override`] so the
/// persistence pipeline assertions don't depend on a live LLM
/// provider being configured in the test workspace.
///
/// The signature mirrors the production body: takes the prompt +
/// agent definition, returns the text the executor persists into
/// `workflow_run_steps.output_json`.
#[cfg(test)]
type TestAgentOverride =
    std::sync::Arc<dyn Fn(&str, &NodeAgentDefinition) -> Result<String> + Send + Sync>;

#[cfg(test)]
static TEST_AGENT_OVERRIDE: std::sync::OnceLock<std::sync::Mutex<Option<TestAgentOverride>>> =
    std::sync::OnceLock::new();

#[cfg(test)]
pub fn set_test_agent_prompt_override(
    f: impl Fn(&str, &NodeAgentDefinition) -> Result<String> + Send + Sync + 'static,
) {
    let slot = TEST_AGENT_OVERRIDE.get_or_init(|| std::sync::Mutex::new(None));
    *slot.lock().expect("override slot poisoned") = Some(std::sync::Arc::new(f));
}

#[cfg(test)]
pub fn clear_test_agent_prompt_override() {
    if let Some(slot) = TEST_AGENT_OVERRIDE.get() {
        *slot.lock().expect("override slot poisoned") = None;
    }
}

#[cfg(test)]
fn current_test_override() -> Option<TestAgentOverride> {
    TEST_AGENT_OVERRIDE
        .get()
        .and_then(|m| m.lock().ok().and_then(|g| g.clone()))
}

/// Execute the `agent_prompt` node's body via the constrained
/// `workflow_node` sub-agent (F-16).
///
/// Behavior:
///
///   1. [`Agent::from_config_for_agent_with_tool_override`] builds
///      the harness against the `workflow_node` archetype, REPLACING
///      its empty base allowlist with `def.allowed_tools` (built per
///      ADR-016 from baseline + connection-resolved + read-only
///      workflow tools). The orchestrator persona, profile, memory,
///      and delegation tree are stripped — the LLM sees only the
///      `workflow_node` system prompt + the user-authored
///      `agent_prompt.prompt` + the explicit tool surface.
///   2. `agent.set_event_context("workflow:<run_id>", "workflow")`
///      tags downstream telemetry so subscribers (and F-16 D's
///      step-status event-bus tap) can filter on this run.
///   3. `agent.run_single(prompt)` returns the agent's final text
///      response, which becomes the persisted
///      `workflow_run_steps.output_json.text` after truncation.
///
/// F-16 motivated this rewrite: the previous body called
/// `Agent::from_config(config)` (the **orchestrator** by default),
/// IGNORED `def.allowed_tools`, and let the LLM pick
/// `delegate_to_integrations_agent` instead of the
/// `composio_execute` tool the workflow had granted — which then
/// died silently inside integrations_agent due to a Composio-action-
/// name issue, while step status still recorded `Succeeded`. Live
/// repro on 2026-05-21 22:13; full diagnosis in F-16.md.
///
/// Tests inject a deterministic stub via
/// [`set_test_agent_prompt_override`]; the override is only
/// honoured under `#[cfg(test)]`. In production the override slot
/// never exists, and the constrained agent path above is what runs.
async fn run_agent_prompt(
    config: &Config,
    workflow_id: &WorkflowId,
    run_id: &RunId,
    agent_prompt_config: &AgentPromptConfig,
    def: &NodeAgentDefinition,
) -> Result<NodeOutput> {
    // F-16 D: subscribe to ToolExecutionCompleted events scoped to
    // this run BEFORE the agent runs. The handle drops at the end
    // of this function, cancelling the subscriber. Any
    // `success: false` event with a matching `session_id`
    // increments the shared counter, which the caller checks to
    // decide whether to override the step status to Failed.
    //
    // Subscriber install happens BEFORE the test-override check so
    // tests that exercise the honest-status path can publish
    // synthetic ToolExecutionCompleted events from inside the stub
    // and observe them increment the counter (otherwise the test
    // override would short-circuit past the entire F-16 logic).
    let session_id = format!("workflow:{run_id}");
    let failure_counter = Arc::new(AtomicU32::new(0));
    let observations: Arc<parking_lot::Mutex<Vec<ToolCallObservation>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let receipts: Arc<
        parking_lot::Mutex<Vec<crate::openhuman::workflows::types::DeliveryReceipt>>,
    > = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let _sub_handle = subscribe_tool_call_recorder(
        session_id.clone(),
        failure_counter.clone(),
        observations.clone(),
        receipts.clone(),
    );

    // F-17 deliverable B: pre-run memory recall. Fetch up to 3 prior
    // runs of this workflow, render as a Markdown preamble, prepend
    // to the user-message prompt. Best-effort — when the global
    // memory client isn't initialised, recall_prior_runs returns []
    // and render_recall_block emits the "first execution" fallback.
    let prior_runs = workflow_memory::recall_prior_runs(workflow_id, 3).await;
    let recall_block =
        workflow_memory::render_recall_block(&prior_runs, workflow_memory::RECALL_BLOCK_MAX_CHARS);
    let composed_prompt =
        workflow_memory::compose_prompt_with_recall(&recall_block, &agent_prompt_config.prompt);
    tracing::info!(
        target: "workflows-run",
        run_id = %run_id,
        prior_runs = prior_runs.len(),
        recall_chars = recall_block.len(),
        composed_chars = composed_prompt.len(),
        "[workflows-run] pre-run recall composed into user prompt"
    );

    // T-1 (Phase 2.5): enter the workflow-run task-local scope around
    // the agent body so write tools (composio_execute today; others
    // later) can publish DeliveryReceiptObserved events tagged with
    // this run's session id. Outside the scope tools see `None` and
    // skip emission — preserves the chat-driven Composio path
    // unchanged.
    let text = run_context::scope_workflow_run(session_id.clone(), async {
        #[cfg(test)]
        {
            if let Some(stub) = current_test_override() {
                let stubbed = stub(&composed_prompt, def)?;
                tracing::debug!(
                    target: "workflows-run",
                    "[workflows-run] run_agent_prompt via test override (text_len={})",
                    stubbed.len()
                );
                Ok::<String, anyhow::Error>(stubbed)
            } else {
                run_workflow_node_agent(config, &session_id, &composed_prompt, def).await
            }
        }
        #[cfg(not(test))]
        {
            run_workflow_node_agent(config, &session_id, &composed_prompt, def).await
        }
    })
    .await?;

    // Subscriber drains lazily; give it one tokio tick to consume
    // any in-flight events that arrived after the agent returned.
    // (broadcast::Receiver dispatch is sub-microsecond; one yield is
    // overkill but cheap insurance against the agent loop publishing
    // ToolExecutionCompleted on its way out.)
    tokio::task::yield_now().await;
    let tool_failure_count = failure_counter.load(Ordering::Relaxed);
    let tool_calls: Vec<ToolCallObservation> = observations.lock().clone();
    let delivery_receipts: Vec<crate::openhuman::workflows::types::DeliveryReceipt> =
        receipts.lock().clone();
    if tool_failure_count > 0 {
        tracing::warn!(
            target: "workflows-run",
            run_id = %run_id,
            tool_failure_count,
            tool_call_count = tool_calls.len(),
            receipt_count = delivery_receipts.len(),
            "[workflows-run] observed tool failures during run — step will be marked Failed"
        );
    } else {
        tracing::debug!(
            target: "workflows-run",
            run_id = %run_id,
            tool_call_count = tool_calls.len(),
            receipt_count = delivery_receipts.len(),
            "[workflows-run] agent finished cleanly"
        );
    }
    Ok(NodeOutput {
        text,
        tool_failure_count,
        tool_calls,
        delivery_receipts,
    })
}

/// The real (non-test-override) body of [`run_agent_prompt`].
/// Spawns the `workflow_node` sub-agent against the project config
/// with the per-run `allowed_tools` AND `iteration_cap` overrides,
/// sets the event context, calls `run_single`, returns the agent's
/// text response.
///
/// F-16 follow-up: applies `def.iteration_cap` to
/// `config.agent.max_tool_iterations` via the same clone-and-mutate
/// pattern `cron::scheduler::handle_scheduled_job` uses. Without
/// this override, the agent ran with the workflow_node TOML's
/// `max_iterations` default — which capped discovery+execute runs
/// too tightly: live testing 2026-05-22 10:05 showed the LLM
/// burning 9 iterations on parallel `composio_list_tools` discovery
/// across 8 toolkits, then hitting iteration 10 ("emit final
/// summary") before reaching any execute step. The agent reported
/// `success=true` for all the list calls and the run terminated
/// `Succeeded` — but no Slack DM or Calendar event was ever
/// produced because the actual action calls never fired.
async fn run_workflow_node_agent(
    config: &Config,
    session_id: &str,
    composed_prompt: &str,
    def: &NodeAgentDefinition,
) -> Result<String> {
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] run_agent_prompt spawning workflow_node sub-agent \
         session={session_id} iteration_cap={} allowed_tools={} model_tier={:?}",
        def.iteration_cap,
        def.allowed_tools.len(),
        def.model_tier,
    );
    if def.model_tier.is_some() {
        tracing::info!(
            target: "workflows-run",
            "[workflows-run] model_tier override requested ({:?}) but not yet \
             wired through workflow_node's agent definition — using the \
             archetype's default model. Phase 2 follow-up.",
            def.model_tier
        );
    }

    // Clone the config so we can apply the per-workflow iteration
    // cap without mutating the caller's value. Pattern mirrors
    // `cron::scheduler::handle_scheduled_job`.
    let mut effective = config.clone();
    effective.agent.max_tool_iterations = def.iteration_cap as usize;
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] applying per-workflow max_tool_iterations override: {} \
         (was {}; default from workflow_node TOML is the workflow_node \
         max_iterations field)",
        effective.agent.max_tool_iterations,
        config.agent.max_tool_iterations,
    );

    let mut agent = crate::openhuman::agent::Agent::from_config_for_agent_with_tool_override(
        &effective,
        "workflow_node",
        def.allowed_tools.clone(),
    )?;
    agent.set_event_context(session_id.to_string(), "workflow");
    agent.run_single(composed_prompt).await
}

/// F-17 deliverable C: build a [`workflow_memory::WorkflowRunMemory`]
/// chunk from the run's terminal state + the F-16 tool-call trace +
/// the connection auto-tags + the agent-authored entity tags, then
/// persist it under namespace `workflow:{workflow_id}` / key
/// `run:{run_id}` via the global memory client.
///
/// **Best-effort.** A failed store logs a warn and returns. The run's
/// terminal status is already persisted; missing memory is recoverable
/// (the next run sees one fewer prior summary) but a rollback would be
/// catastrophic.
///
/// The narrative-vs-actual drift detector
/// ([`workflow_memory::compute_drift`]) runs here so the stored chunk
/// records the honest summary the next-run recall will surface.
async fn persist_run_memory(
    run: &Run,
    terminal_status: RunStatus,
    agent_narrative: &str,
    observed_tool_calls: &[ToolCallObservation],
    allowed_connections: &[ConnectionRef],
    terminal_error: Option<&str>,
) {
    // 1. Convert F-16 event-bus observations into structured
    //    ToolCallTrace entries. Phase 1.5 leaves redacted_args empty
    //    and inner_status None — the event surface today doesn't
    //    carry those fields. A future ticket extends the event +
    //    fills them in here.
    let tool_calls: Vec<workflow_memory::ToolCallTrace> = observed_tool_calls
        .iter()
        .map(|obs| workflow_memory::ToolCallTrace {
            tool_name: obs.tool_name.clone(),
            redacted_args: serde_json::Value::Null,
            success: obs.success,
            elapsed_ms: obs.elapsed_ms,
            inner_status: None,
        })
        .collect();

    // 2. Anomalies: terminal_error from F-16's gate, plus a per-failure
    //    line for each failed tool call (with name + elapsed_ms).
    let mut anomalies: Vec<String> = Vec::new();
    if let Some(err) = terminal_error {
        anomalies.push(err.to_string());
    }
    for obs in observed_tool_calls.iter().filter(|o| !o.success) {
        anomalies.push(format!(
            "tool {} failed after {}ms",
            obs.tool_name, obs.elapsed_ms
        ));
    }

    let actual = workflow_memory::ActualOutcome {
        tool_calls,
        side_effects_confirmed: Vec::new(),
        side_effects_claimed_unverified: Vec::new(),
        anomalies,
    };

    // 3. Drift detection on the agent's text vs the ground-truth trace.
    let (narrative_matches_actual, narrative_drift) =
        workflow_memory::compute_drift(agent_narrative, &actual);

    // 4. Entity tags: auto from connections + agent's `## Entities
    //    touched` section.
    let auto_tags = workflow_memory::auto_entity_tags(allowed_connections);
    let agent_tags = workflow_memory::parse_agent_entity_tags(agent_narrative);
    let entity_tags = workflow_memory::merge_entity_tags(auto_tags, agent_tags);

    // 5. Truncate narrative to 600 chars per spec (keeps recall block
    //    bounded; the full prose still lives in the run-step output).
    let narrative = truncate_chars(agent_narrative, 600);

    let memory_chunk = workflow_memory::WorkflowRunMemory {
        workflow_id: run.workflow_id.clone(),
        run_id: run.id.clone(),
        triggered_at: run.started_at,
        trigger_source: run.trigger_source.clone(),
        status: terminal_status,
        actual,
        narrative,
        narrative_matches_actual,
        narrative_drift,
        entity_tags,
    };

    let content = memory_chunk.to_storage_markdown();
    let namespace = workflow_memory::namespace_for(&run.workflow_id);
    let key = workflow_memory::key_for_run(&run.id);
    let session_id = format!("workflow:{}", run.id);

    let Some(client) = crate::openhuman::memory::global::client_if_ready() else {
        tracing::warn!(
            target: "workflows-memory",
            run_id = %run.id,
            "[workflows-memory] global memory client not initialised; \
             skipping post-run store (this run will not appear in future recall)"
        );
        return;
    };
    let memory = client.memory_handle();
    match memory
        .store(
            &namespace,
            &key,
            &content,
            workflow_memory::WORKFLOW_MEMORY_CATEGORY,
            Some(&session_id),
        )
        .await
    {
        Ok(()) => {
            tracing::info!(
                target: "workflows-memory",
                run_id = %run.id,
                namespace = %namespace,
                drift = !narrative_matches_actual,
                "[workflows-memory] post-run chunk stored"
            );
        }
        Err(err) => {
            tracing::warn!(
                target: "workflows-memory",
                run_id = %run.id,
                "[workflows-memory] Memory::store failed: {err:#}; \
                 terminal status already persisted, proceeding"
            );
        }
    }
}

/// Helper: take the first `max_chars` Unicode scalars; append an
/// ellipsis when truncated. Mirrors the `truncate_for_recall`
/// utility in memory.rs but lives here so executor doesn't depend on
/// the recall internals.
fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{truncated}…")
}

/// Subscribe to the global event bus for the duration of a workflow
/// run and record every [`DomainEvent::ToolExecutionCompleted`] with
/// a matching `session_id` into `observations`. Failures also bump
/// `counter` — the F-16 D honest-status gate reads the counter; F-17
/// deliverable C reads the observations to build
/// `ActualOutcome.tool_calls`.
///
/// Returning the `SubscriptionHandle` is load-bearing — dropping it
/// would abort the subscriber task immediately, before any events
/// reach it. The caller binds it to `let _sub_handle = ...` so the
/// handle lives until the enclosing scope ends.
///
/// When the global event bus isn't initialised (which is the case in
/// some unit-test workspaces that don't go through the full RPC
/// bootstrap), this returns `None`. The counter never increments,
/// `tool_failure_count` stays 0, observations stay empty, and the
/// step status reverts to its pre-F-16 behaviour (Succeeded if the
/// agent returned text). This is the safe failure mode:
/// under-detection is preferred over over-detection of phantom
/// failures.
fn subscribe_tool_call_recorder(
    target_session_id: String,
    counter: Arc<AtomicU32>,
    observations: Arc<parking_lot::Mutex<Vec<ToolCallObservation>>>,
    receipts: Arc<parking_lot::Mutex<Vec<crate::openhuman::workflows::types::DeliveryReceipt>>>,
) -> Option<crate::core::event_bus::SubscriptionHandle> {
    use crate::core::event_bus::{subscribe_global, DomainEvent, EventHandler};
    use async_trait::async_trait;

    struct ToolCallRecorder {
        target_session_id: String,
        counter: Arc<AtomicU32>,
        observations: Arc<parking_lot::Mutex<Vec<ToolCallObservation>>>,
        // T-1 (Phase 2.5): receipts are recorded by the same
        // subscriber as tool calls so a single subscription handle
        // covers both event kinds; dropping the handle cancels both.
        receipts: Arc<parking_lot::Mutex<Vec<crate::openhuman::workflows::types::DeliveryReceipt>>>,
    }

    #[async_trait]
    impl EventHandler for ToolCallRecorder {
        fn name(&self) -> &str {
            "workflows-run::tool_call_recorder"
        }

        fn domains(&self) -> Option<&[&str]> {
            // Both ToolExecutionCompleted and DeliveryReceiptObserved
            // live in the "tool" domain; one filter covers both.
            Some(&["tool"])
        }

        async fn handle(&self, event: &DomainEvent) {
            match event {
                DomainEvent::ToolExecutionCompleted {
                    tool_name,
                    session_id,
                    success,
                    elapsed_ms,
                } => {
                    if session_id != &self.target_session_id {
                        return;
                    }
                    if !*success {
                        self.counter.fetch_add(1, Ordering::Relaxed);
                    }
                    self.observations.lock().push(ToolCallObservation {
                        tool_name: tool_name.clone(),
                        success: *success,
                        elapsed_ms: *elapsed_ms,
                    });
                }
                DomainEvent::DeliveryReceiptObserved {
                    session_id,
                    receipt_json,
                } => {
                    if session_id != &self.target_session_id {
                        return;
                    }
                    // Decode best-effort; a malformed receipt drops
                    // silently with a warn rather than failing the
                    // subscriber. The receipts surface is purely
                    // user-visible UX — never gates run correctness.
                    match serde_json::from_str::<crate::openhuman::workflows::types::DeliveryReceipt>(
                        receipt_json,
                    ) {
                        Ok(receipt) => {
                            self.receipts.lock().push(receipt);
                        }
                        Err(err) => {
                            tracing::warn!(
                                target: "workflows-run",
                                session_id = %session_id,
                                "[workflows-run] failed to decode \
                                 DeliveryReceiptObserved payload: \
                                 {err:#} — dropping receipt"
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    subscribe_global(Arc::new(ToolCallRecorder {
        target_session_id,
        counter,
        observations,
        receipts,
    }))
}
