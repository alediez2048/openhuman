/**
 * Composio section of the Connections Hub.
 *
 * **P0-5 minimal:** renders cards from the unified `ConnectionView` aggregator
 * output. Full reuse of the existing `app/src/components/composio/**`
 * detail UI is filed as **follow-up P0-5a** — that ticket extracts the OAuth
 * flow + per-toolkit deep-link affordances and wires them up here.
 */
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

export default function ComposioSection({ items }: Props) {
  return (
    <section data-testid="connections-section-composio">
      <SectionHeader
        title="Composio Integrations"
        count={items.length}
        subtitle="OAuth-based services brokered by Composio (~1000 services)"
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No Composio connections yet. Connect a service from chat or the existing Composio settings
          panel.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => (
            <ConnectionCard
              key={`composio-${i}`}
              name={c.display_name}
              subtitle={c.mechanism_label}
              status={c.status}
              testId={`connection-card-composio-${i}`}
            />
          ))}
        </div>
      )}
    </section>
  );
}
