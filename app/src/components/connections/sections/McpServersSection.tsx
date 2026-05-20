/**
 * MCP servers section of the Connections Hub.
 *
 * Tile grid matching every other section. One tile per registered MCP
 * server (from `McpServerRegistry::from_config`) plus a `+ Add custom`
 * tile that opens the McpAddModal (lands in Pass D).
 *
 * Featured community-vetted servers (Linear / Notion / GitHub / Postgres /
 * Filesystem / Brave / Memory) will populate alongside connected servers
 * once the Featured catalog lands in Pass D.
 */
import { useNavigate } from 'react-router-dom';

import type { ConnectionView } from '../../../types/connections';
import { mcpIcon } from '../connectorIcons';
import ConnectorTile, { AddCustomTile } from '../ConnectorTile';
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
        subtitle={`${items.length} available · custom tool surfaces the agent can call via the Model Context Protocol`}
      />
      <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-2">
        {items.map((c, i) => {
          const id = serverIdOf(c, i);
          return (
            <ConnectorTile
              key={id}
              name={c.display_name}
              icon={mcpIcon(id)}
              status={c.status}
              onClick={() => navigate('/intelligence')}
              title={`${c.display_name} — extends the agent's tool surface`}
              testId={`connection-card-mcp-${id}`}
            />
          );
        })}
        {/* Pass D will swap this navigate fallback for the actual McpAddModal. */}
        <AddCustomTile
          label="MCP server"
          onClick={() => navigate('/intelligence')}
          testId="connection-card-mcp-add"
        />
      </div>
    </section>
  );
}
