/**
 * MCP servers section of the Connections Hub.
 *
 * **P0-5: structural stub. "Add server" + per-server config land in P0-6.**
 */
import type { ConnectionView } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';

interface Props {
  items: ConnectionView[];
}

export default function McpServersSection({ items }: Props) {
  return (
    <section data-testid="connections-section-mcp">
      <SectionHeader
        title="MCP Servers"
        count={items.length}
        subtitle="Your own + community-built MCP tools"
        // P0-6: cta={<AddMcpButton />}
      />
      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No MCP servers registered yet.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => (
            <ConnectionCard
              key={`mcp-${i}`}
              name={c.display_name}
              subtitle={c.mechanism_label}
              status={c.status}
              testId={`connection-card-mcp-${i}`}
            />
          ))}
        </div>
      )}
    </section>
  );
}
