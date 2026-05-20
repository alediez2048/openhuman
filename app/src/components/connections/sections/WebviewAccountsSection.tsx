/**
 * Browser-account (webview-scraped) section of the Connections Hub.
 *
 * Renders one card per CEF-hosted webview provider OpenHuman knows about
 * (Gmail, WhatsApp, Telegram, Slack, Discord, LinkedIn, Zoom, Google
 * Messages). Status comes from the cookie-store probe in
 * `webview_accounts::detect_webview_logins` — `Connected` when at least
 * one known session cookie is present, `NotConnected` otherwise.
 *
 * Clicking a card navigates to `/chat`, which is where the live webview
 * sidebar lives — that surface owns the bounds + lifecycle required to
 * spawn a CEF webview. Inline modal-style sign-in is filed as **P0-5c**.
 */
import { useNavigate } from 'react-router-dom';

import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

function providerSlugOf(c: ConnectionView, fallbackIndex: number): string {
  return c.ref.type === 'webview' ? c.ref.provider : `unknown-${fallbackIndex}`;
}

export default function WebviewAccountsSection({ items }: Props) {
  const navigate = useNavigate();
  const connectedCount = items.filter(c => c.status.kind === 'connected').length;

  return (
    <section data-testid="connections-section-webview">
      <SectionHeader
        title="Browser Accounts"
        count={connectedCount}
        subtitle={`${items.length} available · CEF-hosted login sessions · click to manage in chat`}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No browser accounts available.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => {
            const slug = providerSlugOf(c, i);
            return (
              <button
                key={`webview-${slug}`}
                type="button"
                onClick={() => navigate('/chat')}
                className="block w-full text-left rounded-xl hover:bg-stone-50 dark:hover:bg-neutral-800 focus:outline-none focus:ring-2 focus:ring-primary-500 transition-colors"
                data-testid={`connection-card-webview-${slug}`}>
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
    </section>
  );
}
