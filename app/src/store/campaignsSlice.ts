/**
 * Redux slice for the Campaigns list view (Phase 4 / F4-11).
 *
 * Holds the campaign list returned by `campaigns_list`. Not persisted
 * — re-fetched on every visit to `/campaigns`. Optimistic updates on
 * pause/resume/archive so the UI flips the status pill before the
 * RPC round-trip completes.
 */
import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';

import { campaignsApi } from '../services/api/campaigns';
import type { Campaign, CampaignId, ListCampaignsFilter } from '../types/campaigns';

export type CampaignsLoadStatus = 'idle' | 'loading' | 'success' | 'error';

export interface CampaignsState {
  campaigns: Campaign[];
  loadStatus: CampaignsLoadStatus;
  error: string | null;
  /** Per-id pending flag for pause/resume/archive/delete. */
  pending: Record<CampaignId, boolean>;
}

const initialState: CampaignsState = {
  campaigns: [],
  loadStatus: 'idle',
  error: null,
  pending: {},
};

export const fetchCampaigns = createAsyncThunk<Campaign[], ListCampaignsFilter | undefined>(
  'campaigns/fetch',
  async (filter, { rejectWithValue }) => {
    try {
      return await campaignsApi.list(filter ?? {});
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

export const pauseCampaign = createAsyncThunk<Campaign, CampaignId>(
  'campaigns/pause',
  async (id, { rejectWithValue }) => {
    try {
      return await campaignsApi.pause(id);
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

export const resumeCampaign = createAsyncThunk<Campaign, CampaignId>(
  'campaigns/resume',
  async (id, { rejectWithValue }) => {
    try {
      return await campaignsApi.resume(id);
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

export const archiveCampaign = createAsyncThunk<Campaign, CampaignId>(
  'campaigns/archive',
  async (id, { rejectWithValue }) => {
    try {
      return await campaignsApi.archive(id);
    } catch (e) {
      return rejectWithValue(e instanceof Error ? e.message : String(e));
    }
  }
);

const slice = createSlice({
  name: 'campaigns',
  initialState,
  reducers: {
    /** Socket-driven: a fresh CampaignDefined / Updated event arrived
     *  — re-fetch so the list stays in sync without a manual reload.
     *  Caller dispatches `fetchCampaigns` after this; the reducer
     *  itself is a no-op marker so future subscribers can hook in. */
    campaignsEventReceived(state) {
      state.loadStatus = state.loadStatus === 'idle' ? 'loading' : state.loadStatus;
    },
    upsertCampaign(state, action: PayloadAction<Campaign>) {
      const idx = state.campaigns.findIndex(c => c.id === action.payload.id);
      if (idx >= 0) {
        state.campaigns[idx] = action.payload;
      } else {
        state.campaigns.unshift(action.payload);
      }
    },
    removeCampaign(state, action: PayloadAction<CampaignId>) {
      state.campaigns = state.campaigns.filter(c => c.id !== action.payload);
    },
  },
  extraReducers: builder => {
    builder
      .addCase(fetchCampaigns.pending, state => {
        state.loadStatus = 'loading';
        state.error = null;
      })
      .addCase(fetchCampaigns.fulfilled, (state, action) => {
        state.campaigns = action.payload;
        state.loadStatus = 'success';
      })
      .addCase(fetchCampaigns.rejected, (state, action) => {
        state.loadStatus = 'error';
        state.error = (action.payload as string) ?? action.error.message ?? 'unknown';
      })
      .addCase(pauseCampaign.pending, (state, action) => {
        state.pending[action.meta.arg] = true;
        // Optimistic: flip status now; RPC may overwrite on fulfilled.
        const c = state.campaigns.find(x => x.id === action.meta.arg);
        if (c && c.status === 'active') c.status = 'paused';
      })
      .addCase(pauseCampaign.fulfilled, (state, action) => {
        delete state.pending[action.payload.id];
        const idx = state.campaigns.findIndex(c => c.id === action.payload.id);
        if (idx >= 0) state.campaigns[idx] = action.payload;
      })
      .addCase(pauseCampaign.rejected, (state, action) => {
        delete state.pending[action.meta.arg];
      })
      .addCase(resumeCampaign.pending, (state, action) => {
        state.pending[action.meta.arg] = true;
        const c = state.campaigns.find(x => x.id === action.meta.arg);
        if (c && (c.status === 'paused' || c.status === 'draft')) c.status = 'active';
      })
      .addCase(resumeCampaign.fulfilled, (state, action) => {
        delete state.pending[action.payload.id];
        const idx = state.campaigns.findIndex(c => c.id === action.payload.id);
        if (idx >= 0) state.campaigns[idx] = action.payload;
      })
      .addCase(resumeCampaign.rejected, (state, action) => {
        delete state.pending[action.meta.arg];
      })
      .addCase(archiveCampaign.pending, (state, action) => {
        state.pending[action.meta.arg] = true;
      })
      .addCase(archiveCampaign.fulfilled, (state, action) => {
        delete state.pending[action.payload.id];
        const idx = state.campaigns.findIndex(c => c.id === action.payload.id);
        if (idx >= 0) state.campaigns[idx] = action.payload;
      })
      .addCase(archiveCampaign.rejected, (state, action) => {
        delete state.pending[action.meta.arg];
      });
  },
});

export const { campaignsEventReceived, upsertCampaign, removeCampaign } = slice.actions;
export default slice.reducer;

// ── Selectors ───────────────────────────────────────────────────────

type RootSliceState = { campaigns: CampaignsState };

export const selectCampaigns = (s: RootSliceState) => s.campaigns.campaigns;
export const selectCampaignsLoadStatus = (s: RootSliceState) => s.campaigns.loadStatus;
export const selectCampaignsError = (s: RootSliceState) => s.campaigns.error;
export const selectCampaignPending = (id: CampaignId) => (s: RootSliceState) =>
  Boolean(s.campaigns.pending[id]);
