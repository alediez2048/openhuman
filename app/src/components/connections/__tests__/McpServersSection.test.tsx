/**
 * Tests for `<McpServersSection>` (P0-6) — verifies the section renders one
 * card per fixture row, derives the testid from the MCP `server_id`, and
 * surfaces the empty-state copy when no items are passed.
 */
import { render, screen } from '@testing-library/react';
import { type ReactNode } from 'react';
import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';

import type { ConnectionView } from '../../../types/connections';
import McpServersSection from '../sections/McpServersSection';

function renderInRouter(ui: ReactNode) {
  return render(<MemoryRouter>{ui}</MemoryRouter>);
}

function makeMcp(serverId: string, displayName: string): ConnectionView {
  return {
    ref: { type: 'mcp', server_id: serverId, tool_name: null },
    display_name: displayName,
    status: { kind: 'connected' },
    last_used_at: null,
    mechanism_label: 'MCP',
  };
}

describe('<McpServersSection>', () => {
  it('renders the empty-state copy when items is empty', () => {
    renderInRouter(<McpServersSection items={[]} />);
    expect(screen.getByTestId('connections-section-mcp')).toBeInTheDocument();
    expect(screen.getByText(/No MCP servers registered yet/i)).toBeInTheDocument();
  });

  it('renders one card per fixture row with stable testids per server_id', () => {
    const items = [makeMcp('gitbooks', 'gitbooks'), makeMcp('my-custom', 'my-custom')];
    renderInRouter(<McpServersSection items={items} />);

    expect(screen.getByTestId('connection-card-mcp-gitbooks')).toBeInTheDocument();
    expect(screen.getByTestId('connection-card-mcp-my-custom')).toBeInTheDocument();
    expect(screen.getByText('gitbooks')).toBeInTheDocument();
    expect(screen.getByText('my-custom')).toBeInTheDocument();
  });

  it('surfaces the mechanism label as subtitle on each card', () => {
    renderInRouter(<McpServersSection items={[makeMcp('gitbooks', 'gitbooks')]} />);
    // The header reads "MCP Servers"; the card subtitle reads "· MCP".
    // Scope the assertion to the card so the header doesn't shadow the subtitle.
    const card = screen.getByTestId('connection-card-mcp-gitbooks');
    expect(card.textContent).toMatch(/MCP/);
  });
});
