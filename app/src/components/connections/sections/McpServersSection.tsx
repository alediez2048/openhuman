/**
 * MCP servers section of the Connections Hub.
 *
 * Tile grid matching every other section. Click a server tile → opens
 * `<McpManageModal>` with a Remove action. Click the `+ Add custom` tile
 * → opens `<McpAddModal>`.
 */
import { useState } from 'react';

import type { ConnectionView } from '../../../types/connections';
import { mcpIcon } from '../connectorIcons';
import ConnectorTile, { AddCustomTile } from '../ConnectorTile';
import SectionHeader from '../SectionHeader';
import McpAddModal from './McpAddModal';
import McpManageModal from './McpManageModal';

interface Props {
  items: ConnectionView[];
}

function serverIdOf(item: ConnectionView, fallbackIndex: number): string {
  return item.ref.type === 'mcp' ? item.ref.server_id : `unknown-${fallbackIndex}`;
}

export default function McpServersSection({ items }: Props) {
  const [addOpen, setAddOpen] = useState(false);
  const [managing, setManaging] = useState<ConnectionView | null>(null);
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
              verification={c.verification}
              requireVerification
              onClick={() => setManaging(c)}
              title={`${c.display_name} — click to manage`}
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
      {managing ? (
        <McpManageModal
          serverId={serverIdOf(managing, 0)}
          displayName={managing.display_name}
          status={managing.status}
          onClose={() => setManaging(null)}
        />
      ) : null}
    </section>
  );
}
