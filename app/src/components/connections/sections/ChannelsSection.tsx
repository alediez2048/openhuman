/**
 * Chat-channels section of the Connections Hub.
 *
 * Tile grid that matches every other section. One tile per chat channel
 * (Telegram / Discord / Web / iMessage); click opens the legacy
 * `<ChannelSetupModal>` with the matching `ChannelDefinition`.
 *
 * Channel definitions come from `useChannelDefinitions()` which falls back
 * to bundled `FALLBACK_DEFINITIONS` so the tiles are always clickable on
 * first paint even before the RPC round-trip completes.
 */
import { useState } from 'react';

import { useChannelDefinitions } from '../../../hooks/useChannelDefinitions';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch } from '../../../store/hooks';
import type { ConnectionView } from '../../../types/connections';
import ChannelSetupModal from '../../channels/ChannelSetupModal';
import { channelIcon } from '../connectorIcons';
import ConnectorTile from '../ConnectorTile';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

function channelSlugOf(c: ConnectionView, fallbackIndex: number): string {
  return c.ref.type === 'channel' ? c.ref.provider : `unknown-${fallbackIndex}`;
}

const CHANNEL_DESCRIPTIONS: Record<string, string> = {
  telegram: 'Managed-DM, bot tokens, or webhooks',
  discord: 'OAuth or bot token — DMs and threads',
  web: 'Embedded chat widget for your site',
  imessage: 'macOS AppleScript bridge',
};

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
        subtitle={`${items.length} available · native messaging integrations agents send/receive on`}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No chat channels available.
        </div>
      ) : (
        <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-2">
          {items.map((c, i) => {
            const slug = channelSlugOf(c, i);
            const description = CHANNEL_DESCRIPTIONS[slug] ?? c.mechanism_label;
            return (
              <ConnectorTile
                key={`channel-${slug}`}
                name={c.display_name}
                icon={channelIcon(slug)}
                status={c.status}
                verification={c.verification}
                requireVerification
                onClick={() => setOpenSlug(slug)}
                title={`${c.display_name} — ${description}`}
                testId={`connection-card-channel-${slug}`}
              />
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
