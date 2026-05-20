/**
 * Redux slice for the Connections Hub (Phase 0 / P0-5).
 *
 * Holds the aggregated `Vec<ConnectionView>` from `connections_list`. Not
 * persisted — connection state is server-derived and recomputed on mount.
 * Filters live in URL state (read via `useSearchParams`), not here.
 */
import { createAsyncThunk, createSlice, type PayloadAction } from '@reduxjs/toolkit';

import { connectionsApi } from '../services/api/connectionsApi';
import type {
  ConnectionsListRequest,
  ConnectionsListResponse,
  ConnectionView,
} from '../types/connections';

export type ConnectionsLoadStatus = 'idle' | 'loading' | 'success' | 'error';

export interface ConnectionsState {
  connections: ConnectionView[];
  loadStatus: ConnectionsLoadStatus;
  error: string | null;
  lastFetchedAt: number | null;
}

const initialState: ConnectionsState = {
  connections: [],
  loadStatus: 'idle',
  error: null,
  lastFetchedAt: null,
};

export const fetchConnections = createAsyncThunk<
  ConnectionsListResponse,
  ConnectionsListRequest | undefined
>('connections/fetch', async (req, { rejectWithValue }) => {
  try {
    return await connectionsApi.list(req ?? {});
  } catch (e) {
    return rejectWithValue(e instanceof Error ? e.message : String(e));
  }
});

const connectionsSlice = createSlice({
  name: 'connections',
  initialState,
  reducers: {
    clearError(state) {
      state.error = null;
    },
  },
  extraReducers: builder => {
    builder
      .addCase(fetchConnections.pending, state => {
        state.loadStatus = 'loading';
        state.error = null;
      })
      .addCase(
        fetchConnections.fulfilled,
        (state, action: PayloadAction<ConnectionsListResponse>) => {
          // Defensive fallback: even though `connectionsApi.list` unwraps the
          // `{ result, logs }` envelope, an unexpected wire shape must not
          // crash the hub. Empty array keeps the UI in a renderable state.
          state.connections = Array.isArray(action.payload?.connections)
            ? action.payload.connections
            : [];
          state.loadStatus = 'success';
          state.lastFetchedAt = Date.now();
        }
      )
      .addCase(fetchConnections.rejected, (state, action) => {
        state.loadStatus = 'error';
        state.error =
          (action.payload as string | undefined) ?? action.error.message ?? 'unknown error';
      });
  },
});

export const { clearError } = connectionsSlice.actions;
export default connectionsSlice.reducer;
