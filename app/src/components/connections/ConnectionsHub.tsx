/**
 * Connections Hub — the unified Phase 0 page.
 *
 * Calls `connections_list` on mount and distributes the unified
 * `ConnectionView[]` into 6 section components. Search + kind-filter chips
 * apply across all sections; both live in URL state (`?search=…&kind=…`).
 *
 * Visible at `/connections`. Hash `#channels` (via the legacy redirect from
 * P0-4) scrolls the `<ChannelsSection>` into view on first paint.
 *
 * See `Automations/systemsdesign.md §9.3` and `requirements.md §1.9`.
 */
import { useEffect, useMemo, useRef } from 'react';
import { useLocation, useSearchParams } from 'react-router-dom';

import { fetchConnections } from '../../store/connectionsSlice';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import {
  CONNECTION_KIND_ORDER,
  type ConnectionKind,
  type ConnectionView,
} from '../../types/connections';
import BuiltinIntegrationsSection from './sections/BuiltinIntegrationsSection';
import ChannelsSection from './sections/ChannelsSection';
import ComposioSection from './sections/ComposioSection';
import GenericHttpSection from './sections/GenericHttpSection';
import McpServersSection from './sections/McpServersSection';
import WebviewAccountsSection from './sections/WebviewAccountsSection';

const FILTER_CHIPS: { value: ConnectionKind | 'all'; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'composio', label: 'Composio' },
  { value: 'channel', label: 'Channels' },
  { value: 'webview', label: 'Browser' },
  { value: 'builtin', label: 'Built-in' },
  { value: 'mcp', label: 'MCP' },
  { value: 'generic_http', label: 'HTTP' },
];

function classify(c: ConnectionView): ConnectionKind {
  return c.ref.type;
}

