/**
 * Browser-account (webview-scraped) section of the Connections Hub.
 *
 * **P0-5 minimal:** renders cards from the aggregator output. Full reuse of
 * the existing `webviewAccountService` for "Add account" / "Re-login" is
 * **follow-up P0-5c**.
 */
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

export default function WebviewAccountsSection({ items }: Props) {
  return (
    <section data-testid="connections-section-webview">
      <SectionHeader
        title="Browser Accounts"
        count={items.length}
        subtitle="CEF-hosted login sessions — LinkedIn, Twitter, WhatsApp, …"
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No browser accounts connected yet.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => (
            <ConnectionCard
              key={`webview-${i}`}
              name={c.display_name}
              subtitle={c.mechanism_label}
              status={c.status}
              testId={`connection-card-webview-${i}`}
            />
          ))}
        </div>
      )}
    </section>
  );
}
