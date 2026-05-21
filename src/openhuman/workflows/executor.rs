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
//! ## Agent invocation (F-15)
//!
//! [`run_agent_prompt`] calls `Agent::from_config(config).run_single(prompt)`
//! — the same pattern `cron::scheduler::handle_scheduled_job` uses for
//! its session-target=Main/Isolated runs. The event channel is set to
//! `"workflow"` and the session id is `"workflow:<run_id>"` so
//! downstream subscribers (token-usage accounting, telemetry,
//! Sentry) can filter workflow-driven turns from CLI / cron / chat.
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
use std::sync::OnceLock;
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

/// Skeletal agent definition the executor builds for an
/// `agent_prompt` node. Phase 1 keeps this as a plain struct (no
/// dependency on the harness's `AgentDefinition` type) — F-15 maps it
/// into `crate::openhuman::agent::harness::definition::AgentDefinition`
/// when the placeholder is swapped for the real call.
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
    /// the project default.
    pub model_tier: Option<String>,
}

/// Build the allowlist for an `agent_prompt` node. Per ADR-016 the
/// shape is exactly:
///
///   `baseline + connection-resolved + read-only workflow tools`
///
/// — and nothing else (no `workflow_propose_*`, no mutating workflow
/// tools, no skill-creator surfaces).
pub fn build_node_agent_definition(
    allowed_connections: &[ConnectionRef],
    iteration_cap: u32,
    model_tier: Option<String>,
) -> NodeAgentDefinition {
    let mut allowed_tools: Vec<String> =
        BASELINE_TOOL_NAMES.iter().map(|s| s.to_string()).collect();
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
                (RunStatus::Succeeded, Some(payload), None)
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

/// Node-execution output. Currently just a text body; a future
/// ticket will extend this to carry the agent's tool-call history
/// if the run-detail view needs it.
#[derive(Debug, Clone)]
pub struct NodeOutput {
    pub text: String,
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

/// Execute the `agent_prompt` node's body via the real agent
/// harness. Mirrors the cron domain's pattern from
/// `cron/scheduler.rs::handle_scheduled_job`:
///
///   1. `Agent::from_config(config)` builds the harness with the
///      project's configured provider + tools + memory.
///   2. `agent.set_event_context("workflow:<run_id>", "workflow")`
///      tags downstream telemetry so subscribers can filter
///      workflow-driven turns from CLI / cron / chat.
///   3. `agent.run_single(prompt)` returns the agent's final text
///      response, which becomes the persisted
///      `workflow_run_steps.output_json.text` after truncation.
///
/// Tests inject a deterministic stub via
/// [`set_test_agent_prompt_override`]; the override is only
/// honoured under `#[cfg(test)]`. In production the override slot
/// never exists.
async fn run_agent_prompt(
    config: &Config,
    run_id: &RunId,
    agent_prompt_config: &AgentPromptConfig,
    def: &NodeAgentDefinition,
) -> Result<NodeOutput> {
    #[cfg(test)]
    if let Some(stub) = current_test_override() {
        let text = stub(&agent_prompt_config.prompt, def)?;
        tracing::debug!(
            target: "workflows-run",
            "[workflows-run] run_agent_prompt via test override (text_len={})",
            text.len()
        );
        return Ok(NodeOutput { text });
    }

    tracing::info!(
        target: "workflows-run",
        "[workflows-run] run_agent_prompt invoking agent run={run_id} iteration_cap={} allowed_tools={}",
        def.iteration_cap,
        def.allowed_tools.len()
    );
    let mut agent = crate::openhuman::agent::Agent::from_config(config)?;
    agent.set_event_context(format!("workflow:{run_id}"), "workflow");
    let text = agent.run_single(&agent_prompt_config.prompt).await?;
    Ok(NodeOutput { text })
}
