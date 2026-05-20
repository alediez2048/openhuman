/**
 * Chat-channels section of the Connections Hub.
 *
 * Renders one card per chat channel OpenHuman supports (Telegram, Discord,
 * Web, iMessage). Status comes from the aggregator (`connected_channel_slugs`
 * — same source the chat runtime uses). Clicking a card opens the existing
 * `<ChannelSetupModal>` for that channel; on close the Hub re-fetches
 * `connections_list` so the status badge updates.
 *
 * Channel definitions (auth modes, field specs, capabilities) are fetched
 * once via `channelConnectionsApi.listDefinitions()` and looked up by slug
 * at click time.
 *
 * Note: WhatsApp / LinkedIn / etc. are CEF webview accounts, surfaced
 * separately in `<WebviewAccountsSection>`.
 */
import { useEffect, useState } from 'react';

import { channelConnectionsApi } from '../../../services/api/channelConnectionsApi';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch } from '../../../store/hooks';
import type { ChannelDefinition } from '../../../types/channels';
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
  const [definitions, setDefinitions] = useState<Map<string, ChannelDefinition>>(new Map());
  const [openSlug, setOpenSlug] = useState<string | null>(null);

  // One-shot fetch of the channel definition catalog. Definitions are
  // process-stable on the Rust side; no need to poll.
  useEffect(() => {
    let mounted = true;
    void channelConnectionsApi
      .listDefinitions()
      .then(defs => {
        if (!mounted) return;
        const next = new Map<string, ChannelDefinition>();
        for (const d of defs) next.set(d.id, d);
        setDefinitions(next);
      })
      .catch(err => {
        console.warn('[connections] channels listDefinitions failed', err);
      });
    return () => {
      mounted = false;
    };
  }, []);

  const connectedCount = items.filter(c => c.status.kind === 'connected').length;
  const openDefinition = openSlug ? definitions.get(openSlug) : undefined;

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
            const def = definitions.get(slug);
            const clickable = def != null;
            return (
              <button
                key={`channel-${slug}`}
                type="button"
                disabled={!clickable}
                onClick={() => setOpenSlug(slug)}
                className="block w-full text-left disabled:cursor-default"
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
