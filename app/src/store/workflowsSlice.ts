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
import type {
  CreateWorkflowRequest,
  ListFilter,
  StarterTemplateView,
  Workflow,
  WorkflowId,
} from '../types/workflows';

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
  // ── Starter templates (F-6) ───────────────────────────────────────────
  /** Catalog rows from `workflows_list_starter_templates`. */
  starterTemplates: StarterTemplateView[];
  starterLoadStatus: WorkflowsLoadStatus;
  starterError: string | null;
  /** Per-template_id pending flag for the [Add] / [Add & Enable] flow. */
  starterPending: Record<string, boolean>;
}

const initialState: WorkflowsState = {
  workflows: [],
  loadStatus: 'idle',
  error: null,
  pending: {},
  hideStarterSection: false,
  starterTemplates: [],
  starterLoadStatus: 'idle',
  starterError: null,
  starterPending: {},
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

// ── Starter templates (F-6) ──────────────────────────────────────────────

export const fetchStarterTemplates = createAsyncThunk<StarterTemplateView[], void>(
  'workflows/fetchStarterTemplates',
  async (_, { rejectWithValue }) => {
    try {
      return await workflowsApi.listStarterTemplates();
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

export interface AddStarterTemplateArg {
  template: StarterTemplateView;
  enableImmediately: boolean;
}

/**
 * Promote a starter template into the user's workflows.
 *
 * Pipeline:
 *   1. `workflows_create` with the template's `raw_payload` +
 *      `origin: { type: 'seed', template_id }` — backend stamps the
 *      stable provenance F-5's dedup query reads.
 *   2. If `enableImmediately`, `workflows_enable` on the new id.
 *   3. Re-fetch `workflows_list` (so the new row shows up in the
 *      list view) AND `workflows_list_starter_templates` (server-
 *      side dedup drops the just-added template from the catalog).
 *
 * Returns the created Workflow so the UI can navigate / focus the new row.
 */
export const addStarterTemplate = createAsyncThunk<
  Workflow,
  AddStarterTemplateArg,
  { rejectValue: string }
>(
  'workflows/addStarterTemplate',
  async ({ template, enableImmediately }, { dispatch, rejectWithValue }) => {
    try {
      // Build the CreateWorkflowRequest by EXPLICITLY picking the
      // fields the strict `#[serde(deny_unknown_fields)]` schema
      // accepts — never spread `raw_payload` whole. The template body
      // carries extras (template_id, min_phase, tags,
      // required_connections, rationale_at_seed) that the server
      // rejects with `unknown field`; picking by name keeps this
      // robust against a future F-5 template-shape addition.
      const payload = template.raw_payload as Record<string, unknown>;
      const createReq: CreateWorkflowRequest = {
        name: payload.name as CreateWorkflowRequest['name'],
        description:
          payload.description === undefined
            ? null
            : (payload.description as CreateWorkflowRequest['description']),
        trigger: payload.trigger as CreateWorkflowRequest['trigger'],
        nodes: payload.nodes as CreateWorkflowRequest['nodes'],
        edges: payload.edges === undefined ? [] : (payload.edges as CreateWorkflowRequest['edges']),
        settings:
          payload.settings === undefined
            ? null
            : (payload.settings as CreateWorkflowRequest['settings']),
        origin: { type: 'seed', template_id: template.template_id },
      };
      const created = await workflowsApi.create(createReq);

      let result = created;
      if (enableImmediately) {
        result = await workflowsApi.enable(created.id);
      }

      // Refresh both lists in parallel. The catalog server-dedups
      // against the new Seed origin, so the just-added template falls
      // out of `fetchStarterTemplates`'s next response.
      await Promise.all([
        dispatch(fetchWorkflows())
          .unwrap()
          .catch(() => undefined),
        dispatch(fetchStarterTemplates())
          .unwrap()
          .catch(() => undefined),
      ]);

      return result;
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

    // ── F-6 starter templates ───────────────────────────────────────
    builder
      .addCase(fetchStarterTemplates.pending, state => {
        state.starterLoadStatus = 'loading';
        state.starterError = null;
      })
      .addCase(
        fetchStarterTemplates.fulfilled,
        (state, action: PayloadAction<StarterTemplateView[]>) => {
          state.starterLoadStatus = 'success';
          state.starterTemplates = Array.isArray(action.payload) ? action.payload : [];
        }
      )
      .addCase(fetchStarterTemplates.rejected, (state, action) => {
        state.starterLoadStatus = 'error';
        state.starterError =
          (action.payload as string | undefined) ?? action.error.message ?? 'unknown error';
      });

    builder
      .addCase(addStarterTemplate.pending, (state, action) => {
        state.starterPending[action.meta.arg.template.template_id] = true;
      })
      .addCase(addStarterTemplate.fulfilled, (state, action) => {
        state.starterPending[action.meta.arg.template.template_id] = false;
        // fetchWorkflows + fetchStarterTemplates above already refreshed
        // the list / catalog — nothing else to do here.
        void action;
      })
      .addCase(addStarterTemplate.rejected, (state, action) => {
        state.starterPending[action.meta.arg.template.template_id] = false;
        state.starterError =
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

// ── F-6 starter selectors ──────────────────────────────────────────────
export const selectStarterTemplates = (s: { workflows: WorkflowsState }) =>
  s.workflows.starterTemplates;
export const selectStarterLoadStatus = (s: { workflows: WorkflowsState }) =>
  s.workflows.starterLoadStatus;
export const selectStarterError = (s: { workflows: WorkflowsState }) => s.workflows.starterError;
export const selectStarterPending = (templateId: string) => (s: { workflows: WorkflowsState }) =>
  Boolean(s.workflows.starterPending[templateId]);
