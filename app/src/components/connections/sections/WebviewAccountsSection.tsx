/**
 * Browser-account (webview-scraped) section of the Connections Hub.
 *
 * Renders one card per CEF-hosted webview provider OpenHuman knows about
 * (Gmail, WhatsApp, Telegram, Slack, Discord, LinkedIn, Zoom, Google
 * Messages). Status comes from the cookie-store probe in
 * `webview_accounts::detect_webview_logins` — `Connected` when at least
 * one known session cookie is present, `NotConnected` otherwise.
 *
 * Full reuse of the existing `webviewAccountService` for "Add account"
 * / "Re-login" is filed as **P0-5c** — for now the cards are read-only;
 * users still sign in from the dedicated CEF webview windows.
 */
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
  const connectedCount = items.filter(c => c.status.kind === 'connected').length;
  return (
    <section data-testid="connections-section-webview">
      <SectionHeader
        title="Browser Accounts"
        count={connectedCount}
        subtitle={`${items.length} available · CEF-hosted login sessions`}
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
              <ConnectionCard
                key={`webview-${slug}`}
                name={c.display_name}
                subtitle={c.mechanism_label}
                status={c.status}
                testId={`connection-card-webview-${slug}`}
              />
            );
          })}
        </div>
      )}
    </section>
  );
}
