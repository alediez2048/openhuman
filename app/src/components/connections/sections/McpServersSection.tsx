/**
 * MCP servers section of the Connections Hub.
 *
 * Surfaces every MCP server registered via `McpServerRegistry::from_config`
 * (P0-6) — including the auto-registered `gitbooks` legacy server and any
 * user-defined `[[mcp_client.servers]]` entries in OpenHuman config.
 *
 * Read-only in Phase 0: the registry has no in-process "restart" verb today
 * (HTTP clients are lazy, stdio is per-call) and the enabled-flag lives in
 * TOML config rather than a runtime store. Restart / enable / disable +
 * inline "Add MCP server" land in **P0-6b** once a first-class MCP lifecycle
 * RPC exists.
 *
 * See `Automations/Tickets/phase-0-connections-hub/P0-6.md` and
 * `src/openhuman/connections/aggregator.rs::collect_mcp`.
 */
import { useNavigate } from 'react-router-dom';

import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

function serverIdOf(item: ConnectionView, fallbackIndex: number): string {
  return item.ref.type === 'mcp' ? item.ref.server_id : `unknown-${fallbackIndex}`;
}

export default function McpServersSection({ items }: Props) {
  const navigate = useNavigate();
  const connectedCount = items.filter(c => c.status.kind === 'connected').length;

  return (
    <section data-testid="connections-section-mcp">
      <SectionHeader
        title="MCP Servers"
        count={connectedCount}
        subtitle={`${items.length} available · click to manage in intelligence settings`}
        // P0-6b: cta={<AddMcpButton />}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No MCP servers registered yet.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => {
            const id = serverIdOf(c, i);
            return (
              <button
                key={id}
                type="button"
                onClick={() => navigate('/intelligence')}
                className="block w-full text-left rounded-xl hover:bg-stone-50 dark:hover:bg-neutral-800 focus:outline-none focus:ring-2 focus:ring-primary-500 transition-colors"
                data-testid={`connection-card-mcp-${id}`}>
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
