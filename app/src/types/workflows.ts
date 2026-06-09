/**
 * TypeScript types mirroring `src/openhuman/workflows/types.rs` (F-1).
 *
 * Kept hand-written rather than codegen'd; the surface is small enough
 * for manual sync. Update both this file and the Rust types in
 * lock-step. The serialized JSON shape matches Rust's
 * `#[serde(tag = "type", rename_all = "snake_case")]` convention.
 */
import type { ConnectionRef } from './connections';

// ── Type aliases ────────────────────────────────────────────────────────

export type WorkflowId = string;
export type NodeId = string;
export type RunId = string;
export type RunStepId = string;

// ── Enums ───────────────────────────────────────────────────────────────

export type WorkflowOrigin =
  | { type: 'user_chat' }
  | { type: 'user_form' }
  | { type: 'seed'; template_id: string }
  | { type: 'imported' };

export type WorkflowHealth =
  | { type: 'ready' }
  | { type: 'needs_connections'; missing: ConnectionRef[] }
  | { type: 'last_run_failed'; run_id: RunId; reason: string }
  | { type: 'session_expired'; connection: ConnectionRef };

export interface ActiveHours {
  start: string;
  end: string;
}

export interface MessageFilter {
  contains?: string | null;
  direct_only?: boolean;
}

export type Trigger =
  | { type: 'cron'; expr: string; tz?: string | null; active_hours?: ActiveHours | null }
  | { type: 'manual' }
  | { type: 'webhook'; tunnel_uuid: string; target_path: string }
  | { type: 'composio_event'; trigger_id: string; toolkit: string }
  | { type: 'channel_message'; provider: string; filter?: MessageFilter | null };

export type NodeKind =
  | 'agent_prompt'
  | 'tool_call'
  | 'http_request'
  | 'channel_message'
  | 'condition'
  | 'delay'
  | 'transform'
  | 'await_human_approval'
  | 'fan_out'
  | 'for_each'
  | 'browser_action';

export interface AgentPromptConfig {
  prompt: string;
  allowed_connections?: ConnectionRef[];
  iteration_cap?: number;
  model_tier?: string | null;
}

/**
 * F3-4: BrowserAction node config — drives a CDP-attached browser
 * sub-agent (browser_observe / browser_act / browser_extract) against
 * the user's authenticated webview when `profile.type === 'reuse_authenticated'`.
 *
 * The desktop shell wires the live WebSocket transport in F3-5/F3-6;
 * until then production runs fail at session-open with a clear "live
 * CDP transport not yet wired in Phase 3.1" error.
 */
export type BrowserProfile =
  | { type: 'reuse_authenticated'; provider: string }
  | { type: 'ephemeral_isolated' }
  | { type: 'named_persistent'; name: string };

export interface BrowserActionConfig {
  goal: string;
  start_url?: string | null;
  profile?: BrowserProfile;
  iteration_cap?: number;
  allowed_hosts?: string[];
  output_schema?: unknown | null;
  allowed_connections?: ConnectionRef[];
}

export type NodeConfig =
  | ({ kind: 'agent_prompt' } & AgentPromptConfig)
  | ({ kind: 'browser_action' } & BrowserActionConfig);

export interface CanvasPosition {
  x: number;
  y: number;
}

export interface Node {
  id: NodeId;
  kind: NodeKind;
  config: NodeConfig;
  position?: CanvasPosition | null;
}

export interface Edge {
  from: NodeId;
  to: NodeId;
}

export type RunStatus = 'pending' | 'running' | 'succeeded' | 'failed' | 'cancelled' | 'timed_out';

export type OnErrorPolicy = 'halt' | 'continue';

export interface WorkflowSettings {
  timeout_secs: number;
  on_error: OnErrorPolicy;
}

// ── Entity ──────────────────────────────────────────────────────────────

export interface Workflow {
  id: WorkflowId;
  schema_version: number;
  name: string;
  description?: string | null;
  enabled: boolean;
  origin: WorkflowOrigin;
  health: WorkflowHealth;
  trigger: Trigger;
  nodes: Node[];
  edges: Edge[];
  settings: WorkflowSettings;
  created_at: string;
  updated_at: string;
  last_run_at?: string | null;
}

// ── List filter ─────────────────────────────────────────────────────────

export type HealthFilter = 'ready' | 'needs_connections' | 'last_run_failed' | 'session_expired';

export interface ListFilter {
  enabled?: boolean | null;
  health_state?: HealthFilter | null;
  search?: string | null;
}

