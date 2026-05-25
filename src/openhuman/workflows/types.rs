//! Types for the Workflows domain (Phase 1 — Workflows & Automations).
//!
//! Locks the **full** type vocabulary in one place so every downstream
//! ticket (F-2..F-15) can build on a stable shape without redefining types.
//! The persistence layer (`store.rs`) stores `Workflow`, `Trigger`, `nodes`,
//! `edges`, and `WorkflowHealth` as JSON blobs in TEXT columns; the
//! serializable shapes here are the canonical wire format.
//!
//! Phase 2 / Phase 3 variants (`Trigger::Webhook` / `ComposioEvent` /
//! `ChannelMessage`; `NodeKind::ToolCall` / `HttpRequest` / `ChannelMessage`
//! / `Condition` / `Delay` / `Transform` / `AwaitHumanApproval` / `FanOut`)
//! are declared from day one. Reasons:
//!   1. Adding variants to a Serde-tagged enum stored as JSON is a
//!      schema-free change — existing rows continue to deserialize.
//!   2. The validator (F-11) needs to reject Phase-2 kinds with
//!      `UnsupportedNodeKind { kind, phase }`; that requires the variant
//!      to exist.
//!   3. Exhaustive match coverage in downstream tickets catches the
//!      upgrade path automatically.
//!
//! See `Automations/systemsdesign.md §2.2/§2.3` and ADR-017, ADR-018, ADR-019.

use crate::openhuman::connections::types::ConnectionRef;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Type aliases ────────────────────────────────────────────────────────
//
// All four ids are UUIDv7 strings. The string-typed alias keeps the JSON
// round-trip flat (Serde would otherwise emit `{"bytes": [...]}`) and the
// SQLite TEXT PRIMARY KEY columns line up directly.

pub type WorkflowId = String;
pub type NodeId = String;
pub type RunId = String;
pub type RunStepId = String;

// ── Entity types ────────────────────────────────────────────────────────

/// A complete workflow definition. Persisted as one row in `workflows`,
/// with the `trigger`, `nodes`, `edges`, `settings`, `origin`, and `health`
/// fields each round-tripped as JSON in their dedicated TEXT columns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Workflow {
    pub id: WorkflowId,
    /// Persisted schema version. Bumped only when the wire format breaks
    /// backwards compatibility — additive Serde changes do not require a
    /// bump.
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub enabled: bool,
    pub origin: WorkflowOrigin,
    pub health: WorkflowHealth,
    pub trigger: Trigger,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub settings: WorkflowSettings,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub last_run_at: Option<DateTime<Utc>>,
}

/// One execution unit inside a workflow. Phase 1 ships a single
/// `AgentPrompt` node per workflow (per FR-1.5.1.1); the validator
/// rejects anything else with `UnsupportedNodeKind`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub config: NodeConfig,
    /// Display position on the future canvas (Phase 3). Always present so
    /// the JSON round-trip is stable, even when Phase 1's UI ignores it.
    #[serde(default)]
    pub position: Option<CanvasPosition>,
}

/// Directional edge between two nodes. Phase 1 workflows have at most one
/// node, so `edges` is typically `[]`; the type exists so Phase 2 multi-
/// node graphs land without a schema bump.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
}

/// X/Y position on the Phase 3 visual canvas. Stored alongside each node
/// so the canvas can render without recomputing layout.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CanvasPosition {
    pub x: f32,
    pub y: f32,
}

/// Per-workflow runtime settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSettings {
    /// Wall-clock cap for a single run, in seconds. Clamped to [1, 3600]
    /// by the executor (F-8). 300s default per FR-1.6.5.
    pub timeout_secs: u32,
    /// Per FR-1.6.4: Phase 1 hard-codes `Halt`. The variant exists so the
    /// shape doesn't change when Phase 2 enables `Continue`.
    pub on_error: OnErrorPolicy,
}

impl Default for WorkflowSettings {
    fn default() -> Self {
        Self {
            timeout_secs: 300,
            on_error: OnErrorPolicy::Halt,
        }
    }
}

