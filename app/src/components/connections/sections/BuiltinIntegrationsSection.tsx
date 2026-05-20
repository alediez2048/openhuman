/**
 * Built-in integrations section of the Connections Hub.
 *
 * **P0-5: structural stub. Interactivity (enable/disable, scope controls)
 * lands in P0-6.**
 */
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
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
          {/* P0-6: render the in-config built-in integrations even when the
              aggregator returns 0 — disabled cards with "Enable" CTAs. */}
          No built-in integrations enabled yet.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => (
            <ConnectionCard
              key={`builtin-${i}`}
              name={c.display_name}
              subtitle={c.mechanism_label}
              status={c.status}
              testId={`connection-card-builtin-${i}`}
              // P0-6: action slot with enable/disable Toggle + scope dropdown.
            />
          ))}
        </div>
      )}
    </section>
  );
}
