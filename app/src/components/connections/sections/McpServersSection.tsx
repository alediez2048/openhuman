/**
 * MCP servers section of the Connections Hub.
 *
 * Tile grid matching every other section. One tile per registered MCP
 * server (from `McpServerRegistry::from_config`) plus a `+ Add custom`
 * tile that opens the McpAddModal — no more redirect to /intelligence.
 */
import { useState } from 'react';
import { useNavigate } from 'react-router-dom';

import type { ConnectionView } from '../../../types/connections';
import { mcpIcon } from '../connectorIcons';
import ConnectorTile, { AddCustomTile } from '../ConnectorTile';
import SectionHeader from '../SectionHeader';
import McpAddModal from './McpAddModal';

interface Props {
  items: ConnectionView[];
}

function serverIdOf(item: ConnectionView, fallbackIndex: number): string {
  return item.ref.type === 'mcp' ? item.ref.server_id : `unknown-${fallbackIndex}`;
}

export default function McpServersSection({ items }: Props) {
  const navigate = useNavigate();
  const [addOpen, setAddOpen] = useState(false);
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
        <AddCustomTile
          label="MCP server"
          onClick={() => setAddOpen(true)}
          testId="connection-card-mcp-add"
        />
      </div>

      {addOpen ? <McpAddModal onClose={() => setAddOpen(false)} /> : null}
    </section>
  );
}