// ── RPC requests ────────────────────────────────────────────────────────

export interface CreateWorkflowRequest {
  name: string;
  description?: string | null;
  trigger: Trigger;
  nodes: Node[];
  edges?: Edge[];
  settings?: WorkflowSettings | null;
  origin: WorkflowOrigin;
}

export interface WorkflowPatch {
  name?: string | null;
  description?: string | null;
  trigger?: Trigger | null;
  nodes?: Node[] | null;
  edges?: Edge[] | null;
  settings?: WorkflowSettings | null;
}

export interface UpdateWorkflowRequest {
  id: WorkflowId;
  patches: WorkflowPatch;
}

// ── Starter templates (F-5 backend, F-6 UI) ─────────────────────────────

/** Catalog response row returned by `workflows_list_starter_templates`. */
export interface StarterTemplateView {
  template_id: string;
  name: string;
  description: string;
  tags: string[];
  trigger_summary: string;
  required_connections: ConnectionRef[];
  missing_connections: ConnectionRef[];
  rationale_at_seed: string[];
  /**
   * Full template body as JSON. F-6's [Add] flow passes this back to
   * `workflows_create` unmodified — the server preserves every
   * forward-compat field the template carries (per-node `name`,
   * `on_error`, etc.) that Phase 1's typed `Workflow` shape doesn't
   * yet model.
   */
  raw_payload: Record<string, unknown>;
}

export interface ListStarterTemplatesRequest {
  /** Optional Phase override; defaults to the current Phase server-side. */
  phase?: number | null;
}

// ── Runs (F-8) ──────────────────────────────────────────────────────────

export type TriggerSource =
  | { type: 'cron' }
  | { type: 'manual'; initiator: string }
  | { type: 'webhook' }
  | { type: 'composio_event' }
  | { type: 'channel_message' };

export interface Run {
  id: RunId;
  workflow_id: WorkflowId;
  trigger_source: TriggerSource;
  status: RunStatus;
  started_at: string;
  completed_at?: string | null;
  error?: string | null;
  cancelled: boolean;
  /**
   * T-4 (Phase 2.5 Trust UX): structured classification of why this
   * run failed. `null` for Succeeded / Cancelled runs and for pre-T-4
   * rows. Drives `<RunOutcomeCard>`'s "Why this run failed" section
   * — each variant maps to a curated one-liner + fix-it action.
   */
  failure_reason?: FailureReason | null;
}

/**
 * T-4 (Phase 2.5 Trust UX): stable classification of a workflow run
 * failure. Mirror of Rust's `FailureReason` enum. The catalog is
 * deliberately small + stable; the UI renderer matches exhaustively
 * and unknown signals fall through to `{ kind: 'unknown' }`.
 */
export type FailureReason =
  | { kind: 'agent_narrated_without_acting'; narrative_chars: number }
  | { kind: 'composio_upstream_rejected'; tool: string; detail: string }
  | { kind: 'model_unavailable'; model_tried: string; valid_tiers: string[] }
  | { kind: 'llm_auth_failed'; provider: string }
  | { kind: 'connection_expired'; provider: string }
  | { kind: 'tool_slug_invalid'; slug: string }
  | { kind: 'unknown'; raw_detail: string };

export interface RunStep {
  id: RunStepId;
  run_id: RunId;
  node_id: NodeId;
  status: RunStatus;
  started_at: string;
  completed_at?: string | null;
  output_json?: string | null;
  error?: string | null;
  /**
   * T-1 (Phase 2.5 Trust UX): structured records of every real-world
   * side effect the agent triggered during this step (email sent,
   * message posted, file created). Empty for read-only steps and for
   * pre-T-1 rows (the migration defaults the column to `'[]'`).
   *
   * Rendered by `<RunOutcomeCard>` as plain-English rows with deep
   * links so the user can verify what actually happened without
   * grepping the SQLite DB or trusting the agent's narrative.
   */
  delivery_receipts?: DeliveryReceipt[];
}

/**
 * T-1: coarse classification of the side effect a delivery receipt
 * describes. Mirror of Rust's `SideEffectKind` enum. The catalog is
 * deliberately small + stable; unknown write tools fall through to
 * `{ kind: 'other', verb }` rather than hallucinating a richer
 * classification.
 */
export type SideEffectKind =
  | { kind: 'email_sent' }
  | { kind: 'message_posted'; provider: string }
  | { kind: 'file_created'; provider: string }
  | { kind: 'record_created'; provider: string }
  | { kind: 'record_updated'; provider: string }
  | { kind: 'calendar_event_created' }
  | { kind: 'issue_created'; provider: string }
  | { kind: 'social_post_created'; provider: string }
  | { kind: 'other'; verb: string };

