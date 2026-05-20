/**
 * RPC client for the Connections domain (Phase 0).
 *
 * Wraps `connections_list` (P0-2), `connections_generic_http_create/_update/
 * _delete` (P0-3), and `connections_test` (P0-3 stub) via `callCoreRpc`.
 * Frontend code should always go through this module — never `callCoreRpc`
 * directly — so we have a single audit point for the connections surface.
 *
 * ## Envelope unwrapping
 *
 * The Rust controllers all publish a one-line `tracing::info` summary via
 * `RpcOutcome::single_log(...)`. When `into_cli_compatible_json()` sees logs
 * it wraps the typed value in `{ result, logs }`; with no logs it returns the
 * bare value. `unwrapRpcOutcome` collapses both shapes so callers always get
 * the typed value back.
 */
import type {
  ConnectionsListRequest,
  ConnectionsListResponse,
  CreateGenericHttpRequest,
  GenericHttpConnection,
  GenericHttpConnectionId,
  McpAddRequest,
  McpServerConfig,
  TestProbeResult,
  UpdateGenericHttpRequest,
} from '../../types/connections';
import { callCoreRpc } from '../coreRpcClient';

/** `RpcOutcome::into_cli_compatible_json()` envelope when logs were attached. */
interface RpcOutcomeEnvelope<T> {
  result: T;
  logs?: string[];
}

/**
 * Collapses the `{ result, logs }` envelope produced by
 * `RpcOutcome::single_log` on the Rust side. Bare values (RpcOutcome::new with
 * no logs) pass through unchanged.
 */
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

export const connectionsApi = {
  list: async (req: ConnectionsListRequest = {}): Promise<ConnectionsListResponse> => {
    const raw = await callCoreRpc<
      ConnectionsListResponse | RpcOutcomeEnvelope<ConnectionsListResponse>
    >({ method: 'openhuman.connections_list', params: req });
    return unwrapRpcOutcome(raw);
  },

  createGenericHttp: async (req: CreateGenericHttpRequest): Promise<GenericHttpConnection> => {
    const raw = await callCoreRpc<
      GenericHttpConnection | RpcOutcomeEnvelope<GenericHttpConnection>
    >({ method: 'openhuman.connections_generic_http_create', params: { request: req } });
    return unwrapRpcOutcome(raw);
  },

  updateGenericHttp: async (
    id: GenericHttpConnectionId,
    req: UpdateGenericHttpRequest
  ): Promise<GenericHttpConnection> => {
    const raw = await callCoreRpc<
      GenericHttpConnection | RpcOutcomeEnvelope<GenericHttpConnection>
    >({ method: 'openhuman.connections_generic_http_update', params: { id, request: req } });
    return unwrapRpcOutcome(raw);
  },

  deleteGenericHttp: async (id: GenericHttpConnectionId): Promise<boolean> => {
    const raw = await callCoreRpc<boolean | RpcOutcomeEnvelope<boolean>>({
      method: 'openhuman.connections_generic_http_delete',
      params: { id },
    });
    return unwrapRpcOutcome(raw);
  },

  test: async (id: GenericHttpConnectionId): Promise<TestProbeResult> => {
    const raw = await callCoreRpc<TestProbeResult | RpcOutcomeEnvelope<TestProbeResult>>({
      method: 'openhuman.connections_test',
      params: { id },
    });
    return unwrapRpcOutcome(raw);
  },

  getGenericHttp: async (id: GenericHttpConnectionId): Promise<GenericHttpConnection | null> => {
    const raw = await callCoreRpc<
      GenericHttpConnection | null | RpcOutcomeEnvelope<GenericHttpConnection | null>
    >({ method: 'openhuman.connections_generic_http_get', params: { id } });
    return unwrapRpcOutcome(raw);
  },

  mcpTest: async (serverId: string): Promise<TestProbeResult> => {
    const raw = await callCoreRpc<TestProbeResult | RpcOutcomeEnvelope<TestProbeResult>>({
      method: 'openhuman.connections_mcp_test',
      params: { server_id: serverId },
    });
    return unwrapRpcOutcome(raw);
  },

  mcpAdd: async (req: McpAddRequest): Promise<McpServerConfig> => {
    const raw = await callCoreRpc<McpServerConfig | RpcOutcomeEnvelope<McpServerConfig>>({
      method: 'openhuman.connections_mcp_add',
      params: { request: req },
    });
    return unwrapRpcOutcome(raw);
  },

  mcpRemove: async (name: string): Promise<boolean> => {
    const raw = await callCoreRpc<boolean | RpcOutcomeEnvelope<boolean>>({
      method: 'openhuman.connections_mcp_remove',
      params: { name },
    });
    return unwrapRpcOutcome(raw);
  },
};
