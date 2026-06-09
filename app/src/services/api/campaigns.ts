/**
 * RPC client for the Campaigns domain (Phase 4 / F4-3 + F4-8 + F4-9).
 *
 * Wraps `campaigns_*` + `approvals_*` via `callCoreRpc`. Same
 * envelope-unwrap rationale as `workflowsApi` — Rust controllers
 * use `RpcOutcome::single_log` which wraps the typed value in
 * `{ result, logs }` on the wire.
 */
import type {
  ApprovalEntry,
  Campaign,
  CampaignId,
  ListCampaignsFilter,
  ThrottleSnapshot,
} from '../../types/campaigns';
import { callCoreRpc } from '../coreRpcClient';

interface RpcOutcomeEnvelope<T> {
  result: T;
  logs?: string[];
}

function unwrap<T>(raw: T | RpcOutcomeEnvelope<T>): T {
  if (
    raw !== null &&
    typeof raw === 'object' &&
    'result' in (raw as object) &&
    'logs' in (raw as object) &&
    Array.isArray((raw as RpcOutcomeEnvelope<T>).logs)
  ) {
    return (raw as RpcOutcomeEnvelope<T>).result;
  }
  return raw as T;
}

export const campaignsApi = {
  list: async (filter: ListCampaignsFilter = {}): Promise<Campaign[]> => {
    const raw = await callCoreRpc<Campaign[] | RpcOutcomeEnvelope<Campaign[]>>({
      method: 'openhuman.campaigns_list',
      params: { filter },
    });
    return unwrap(raw);
  },

  get: async (id: CampaignId): Promise<Campaign | null> => {
    const raw = await callCoreRpc<Campaign | null | RpcOutcomeEnvelope<Campaign | null>>({
      method: 'openhuman.campaigns_get',
      params: { id },
    });
    return unwrap(raw);
  },

  pause: async (id: CampaignId): Promise<Campaign> => {
    const raw = await callCoreRpc<Campaign | RpcOutcomeEnvelope<Campaign>>({
      method: 'openhuman.campaigns_pause',
      params: { id },
    });
    return unwrap(raw);
  },

  resume: async (id: CampaignId): Promise<Campaign> => {
    const raw = await callCoreRpc<Campaign | RpcOutcomeEnvelope<Campaign>>({
      method: 'openhuman.campaigns_resume',
      params: { id },
    });
    return unwrap(raw);
  },

  archive: async (id: CampaignId): Promise<Campaign> => {
    const raw = await callCoreRpc<Campaign | RpcOutcomeEnvelope<Campaign>>({
      method: 'openhuman.campaigns_archive',
      params: { id },
    });
    return unwrap(raw);
  },

  windDown: async (id: CampaignId): Promise<Campaign> => {
    const raw = await callCoreRpc<Campaign | RpcOutcomeEnvelope<Campaign>>({
      method: 'openhuman.campaigns_wind_down',
      params: { id },
    });
    return unwrap(raw);
  },

  delete: async (id: CampaignId): Promise<boolean> => {
    const raw = await callCoreRpc<boolean | RpcOutcomeEnvelope<boolean>>({
      method: 'openhuman.campaigns_delete',
      params: { id },
    });
    return unwrap(raw);
  },

  throttleStatus: async (id: CampaignId): Promise<ThrottleSnapshot | null> => {
    const raw = await callCoreRpc<
      ThrottleSnapshot | null | RpcOutcomeEnvelope<ThrottleSnapshot | null>
    >({ method: 'openhuman.campaigns_throttle_status', params: { id } });
    return unwrap(raw);
  },

  // ── F4-9 approval queue ──────────────────────────────────────────

  listPendingApprovals: async (campaignId?: CampaignId): Promise<ApprovalEntry[]> => {
    const raw = await callCoreRpc<ApprovalEntry[] | RpcOutcomeEnvelope<ApprovalEntry[]>>({
      method: 'openhuman.campaigns_approvals_list_pending',
      params: { campaign_id: campaignId ?? null },
    });
    return unwrap(raw);
  },

  approveDraft: async (
    id: string,
    editedPayload?: unknown,
    decidedBy?: string
  ): Promise<ApprovalEntry> => {
    const raw = await callCoreRpc<ApprovalEntry | RpcOutcomeEnvelope<ApprovalEntry>>({
      method: 'openhuman.campaigns_approvals_approve',
      params: { id, edited_payload: editedPayload ?? null, decided_by: decidedBy ?? null },
    });
    return unwrap(raw);
  },

  rejectDraft: async (id: string, reason?: string, decidedBy?: string): Promise<ApprovalEntry> => {
    const raw = await callCoreRpc<ApprovalEntry | RpcOutcomeEnvelope<ApprovalEntry>>({
      method: 'openhuman.campaigns_approvals_reject',
      params: { id, reason: reason ?? null, decided_by: decidedBy ?? null },
    });
    return unwrap(raw);
  },
};
