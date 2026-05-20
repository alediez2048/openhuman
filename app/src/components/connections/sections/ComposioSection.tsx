/**
 * Composio section of the Connections Hub.
 *
 * Renders the **full Composio managed-auth catalog** (the ~118-entry
 * `KNOWN_COMPOSIO_TOOLKITS` list) so users can browse every toolkit
 * OpenHuman knows how to connect to — matching the legacy Skills grid.
 * Each row's status is derived from the unfiltered Composio connections
 * pulled from the connections slice; the catalog itself is static.
 *
 * Branded name + logo via `composioToolkitMeta(slug)`. Hub search
 * narrows visible cards by toolkit name (read directly from URL params
 * so the section can filter across the whole catalog, not just the
 * Hub-pre-filtered `items`).
 *
 * Full per-toolkit connect / disconnect / scope UI remains filed as
 * **P0-5a** — for now the catalog is read-only; users still authorize
 * from chat or the existing Composio settings panel.
 */
import { useMemo } from 'react';
import { useSearchParams } from 'react-router-dom';

import { useAppSelector } from '../../../store/hooks';
import type { ConnectionStatus, ConnectionView } from '../../../types/connections';
import { composioToolkitMeta, KNOWN_COMPOSIO_TOOLKITS } from '../../composio/toolkitMeta';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  /**
   * Hub-filtered Composio rows. Kept for prop compatibility / test
   * isolation, but the section computes its own catalog overlay from
   * the full unfiltered Redux state below so unconnected toolkits also
   * surface.
   */
  items: ConnectionView[];
}

interface CatalogRow {
  slug: string;
  name: string;
  status: ConnectionStatus;
  /** True when this slug has at least one ACTIVE/CONNECTED Composio account. */
  connected: boolean;
}

/** Build a lookup map: composio toolkit slug → first connected ConnectionView. */
function connectionsBySlug(connections: ConnectionView[]): Map<string, ConnectionView> {
  const map = new Map<string, ConnectionView>();
  for (const c of connections) {
    if (c.ref.type !== 'composio') continue;
    // First-wins: surfaces the first connected account for a toolkit. Multiple
    // accounts under one toolkit collapse to one row in the catalog view;
    // per-account fan-out is a P0-5a follow-up.
    if (!map.has(c.ref.toolkit_id)) {
      map.set(c.ref.toolkit_id, c);
    }
  }
  return map;
}

export default function ComposioSection({ items: _items }: Props) {
  // `items` is the Hub-filtered subset. For status lookup we want the
  // *full* connections so an unconnected catalog row never gets mislabeled
  // as connected just because the Hub's search hid an active row. Reading
  // the slice directly keeps the section authoritative for Composio.
  const allConnections = useAppSelector(s => s.connections.connections);
  const [searchParams] = useSearchParams();
  const search = (searchParams.get('search') ?? '').trim().toLowerCase();

  const rows = useMemo<CatalogRow[]>(() => {
    const byToolkit = connectionsBySlug(allConnections);
    const built: CatalogRow[] = KNOWN_COMPOSIO_TOOLKITS.map(slug => {
      const meta = composioToolkitMeta(slug);
      const connection = byToolkit.get(slug);
      return {
        slug,
        name: meta.name,
        status: connection ? connection.status : { kind: 'not_connected' },
        connected: connection != null,
      };
    });
    const filtered = search
      ? built.filter(r => r.name.toLowerCase().includes(search) || r.slug.includes(search))
      : built;
    // Connected toolkits float to the top; otherwise alphabetical.
    filtered.sort((a, b) => {
      if (a.connected !== b.connected) return a.connected ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    return filtered;
  }, [allConnections, search]);

  const connectedCount = rows.filter(r => r.connected).length;

  return (
    <section data-testid="connections-section-composio">
      <SectionHeader
        title="Composio Integrations"
        count={connectedCount}
        subtitle={`${rows.length} available · OAuth-brokered by Composio`}
      />
      {rows.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No Composio toolkits match the current search.
        </div>
      ) : (
        <div className="space-y-2">
          {rows.map(row => {
            const meta = composioToolkitMeta(row.slug);
            return (
              <ConnectionCard
                key={`composio-${row.slug}`}
                name={meta.name}
                subtitle={row.connected ? 'Composio' : 'Composio · not connected'}
                status={row.status}
                icon={meta.icon}
                testId={`connection-card-composio-${row.slug}`}
              />
            );
          })}
        </div>
      )}
    </section>
  );
}
