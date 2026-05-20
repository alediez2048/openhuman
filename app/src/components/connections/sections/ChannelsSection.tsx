/**
 * Chat-channels section of the Connections Hub.
 *
 * Renders one card per chat channel OpenHuman supports (Telegram, Discord,
 * Web, iMessage). Status comes from the aggregator (`connected_channel_slugs`
 * — same source the chat runtime uses). Clicking a card opens the existing
 * `<ChannelSetupModal>` for that channel; on close the Hub re-fetches
 * `connections_list` so the status badge updates.
 *
 * Channel definitions come from `useChannelDefinitions()` which falls back
 * to the locally-bundled `FALLBACK_DEFINITIONS` when the RPC isn't yet
 * available — that way the cards are always clickable on first paint even
 * if the network round-trip is in flight.
 *
 * Note: WhatsApp / LinkedIn / etc. are CEF webview accounts, surfaced
 * separately in `<WebviewAccountsSection>`.
 */
import { useState } from 'react';

import { useChannelDefinitions } from '../../../hooks/useChannelDefinitions';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch } from '../../../store/hooks';
import type { ConnectionView } from '../../../types/connections';
import ChannelSetupModal from '../../channels/ChannelSetupModal';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

function channelSlugOf(c: ConnectionView, fallbackIndex: number): string {
  return c.ref.type === 'channel' ? c.ref.provider : `unknown-${fallbackIndex}`;
}

export default function ChannelsSection({ items }: Props) {
  const dispatch = useAppDispatch();
  const { definitions } = useChannelDefinitions();
  const [openSlug, setOpenSlug] = useState<string | null>(null);

  const connectedCount = items.filter(c => c.status.kind === 'connected').length;
  const openDefinition = openSlug ? definitions.find(d => d.id === openSlug) : undefined;

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
              <button
                key={`channel-${slug}`}
                type="button"
                onClick={() => setOpenSlug(slug)}
                className="block w-full text-left rounded-xl hover:bg-stone-50 dark:hover:bg-neutral-800 focus:outline-none focus:ring-2 focus:ring-primary-500 transition-colors"
                data-testid={`connection-card-channel-${slug}`}>
                <ConnectionCard
                  name={c.display_name}
                  subtitle={c.mechanism_label}
                  status={c.status}
                />
              </button>
            );
          })}
        </div>
      )}

      {openDefinition ? (
        <ChannelSetupModal
          definition={openDefinition}
          onClose={() => {
            setOpenSlug(null);
            void dispatch(fetchConnections());
          }}
        />
      ) : null}
    </section>
  );
}
