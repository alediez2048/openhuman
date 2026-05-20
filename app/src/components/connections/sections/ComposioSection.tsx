/**
 * Composio section of the Connections Hub.
 *
 * Visual style matches the legacy Skills "Integrations" grid:
 *   - Category filter chips (All / Chat / Productivity / Tools & Automation /
 *     Social / Platform) above a tight square-tile grid.
 *   - One tile per toolkit in the static `KNOWN_COMPOSIO_TOOLKITS` catalog
 *     (~118 managed-auth services). Connection state comes from the
 *     unfiltered Composio rows in the connections slice.
 *   - Each tile: branded logo on top, toolkit name, status pill below
 *     (Connected / Error / Not connected).
 *
 * The full per-toolkit connect/disconnect/scope UI remains filed as
 * **P0-5a** — tiles are read-only today; users authorize from chat or
 * the existing Composio settings panel.
 */
import { useMemo, useState } from 'react';
import { useSearchParams } from 'react-router-dom';

import { useComposioIntegrations } from '../../../lib/composio/hooks';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch, useAppSelector } from '../../../store/hooks';
import type { ConnectionStatus, ConnectionView } from '../../../types/connections';
import ComposioConnectModal from '../../composio/ComposioConnectModal';
import { composioToolkitMeta, KNOWN_COMPOSIO_TOOLKITS } from '../../composio/toolkitMeta';
import type { SkillCategory } from '../../skills/skillCategories';
import ConnectorTile from '../ConnectorTile';
import SectionHeader from '../SectionHeader';

interface Props {
  /**
   * Hub-filtered Composio rows. Retained for prop compatibility — the
   * section computes its own catalog overlay from unfiltered Redux state
   * below so unconnected toolkits also surface.
   */
  items: ConnectionView[];
}

/** Composio-relevant subset of the global SkillCategory enum. */
const COMPOSIO_CATEGORY_CHIPS: readonly (SkillCategory | 'All')[] = [
  'All',
  'Chat',
  'Productivity',
  'Tools & Automation',
  'Social',
  'Platform',
] as const;

interface CatalogRow {
  slug: string;
  name: string;
  category: SkillCategory;
  status: ConnectionStatus;
  connected: boolean;
}

function connectionsBySlug(connections: ConnectionView[]): Map<string, ConnectionView> {
  const map = new Map<string, ConnectionView>();
  for (const c of connections) {
    if (c.ref.type !== 'composio') continue;
    if (!map.has(c.ref.toolkit_id)) map.set(c.ref.toolkit_id, c);
  }
  return map;
}

function ToolkitTile({ row, onClick }: { row: CatalogRow; onClick: () => void }) {
  const meta = composioToolkitMeta(row.slug);
  return (
    <ConnectorTile
      name={meta.name}
      icon={meta.icon}
      status={row.status}
      onClick={onClick}
      title={`${meta.name} — Composio managed-auth toolkit`}
      testId={`connection-card-composio-${row.slug}`}
    />
  );
}

export default function ComposioSection({ items: _items }: Props) {
  const allConnections = useAppSelector(s => s.connections.connections);
  const dispatch = useAppDispatch();
  const [searchParams] = useSearchParams();
  const search = (searchParams.get('search') ?? '').trim().toLowerCase();
  const [activeCategory, setActiveCategory] = useState<SkillCategory | 'All'>('All');
  const [openSlug, setOpenSlug] = useState<string | null>(null);

  // ComposioConnectModal needs the live Composio connection object + a
  // refresh callback so the polling roundtrip flips Connected back to the
  // Hub. The existing `useComposioIntegrations` hook owns the polling
  // contract — reuse it instead of duplicating the polling state machine.
  const composio = useComposioIntegrations();
  const openConnection = openSlug
    ? composio.connectionByToolkit.get(openSlug.toLowerCase())
    : undefined;

  const rows = useMemo<CatalogRow[]>(() => {
    const byToolkit = connectionsBySlug(allConnections);
    const built: CatalogRow[] = KNOWN_COMPOSIO_TOOLKITS.map(slug => {
      const meta = composioToolkitMeta(slug);
      const connection = byToolkit.get(slug);
      return {
        slug,
        name: meta.name,
        category: meta.category,
        status: connection ? connection.status : { kind: 'not_connected' },
        connected: connection != null,
      };
    });

    const afterCategory =
      activeCategory === 'All' ? built : built.filter(r => r.category === activeCategory);
    const afterSearch = search
      ? afterCategory.filter(r => r.name.toLowerCase().includes(search) || r.slug.includes(search))
      : afterCategory;

    // Connected toolkits float to the top; otherwise alphabetical.
    afterSearch.sort((a, b) => {
      if (a.connected !== b.connected) return a.connected ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    return afterSearch;
  }, [allConnections, activeCategory, search]);

  const totalConnected = useMemo(() => connectionsBySlug(allConnections).size, [allConnections]);

  return (
    <section data-testid="connections-section-composio">
      <SectionHeader
        title="Composio Integrations"
        count={totalConnected}
        subtitle="Connected services give your agents access to the tools they need to perform tasks"
      />

      <div className="mb-3 flex flex-wrap gap-2" data-testid="composio-category-chips">
        {COMPOSIO_CATEGORY_CHIPS.map(chip => {
          const active = chip === activeCategory;
          return (
            <button
              key={chip}
              type="button"
              onClick={() => setActiveCategory(chip)}
              className={`px-3 py-1 text-xs rounded-full transition-colors ${
                active
                  ? 'bg-stone-900 text-white dark:bg-neutral-100 dark:text-neutral-900'
                  : 'bg-white dark:bg-neutral-900 text-stone-700 dark:text-neutral-300 border border-stone-300 dark:border-neutral-700 hover:bg-stone-50 dark:hover:bg-neutral-800'
              }`}
              data-testid={`composio-category-chip-${chip.toLowerCase().replace(/[^a-z0-9]+/g, '-')}`}>
              {chip}
            </button>
          );
        })}
      </div>

      {rows.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No Composio toolkits match the current filter.
        </div>
      ) : (
        <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-2">
          {rows.map(row => (
            <ToolkitTile
              key={`composio-tile-${row.slug}`}
              row={row}
              onClick={() => setOpenSlug(row.slug)}
            />
          ))}
        </div>
      )}

      {openSlug ? (
        <ComposioConnectModal
          toolkit={composioToolkitMeta(openSlug)}
          connection={openConnection}
          onChanged={() => {
            // Refresh both data sources: the Composio hook (so the modal's
            // local state flips immediately) and the aggregator (so the
            // Hub-level status badge re-renders).
            void composio.refresh();
            void dispatch(fetchConnections());
          }}
          onClose={() => setOpenSlug(null)}
        />
      ) : null}
    </section>
  );
}