/// One execution attempt of a workflow. Rows live in `workflow_runs`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Run {
    pub id: RunId,
    pub workflow_id: WorkflowId,
    pub trigger_source: TriggerSource,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub error: Option<String>,
    /// Set when F-9's soft-cancel path observed the cancel before the run
    /// reached a natural terminal state. Read alongside `status` to render
    /// "Cancelled" in the UI.
    pub cancelled: bool,
}

/// One node-level step within a [`Run`]. Output is capped at 64 KiB
/// (NFR-2.3.5) at write time by the executor.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunStep {
    pub id: RunStepId,
    pub run_id: RunId,
    pub node_id: NodeId,
    pub status: RunStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default)]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub output_json: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

// ── Enums ───────────────────────────────────────────────────────────────

/// Where a workflow originated. Persisted in `workflows.origin` as JSON.
/// The `Seed { template_id }` carrying the id is the key dedup signal for
/// the F-5 starter-templates catalog (FR-1.8.2 / FR-1.8.3).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowOrigin {
    /// User described the workflow in chat (F-12 propose-then-click).
    UserChat,
    /// User filled the explicit form (Phase 3+).
    UserForm,
    /// User added from the starter catalog. `template_id` enables the
    /// "hide already-seeded" filter without inference.
    Seed { template_id: String },
    /// Reserved for future import paths. F-2 rejects this at create time
    /// because no importer exists yet.
    Imported,
}

/// Persisted, computed-on-event health field. ADR-017 keeps this in a
/// dedicated column so list-view reads stay cheap and recomputation is
/// scoped to a single bounded UPDATE per [`crate::core::event_bus::DomainEvent::ConnectionAdded`]
/// event in F-3.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowHealth {
    /// All referenced connections are present and live.
    Ready,
    /// One or more referenced connections are missing or not `Connected`.
    /// `missing` lists every offending ref so the UI / the chat agent can
    /// surface exact next steps.
    NeedsConnections { missing: Vec<ConnectionRef> },
    /// The most recent run failed; the workflow is still runnable but
    /// flagged for user attention. `reason` is the short error summary.
    LastRunFailed { run_id: RunId, reason: String },
    /// A referenced connection was deauthorised / expired. Mirrors
    /// `NeedsConnections` semantically but carries a single specific
    /// `ConnectionRef` so the UI can prompt re-auth in-place.
    SessionExpired { connection: ConnectionRef },
}

/// What fires this workflow. Phase 1 supports `Cron` and `Manual`; the
/// other three variants are Phase 2 stubs declared from day one so the
/// validator can reject them with `UnsupportedNodeKind`-style errors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// 5-field cron expression in UTC unless `tz` is set.
    Cron {
        expr: String,
        #[serde(default)]
        tz: Option<String>,
        #[serde(default)]
        active_hours: Option<ActiveHours>,
    },
    /// User-fired only. Manual workflows do not auto-run.
    Manual,
    /// Phase 2 — declared for forward compat.
    Webhook {
        tunnel_uuid: uuid::Uuid,
        target_path: String,
    },
    /// Phase 2 — Composio trigger event.
    ComposioEvent { trigger_id: String, toolkit: String },
    /// Phase 2 — channel message that matches a filter.
    ChannelMessage {
        provider: String,
        #[serde(default)]
        filter: Option<MessageFilter>,
    },
}

impl Trigger {
    /// True iff this trigger is a Phase 1 `Cron` expression. The
    /// scheduler hooks in `workflows::ops` short-circuit on this so
    /// `Manual` workflows never touch the registry.
    pub fn is_cron(&self) -> bool {
        matches!(self, Trigger::Cron { .. })
    }
}

/// Active-hours window for a `Trigger::Cron`. Optional; when unset, the
/// trigger fires whenever its cron expression matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActiveHours {
    /// `"HH:MM"` 24-hour, in the trigger's timezone.
    pub start: String,
    /// `"HH:MM"` 24-hour, in the trigger's timezone.
    pub end: String,
}

/// Phase-2 placeholder filter for `Trigger::ChannelMessage`. The exact
/// shape lands when channel triggers ship; declared here so the type
/// universe is locked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageFilter {
    /// Substring match against the message body (case-insensitive).
    #[serde(default)]
    pub contains: Option<String>,
    /// Match only direct messages (vs. channel/group messages).
    #[serde(default)]
    pub direct_only: bool,
}

