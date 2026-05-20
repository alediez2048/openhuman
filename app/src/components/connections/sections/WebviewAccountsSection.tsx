/**
 * Browser-account (webview-scraped) section of the Connections Hub.
 *
 * Tile-grid layout that matches every other section. Each tile is one
 * CEF-hosted provider; clicking a supported provider opens an inline
 * `<BrowserAccountConnectModal>` that hosts the live `<WebviewHost>` so
 * the user can sign in directly in the Hub.
 *
 * **What Browser Accounts are.** OpenHuman embeds a real Chromium browser
 * for providers without first-class APIs (or where the user wants the
 * agent to act as themselves). Distinct from API-based Composio
 * integrations — clarified in the section subtitle.
 */
import { useState } from 'react';

import type { AccountProvider } from '../../../types/accounts';
import type { ConnectionView } from '../../../types/connections';
import { webviewIcon } from '../connectorIcons';
import ConnectorTile from '../ConnectorTile';
import SectionHeader from '../SectionHeader';
import BrowserAccountConnectModal from './BrowserAccountConnectModal';

interface Props {
  items: ConnectionView[];
}

/** Per-provider description that tells the user what the browser account is for. */
const WEBVIEW_DESCRIPTIONS: Record<string, string> = {
  whatsapp: 'WhatsApp Web — DMs and groups',
  telegram: 'Telegram Web — DMs and channels',
  slack: 'Slack — workspaces and DMs',
  discord: 'Discord — servers and DMs',
  linkedin: 'LinkedIn — DMs and feed',
  twitter: 'X — DMs and posting',
  instagram: 'Instagram — DMs and profile',
  messenger: 'Messenger — DMs and group chats',
};

const PROBE_TO_PROVIDER: Record<string, AccountProvider | undefined> = {
  whatsapp: 'whatsapp',
  telegram: 'telegram',
  slack: 'slack',
  discord: 'discord',
  linkedin: 'linkedin',
  twitter: 'twitter',
  instagram: 'instagram',
  messenger: 'messenger',
};

function providerSlugOf(c: ConnectionView, fallbackIndex: number): string {
  return c.ref.type === 'webview' ? c.ref.provider : `unknown-${fallbackIndex}`;
}

export default function WebviewAccountsSection({ items }: Props) {
  const [openProvider, setOpenProvider] = useState<AccountProvider | null>(null);
  const connectedCount = items.filter(c => c.status.kind === 'connected').length;

  return (
    <section data-testid="connections-section-webview">
      <SectionHeader
        title="Browser Accounts"
        count={connectedCount}
        subtitle={`${items.length} available · embedded Chromium sessions (not the same as API-based Composio)`}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No browser accounts available.
        </div>
      ) : (
        <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-2">
          {items.map((c, i) => {
            const slug = providerSlugOf(c, i);
            const provider = PROBE_TO_PROVIDER[slug];
            const description = WEBVIEW_DESCRIPTIONS[slug] ?? 'CEF-hosted browser account.';
            return (
              <ConnectorTile
                key={`webview-${slug}`}
                name={c.display_name}
                icon={webviewIcon(slug)}
                status={c.status}
                onClick={provider ? () => setOpenProvider(provider) : undefined}
                disabled={!provider}
                title={
                  provider
                    ? `Click to sign in to ${c.display_name}\n${description}`
                    : 'Web sign-in not yet supported for this provider'
                }
                testId={`connection-card-webview-${slug}`}
              />
            );
          })}
        </div>
      )}

      {openProvider ? (
        <BrowserAccountConnectModal provider={openProvider} onClose={() => setOpenProvider(null)} />
      ) : null}
    </section>
  );
}
