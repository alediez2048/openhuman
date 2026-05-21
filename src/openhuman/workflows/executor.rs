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
//! ## Agent-invocation placeholder
//!
//! Per the F-8 dependency survey, invoking the agent from a non-Turn
//! context (cron-fired tokio task) requires constructing an
//! `Agent` and calling `run_single()` — the `subagent_runner::run_subagent`
//! entry the ticket assumed errors with `NoParentContext` outside a
//! harness turn. That `Agent`-driven invocation is bigger than F-8's
//! responsible budget; F-15's hero E2E will swap the placeholder for
//! the real call when it walks the live cron → agent → memory path
//! end-to-end.
//!
//! Until then, [`run_agent_prompt`] returns a deterministic stub
//! `NodeOutput { text }` carrying a clearly-labelled placeholder body
//! so:
//!   - the run-row + step-row pipeline gets exercised end-to-end,
//!   - F-9's single-flight + cancel paths have a working integration
//!     surface to layer on top of,
//!   - F-10 / F-12 land their agent-tool surfaces against the same
//!     `build_node_agent_definition` allowlist that the live path
//!     will use.
//!
//! Swap point at F-15: replace the placeholder body inside
//! [`run_agent_prompt`] with the `Agent::from_config(...).run_single(prompt)`
//! invocation. Signature is locked.
//!
//! F-9 fills `cancel_run` + the `ExecutorState.in_flight` HashMap.
//! F-8 leaves the state struct in place (initialised via
//! `OnceLock`) so the wire-up is a body-only change there too.

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

// ── ExecutorState (F-9 fills) ──────────────────────────────────────────

/// Process-global executor state — F-9 lands the single-flight
/// invariant + soft-cancel + orphan-recovery sweep on top of this. F-8
/// leaves the fields uninitialised-consulted: the `in_flight` map
/// exists so F-9 doesn't need to introduce a new singleton; the
/// `cancel_requested` map similarly carries the soft-cancel intent
/// from `cancel_run` into `execute_inner`.
pub struct ExecutorState {
    pub in_flight: Mutex<HashMap<WorkflowId, RunId>>,
    pub cancel_requested: Mutex<HashMap<RunId, ()>>,
}

impl ExecutorState {
    fn new() -> Self {
        Self {
            in_flight: Mutex::new(HashMap::new()),
            cancel_requested: Mutex::new(HashMap::new()),
        }
    }
}

fn state() -> &'static ExecutorState {
    static STATE: OnceLock<ExecutorState> = OnceLock::new();
    STATE.get_or_init(ExecutorState::new)
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
    #[error("store error: {0}")]
    Store(String),
}

impl DispatchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound(_) => "not_found",
            Self::PhaseConstraint(_) => "phase_constraint",
            Self::UnsupportedNodeKind(..) => "unsupported_node_kind",
            Self::Store(_) => "store_error",
        }
    }
}

/// Failure modes for [`cancel_run`]. F-9 lands the real soft-cancel
/// observer; F-8 only ever returns `NotImplemented`.
#[derive(Debug, Clone, Error)]
pub enum CancelError {
    #[error("cancel_run not implemented yet (F-9 will land it)")]
    NotImplemented,
    #[error("run id `{0}` not found")]
    NotFound(RunId),
}

impl CancelError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotImplemented => "not_implemented",
            Self::NotFound(_) => "not_found",
        }
    }
}

// ── dispatch_run ───────────────────────────────────────────────────────

/// Dispatch a workflow run. Validates Phase 1 invariants, persists the
/// `workflow_runs` row with `status = Running`, publishes
/// `WorkflowRunStarted`, spawns `execute_inner` on a tokio task, and
/// returns the new `RunId` immediately. The async caller (scheduler
/// poll loop or `workflows_run_now`) treats the run as fire-and-forget
/// from here on; status updates flow through the event bus + the
/// `workflow_runs` table.
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

    let now = Utc::now();
    let run = Run {
        id: Uuid::new_v4().to_string(),
        workflow_id: workflow.id.clone(),
        trigger_source: trigger_source.clone(),
        status: RunStatus::Running,
        started_at: now,
        completed_at: None,
        error: None,
        cancelled: false,
    };
    let run_id = run.id.clone();

    store::insert_run(config, &run).map_err(|err| DispatchError::Store(format!("{err:#}")))?;
    // F-9 will assert the single-flight invariant here. For F-8 we
    // record the entry so the wire-up is in place; reading it costs
    // nothing.
    state()
        .in_flight
        .lock()
        .insert(workflow.id.clone(), run.id.clone());

    publish_global(DomainEvent::WorkflowRunStarted {
        workflow_id: workflow.id.clone(),
        run_id: run.id.clone(),
    });
    tracing::info!(
        target: "workflows-run",
        "[workflows-run] dispatch_run wf={} run={run_id} source={trigger_source:?}",
        workflow.id
    );

    let config_owned = config.clone();
    tokio::spawn(async move {
        execute_inner(config_owned, workflow, run).await;
    });
    Ok(run_id)
}

