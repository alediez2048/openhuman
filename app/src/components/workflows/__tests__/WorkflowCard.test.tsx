/**
 * Vitest unit tests for `<WorkflowCard>` + the enable toggle wired
 * through the workflowsSlice + mocked `workflowsApi`.
 */
import { configureStore } from '@reduxjs/toolkit';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { Provider } from 'react-redux';
import { MemoryRouter } from 'react-router-dom';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { workflowsApi } from '../../../services/api/workflows';
import workflowsReducer from '../../../store/workflowsSlice';
import type { Workflow, WorkflowHealth } from '../../../types/workflows';
import WorkflowCard from '../WorkflowCard';

vi.mock('../../../services/api/workflows', () => ({
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

function buildWorkflow(over: Partial<Workflow> = {}): Workflow {
  return {
    id: 'wf-1',
    schema_version: 1,
    name: 'Morning brief',
    description: null,
    enabled: false,
    origin: { type: 'user_chat' },
    health: { type: 'ready' },
    trigger: { type: 'cron', expr: '0 8 * * 1-5', tz: 'America/Chicago', active_hours: null },
    nodes: [{ id: 'n1', kind: 'agent_prompt', config: { kind: 'agent_prompt', prompt: 'x' } }],
    edges: [],
    settings: { timeout_secs: 300, on_error: 'halt' },
    created_at: '2026-05-20T00:00:00Z',
    updated_at: '2026-05-20T00:00:00Z',
    last_run_at: null,
    ...over,
  };
}

function renderCard(workflow: Workflow) {
  const store = configureStore({ reducer: { workflows: workflowsReducer } });
  return {
    store,
    ...render(
      <Provider store={store}>
        <MemoryRouter>
          <WorkflowCard workflow={workflow} />
        </MemoryRouter>
      </Provider>
    ),
  };
}

describe('<WorkflowCard>', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('compact row shows name + trigger summary; expanded shows health badge + step count', () => {
    // Post-Linear-layout: the compact row carries name + trigger
    // summary inline. Health badge + step count moved into the
    // expanded body so the dense single-line view stays scannable
    // for users with 20+ workflows. Asserting both states pins
    // that contract.
    const wf = buildWorkflow();
    renderCard(wf);
    // Compact: name is the primary affordance; trigger summary
    // sits beside it (hidden on very small viewports via Tailwind's
    // `hidden sm:inline`, but jsdom doesn't apply media queries
    // so it's present in the DOM here).
    expect(screen.getByText('Morning brief')).toBeInTheDocument();
    expect(screen.getByText(/0 8 \* \* 1-5/)).toBeInTheDocument();

    // Expand → health badge + step count visible.
    fireEvent.click(screen.getByTestId(`workflow-card-toggle-details-${wf.id}`));
    expect(screen.getByTestId('workflow-health-badge-ready')).toBeInTheDocument();
    expect(screen.getByText(/1 step/i)).toBeInTheDocument();
  });

  it('toggle calls workflowsApi.enable on a Ready disabled workflow', async () => {
    const wf = buildWorkflow({ enabled: false, health: { type: 'ready' } });
    (workflowsApi.enable as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      ...wf,
      enabled: true,
    });
    renderCard(wf);
    const toggle = screen.getByTestId(`workflow-enable-toggle-${wf.id}`);
    fireEvent.click(toggle);
    // Thunk dispatches synchronously; await microtask drain.
    await Promise.resolve();
    expect(workflowsApi.enable).toHaveBeenCalledWith('wf-1');
  });

  it('toggle is aria-disabled when health.type !== ready', () => {
    const wf = buildWorkflow({
      enabled: false,
      health: {
        type: 'needs_connections',
        missing: [{ type: 'composio', toolkit_id: 'gmail' }],
      } as WorkflowHealth,
    });
    renderCard(wf);
    const toggle = screen.getByTestId(`workflow-enable-toggle-${wf.id}`);
    expect(toggle).toHaveAttribute('aria-disabled', 'true');
    expect(toggle).toBeDisabled();
    expect(toggle).toHaveAttribute('title', expect.stringContaining('gmail'));
  });

  it('toggle allows DISABLE on an already-enabled workflow even when health is unhealthy', async () => {
    const wf = buildWorkflow({
      enabled: true,
      health: {
        type: 'needs_connections',
        missing: [{ type: 'composio', toolkit_id: 'gmail' }],
      } as WorkflowHealth,
    });
    (workflowsApi.disable as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      ...wf,
      enabled: false,
    });
    renderCard(wf);
    const toggle = screen.getByTestId(`workflow-enable-toggle-${wf.id}`);
    expect(toggle).not.toBeDisabled();
    fireEvent.click(toggle);
    await Promise.resolve();
    expect(workflowsApi.disable).toHaveBeenCalledWith('wf-1');
  });

  it('overflow menu opens on click and shows Edit / Run now / Delete entries', () => {
    const wf = buildWorkflow();
    renderCard(wf);
    const trigger = screen.getByTestId(`workflow-card-overflow-${wf.id}`);
    fireEvent.click(trigger);
    const menu = screen.getByTestId(`workflow-card-menu-${wf.id}`);
    const items = within(menu).getAllByRole('menuitem');
    expect(items.map(i => i.textContent)).toEqual(['Edit', 'Run now', 'Delete']);
  });
});
