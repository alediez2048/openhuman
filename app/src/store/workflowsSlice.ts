/**
 * Redux slice for the Workflows list view (Phase 1 / F-4).
 *
 * Holds the workflow list returned by `workflows_list`. The list itself
 * is NOT persisted — it's re-fetched on every visit to `/workflows`.
 * Only `hideStarterSection` is persisted (a user preference that
 * survives reloads; F-5 / F-6 surface the catalog toggle).
 */
import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';

import { workflowsApi } from '../services/api/workflows';
import type { ListFilter, Workflow, WorkflowId } from '../types/workflows';

export type WorkflowsLoadStatus = 'idle' | 'loading' | 'success' | 'error';

export interface WorkflowsState {
  workflows: Workflow[];
  loadStatus: WorkflowsLoadStatus;
  error: string | null;
  /** Per-id pending flag for enable/disable/delete actions. */
  pending: Record<WorkflowId, boolean>;
  /**
   * User preference to hide the Starter section (F-5/F-6 reads/writes
   * this). Persisted via redux-persist's `whitelist` in `store/index.ts`.
   */
  hideStarterSection: boolean;
}

const initialState: WorkflowsState = {
  workflows: [],
  loadStatus: 'idle',
  error: null,
  pending: {},
  hideStarterSection: false,
};

export const fetchWorkflows = createAsyncThunk<Workflow[], ListFilter | undefined>(
  'workflows/fetch',
  async (filter, { rejectWithValue }) => {
    try {
      return await workflowsApi.list(filter ?? {});
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

export const enableWorkflow = createAsyncThunk<Workflow, WorkflowId>(
  'workflows/enable',
  async (id, { rejectWithValue }) => {
    try {
      return await workflowsApi.enable(id);
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

export const disableWorkflow = createAsyncThunk<Workflow, WorkflowId>(
  'workflows/disable',
  async (id, { rejectWithValue }) => {
    try {
      return await workflowsApi.disable(id);
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

export const deleteWorkflow = createAsyncThunk<WorkflowId, WorkflowId>(
  'workflows/delete',
  async (id, { rejectWithValue }) => {
    try {
      await workflowsApi.delete(id);
      return id;
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

function upsertWorkflow(list: Workflow[], next: Workflow): Workflow[] {
  const idx = list.findIndex(w => w.id === next.id);
  if (idx === -1) return [next, ...list];
  const out = list.slice();
  out[idx] = next;
  return out;
}

const workflowsSlice = createSlice({
  name: 'workflows',
  initialState,
  reducers: {
    setHideStarterSection(state, action: PayloadAction<boolean>) {
      state.hideStarterSection = action.payload;
    },
    clearError(state) {
      state.error = null;
    },
  },
  extraReducers: builder => {
    builder
      .addCase(fetchWorkflows.pending, state => {
        state.loadStatus = 'loading';
        state.error = null;
      })
      .addCase(fetchWorkflows.fulfilled, (state, action: PayloadAction<Workflow[]>) => {
        state.workflows = Array.isArray(action.payload) ? action.payload : [];
        state.loadStatus = 'success';
      })
      .addCase(fetchWorkflows.rejected, (state, action) => {
        state.loadStatus = 'error';
        state.error =
          (action.payload as string | undefined) ?? action.error.message ?? 'unknown error';
      });

    for (const thunk of [enableWorkflow, disableWorkflow]) {
      builder
        .addCase(thunk.pending, (state, action) => {
          state.pending[action.meta.arg] = true;
        })
        .addCase(thunk.fulfilled, (state, action: PayloadAction<Workflow>) => {
          state.pending[action.payload.id] = false;
          state.workflows = upsertWorkflow(state.workflows, action.payload);
        })
        .addCase(thunk.rejected, (state, action) => {
          state.pending[action.meta.arg] = false;
          state.error =
            (action.payload as string | undefined) ?? action.error.message ?? 'unknown error';
        });
    }

    builder
      .addCase(deleteWorkflow.pending, (state, action) => {
        state.pending[action.meta.arg] = true;
      })
      .addCase(deleteWorkflow.fulfilled, (state, action: PayloadAction<WorkflowId>) => {
        state.pending[action.payload] = false;
        state.workflows = state.workflows.filter(w => w.id !== action.payload);
      })
      .addCase(deleteWorkflow.rejected, (state, action) => {
        state.pending[action.meta.arg] = false;
        state.error =
          (action.payload as string | undefined) ?? action.error.message ?? 'unknown error';
      });
  },
});

export const { setHideStarterSection, clearError } = workflowsSlice.actions;
export default workflowsSlice.reducer;

// Selectors
export const selectWorkflows = (s: { workflows: WorkflowsState }) => s.workflows.workflows;
export const selectWorkflowsLoadStatus = (s: { workflows: WorkflowsState }) =>
  s.workflows.loadStatus;
export const selectWorkflowsError = (s: { workflows: WorkflowsState }) => s.workflows.error;
export const selectHideStarterSection = (s: { workflows: WorkflowsState }) =>
  s.workflows.hideStarterSection;
export const selectWorkflowPending = (id: WorkflowId) => (s: { workflows: WorkflowsState }) =>
  Boolean(s.workflows.pending[id]);
