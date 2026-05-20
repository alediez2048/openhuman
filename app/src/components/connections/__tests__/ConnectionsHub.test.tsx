/**
 * Vitest unit tests for `<ConnectionsHub>`.
 *
 * Stubs out the API client + the connections slice's fetch thunk so we can
 * inject a fixed `connections` array and verify section rendering + filter
 * behavior without hitting any real RPC.
 */
import { configureStore } from '@reduxjs/toolkit';
import { render, screen } from '@testing-library/react';
import { Provider } from 'react-redux';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it, vi } from 'vitest';

import channelConnectionsReducer from '../../../store/channelConnectionsSlice';
import connectionsReducer from '../../../store/connectionsSlice';
import type { ConnectionView } from '../../../types/connections';
import ConnectionsHub from '../ConnectionsHub';

vi.mock('../../../services/api/connectionsApi', () => ({
  connectionsApi: {
    list: vi.fn().mockResolvedValue({ connections: [], generated_at: '2026-05-19T00:00:00Z' }),
    test: vi.fn(),
    createGenericHttp: vi.fn(),
    updateGenericHttp: vi.fn(),
    deleteGenericHttp: vi.fn(),
  },
}));

const FIXTURE_CONNECTIONS: ConnectionView[] = [
  {
    ref: { type: 'composio', toolkit_id: 'gmail', account_id: 'jad@example.com' },
    display_name: 'Gmail',
    status: { kind: 'connected' },
    last_used_at: null,
    mechanism_label: 'Composio',
  },
  {
    ref: { type: 'channel', provider: 'telegram', channel_id: '@jad' },
    display_name: 'Telegram',
    status: { kind: 'connected' },
    last_used_at: null,
    mechanism_label: 'Channel',
  },
  {
    ref: { type: 'generic_http', connection_id: 'http-abc' },
    display_name: 'my-zapier-hook',
    status: { kind: 'connected' },
    last_used_at: null,
    mechanism_label: 'Generic HTTP',
  },
];

function makeStore(prefilled: ConnectionView[]) {
  // ChannelsSection reaches into state.channelConnections via
  // useChannelDefinitions — include the slice with its default initial
  // state so the section renders without crashing.
  const store = configureStore({
    reducer: { connections: connectionsReducer, channelConnections: channelConnectionsReducer },
    preloadedState: {
      connections: {
        connections: prefilled,
        loadStatus: 'success' as const,
        error: null,
        lastFetchedAt: Date.now(),
      },
    },
  });
  return store;
}

function renderHub(initialEntries: string[] = ['/connections'], items = FIXTURE_CONNECTIONS) {
  const store = makeStore(items);
  return render(
    <Provider store={store}>
      <MemoryRouter initialEntries={initialEntries}>
        <ConnectionsHub />
      </MemoryRouter>
    </Provider>
  );
}

describe('<ConnectionsHub>', () => {
  it('renders all 6 section components', () => {
    renderHub();
    expect(screen.getByTestId('connections-section-composio')).toBeInTheDocument();
    expect(screen.getByTestId('connections-section-channels')).toBeInTheDocument();
    expect(screen.getByTestId('connections-section-webview')).toBeInTheDocument();
    expect(screen.getByTestId('connections-section-builtin')).toBeInTheDocument();
    expect(screen.getByTestId('connections-section-mcp')).toBeInTheDocument();
    expect(screen.getByTestId('connections-section-generic-http')).toBeInTheDocument();
  });

  it('renders fixture cards in the correct sections', () => {
    renderHub();
    expect(screen.getByText('Gmail')).toBeInTheDocument();
    expect(screen.getByText('Telegram')).toBeInTheDocument();
    expect(screen.getByText('my-zapier-hook')).toBeInTheDocument();
  });

  // Filter chip + search input tests are deferred — they require a stable
  // React Router v7 + URL-param round-trip in MemoryRouter that has timing
  // quirks under userEvent. The URL-state semantics work in manual testing
  // and via the static render path (initial entries with `?kind=` /
  // `?search=` query strings) below. Filed as P0-5d follow-up.

  it('respects a kind filter passed via initial URL params', () => {
    renderHub(['/connections?kind=composio']);
    expect(screen.getByText('Gmail')).toBeInTheDocument();
    expect(screen.queryByText('Telegram')).not.toBeInTheDocument();
    expect(screen.queryByText('my-zapier-hook')).not.toBeInTheDocument();
  });

  it('respects a search query passed via initial URL params', () => {
    renderHub(['/connections?search=zapier']);
    expect(screen.getByText('my-zapier-hook')).toBeInTheDocument();
    expect(screen.queryByText('Gmail')).not.toBeInTheDocument();
    expect(screen.queryByText('Telegram')).not.toBeInTheDocument();
  });

  it('shows the page-root testid for redirect tests to assert on', () => {
    renderHub();
    expect(screen.getByTestId('connections-page-root')).toBeInTheDocument();
  });
});