/// The full set of node kinds across all 3 phases. Phase 1 only supports
/// `AgentPrompt`; the validator (F-11) rejects every other variant with
/// `ProposalValidationError::UnsupportedNodeKind { kind, phase }`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    /// LLM sub-agent call with an allowlist of connections.
    AgentPrompt,
    /// Phase 2 — direct tool/function call with deterministic params.
    ToolCall,
    /// Phase 2 — HTTP request against a `GenericHttp` connection.
    HttpRequest,
    /// Phase 2 — send a message to a chat channel.
    ChannelMessage,
    /// Phase 2 — branch on a predicate.
    Condition,
    /// Phase 2 — pause for a fixed duration.
    Delay,
    /// Phase 2 — transform/extract fields from inputs.
    Transform,
    /// Phase 2 — block until a human approves via UI.
    AwaitHumanApproval,
    /// Phase 3 — run children in parallel.
    FanOut,
}

/// Per-node configuration payload. Discriminated by `kind` at the wire
/// level so the validator can match it against [`NodeKind`] without two
/// parallel enums.
///
/// F2-1 added the Phase 2 variants (`ToolCall`, `HttpRequest`,
/// `ChannelMessage`, `Condition`, `Delay`); their `*Config` payload
/// shapes are sized to the OQ-7 / OQ-21 / OQ-22 locks recorded in
/// `Automations/requirements.md §8`. Per-kind execution bodies land in
/// F2-3..F2-7; F2-1 only makes the variants reachable end-to-end with
/// the validator + a `NotImplementedYet` executor dispatch arm.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeConfig {
    AgentPrompt(AgentPromptConfig),
    /// Phase 2 — F2-3.
    ToolCall(ToolCallConfig),
    /// Phase 2 — F2-4.
    HttpRequest(HttpRequestConfig),
    /// Phase 2 — F2-5.
    ChannelMessage(ChannelMessageConfig),
    /// Phase 2 — F2-6.
    Condition(ConditionConfig),
    /// Phase 2 — F2-7.
    Delay(DelayConfig),
    // `Transform` / `AwaitHumanApproval` / `FanOut` stay unreachable
    // until Phase 3+ — `NodeKind` carries them so the wire enum doesn't
    // bump, but no `NodeConfig` arm = the validator rejects them via
    // `UnsupportedNodeKind` (Phase 2's `allowed_node_kinds` already
    // excludes `FanOut`; Transform / AwaitHumanApproval are still
    // listed there but will be moved out in the F2-1 follow-up patch
    // once their config shapes are designed).
}

/// Configuration for a [`NodeKind::AgentPrompt`] node.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AgentPromptConfig {
    /// User-authored prompt passed verbatim to the sub-agent.
    pub prompt: String,
    /// Connections the sub-agent is allowed to use. F-3's health
    /// recompute walks this list against the connections snapshot.
    #[serde(default)]
    pub allowed_connections: Vec<ConnectionRef>,
    /// Hard cap on agent iterations. Defaults to 12 if omitted (sane
    /// upper bound for the Phase 1 sub-agent budget).
    #[serde(default = "default_iteration_cap")]
    pub iteration_cap: u32,
    /// Optional model tier (`"fast"` / `"medium"` / `"reasoning"`). When
    /// `None`, the executor picks the project default.
    #[serde(default)]
    pub model_tier: Option<String>,
}

fn default_iteration_cap() -> u32 {
    12
}

/// Configuration for a [`NodeKind::ToolCall`] node (F2-3).
///
/// Invokes a single named tool from `crate::openhuman::tools::registry`.
/// `arguments_template` is a JSON value whose string leaves are subject
/// to the OQ-7 templating surface (`{{node.<id>.output.<jsonpath>}}` /
/// `{{trigger.<jsonpath>}}`). The executor substitutes templates at
/// dispatch time, then forwards the resolved JSON to `Tool::execute`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCallConfig {
    /// Tool name as registered in
    /// [`crate::openhuman::tools::registry`]. F2-3 resolves this at
    /// dispatch time; F2-1's validator only enforces non-empty.
    pub tool_name: String,
    /// JSON arguments. String leaves may carry `{{...}}` template
    /// references (OQ-7). Empty object is allowed for tools with no
    /// arguments.
    #[serde(default = "default_empty_object")]
    pub arguments_template: serde_json::Value,
}

