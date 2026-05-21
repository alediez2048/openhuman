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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeConfig {
    AgentPrompt(AgentPromptConfig),
    // Phase 2/3 variants intentionally omitted — declared via NodeKind
    // alone so a Phase 2 ticket adds the matching config variant in one
    // place, not two. Until then, a workflow whose node has any non-
    // AgentPrompt kind cannot deserialize a matching NodeConfig and the
    // validator surfaces UnsupportedNodeKind.
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
        }
    }
}

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
