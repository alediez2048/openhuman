/**
 * Phase 4 campaign types — mirror of the Rust shapes in
 * `src/openhuman/campaigns/types.rs` + the approval-queue shapes in
 * `src/openhuman/campaigns/approval/types.rs`.
 *
 * Kept narrow on purpose: only what the `/campaigns` UI surface
 * consumes. Wire format is `#[serde(tag = "type"/"kind", rename_all
 * = "snake_case")]` for every enum, so the JSON shape mirrors the
 * Rust 1:1.
 */

export type CampaignId = string;

export type CampaignStatus = 'draft' | 'active' | 'paused' | 'wound_down' | 'archived';

export interface EntityRefGoogleSheet {
  type: 'google_sheet';
  spreadsheet_id: string;
  range: string;
}

export interface EntityRefAttio {
  type: 'attio';
  workspace_id: string;
  object_type: string;
}

export type EntityRef = EntityRefGoogleSheet | EntityRefAttio;

export type ThrottleWindow = 'per_day' | 'per_hour' | 'per_minute';

export interface Throttle {
  max_per_window: number;
  window: { type: ThrottleWindow };
}

export type ApprovalPolicyKind = 'draft_and_approve' | 'auto_reply' | 'notify' | 'read_only';

export interface ApprovalPolicy {
  kind: ApprovalPolicyKind;
}

export type OutcomeSpec =
  | { kind: 'count'; metric: string; target: number }
  | { kind: 'rate'; metric: string; target: number };

export interface Campaign {
  id: CampaignId;
  schema_version: number;
  name: string;
  description?: string | null;
  status: CampaignStatus;
  entity_binding: EntityRef;
  throttle?: Throttle | null;
  approval_policy: ApprovalPolicy;
  target_outcome?: OutcomeSpec | null;
  created_at: string;
  updated_at: string;
  last_run_at?: string | null;
}

export interface ListCampaignsFilter {
  status?: CampaignStatus;
  include_deleted?: boolean;
}

// ── F4-8 throttle snapshot ──────────────────────────────────────────

export interface ThrottleSnapshot {
  window_start: string;
  window: { type: ThrottleWindow };
  consumed: number;
  limit: number;
  remaining: number;
  next_window_at: string;
}

// ── F4-9 approval queue ─────────────────────────────────────────────

export type ApprovalStatus = 'pending' | 'approved' | 'rejected' | 'sent' | 'failed';

export interface ApprovalEntry {
  id: string;
  campaign_id: string;
  workflow_id: string;
  run_id: string;
  node_id: string;
  action_kind: string;
  target: string;
  payload: unknown;
  context?: unknown | null;
  status: ApprovalStatus;
  created_at: string;
  decided_at?: string | null;
  decided_by?: string | null;
  error?: string | null;
}
