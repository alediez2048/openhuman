/**
 * Tests for `connectionsApi` — asserts each method calls `callCoreRpc` with
 * the right method name + payload shape.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { connectionsApi } from '../connectionsApi';

// vi.hoisted ensures the mock fn exists before vi.mock executes (which is
// itself hoisted to the top of the module by the transformer).
const { callCoreRpcMock } = vi.hoisted(() => ({ callCoreRpcMock: vi.fn() }));
vi.mock('../../coreRpcClient', () => ({ callCoreRpc: callCoreRpcMock }));

describe('connectionsApi', () => {
  beforeEach(() => {
    callCoreRpcMock.mockReset();
    callCoreRpcMock.mockResolvedValue({});
  });

  it('list() calls openhuman.connections_list with the request payload', async () => {
    await connectionsApi.list({ search: 'gmail' });
    expect(callCoreRpcMock).toHaveBeenCalledWith({
      method: 'openhuman.connections_list',
      params: { search: 'gmail' },
    });
  });

  it('list() with no args sends an empty params payload', async () => {
    await connectionsApi.list();
    expect(callCoreRpcMock).toHaveBeenCalledWith({
      method: 'openhuman.connections_list',
      params: {},
    });
  });

  it('createGenericHttp() wraps the request in a `request` field', async () => {
    const req = {
      name: 'n8n',
      base_url: 'https://n8n.cloud',
      auth_kind: { kind: 'bearer' as const },
      auth_credential: { secret: 'token' },
      default_headers: [] as Array<[string, string]>,
    };
    await connectionsApi.createGenericHttp(req);
    expect(callCoreRpcMock).toHaveBeenCalledWith({
      method: 'openhuman.connections_generic_http_create',
      params: { request: req },
    });
  });

  it('updateGenericHttp() sends id + request together', async () => {
    await connectionsApi.updateGenericHttp('abc-123', { name: 'renamed' });
    expect(callCoreRpcMock).toHaveBeenCalledWith({
      method: 'openhuman.connections_generic_http_update',
      params: { id: 'abc-123', request: { name: 'renamed' } },
    });
  });

  it('deleteGenericHttp() sends id only', async () => {
    await connectionsApi.deleteGenericHttp('abc-123');
    expect(callCoreRpcMock).toHaveBeenCalledWith({
      method: 'openhuman.connections_generic_http_delete',
      params: { id: 'abc-123' },
    });
  });

  it('test() sends id only', async () => {
    await connectionsApi.test('abc-123');
    expect(callCoreRpcMock).toHaveBeenCalledWith({
      method: 'openhuman.connections_test',
      params: { id: 'abc-123' },
    });
  });
});
