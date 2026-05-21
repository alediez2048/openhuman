/**
 * RPC client for the Workflows domain (Phase 1 / F-2).
 *
 * Wraps `workflows_list` / `_get` / `_create` / `_update` / `_delete` /
 * `_enable` / `_disable` via `callCoreRpc`. Frontend code should always
 * go through this module — never `callCoreRpc` directly — so we have a
 * single audit point for the workflows surface.
 *
 * Envelope-unwrap rationale identical to `connectionsApi.ts`: the Rust
 * controllers use `RpcOutcome::single_log(...)` which wraps the typed
 * value in `{ result, logs }` when serialised. `unwrapRpcOutcome`
 * collapses both shapes so callers always get the typed value back.
 */
import type {
  CreateWorkflowRequest,
  ListFilter,
  ListStarterTemplatesRequest,
  StarterTemplateView,
  UpdateWorkflowRequest,
  Workflow,
  WorkflowId,
} from '../../types/workflows';
import { callCoreRpc } from '../coreRpcClient';

interface RpcOutcomeEnvelope<T> {
  result: T;
  logs?: string[];
}

function unwrapRpcOutcome<T>(raw: T | RpcOutcomeEnvelope<T>): T {
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

export const workflowsApi = {
  list: async (filter: ListFilter = {}): Promise<Workflow[]> => {
    const raw = await callCoreRpc<Workflow[] | RpcOutcomeEnvelope<Workflow[]>>({
      method: 'openhuman.workflows_list',
      params: { filter },
    });
    return unwrapRpcOutcome(raw);
  },

  get: async (id: WorkflowId): Promise<Workflow | null> => {
    const raw = await callCoreRpc<Workflow | null | RpcOutcomeEnvelope<Workflow | null>>({
      method: 'openhuman.workflows_get',
      params: { id },
    });
    return unwrapRpcOutcome(raw);
  },

  create: async (req: CreateWorkflowRequest): Promise<Workflow> => {
    const raw = await callCoreRpc<Workflow | RpcOutcomeEnvelope<Workflow>>({
      method: 'openhuman.workflows_create',
      params: { request: req },
    });
    return unwrapRpcOutcome(raw);
  },

  update: async (req: UpdateWorkflowRequest): Promise<Workflow> => {
    const raw = await callCoreRpc<Workflow | RpcOutcomeEnvelope<Workflow>>({
      method: 'openhuman.workflows_update',
      params: { request: req },
    });
    return unwrapRpcOutcome(raw);
  },

  delete: async (id: WorkflowId): Promise<boolean> => {
    const raw = await callCoreRpc<boolean | RpcOutcomeEnvelope<boolean>>({
      method: 'openhuman.workflows_delete',
      params: { id },
    });
    return unwrapRpcOutcome(raw);
  },

  enable: async (id: WorkflowId): Promise<Workflow> => {
    const raw = await callCoreRpc<Workflow | RpcOutcomeEnvelope<Workflow>>({
      method: 'openhuman.workflows_enable',
      params: { id },
    });
    return unwrapRpcOutcome(raw);
  },

  disable: async (id: WorkflowId): Promise<Workflow> => {
    const raw = await callCoreRpc<Workflow | RpcOutcomeEnvelope<Workflow>>({
      method: 'openhuman.workflows_disable',
      params: { id },
    });
    return unwrapRpcOutcome(raw);
  },

  /**
   * Read-only catalog query (F-5). Server filters by `min_phase`,
   * dedupes against the user's existing Seed{template_id} workflows,
   * and computes `missing_connections` for each surviving template.
   */
  listStarterTemplates: async (
    req: ListStarterTemplatesRequest = {}
  ): Promise<StarterTemplateView[]> => {
    const raw = await callCoreRpc<
      StarterTemplateView[] | RpcOutcomeEnvelope<StarterTemplateView[]>
    >({ method: 'openhuman.workflows_list_starter_templates', params: { request: req } });
    return unwrapRpcOutcome(raw);
  },
};
