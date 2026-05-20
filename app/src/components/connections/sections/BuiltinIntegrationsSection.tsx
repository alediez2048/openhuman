/**
 * Built-in integrations section of the Connections Hub.
 *
 * Surfaces the six OpenHuman-backend-proxied agent integrations
 * (Twilio, Apify, Google Places, Parallel, Seltz, Stock Prices) wired in
 * via the `collect_builtin` aggregator collector (P0-6).
 *
 * Status is derived from session-token presence: `Connected` once the user
 * has signed in to the OpenHuman backend, `NotConnected` otherwise. There is
 * no local on/off toggle today — these integrations are gated by the backend's
 * per-account availability rather than a config flag. A per-account toggle +
 * inline credential rotation lands in **P0-6a** once the backend exposes the
 * matching account-management surface.
 *
 * See `Automations/Tickets/phase-0-connections-hub/P0-6.md` and
 * `src/openhuman/connections/aggregator.rs::collect_builtin`.
 */
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

/** Short human-readable subtitle per integration slug. */
const BUILTIN_DESCRIPTIONS: Record<string, string> = {
  twilio: 'SMS, voice calls, and phone-number lookups',
  apify: 'Run Apify actors for scraping and automation',
  google_places: 'Google Places search and details lookup',
  parallel: 'Parallel.ai research, search, and enrichment',
  seltz: 'Seltz business and contact search',
  stock_prices: 'Live quotes, options, FX, and crypto series',
};

function subtitleFor(item: ConnectionView): string {
  if (item.ref.type !== 'builtin') return item.mechanism_label;
  return BUILTIN_DESCRIPTIONS[item.ref.integration] ?? item.mechanism_label;
}

function integrationIdOf(item: ConnectionView, fallbackIndex: number): string {
  return item.ref.type === 'builtin' ? item.ref.integration : `unknown-${fallbackIndex}`;
}

export default function BuiltinIntegrationsSection({ items }: Props) {
  return (
    <section data-testid="connections-section-builtin">
      <SectionHeader
        title="Built-in Integrations"
        count={items.length}
        subtitle="Twilio, Apify, Google Places, Parallel, Seltz, Stock Prices"
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No built-in integrations available. (Sign in to OpenHuman to enable them.)
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => {
            const id = integrationIdOf(c, i);
            return (
              <ConnectionCard
                key={id}
                name={c.display_name}
                subtitle={subtitleFor(c)}
                status={c.status}
                testId={`connection-card-builtin-${id}`}
                // P0-6a: per-account toggle + credential-rotation action slot.
              />
            );
          })}
        </div>
      )}
    </section>
  );
}