/// F-8 stub. F-9 fills the body. Always returns
/// [`CancelError::NotImplemented`].
pub async fn cancel_run(_config: &Config, run_id: RunId) -> Result<(), CancelError> {
    tracing::debug!(
        target: "workflows-run",
        "[workflows-run] cancel_run called for run={run_id} — NotImplemented stub (F-9)"
    );
    Err(CancelError::NotImplemented)
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
async fn execute_inner(config: Config, workflow: Workflow, run: Run) {
    let timeout_secs = workflow.settings.timeout_secs.clamp(1, 3600);
    let node = workflow.nodes[0].clone();
    let workflow_id = workflow.id.clone();
    let run_id = run.id.clone();

    let outcome = tokio::time::timeout(
        Duration::from_secs(timeout_secs as u64),
        execute_agent_prompt(&config, &run, &node),
    )
    .await;

    let (terminal_status, terminal_error) = match outcome {
        Ok(Ok(())) => (RunStatus::Succeeded, None),
        Ok(Err(err)) => (RunStatus::Failed, Some(err.to_string())),
        Err(_elapsed) => (
            RunStatus::TimedOut,
            Some(format!("run exceeded {timeout_secs}s timeout")),
        ),
    };

    if let Err(err) = store::mark_run_terminal(
        &config,
        &run.id,
        terminal_status,
        Utc::now(),
        terminal_error.clone(),
    ) {
        tracing::error!(
            target: "workflows-run",
            "[workflows-run] mark_run_terminal failed wf={workflow_id} run={run_id}: {err:#}"
        );
    }

    // Release the single-flight slot now that the run reached terminal
    // status. F-9 swaps this for the proper invariant check.
    {
        let mut in_flight = state().in_flight.lock();
        if in_flight.get(&workflow_id) == Some(&run_id) {
            in_flight.remove(&workflow_id);
        }
    }

    let status_json = serde_json::to_value(&terminal_status).unwrap_or(serde_json::Value::Null);
    publish_global(DomainEvent::WorkflowRunCompleted {
        workflow_id: workflow_id.clone(),
        run_id: run_id.clone(),
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
        match run_agent_prompt(agent_prompt_config, &agent_def).await {
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

/// Node-execution output. Currently just a text body; F-15 will extend
/// to carry the agent's tool-call history if the run-detail view
/// needs it.
#[derive(Debug, Clone)]
pub struct NodeOutput {
    pub text: String,
}

/// **F-8 placeholder.** Returns a deterministic stub output so the
/// rest of the pipeline (step persistence, event publishing,
/// truncation) can be exercised end-to-end. F-15 swaps this for the
/// live `Agent::from_config(config).run_single(prompt)` invocation
/// per the dependency survey in the F-8 DEVLOG.
async fn run_agent_prompt(
    config: &AgentPromptConfig,
    def: &NodeAgentDefinition,
) -> Result<NodeOutput> {
    tracing::debug!(
        target: "workflows-run",
        "[workflows-run] run_agent_prompt PLACEHOLDER iteration_cap={} allowed_tools={}",
        def.iteration_cap,
        def.allowed_tools.len()
    );
    let body = format!(
        "[F-8 placeholder] Agent did not actually run. The real invocation \
         lands at F-15 (hero E2E) via Agent::run_single().\n\n\
         Prompt was ({} chars):\n{}\n\nAllowed tools: {}",
        config.prompt.chars().count(),
        config.prompt,
        def.allowed_tools.join(", ")
    );
    Ok(NodeOutput { text: body })
}
