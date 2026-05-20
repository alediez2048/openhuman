/**
 * Chat-channels section of the Connections Hub.
 *
 * Renders one card per chat channel OpenHuman supports (Telegram, Discord,
 * Web, iMessage) using the aggregator's `Channel` rows. Each card shows
 * `Connected` when the slug has live credentials and `Not connected`
 * otherwise — same source of truth (`connected_channel_slugs`) the chat
 * runtime uses.
 *
 * Note: WhatsApp / LinkedIn / etc. are CEF webview accounts, surfaced
 * separately in `<WebviewAccountsSection>`.
 *
 * Per-provider setup modals (the legacy Channels page UX) is filed as
 * **P0-5b** — for now the cards are read-only; users still configure
 * channels from chat or the existing Channels settings panel.
 */
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

function channelSlugOf(c: ConnectionView, fallbackIndex: number): string {
  return c.ref.type === 'channel' ? c.ref.provider : `unknown-${fallbackIndex}`;
}

export default function ChannelsSection({ items }: Props) {
  const connectedCount = items.filter(c => c.status.kind === 'connected').length;
  return (
    <section id="channels" data-testid="connections-section-channels">
      <SectionHeader
        title="Chat Channels"
        count={connectedCount}
        subtitle={`${items.length} available · Telegram, Discord, Web, iMessage`}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No chat channels available.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => {
            const slug = channelSlugOf(c, i);
            return (
              <ConnectionCard
                key={`channel-${slug}`}
                name={c.display_name}
                subtitle={c.mechanism_label}
                status={c.status}
                testId={`connection-card-channel-${slug}`}
              />
            );
          })}
        </div>
      )}
    </section>
  );
}
