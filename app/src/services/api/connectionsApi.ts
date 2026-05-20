/**
 * RPC client for the Connections domain (Phase 0).
 *
 * Wraps `connections_list` (P0-2), `connections_generic_http_create/_update/
 * _delete` (P0-3), and `connections_test` (P0-3 stub) via `callCoreRpc`.
 * Frontend code should always go through this module — never `callCoreRpc`
 * directly — so we have a single audit point for the connections surface.
 */
import type {
  ConnectionsListRequest,
  ConnectionsListResponse,
  CreateGenericHttpRequest,
  GenericHttpConnection,
  GenericHttpConnectionId,
  TestProbeResult,
  UpdateGenericHttpRequest,
} from '../../types/connections';
import { callCoreRpc } from '../coreRpcClient';

export const connectionsApi = {
  list: (req: ConnectionsListRequest = {}): Promise<ConnectionsListResponse> =>
    callCoreRpc<ConnectionsListResponse>({ method: 'openhuman.connections_list', params: req }),

  createGenericHttp: (req: CreateGenericHttpRequest): Promise<GenericHttpConnection> =>
    callCoreRpc<GenericHttpConnection>({
      method: 'openhuman.connections_generic_http_create',
      params: { request: req },
    }),

  updateGenericHttp: (
    id: GenericHttpConnectionId,
    req: UpdateGenericHttpRequest
  ): Promise<GenericHttpConnection> =>
    callCoreRpc<GenericHttpConnection>({
      method: 'openhuman.connections_generic_http_update',
      params: { id, request: req },
    }),

  deleteGenericHttp: (id: GenericHttpConnectionId): Promise<boolean> =>
    callCoreRpc<boolean>({ method: 'openhuman.connections_generic_http_delete', params: { id } }),

  test: (id: GenericHttpConnectionId): Promise<TestProbeResult> =>
    callCoreRpc<TestProbeResult>({ method: 'openhuman.connections_test', params: { id } }),
};
