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
use crate::openhuman::workflows::store;
use crate::openhuman::workflows::types::{
    AgentPromptConfig, Node, NodeConfig, NodeKind, Run, RunId, RunStatus, RunStep, RunStepId,
    TriggerSource, Workflow, WorkflowId,
};
use anyhow::Result;
use chrono::Utc;
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
    let workflow = match store::get_workflow(config, &workflow_id) {
        Ok(Some(w)) => w,
        Ok(None) => return Err(DispatchError::NotFound(workflow_id).into()),
        Err(err) => return Err(DispatchError::Store(format!("{err:#}")).into()),
    };

    validate_phase_1_workflow(&workflow)?;

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
    tokio::spawn(async move {
        // Move the slot into the task so Drop fires on every exit
        // path — including a panic inside execute_inner.
        let _slot_guard = slot;
        execute_inner(config_owned, workflow, run).await;
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

// ── execute_inner ──────────────────────────────────────────────────────

/// Phase 1 invariant: exactly one node, kind = AgentPrompt. The
/// validator (F-11) catches this at create time; the executor
/// belts-and-suspenders the runtime check so a direct-RPC client can't
/// bypass it.
fn validate_phase_1_workflow(workflow: &Workflow) -> Result<(), DispatchError> {
    if workflow.nodes.len() != 1 {
        return Err(DispatchError::PhaseConstraint(workflow.id.clone()));
    }
    let node = &workflow.nodes[0];
    if !matches!(node.kind, NodeKind::AgentPrompt) {
        return Err(DispatchError::UnsupportedNodeKind(
            workflow.id.clone(),
            node.kind,
        ));
    }
    Ok(())
}

/// Drives the run to a terminal status. Spawned on a tokio task by
/// `dispatch_run`; doesn't return anything because every state
/// transition flows through the event bus + the `workflow_runs` table.
///
/// Soft-cancel observation (ADR-014, FR-1.6.9): between nodes the
/// loop reads `workflow_runs.cancelled` via [`store::is_cancelled`].
/// Phase 1 has one node so the practical effect is a check right
/// before the agent starts and once after it returns; Phase 2's
/// multi-node graphs reuse the same loop structure without changes
/// here. The current node's LLM call is **not** aborted on cancel.
async fn execute_inner(config: Config, workflow: Workflow, run: Run) {
    let timeout_secs = workflow.settings.timeout_secs.clamp(1, 3600);
    let node = workflow.nodes[0].clone();
    let workflow_id = workflow.id.clone();
    let run_id = run.id.clone();

    // Pre-node cancel check — handles the case where cancel_run fired
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

    let outcome = tokio::time::timeout(
        Duration::from_secs(timeout_secs as u64),
        execute_agent_prompt(&config, &run, &node),
    )
    .await;

    let (terminal_status, terminal_error) = match outcome {
        Ok(Ok(())) => {
            // Between-nodes check (Phase 2 reuses this loop slot).
            // Even with a single-node graph, a cancel that arrived
            // during the agent body upgrades a successful return to
            // a Cancelled terminal status — the FR-1.6.9 cooperative
            // pattern (current node completes; status flips).
            if cancellation_observed(&config, &workflow_id, &run_id) {
                (RunStatus::Cancelled, Some("cancelled mid-run".into()))
            } else {
                (RunStatus::Succeeded, None)
            }
        }
        Ok(Err(err)) => (RunStatus::Failed, Some(err.to_string())),
        Err(_elapsed) => (
            RunStatus::TimedOut,
            Some(format!("run exceeded {timeout_secs}s timeout")),
        ),
    };

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
    if let Err(err) = store::mark_run_terminal(
        config,
        &run_id.to_string(),
        terminal_status,
        Utc::now(),
        terminal_error,
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

// ── execute_agent_prompt ───────────────────────────────────────────────

/// Phase 1 node body: persist a step row, fire `WorkflowRunStepStarted`,
/// run the agent (PLACEHOLDER per the module-doc), truncate + persist
/// output, fire `WorkflowRunStepCompleted`.
async fn execute_agent_prompt(config: &Config, run: &Run, node: &Node) -> Result<()> {
    let NodeConfig::AgentPrompt(ref agent_prompt_config) = node.config;
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
        agent_prompt_config.prompt.chars().count()
    );

    let agent_def = build_node_agent_definition(
        &agent_prompt_config.allowed_connections,
        agent_prompt_config.iteration_cap,
        agent_prompt_config.model_tier.clone(),
    );

    let (terminal_status, output_json, error) =
        match run_agent_prompt(config, &run.id, agent_prompt_config, &agent_def).await {
            Ok(output) => {
                let truncated = store::truncate_output_to_64kib(output.text);
                let payload = serde_json::to_string(&serde_json::json!({ "text": truncated }))
                    .unwrap_or_else(|_| "{}".into());
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
                    (RunStatus::Failed, Some(payload), Some(summary))
                } else {
                    (RunStatus::Succeeded, Some(payload), None)
                }
            }
            Err(err) => (RunStatus::Failed, None, Some(format!("{err:#}"))),
        };

    if let Err(err) = store::update_run_step_terminal(
        config,
        &step_id,
        terminal_status,
        Utc::now(),
        output_json,
        error.clone(),
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
    Ok(())
}

/// Node-execution output. Carries the agent's final text response
/// AND a count of tool calls that the harness reported as
/// `success = false` (per the
/// [`crate::core::event_bus::DomainEvent::ToolExecutionCompleted`]
/// tap installed in [`run_agent_prompt`]).
///
/// F-16 D: the caller in [`execute_agent_prompt_node`] uses
/// `tool_failure_count > 0` to override the step status to `Failed`
/// even when the agent itself returned text — so a workflow that
/// "completed" by emitting an apology after every tool call got
/// denied no longer lies in run history.
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
    let _sub_handle = subscribe_tool_failure_counter(session_id.clone(), failure_counter.clone());

    let text = {
        #[cfg(test)]
        if let Some(stub) = current_test_override() {
            let stubbed = stub(&agent_prompt_config.prompt, def)?;
            tracing::debug!(
                target: "workflows-run",
                "[workflows-run] run_agent_prompt via test override (text_len={})",
                stubbed.len()
            );
            stubbed
        } else {
            run_workflow_node_agent(config, &session_id, agent_prompt_config, def).await?
        }
        #[cfg(not(test))]
        {
            run_workflow_node_agent(config, &session_id, agent_prompt_config, def).await?
        }
    };

    // Subscriber drains lazily; give it one tokio tick to consume
    // any in-flight events that arrived after the agent returned.
    // (broadcast::Receiver dispatch is sub-microsecond; one yield is
    // overkill but cheap insurance against the agent loop publishing
    // ToolExecutionCompleted on its way out.)
    tokio::task::yield_now().await;
    let tool_failure_count = failure_counter.load(Ordering::Relaxed);
    if tool_failure_count > 0 {
        tracing::warn!(
            target: "workflows-run",
            run_id = %run_id,
            tool_failure_count,
            "[workflows-run] observed tool failures during run — step will be marked Failed"
        );
    }
    Ok(NodeOutput {
        text,
        tool_failure_count,
    })
}

/// The real (non-test-override) body of [`run_agent_prompt`].
/// Spawns the `workflow_node` sub-agent against the project config
/// with the per-run `allowed_tools` override, sets the event
/// context, calls `run_single`, returns the agent's text response.
async fn run_workflow_node_agent(
    config: &Config,
    session_id: &str,
    agent_prompt_config: &AgentPromptConfig,
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
    let mut agent = crate::openhuman::agent::Agent::from_config_for_agent_with_tool_override(
        config,
        "workflow_node",
        def.allowed_tools.clone(),
    )?;
    agent.set_event_context(session_id.to_string(), "workflow");
    agent.run_single(&agent_prompt_config.prompt).await
}

/// Subscribe to the global event bus for the duration of a workflow
/// run and increment `counter` every time the harness publishes a
/// [`DomainEvent::ToolExecutionCompleted`] with the matching
/// `session_id` and `success = false`.
///
/// Returning the `SubscriptionHandle` is load-bearing — dropping it
/// would abort the subscriber task immediately, before any events
/// reach it. The caller binds it to `let _sub_handle = ...` so the
/// handle lives until the enclosing scope ends.
///
/// When the global event bus isn't initialised (which is the case in
/// some unit-test workspaces that don't go through the full RPC
/// bootstrap), this returns `None`. The counter never increments,
/// `tool_failure_count` stays 0, and the step status reverts to its
/// pre-F-16 behaviour (Succeeded if the agent returned text). This
/// is the safe failure mode: under-detection is preferred over
/// over-detection of phantom failures.
fn subscribe_tool_failure_counter(
    target_session_id: String,
    counter: Arc<AtomicU32>,
) -> Option<crate::core::event_bus::SubscriptionHandle> {
    use crate::core::event_bus::{subscribe_global, DomainEvent, EventHandler};
    use async_trait::async_trait;

    struct ToolFailureCounter {
        target_session_id: String,
        counter: Arc<AtomicU32>,
    }

    #[async_trait]
    impl EventHandler for ToolFailureCounter {
        fn name(&self) -> &str {
            "workflows-run::tool_failure_counter"
        }

        fn domains(&self) -> Option<&[&str]> {
            // ToolExecutionCompleted lives in the "tool" domain; the
            // filter saves us from waking on every memory / channel
            // event during the run.
            Some(&["tool"])
        }

        async fn handle(&self, event: &DomainEvent) {
            if let DomainEvent::ToolExecutionCompleted {
                session_id,
                success,
                ..
            } = event
            {
                if !*success && session_id == &self.target_session_id {
                    self.counter.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    subscribe_global(Arc::new(ToolFailureCounter {
        target_session_id,
        counter,
    }))
}
