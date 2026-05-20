/**
 * Browser-account (webview-scraped) section of the Connections Hub.
 *
 * Renders one card per CEF-hosted webview provider OpenHuman knows about.
 * Status comes from the cookie-store probe in
 * `webview_accounts::detect_webview_logins`.
 *
 * **What Browser Accounts are.** OpenHuman embeds a real Chromium browser
 * (CEF) for providers that don't have first-class APIs or where the user
 * wants the agent to act exactly as they would. Signing in here gives the
 * agent read/write access by interacting with the rendered page via CDP —
 * fundamentally different from Composio integrations which use OAuth APIs.
 *
 * Clicking a supported provider opens an inline `<BrowserAccountConnectModal>`
 * that hosts the live `<WebviewHost>` — the user signs in directly in the
 * Hub. Gmail and Google Messages are still surfaced read-only because they
 * lack a CEF-hosted account contract today (cookie probe only).
 */
import { useState } from 'react';

import type { AccountProvider } from '../../../types/accounts';
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';
import BrowserAccountConnectModal from './BrowserAccountConnectModal';

interface Props {
  items: ConnectionView[];
}

/** Per-provider description that tells the user what the browser account is for. */
const WEBVIEW_DESCRIPTIONS: Record<string, string> = {
  whatsapp: 'Read and send messages through WhatsApp Web — no Business API required.',
  telegram: 'Reach Telegram chats via the web client (DMs, channels, supergroups).',
  slack: 'Browse Slack workspaces, threads, and DMs as your signed-in user.',
  discord: 'Operate inside Discord servers and DMs through the web client.',
  linkedin: 'Use LinkedIn DMs, search, and feed actions that have no public API.',
  twitter: 'Send/receive DMs and post on X (Twitter) through the web client.',
  instagram: 'Reach Instagram DMs and profile actions the official API doesn’t expose.',
  messenger: 'Operate inside Facebook Messenger — DMs, group chats, reactions.',
};

/**
 * Cookie-probe slugs that map to a fully-supported `AccountProvider` (the
 * Tauri shell has a CEF webview registration with provider_url + allowed
 * hosts). Every probe slug should map to a provider — keep this aligned
 * with `PROVIDERS` in `src/openhuman/webview_accounts/ops.rs`.
 */
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
        subtitle={`${items.length} available · embedded Chromium sessions (not the same as API-based Composio integrations)`}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No browser accounts available.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => {
            const slug = providerSlugOf(c, i);
            const provider = PROBE_TO_PROVIDER[slug];
            const description = WEBVIEW_DESCRIPTIONS[slug] ?? 'CEF-hosted browser account.';
            const supported = provider != null;
            return (
              <button
                key={`webview-${slug}`}
                type="button"
                disabled={!supported}
                onClick={() => provider && setOpenProvider(provider)}
                className="block w-full text-left rounded-xl hover:bg-stone-50 dark:hover:bg-neutral-800 focus:outline-none focus:ring-2 focus:ring-primary-500 transition-colors disabled:cursor-default disabled:hover:bg-transparent"
                data-testid={`connection-card-webview-${slug}`}
                title={
                  supported
                    ? `Click to sign in to ${c.display_name}`
                    : 'Web sign-in not yet supported for this provider'
                }>
                <ConnectionCard name={c.display_name} subtitle={description} status={c.status} />
              </button>
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
