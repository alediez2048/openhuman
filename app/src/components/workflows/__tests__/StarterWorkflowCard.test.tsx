/**
 * Vitest unit tests for `<StarterWorkflowCard>` + the
 * `addStarterTemplate` thunk wiring.
 */
import { configureStore } from '@reduxjs/toolkit';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { Provider } from 'react-redux';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { workflowsApi } from '../../../services/api/workflows';
import workflowsReducer from '../../../store/workflowsSlice';
import type { StarterTemplateView, Workflow } from '../../../types/workflows';
import StarterWorkflowCard from '../StarterWorkflowCard';

vi.mock('../../../services/api/workflows', () => ({
  workflowsApi: {
    list: vi.fn(),
    get: vi.fn(),
    create: vi.fn(),
    update: vi.fn(),
    delete: vi.fn(),
    enable: vi.fn(),
    disable: vi.fn(),
    listStarterTemplates: vi.fn(),
  },
}));

function buildTemplate(over: Partial<StarterTemplateView> = {}): StarterTemplateView {
  return {
    template_id: 'ru-1-founder-morning-digest',
    name: 'Founder morning digest',
    description: 'Every weekday at 8am, scan Gmail / Linear / Slack and send a Telegram digest.',
    tags: ['productivity'],
    trigger_summary: '0 8 * * 1-5',
    required_connections: [
      { type: 'composio', toolkit_id: 'gmail' },
      { type: 'channel', provider: 'telegram', channel_id: '' },
    ],
    missing_connections: [{ type: 'channel', provider: 'telegram', channel_id: '' }],
    rationale_at_seed: ['Reads memory for the user voice.'],
    raw_payload: {
      name: 'Founder morning digest',
      trigger: { type: 'cron', expr: '0 8 * * 1-5', tz: null, active_hours: null },
      nodes: [],
      edges: [],
      settings: { timeout_secs: 600, on_error: 'halt' },
    },
    ...over,
  };
}

function buildCreatedWorkflow(template_id: string): Workflow {
  return {
    id: 'wf-new',
    schema_version: 1,
    name: 'Founder morning digest',
    description: null,
    enabled: false,
    origin: { type: 'seed', template_id },
    health: { type: 'ready' },
    trigger: { type: 'cron', expr: '0 8 * * 1-5', tz: null, active_hours: null },
    nodes: [],
    edges: [],
    settings: { timeout_secs: 600, on_error: 'halt' },
    created_at: '2026-05-21T00:00:00Z',
    updated_at: '2026-05-21T00:00:00Z',
    last_run_at: null,
  };
}

function renderCard(template: StarterTemplateView) {
  const store = configureStore({ reducer: { workflows: workflowsReducer } });
  return {
    store,
    ...render(
      <Provider store={store}>
        <StarterWorkflowCard template={template} />
      </Provider>
    ),
  };
}

describe('<StarterWorkflowCard>', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    (workflowsApi.list as ReturnType<typeof vi.fn>).mockResolvedValue([]);
    (workflowsApi.listStarterTemplates as ReturnType<typeof vi.fn>).mockResolvedValue([]);
  });

  it('renders name + description + trigger summary', () => {
    const t = buildTemplate();
    renderCard(t);
    expect(screen.getByText('Founder morning digest')).toBeInTheDocument();
    expect(screen.getByText(/scan Gmail/)).toBeInTheDocument();
    expect(screen.getByText(/0 8 \* \* 1-5/)).toBeInTheDocument();
  });

  it('marks missing-connection pills with amber ⚠ + Needs tooltip', () => {
    const t = buildTemplate();
    renderCard(t);
    const pills = screen.getByTestId(`starter-workflow-pills-${t.template_id}`);
    const missingPill = pills.querySelector('[data-missing="true"]');
    expect(missingPill).not.toBeNull();
    expect(missingPill?.textContent).toMatch(/⚠/);
    expect(missingPill?.getAttribute('title')).toMatch(/Needs/i);

    const presentPill = pills.querySelector('[data-missing="false"]');
    expect(presentPill).not.toBeNull();
    expect(presentPill?.textContent).not.toMatch(/⚠/);
  });

  it('[Add] click calls workflows.create exactly once with origin = Seed', async () => {
    const t = buildTemplate();
    (workflowsApi.create as ReturnType<typeof vi.fn>).mockResolvedValueOnce(
      buildCreatedWorkflow(t.template_id)
    );
    renderCard(t);
    fireEvent.click(screen.getByTestId(`starter-workflow-add-${t.template_id}`));
    await waitFor(() => {
      expect(workflowsApi.create).toHaveBeenCalledTimes(1);
    });
    const [req] = (workflowsApi.create as ReturnType<typeof vi.fn>).mock.calls[0];
    expect(req.origin).toEqual({ type: 'seed', template_id: t.template_id });
    expect(workflowsApi.enable).not.toHaveBeenCalled();
  });

  it('[Add & Enable] click calls create then enable in that order', async () => {
    const t = buildTemplate();
    const created = buildCreatedWorkflow(t.template_id);
    (workflowsApi.create as ReturnType<typeof vi.fn>).mockResolvedValueOnce(created);
    (workflowsApi.enable as ReturnType<typeof vi.fn>).mockResolvedValueOnce({
      ...created,
      enabled: true,
    });
    renderCard(t);
    fireEvent.click(screen.getByTestId(`starter-workflow-add-enable-${t.template_id}`));
    await waitFor(() => {
      expect(workflowsApi.create).toHaveBeenCalledTimes(1);
      expect(workflowsApi.enable).toHaveBeenCalledWith(created.id);
    });
    const createOrder = (workflowsApi.create as ReturnType<typeof vi.fn>).mock
      .invocationCallOrder[0];
    const enableOrder = (workflowsApi.enable as ReturnType<typeof vi.fn>).mock
      .invocationCallOrder[0];
    expect(createOrder).toBeLessThan(enableOrder);
  });

  it('disables both buttons while the action is pending', async () => {
    const t = buildTemplate();
    // Mock create to a never-resolving promise so the pending flag stays set.
    let resolve!: (w: Workflow) => void;
    (workflowsApi.create as ReturnType<typeof vi.fn>).mockReturnValueOnce(
      new Promise<Workflow>(r => {
        resolve = r;
      })
    );
    renderCard(t);
    fireEvent.click(screen.getByTestId(`starter-workflow-add-${t.template_id}`));
    await waitFor(() => {
      expect(screen.getByTestId(`starter-workflow-add-${t.template_id}`)).toBeDisabled();
      expect(screen.getByTestId(`starter-workflow-add-enable-${t.template_id}`)).toBeDisabled();
    });
    // Release the promise so the thunk completes; not strictly needed,
    // but avoids dangling timers when other tests run.
    resolve(buildCreatedWorkflow(t.template_id));
  });
});
