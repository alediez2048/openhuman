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
  | 'fan_out';

export interface AgentPromptConfig {
  prompt: string;
  allowed_connections?: ConnectionRef[];
  iteration_cap?: number;
  model_tier?: string | null;
}

export type NodeConfig = { kind: 'agent_prompt' } & AgentPromptConfig;

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
}

export interface RunStep {
  id: RunStepId;
  run_id: RunId;
  node_id: NodeId;
  status: RunStatus;
  started_at: string;
  completed_at?: string | null;
  output_json?: string | null;
  error?: string | null;
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
