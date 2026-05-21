/**
 * Vitest unit tests for `workflowsSlice` — thunks dispatch the right
 * pending/fulfilled/rejected sequence and reducers update state
 * correctly.
 */
import { configureStore } from '@reduxjs/toolkit';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { workflowsApi } from '../../services/api/workflows';
import type { Workflow } from '../../types/workflows';
import workflowsReducer, {
  deleteWorkflow,
  disableWorkflow,
  enableWorkflow,
  fetchWorkflows,
  setHideStarterSection,
} from '../workflowsSlice';

vi.mock('../../services/api/workflows', () => ({
  workflowsApi: {
    list: vi.fn(),
    get: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    delete: vi.fn(),
    enable: vi.fn(),
    disable: vi.fn(),
  },
}));

function makeStore() {
  return configureStore({ reducer: { workflows: workflowsReducer } });
}

function sampleWorkflow(over: Partial<Workflow> = {}): Workflow {
  return {
    id: 'wf-1',
    schema_version: 1,
    name: 'sample',
    description: null,
    enabled: false,
    origin: { type: 'user_chat' },
    health: { type: 'ready' },
    trigger: { type: 'manual' },
    nodes: [{ id: 'n1', kind: 'agent_prompt', config: { kind: 'agent_prompt', prompt: 'x' } }],
    edges: [],
    settings: { timeout_secs: 300, on_error: 'halt' },
    created_at: '2026-05-20T00:00:00Z',
    updated_at: '2026-05-20T00:00:00Z',
    last_run_at: null,
    ...over,
  };
}

describe('workflowsSlice', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('fetchWorkflows.fulfilled stores the returned workflows + flips loadStatus', async () => {
    const wf = sampleWorkflow();
    (workflowsApi.list as ReturnType<typeof vi.fn>).mockResolvedValueOnce([wf]);
    const store = makeStore();
    await store.dispatch(fetchWorkflows());
    const state = store.getState().workflows;
    expect(state.loadStatus).toBe('success');
    expect(state.workflows).toEqual([wf]);
    expect(state.error).toBeNull();
  });

  it('fetchWorkflows.rejected records the error', async () => {
    (workflowsApi.list as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('boom'));
    const store = makeStore();
    await store.dispatch(fetchWorkflows());
    const state = store.getState().workflows;
    expect(state.loadStatus).toBe('error');
    expect(state.error).toBe('boom');
  });

  it('enableWorkflow.fulfilled upserts the workflow row + clears the pending flag', async () => {
    const wf = sampleWorkflow({ enabled: false });
    const enabled = { ...wf, enabled: true };
    (workflowsApi.enable as ReturnType<typeof vi.fn>).mockResolvedValueOnce(enabled);
    const store = makeStore();
    await store.dispatch(enableWorkflow(wf.id));
    const state = store.getState().workflows;
    expect(state.workflows[0]).toEqual(enabled);
    expect(state.pending[wf.id]).toBe(false);
  });

  it('disableWorkflow.rejected leaves the error + clears the pending flag', async () => {
    (workflowsApi.disable as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('nope'));
    const store = makeStore();
    await store.dispatch(disableWorkflow('wf-1'));
    const state = store.getState().workflows;
    expect(state.error).toBe('nope');
    expect(state.pending['wf-1']).toBe(false);
  });

  it('deleteWorkflow.fulfilled drops the workflow from the list', async () => {
    const a = sampleWorkflow({ id: 'wf-a' });
    const b = sampleWorkflow({ id: 'wf-b' });
    (workflowsApi.list as ReturnType<typeof vi.fn>).mockResolvedValueOnce([a, b]);
    (workflowsApi.delete as ReturnType<typeof vi.fn>).mockResolvedValueOnce(true);
    const store = makeStore();
    await store.dispatch(fetchWorkflows());
    await store.dispatch(deleteWorkflow('wf-a'));
    const state = store.getState().workflows;
    expect(state.workflows.map(w => w.id)).toEqual(['wf-b']);
  });

  it('setHideStarterSection flips the persisted preference', () => {
    const store = makeStore();
    expect(store.getState().workflows.hideStarterSection).toBe(false);
    store.dispatch(setHideStarterSection(true));
    expect(store.getState().workflows.hideStarterSection).toBe(true);
  });
});
