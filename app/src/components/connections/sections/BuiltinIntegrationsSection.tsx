/**
 * Built-in integrations section of the Connections Hub.
 *
 * Tile grid matching every other section. Click a tile → opens
 * `<BuiltinDetailModal>` with status + description + a deep-link to
 * `/intelligence`. There's no local toggle (P0-6a still deferred — the
 * backend hasn't exposed a per-account integration-enabled surface yet).
 */
import { useState } from 'react';

import type { ConnectionView } from '../../../types/connections';
import { builtinIcon } from '../connectorIcons';
import ConnectorTile from '../ConnectorTile';
import SectionHeader from '../SectionHeader';
import BuiltinDetailModal from './BuiltinDetailModal';

interface Props {
  items: ConnectionView[];
}

const BUILTIN_DESCRIPTIONS: Record<string, string> = {
  twilio: 'SMS, voice, phone lookups',
  apify: 'Run scraper actors',
  google_places: 'Places search and details',
  parallel: 'Research and enrichment',
  seltz: 'Business and contact search',
  stock_prices: 'Quotes, options, FX, crypto',
};

function integrationIdOf(item: ConnectionView, fallbackIndex: number): string {
  return item.ref.type === 'builtin' ? item.ref.integration : `unknown-${fallbackIndex}`;
}

export default function BuiltinIntegrationsSection({ items }: Props) {
  const [detail, setDetail] = useState<ConnectionView | null>(null);
  const connectedCount = items.filter(c => c.status.kind === 'connected').length;

  return (
    <section data-testid="connections-section-builtin">
      <SectionHeader
        title="Built-in Integrations"
        count={connectedCount}
        subtitle={`${items.length} available · backend-proxied agent tools you don’t need to authorize separately`}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No built-in integrations available. (Sign in to OpenHuman to enable them.)
        </div>
      ) : (
        <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-2">
          {items.map((c, i) => {
            const id = integrationIdOf(c, i);
            const description = BUILTIN_DESCRIPTIONS[id] ?? c.mechanism_label;
            return (
              <ConnectorTile
                key={id}
                name={c.display_name}
                icon={builtinIcon(id)}
                status={c.status}
                onClick={() => setDetail(c)}
                title={`${c.display_name} — ${description}`}
                testId={`connection-card-builtin-${id}`}
              />
            );
          })}
        </div>
      )}

      {detail ? (
        <BuiltinDetailModal
          integrationId={integrationIdOf(detail, 0)}
          displayName={detail.display_name}
          description={BUILTIN_DESCRIPTIONS[integrationIdOf(detail, 0)] ?? detail.mechanism_label}
          status={detail.status}
          onClose={() => setDetail(null)}
        />
      ) : null}
    </section>
  );
}
