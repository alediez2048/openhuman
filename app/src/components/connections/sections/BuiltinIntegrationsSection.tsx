/**
 * Built-in integrations section of the Connections Hub.
 *
 * Tile grid matching every other section. One tile per backend-proxied
 * agent integration (Twilio / Apify / Google Places / Parallel / Seltz /
 * Stock Prices). Status comes from session-token presence in
 * `collect_builtin` — `Connected` once the user has signed in to the
 * OpenHuman backend, `NotConnected` otherwise.
 *
 * Clicking a tile navigates to `/intelligence` for credential management
 * until per-account toggle/credential rotation ships (filed as **P0-6a**).
 */
import { useNavigate } from 'react-router-dom';

import type { ConnectionView } from '../../../types/connections';
import { builtinIcon } from '../connectorIcons';
import ConnectorTile from '../ConnectorTile';
import SectionHeader from '../SectionHeader';

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
  const navigate = useNavigate();
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
                onClick={() => navigate('/intelligence')}
                title={`${c.display_name} — ${description}`}
                testId={`connection-card-builtin-${id}`}
              />
            );
          })}
        </div>
      )}
    </section>
  );
}