/**
 * T-1 (Phase 2.5): structured evidence that a workflow run produced
 * a real-world side effect. Mirror of Rust's `DeliveryReceipt`.
 *
 * `recipient`, `message_id`, and `link` are best-effort: when the
 * provider's response doesn't carry the field (or extraction from
 * dispatch args fails) the receipt still surfaces with the field as
 * `null`. The UI shows whatever it has and doesn't invent placeholders
 * (OQ-T1-A).
 */
export interface DeliveryReceipt {
  tool: string;
  side_effect_kind: SideEffectKind;
  recipient?: string | null;
  message_id?: string | null;
  link?: string | null;
  at: string;
}

export interface RunWithSteps {
  run: Run;
  steps: RunStep[];
}

/**
 * Wire shape of the Rust `ManualInitiator` enum (workflows/types.rs).
 * `#[serde(tag = "type", rename_all = "snake_case")]` — every variant
 * serializes as `{ "type": <snake_case_name>, ...fields }`. The
 * frontend MUST send the discriminated object, not a bare string —
 * passing `"user"` as a string deserializes as `invalid type: string
 * "user", expected internally tagged enum ManualInitiator`.
 */
export type ManualInitiator =
  | { type: 'user' }
  | { type: 'agent'; session_id: string }
  | { type: 'catalog'; template_id: string };

// ── Proposals (F-11 / F-12 / F-14) ──────────────────────────────────────

export type Confidence = 'high' | 'medium' | 'low';

/**
 * Drafting-agent output for "build me a workflow that …". Renders via
 * `<WorkflowProposalPreview>`. Server-emitted; the frontend never
 * constructs these. Mirrors `WorkflowProposal` in
 * `src/openhuman/workflows/types.rs`.
 */
export interface WorkflowProposal {
  name: string;
  description: string;
  trigger: Trigger;
  nodes: Node[];
  edges: Edge[];
  settings: WorkflowSettings;
  required_connections: ConnectionRef[];
  rationale: string[];
  confidence: Confidence;
}

/**
 * Edit preview surfaced by `workflow_propose_update`. Renders via
 * `<WorkflowEditPreview>`. Carries the current + proposed workflow
 * shapes and a pre-computed `diff_summary` bullet list so the UI
 * doesn't have to diff client-side.
 */
export interface WorkflowEditProposal {
  workflow_id: WorkflowId;
  current: Workflow;
  proposed: Workflow;
  diff_summary: string[];
  rationale: string[];
}

/**
 * Delete preview surfaced by `workflow_propose_delete`. Renders via
 * `<WorkflowDeletePreview>`. `retention_days` is currently hard-coded
 * to 30 server-side (FR-1.3.4); declared here so the UI doesn't
 * redefine the literal.
 */
export interface WorkflowDeletePreview {
  workflow_id: WorkflowId;
  name: string;
  run_count: number;
  retention_days: number;
}

/** Which state mutation a `WorkflowStateProposal` previews. */
export type StateAction = 'enable' | 'disable' | 'run_now';

// ── Pre-flight validation (T-3, Phase 2.5 Trust UX) ────────────────────

/**
 * Result of running the pre-flight pipeline against a workflow proposal.
 * The UI inspects `passed` to gate the Save & Enable button; `checks`
 * carries every probe result so the user sees the complete picture.
 */
export interface PreflightReport {
  passed: boolean;
  checks: PreflightCheck[];
}

export interface PreflightCheck {
  kind: PreflightCheckKind;
  status: PreflightStatus;
  detail: string;
  fix_hint?: string | null;
}

export type PreflightCheckKind =
  | { kind: 'model_available'; tier: string }
  | { kind: 'connection_live'; connection: ConnectionRef }
  | { kind: 'aggregator_unreachable' };

export type PreflightStatus = 'pass' | 'warn' | 'fail';

/**
 * Enable / Disable / RunNow preview surfaced by `workflow_propose_*`.
 * Renders via `<WorkflowStatePreview>`. `enabled: false` means the
 * action is gated (e.g. `run_now` on a `NeedsConnections` workflow);
 * the UI renders the rationale but disables the Apply button.
 */
export interface WorkflowStateProposal {
  workflow_id: WorkflowId;
  action: StateAction;
  rationale: string[];
  enabled: boolean;
}