/// Configuration for a [`NodeKind::HttpRequest`] node (F2-4).
///
/// Hits a Phase-0 `GenericHttp` connection. `path_template`, headers,
/// and `body_template` are all subject to OQ-7 templating. The
/// executor resolves the `connection_id` against the encrypted-
/// credential store at dispatch time and assembles the final request
/// with the connection's `AuthKind` baked into the headers.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HttpRequestConfig {
    /// Generic-HTTP connection id (Phase 0 surface).
    pub connection_id: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// Path appended to the connection's `base_url`. Templating
    /// supported (e.g. `"/users/{{trigger.payload.user_id}}"`).
    pub path_template: String,
    /// Extra headers to send. Values support templating. Headers from
    /// the connection's own `default_headers` merge underneath (F2-4
    /// detail).
    #[serde(default)]
    pub headers: std::collections::BTreeMap<String, String>,
    /// Optional request body. Templating supported. Sent as-is — the
    /// executor sets `Content-Type` from the connection or from
    /// `headers["Content-Type"]` if the user provided one.
    #[serde(default)]
    pub body_template: Option<String>,
    /// What to capture into the node's output body. Defaults to
    /// `BodyAndStatus` so downstream nodes get both shape pieces
    /// without an explicit choice.
    #[serde(default)]
    pub response_capture: ResponseCapture,
}

/// How an `http_request` node captures its response into the node
/// output body. The executor always records the full response, but
/// `body_value` (the field downstream nodes template against) is
/// shaped by this enum.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResponseCapture {
    /// Default. Output body = `{ status, headers, body_text,
    /// body_json? }`. Downstream nodes template against any field.
    #[default]
    BodyAndStatus,
    /// Only the HTTP status code lands in the output body —
    /// `{ status }`. Useful when downstream nodes only need to
    /// branch on success/failure without parsing the body.
    StatusOnly,
    /// Pull a JSON-path slice out of the response body and place it
    /// at `body_value.captured`. The walker is the same dotted-path
    /// resolver `substitute_json` uses — F2-7 follow-up may extend
    /// to bracketed-array indexing if a concrete use case appears.
    /// Path is dotted (`data.user.id`), no `$.` prefix.
    JsonPath { path: String },
}

/// HTTP methods exposed to `NodeKind::HttpRequest`. Phase 2 starts
/// with the common four; PATCH / HEAD / OPTIONS can land later if a
/// concrete use case appears.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

/// Configuration for a [`NodeKind::ChannelMessage`] node (F2-5).
///
/// Sends a message to a connected chat channel (Slack, Discord,
/// Telegram, …). `body_template` is the message text; templating
/// substitutes upstream node outputs / trigger payload before send.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelMessageConfig {
    /// Channel-connection id (the user's connected workspace).
    pub connection_id: String,
    /// Optional target channel within the workspace. When `None`, F2-5
    /// uses the connection's default channel (e.g. Slack `#general`).
    /// Distinct from `connection_id` because one Slack workspace can
    /// target many channels.
    #[serde(default)]
    pub channel_id: Option<String>,
    /// Message text. Templating supported.
    pub body_template: String,
}

/// Configuration for a [`NodeKind::Condition`] node (F2-6).
///
/// Branches the workflow's execution path. The executor substitutes
/// `left` + `right` against the live `NodeContext`, evaluates the
/// predicate per `op`, and routes to `then_node_id` (predicate true)
/// or `else_node_id` (predicate false, when present). A missing
/// `else_node_id` halts the run cleanly on false — the workflow
/// terminates `Succeeded` with downstream nodes skipped.
///
/// Phase 2 ships a curated set of compare ops (per OQ-7's lean —
/// predictable for the LLM to emit). Phase 3 canvas inherits the
/// same routing semantics.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConditionConfig {
    /// Left-hand side. Templating supported (e.g.
    /// `"{{node.classify.output.label}}"`). Substituted before the
    /// predicate fires.
    pub left: String,
    /// Predicate operator. See [`CompareOp`].
    pub op: CompareOp,
    /// Right-hand side. Templating supported (lets a condition
    /// compare two upstream outputs). Literal otherwise.
    pub right: String,
    /// Node id to route to when the predicate is true.
    pub then_node_id: NodeId,
    /// Optional node id to route to when the predicate is false.
    /// `None` = halt-on-false (run terminates Succeeded with the
    /// remaining nodes skipped).
    #[serde(default)]
    pub else_node_id: Option<NodeId>,
}

