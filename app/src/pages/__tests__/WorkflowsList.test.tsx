/**
 * Vitest unit tests for the `/workflows` page.
 *
 * Stubs `workflowsApi.list` to drive the empty/non-empty branches and
 * asserts the F-5/F-6 insertion point (`starter-section-placeholder`)
 * is present in both states.
 */
import { configureStore } from '@reduxjs/toolkit';
import { render, screen, waitFor } from '@testing-library/react';
import { Provider } from 'react-redux';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { workflowsApi } from '../../services/api/workflows';
import workflowsReducer from '../../store/workflowsSlice';
import type { Workflow } from '../../types/workflows';
import WorkflowsList from '../Workflows/WorkflowsList';

vi.mock('../../services/api/workflows', () => ({
  workflowsApi: {
    list: vi.fn(),
    get: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    delete: vi.fn(),
    enable: vi.fn(),
    disable: vi.fn(),
  },
}));

const FIXTURE_WORKFLOWS: Workflow[] = [
  {
    id: 'wf-a',
    schema_version: 1,
    name: 'Founder daily brief',
    description: null,
    enabled: true,
    origin: { type: 'user_chat' },
    health: { type: 'ready' },
    trigger: { type: 'cron', expr: '0 8 * * 1-5', tz: null, active_hours: null },
    nodes: [{ id: 'n1', kind: 'agent_prompt', config: { kind: 'agent_prompt', prompt: 'x' } }],
    edges: [],
    settings: { timeout_secs: 300, on_error: 'halt' },
    created_at: '2026-05-20T00:00:00Z',
    updated_at: '2026-05-20T00:00:00Z',
    last_run_at: null,
  },
  {
    id: 'wf-b',
    schema_version: 1,
    name: 'Slack digest',
    description: null,
    enabled: false,
    origin: { type: 'user_chat' },
    health: { type: 'needs_connections', missing: [{ type: 'composio', toolkit_id: 'slack' }] },
    trigger: { type: 'manual' },
    nodes: [{ id: 'n1', kind: 'agent_prompt', config: { kind: 'agent_prompt', prompt: 'x' } }],
    edges: [],
    settings: { timeout_secs: 300, on_error: 'halt' },
    created_at: '2026-05-20T00:00:00Z',
    updated_at: '2026-05-20T00:00:00Z',
    last_run_at: null,
  },
];

function renderPage() {
  const store = configureStore({ reducer: { workflows: workflowsReducer } });
  return render(
    <Provider store={store}>
      <MemoryRouter initialEntries={['/workflows']}>
        <WorkflowsList />
      </MemoryRouter>
    </Provider>
  );
}

describe('<WorkflowsList>', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders both rows when workflows_list returns two workflows', async () => {
    (workflowsApi.list as ReturnType<typeof vi.fn>).mockResolvedValueOnce(FIXTURE_WORKFLOWS);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('workflow-card-wf-a')).toBeInTheDocument();
      expect(screen.getByTestId('workflow-card-wf-b')).toBeInTheDocument();
    });
    expect(screen.getByTestId('starter-section-placeholder')).toBeInTheDocument();
  });

  it('renders the empty state with the chat CTA when the list is empty', async () => {
    (workflowsApi.list as ReturnType<typeof vi.fn>).mockResolvedValueOnce([]);
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('workflows-empty-title')).toBeInTheDocument();
    });
    const cta = screen.getByTestId('workflows-empty-cta');
    expect(cta).toBeInTheDocument();
    expect(cta.textContent).toMatch(/Ask OpenHuman/i);
    expect(screen.getByTestId('starter-section-placeholder')).toBeInTheDocument();
  });

  it('shows an error banner + retry when workflows_list rejects', async () => {
    (workflowsApi.list as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error('boom'));
    renderPage();
    await waitFor(() => {
      expect(screen.getByTestId('workflows-list-retry')).toBeInTheDocument();
    });
  });
});