export default function ConnectionsHub() {
  const dispatch = useAppDispatch();
  const { connections, loadStatus, error } = useAppSelector(s => s.connections);
  const [searchParams, setSearchParams] = useSearchParams();
  const location = useLocation();
  const channelsAnchorRef = useRef<HTMLDivElement | null>(null);

  // Initial fetch + simple refresh-on-route-visit.
  useEffect(() => {
    dispatch(fetchConnections());
  }, [dispatch]);

  // Background polling so delayed CEF cookie flushes (LinkedIn / WhatsApp /
  // etc.) get picked up even after the BrowserAccountConnectModal's own
  // post-close 2.5s grace period has expired. CEF batches cookie writes in
  // memory and the flush-to-SQLite cadence is non-deterministic (observed
  // 5–60s after sign-in) — without this, a "successfully signed in but Hub
  // still says Connect" race-loses indefinitely until the user navigates
  // away and back. 10s is a reasonable trade-off between freshness and
  // RPC churn (the aggregator is ~50ms locally).
  //
  // Also refetch on window focus so returning to the app after signing in
  // through some other path (e.g., opening LinkedIn in the system browser)
  // picks up state immediately.
  useEffect(() => {
    const POLL_MS = 10_000;
    const intervalId = window.setInterval(() => {
      void dispatch(fetchConnections());
    }, POLL_MS);
    const onFocus = () => {
      void dispatch(fetchConnections());
    };
    window.addEventListener('focus', onFocus);
    return () => {
      window.clearInterval(intervalId);
      window.removeEventListener('focus', onFocus);
    };
  }, [dispatch]);

  // Honor #channels hash from the /channels redirect (P0-4).
  useEffect(() => {
    if (location.hash !== '#channels') return;
    if (loadStatus !== 'success') return;
    const node = channelsAnchorRef.current;
    if (node) {
      node.scrollIntoView({ behavior: 'smooth', block: 'start' });
    }
  }, [location.hash, loadStatus]);

  const search = (searchParams.get('search') ?? '').toLowerCase();
  const kindParams = searchParams.getAll('kind');
  const activeKinds = useMemo<Set<ConnectionKind>>(() => {
    if (kindParams.length === 0) return new Set(CONNECTION_KIND_ORDER);
    return new Set(
      kindParams.filter(k =>
        CONNECTION_KIND_ORDER.includes(k as ConnectionKind)
      ) as ConnectionKind[]
    );
  }, [kindParams]);

  const filtered = useMemo(() => {
    return connections.filter(c => {
      if (!activeKinds.has(classify(c))) return false;
      if (search && !c.display_name.toLowerCase().includes(search)) return false;
      return true;
    });
  }, [connections, activeKinds, search]);

  const byKind = useMemo(() => {
    const out: Record<ConnectionKind, ConnectionView[]> = {
      composio: [],
      channel: [],
      webview: [],
      builtin: [],
      mcp: [],
      generic_http: [],
    };
    for (const c of filtered) out[classify(c)].push(c);
    return out;
  }, [filtered]);

  const onChipClick = (value: ConnectionKind | 'all') => {
    const next = new URLSearchParams(searchParams);
    next.delete('kind');
    if (value !== 'all') next.append('kind', value);
    setSearchParams(next, { replace: true });
  };

  const onSearchChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const next = new URLSearchParams(searchParams);
    const v = e.target.value;
    if (v) next.set('search', v);
    else next.delete('search');
    setSearchParams(next, { replace: true });
  };

  const totalCount = connections.length;
  const isActiveChip = (chip: ConnectionKind | 'all'): boolean => {
    if (chip === 'all') return kindParams.length === 0;
    return kindParams.includes(chip);
  };

  return (
    <div data-testid="connections-page-root" className="min-h-full p-4 pt-6 max-w-3xl mx-auto">
      <header className="mb-4 flex items-start justify-between gap-3">
        <div>
          <h1 className="text-2xl font-display font-bold text-stone-900 dark:text-neutral-100">
            Connections
          </h1>
          <p className="mt-1 text-sm text-stone-500 dark:text-neutral-400">
            Every connected service OpenHuman can use, in one place.{' '}
            {totalCount > 0 ? (
              <span className="text-stone-700 dark:text-neutral-300">{totalCount} connected</span>
            ) : null}
          </p>
        </div>
        <button
          type="button"
          onClick={() => dispatch(fetchConnections())}
          disabled={loadStatus === 'loading'}
          className="px-3 py-1.5 text-xs text-stone-700 dark:text-neutral-200 bg-white dark:bg-neutral-900 border border-stone-300 dark:border-neutral-700 rounded-lg shadow-subtle hover:bg-stone-50 dark:hover:bg-neutral-800 disabled:opacity-50"
          data-testid="connections-refresh-button"
          title="Refresh — useful if a browser sign-in hasn’t shown as connected yet (CEF flushes cookies asynchronously).">
          {loadStatus === 'loading' ? 'Refreshing…' : 'Refresh'}
        </button>
      </header>

      <div className="mb-4">
        <input
          type="search"
          placeholder="Search connections…"
          value={searchParams.get('search') ?? ''}
          onChange={onSearchChange}
          className="w-full px-3.5 py-2 text-sm bg-white dark:bg-neutral-900 border border-stone-300 dark:border-neutral-700 rounded-xl shadow-subtle focus:outline-none focus:ring-2 focus:ring-primary-500"
          data-testid="connections-search-input"
        />
      </div>

      <div className="mb-5 flex flex-wrap gap-2" data-testid="connections-filter-chips">
        {FILTER_CHIPS.map(chip => (
          <button
            key={chip.value}
            type="button"
            onClick={() => onChipClick(chip.value)}
            className={`px-3 py-1 text-xs rounded-full transition-colors ${
              isActiveChip(chip.value)
                ? 'bg-primary-500 text-white border border-primary-500'
                : 'bg-white dark:bg-neutral-900 text-stone-700 dark:text-neutral-300 border border-stone-300 dark:border-neutral-700 hover:bg-stone-50 dark:hover:bg-neutral-800'
            }`}
            data-testid={`connections-chip-${chip.value}`}>
            {chip.label}
          </button>
        ))}
      </div>

      {loadStatus === 'loading' && totalCount === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          Loading connections…
        </div>
      ) : null}

      {loadStatus === 'error' ? (
        <div className="mb-4 px-3.5 py-3 text-sm text-coral-700 bg-coral-50 border border-coral-200 rounded-xl">
          Couldn’t load connections: {error}
          <button
            type="button"
            onClick={() => dispatch(fetchConnections())}
            className="ml-3 underline text-coral-700 hover:text-coral-900">
            Retry
          </button>
        </div>
      ) : null}

      {loadStatus !== 'error' ? (
        <div className="space-y-4">
          {/* When a kind filter chip is active, only the sections matching
              that filter render. Otherwise (the "All" chip case) every
              section renders so a user always sees the full surface. */}
          {activeKinds.has('composio') ? <ComposioSection items={byKind.composio} /> : null}
          {activeKinds.has('channel') ? (
            <div ref={channelsAnchorRef}>
              <ChannelsSection items={byKind.channel} />
            </div>
          ) : null}
          {activeKinds.has('webview') ? <WebviewAccountsSection items={byKind.webview} /> : null}
          {activeKinds.has('builtin') ? (
            <BuiltinIntegrationsSection items={byKind.builtin} />
          ) : null}
          {activeKinds.has('mcp') ? <McpServersSection items={byKind.mcp} /> : null}
          {activeKinds.has('generic_http') ? (
            <GenericHttpSection items={byKind.generic_http} />
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