/// Predicates the [`ConditionConfig`] node evaluates. F2-6 ships
/// the minimum useful set; future ops (numeric comparisons,
/// contains-any-of) can land when concrete use cases appear.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompareOp {
    /// `left == right` (string equality, case-sensitive).
    Eq,
    /// `left != right` (string inequality, case-sensitive).
    NotEq,
    /// `left.contains(right)` (substring match).
    Contains,
    /// `left` matches the regex in `right`. Regex compile happens at
    /// dispatch time; an invalid regex fails the step with a clear
    /// reason rather than panicking.
    Matches,
}

/// Configuration for a [`NodeKind::Delay`] node (F2-7).
///
/// Pauses the run for `seconds`. F2-7 makes the pause persistent
/// across core restarts (per the spec). F2-1 caps the value at 24h to
/// keep a runaway workflow from sleeping forever — refine if a
/// concrete use case needs longer.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DelayConfig {
    /// Pause duration in seconds. Validator caps at 86 400 (24 h).
    pub seconds: u64,
}

/// Default for `arguments_template` — an empty JSON object so a
/// tool-call node can omit the field entirely for argumentless tools.
fn default_empty_object() -> serde_json::Value {
    serde_json::json!({})
}

/// Terminal + transient states for a [`Run`] or [`RunStep`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Created but not yet picked up by the executor.
    Pending,
    /// Actively executing.
    Running,
    /// Finished cleanly.
    Succeeded,
    /// Finished with an error.
    Failed,
    /// Soft-cancel observed (F-9).
    Cancelled,
    /// Wall-clock timeout fired (FR-1.6.5).
    TimedOut,
}

/// Origin of a run dispatch. Phase 1 surfaces `Cron` (scheduler tick) and
/// `Manual { initiator }` (UI button click or RPC); the other three are
/// Phase 2 stubs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TriggerSource {
    Cron,
    /// `initiator` carries the human-facing label (`"user"`, `"agent"`,
    /// `"chat:<thread_id>"`) so the run history view can attribute
    /// who/what fired it.
    Manual {
        initiator: String,
    },
    /// Phase 2 — webhook payload triggered the run.
    Webhook,
    /// Phase 2 — Composio event triggered the run.
    ComposioEvent,
    /// Phase 2 — channel message triggered the run.
    ChannelMessage,
}

/// Who/what fired `workflows_run_now`. Phase 1 only emits `User`; the
/// other variants are declared from day one so F-14's chat-driven
/// manual-run handler and F-6's catalog [Run now] entry-point can land
/// without changing this surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ManualInitiator {
    User,
    Agent { session_id: String },
    Catalog { template_id: String },
}

impl ManualInitiator {
    /// Human-facing label embedded in `TriggerSource::Manual { initiator }`.
    pub fn label(&self) -> String {
        match self {
            Self::User => "user".into(),
            Self::Agent { session_id } => format!("agent:{session_id}"),
            Self::Catalog { template_id } => format!("catalog:{template_id}"),
        }
    }
}

/// Failure modes for `workflows_run_now`. The RPC surfaces each as a
/// structured `RpcOutcome::Err { code }` so the UI can branch (e.g.
/// disable the [Run now] button when the badge says
/// `NeedsConnections`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunNowError {
    NotFound,
    HealthBlocked { health: WorkflowHealth },
    Dispatch { reason: String },
}

impl RunNowError {
    /// Stable error-code string for the RPC layer + metrics.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::HealthBlocked { .. } => "health_blocked",
            Self::Dispatch { .. } => "dispatch_failed",
        }
    }
}

/// What the executor does when a node fails mid-run. Phase 1 hard-codes
/// `Halt`; the variant set ships from day one so Phase 2 can flip the bit
/// without a schema change.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum OnErrorPolicy {
    /// Stop the run and mark it `Failed`. Phase 1 default.
    #[default]
    Halt,
    /// Phase 2 — skip the failing node, continue with the rest.
    Continue,
}

