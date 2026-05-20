/**
 * Composio section of the Connections Hub.
 *
 * Renders one card per Composio connected account from the aggregator's
 * unified `ConnectionView[]`. Each row is enriched at render time via
 * `composioToolkitMeta(toolkit_id)` so we get the canonical display name
 * ("Google Calendar" rather than the raw `googlecalendar` slug) and the
 * branded logo badge (`logos.composio.dev`) — matching the legacy Skills
 * grid.
 *
 * Full reuse of the per-toolkit detail UI (OAuth flow, scope controls,
 * disconnect affordance) remains filed as **P0-5a**.
 */
import type { ConnectionView } from '../../../types/connections';
import { composioToolkitMeta } from '../../composio/toolkitMeta';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

function toolkitSlugOf(c: ConnectionView): string | null {
  return c.ref.type === 'composio' ? c.ref.toolkit_id : null;
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
          {items.map((c, i) => {
            const slug = toolkitSlugOf(c);
            // Fall back to the aggregator's display name when the ref shape
            // is unexpected (defense-in-depth — should be unreachable since
            // the Hub's byKind bucket only collects `ref.type === 'composio'`).
            if (!slug) {
              return (
                <ConnectionCard
                  key={`composio-fallback-${i}`}
                  name={c.display_name}
                  subtitle={c.mechanism_label}
                  status={c.status}
                  testId={`connection-card-composio-${i}`}
                />
              );
            }
            const meta = composioToolkitMeta(slug);
            return (
              <ConnectionCard
                key={`composio-${slug}-${i}`}
                name={meta.name}
                subtitle={c.mechanism_label}
                status={c.status}
                icon={meta.icon}
                testId={`connection-card-composio-${slug}`}
              />
            );
          })}
        </div>
      )}
    </section>
  );
}
