/**
 * `/connections` route entry point.
 *
 * Phase 0 / P0-5: replaces the legacy Skills page contents with the new
 * unified Connections Hub. The previous Composio/Skills UI lives at
 * `ConnectionsLegacy.tsx` and will be relocated into the appropriate Hub
 * sections in follow-ups P0-5a (Composio), P0-5b (Channels), P0-5c (Webview).
 *
 * See `Automations/Tickets/phase-0-connections-hub/P0-5.md`.
 */
import ConnectionsHub from '../components/connections/ConnectionsHub';

export default function Connections() {
  return <ConnectionsHub />;
}