/// Confidence band the drafting sub-agent attaches to a proposal. Only
/// the `WorkflowProposal` carries this today; the type is named here so
/// downstream consumers don't redefine it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    High,
    Medium,
    Low,
}

/// Which state mutation a `WorkflowStateProposal` previews.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateAction {
    Enable,
    Disable,
    RunNow,
}

/// Why the executor refused to dispatch a triggered run. Published via
/// `DomainEvent::WorkflowRunSkipped` for ops visibility.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SkippedReason {
    /// Single-flight invariant (ADR-014) blocked dispatch.
    AlreadyRunning,
    /// Workflow's persisted health was not `Ready` at dispatch time.
    HealthBlocked { health: WorkflowHealth },
}

// ── Proposal types (chat-driven creation; consumed by F-11..F-14) ──────

/// Drafting-agent output for "build me a workflow that …". Round-trips
/// through `proposer::draft_with_retries` → `validator::validate` → UI
/// render → [Save & Enable].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowProposal {
    pub name: String,
    pub description: String,
    pub trigger: Trigger,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub settings: WorkflowSettings,
    /// Connections the proposed workflow requires (union of every node's
    /// `allowed_connections`). Validated against the user's current
    /// connections snapshot.
    pub required_connections: Vec<ConnectionRef>,
    /// Drafting-agent rationale bullets shown above the preview.
    #[serde(default)]
    pub rationale: Vec<String>,
    pub confidence: Confidence,
}

/// Edit preview surfaced when the chat agent calls
/// `workflow_propose_update`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowEditProposal {
    pub workflow_id: WorkflowId,
    pub current: Workflow,
    pub proposed: Workflow,
    /// Server-computed human-friendly diff bullets (e.g. `"Renamed from
    /// 'X' to 'Y'."`). Capped at 20 entries by the diff helper; if more
    /// were detected the last bullet is `"... and N more changes."`.
    pub diff_summary: Vec<String>,
    #[serde(default)]
    pub rationale: Vec<String>,
}

/// Delete preview surfaced when the chat agent calls
/// `workflow_propose_delete`. Carries the run-history count so the UI can
/// render a clear "what will be lost" message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowDeletePreview {
    pub workflow_id: WorkflowId,
    pub name: String,
    pub run_count: u32,
    /// FR-1.3.4 — 30 days. Hard-coded in F-12; declared here so the UI
    /// doesn't redefine the literal.
    pub retention_days: u32,
}

/// Enable / Disable / RunNow preview surfaced by `workflow_propose_*`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowStateProposal {
    pub workflow_id: WorkflowId,
    pub action: StateAction,
    #[serde(default)]
    pub rationale: Vec<String>,
    /// When false, the action is gated (e.g. `RunNow` on a
    /// `NeedsConnections` workflow); the UI renders the preview but
    /// disables the Apply button.
    #[serde(default = "default_state_proposal_enabled")]
    pub enabled: bool,
}

fn default_state_proposal_enabled() -> bool {
    true
}

/// Every way a proposal can fail validation (ADR-019). One variant per
/// failure mode so metrics and retry-prompt feedback can be surgical.
///
/// Tag name `"type"` matches the [`ConnectionRef`] / [`Trigger`] /
/// [`WorkflowOrigin`] convention; the field `node_kind` (vs the more
/// natural `kind`) avoids a Serde tag-name collision with the variant's
/// payload.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProposalValidationError {
    /// The proposal payload was not deserializable JSON.
    JsonParse { reason: String },
    /// A `ConnectionRef` referenced by the proposal isn't in the user's
    /// snapshot. `candidates` are fuzzy-matched suggestions the drafting
    /// agent can use to correct typos on its next attempt.
    UnknownConnection {
        r#ref: ConnectionRef,
        candidates: Vec<ConnectionRef>,
    },
    /// A node kind that isn't allowed in the current phase. Phase 1
    /// only allows `AgentPrompt`.
    UnsupportedNodeKind { node_kind: NodeKind, phase: u32 },
    /// `Trigger::Cron { expr }` failed `cron::Schedule::from_str`.
    InvalidCron { expr: String, parse_error: String },
    /// `edges[].from` or `edges[].to` references a node id that doesn't
    /// exist in `nodes`.
    EdgeIntegrity {
        from: NodeId,
        to: NodeId,
        reason: String,
    },
    /// A required scalar (`name`, `description`, `nodes`) was empty.
    MissingRequiredField { field: String },
    /// A `NodeConfig` payload field failed per-kind validation —
    /// e.g. `ToolCallConfig.tool_name` empty, `DelayConfig.seconds`
    /// over the 24-hour cap. F2-1 lands the shape checks; per-kind
    /// dispatch-time checks (tool registry lookup, connection-id
    /// resolution) live in F2-3..F2-7.
    InvalidNodeConfig {
        node_id: NodeId,
        node_kind: NodeKind,
        reason: String,
    },
}

