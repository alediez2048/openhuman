/**
 * Chat-channels section of the Connections Hub.
 *
 * **P0-5 minimal:** renders cards from the aggregator output. Full reuse of
 * the legacy `app/src/pages/Channels.tsx` + the existing per-provider setup
 * modals is **follow-up P0-5b**.
 */
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

export default function ChannelsSection({ items }: Props) {
  return (
    <section id="channels" data-testid="connections-section-channels">
      <SectionHeader
        title="Chat Channels"
        count={items.length}
        subtitle="Slack, Discord, Telegram, WhatsApp, …"
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No chat channels connected yet.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => (
            <ConnectionCard
              key={`channel-${i}`}
              name={c.display_name}
              subtitle={c.mechanism_label}
              status={c.status}
              testId={`connection-card-channel-${i}`}
            />
          ))}
        </div>
      )}
    </section>
  );
}