impl ProposalValidationError {
    /// Stable lowercase snake_case label for metrics. Keep this in sync
    /// with the variant set — F-11's tests assert exhaustiveness.
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::JsonParse { .. } => "json_parse",
            Self::UnknownConnection { .. } => "unknown_connection",
            Self::UnsupportedNodeKind { .. } => "unsupported_node_kind",
            Self::InvalidCron { .. } => "invalid_cron",
            Self::EdgeIntegrity { .. } => "edge_integrity",
            Self::MissingRequiredField { .. } => "missing_required_field",
            Self::InvalidNodeConfig { .. } => "invalid_node_config",
        }
    }
}

/// Failure modes for the F-11 drafting retry loop (ADR-015).
///
/// `proposer::draft_with_retries` returns `Ok(WorkflowProposal)` after
/// any attempt the validator accepts; otherwise one of these:
///
///  - [`DraftFailure::ValidationFailedAfterRetries`] — the drafting
///    sub-agent ran the full `max_attempts` budget without producing a
///    valid proposal. `last_error` carries the most recent validator
///    error so the UI / call site can render a focused message.
///  - [`DraftFailure::RunFailure`] — the sub-agent itself failed
///    (LLM provider error, timeout, no `emit_proposal` call). Distinct
///    from a validation failure so callers can branch on transient vs.
///    semantic errors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DraftFailure {
    /// All `max_attempts` validator runs failed; `last_error` is the
    /// final attempt's error. Retry budget is 3 by FR-1.13.4 / ADR-015.
    ValidationFailedAfterRetries {
        attempts: u32,
        last_error: ProposalValidationError,
    },
    /// The sub-agent itself failed (LLM provider error, hard timeout,
    /// no `emit_proposal` tool call observed). The `reason` is a
    /// short human-readable string; it must never carry proposal
    /// content (NFR-2.4.4).
    RunFailure { reason: String },
}

impl DraftFailure {
    /// Stable lowercase snake_case label for metrics + log filtering.
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::ValidationFailedAfterRetries { .. } => "validation_failed_after_retries",
            Self::RunFailure { .. } => "run_failure",
        }
    }
}

impl std::fmt::Display for DraftFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ValidationFailedAfterRetries {
                attempts,
                last_error,
            } => write!(
                f,
                "drafting failed after {attempts} attempts; last_error={last_error:?}"
            ),
            Self::RunFailure { reason } => write!(f, "drafting sub-agent failed: {reason}"),
        }
    }
}

impl std::error::Error for DraftFailure {}

// ── RPC request / list-filter payloads (F-2) ────────────────────────────

/// Request payload for `workflows_create`. Every field that isn't
/// server-generated lives here; `id`, `created_at`, `updated_at`,
/// `last_run_at`, `health`, `schema_version`, and `enabled` are all
/// stamped by `ops::create`.
///
/// `#[serde(deny_unknown_fields)]` rejects malformed payloads
/// (typo'd field names, leaked `id` / `health` columns) at deserialize
/// time so the handler returns a clean `invalid_argument` rather than
/// silently dropping the field.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CreateWorkflowRequest {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub trigger: Trigger,
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub settings: Option<WorkflowSettings>,
    /// Caller-supplied discriminator. UI / chat agent / catalog each
    /// pass their own (ADR-018). `Imported` is rejected by `ops::create`
    /// until an import path lands.
    pub origin: WorkflowOrigin,
}

/// Partial update payload — every field is optional. `None` means "do
/// not change". `id`, `origin`, `created_at`, `health`, `last_run_at`,
/// and `enabled` are intentionally absent: identity / provenance /
/// computed fields aren't editable through this surface.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WorkflowPatch {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<Option<String>>,
    #[serde(default)]
    pub trigger: Option<Trigger>,
    #[serde(default)]
    pub nodes: Option<Vec<Node>>,
    #[serde(default)]
    pub edges: Option<Vec<Edge>>,
    #[serde(default)]
    pub settings: Option<WorkflowSettings>,
}

/// Request payload for `workflows_update`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct UpdateWorkflowRequest {
    pub id: WorkflowId,
    pub patches: WorkflowPatch,
}

/// Filter chips on the `/workflows` list view (FR-1.2.7).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ListFilter {
    /// `Some(true)` returns enabled workflows only; `Some(false)`
    /// returns disabled only; `None` returns both.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Restrict to a single health-state discriminator.
    #[serde(default)]
    pub health_state: Option<HealthFilter>,
    /// Case-insensitive substring against `name`.
    #[serde(default)]
    pub search: Option<String>,
}

/// Discriminator-only enum used by the [`ListFilter`] chip. Mirrors the
/// four variants of [`WorkflowHealth`] but without their payloads, so
/// the filter matches purely on health kind.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthFilter {
    Ready,
    NeedsConnections,
    LastRunFailed,
    SessionExpired,
}

// ── Starter templates (F-5) ────────────────────────────────────────────

/// Deserialized shape of a `templates/*.json` file. The trigger / nodes
/// / edges / settings fields are kept as opaque `serde_json::Value` so
/// the catalog [Add] flow can pass them through to `workflows_create`
/// untouched — that lets the JSON files include forward-compat fields
/// (e.g. `nodes[].name`, per-node `on_error`) that Phase 1's typed
/// shapes don't model yet without rejecting the file at parse time.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StarterTemplate {
    pub template_id: String,
    /// Minimum Phase number required to support the template's node
    /// kinds + trigger. Phase 1 ships with `min_phase = 1` everywhere;
    /// Phase 2+ templates land later.
    pub min_phase: u32,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Cron / manual / Phase-2-stub trigger payload. Used by the
    /// catalog for `trigger_summary` derivation and passed through to
    /// `workflows_create` on [Add].
    pub trigger: serde_json::Value,
    /// Opaque node list. Phase 1's typed `Vec<Node>` doesn't model
    /// every field the artifact templates include (`name`,
    /// per-node `on_error`); keeping this as JSON avoids losing those
    /// fields on the round-trip.
    pub nodes: serde_json::Value,
    #[serde(default)]
    pub edges: serde_json::Value,
    #[serde(default)]
    pub settings: serde_json::Value,
    /// Connections the workflow needs to be `Ready`. The catalog
    /// computes `missing_connections` against the current snapshot.
    pub required_connections: Vec<ConnectionRef>,
    #[serde(default)]
    pub rationale_at_seed: Vec<String>,
}

/// Catalog response row — what the F-6 `StarterWorkflowsSection`
/// renders, and what `workflows_list_starter_templates` returns.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StarterTemplateView {
    pub template_id: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// Humanized cron / trigger summary (e.g. `"Weekdays at 8:00"`,
    /// `"Run on demand"`). Computed server-side so the UI doesn't pull
    /// a cron-parsing dependency.
    pub trigger_summary: String,
    /// Every connection the template requires.
    pub required_connections: Vec<ConnectionRef>,
    /// Subset of `required_connections` that the user does NOT have
    /// in a "live" state per the Phase 0 honest-connection truth
    /// table. The catalog card surfaces these as amber pills.
    pub missing_connections: Vec<ConnectionRef>,
    #[serde(default)]
    pub rationale_at_seed: Vec<String>,
    /// Full template body as JSON — F-6's [Add] button passes this
    /// straight back to `workflows_create` so the round-trip doesn't
    /// require parsing/reserializing on the client.
    pub raw_payload: serde_json::Value,
}

/// Request payload for `workflows_list_starter_templates`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ListStarterTemplatesRequest {
    /// Override the current Phase (defaults to Phase 1 server-side).
    /// Mainly useful for tests + future cross-phase UIs.
    #[serde(default)]
    pub phase: Option<u32>,
}
