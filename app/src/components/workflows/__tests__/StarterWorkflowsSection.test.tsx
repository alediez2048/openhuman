/**
 * Vitest unit tests for `<StarterWorkflowsSection>` — fetch-on-mount,
 * empty-catalog message, and the hide-preference rules from FR-1.2.6.
 */
import { configureStore } from '@reduxjs/toolkit';
import { render, screen, waitFor } from '@testing-library/react';
import { Provider } from 'react-redux';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import { workflowsApi } from '../../../services/api/workflows';
import workflowsReducer, { setHideStarterSection } from '../../../store/workflowsSlice';
import type { StarterTemplateView, Workflow } from '../../../types/workflows';
import StarterWorkflowsSection from '../StarterWorkflowsSection';

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

function makeTemplate(id: string): StarterTemplateView {
  return {
    template_id: id,
    name: id,
    description: `description for ${id}`,
    tags: [],
    trigger_summary: 'Run on demand',
    required_connections: [],
    missing_connections: [],
    rationale_at_seed: [],
    raw_payload: {
      name: id,
      trigger: { type: 'manual' },
      nodes: [],
      edges: [],
      settings: { timeout_secs: 300, on_error: 'halt' },
    },
  };
}

function makeWorkflow(id: string): Workflow {
  return {
    id,
    schema_version: 1,
    name: id,
    description: null,
    enabled: false,
    origin: { type: 'user_chat' },
    health: { type: 'ready' },
    trigger: { type: 'manual' },
    nodes: [],
    edges: [],
    settings: { timeout_secs: 300, on_error: 'halt' },
    created_at: '2026-05-21T00:00:00Z',
    updated_at: '2026-05-21T00:00:00Z',
    last_run_at: null,
  };
}

function renderSection(opts: { workflows?: Workflow[]; hideStarterSection?: boolean }) {
  const store = configureStore({ reducer: { workflows: workflowsReducer } });
  if (opts.workflows && opts.workflows.length > 0) {
    // Hydrate the workflows list — the section consults it for the
    // FR-1.2.6 visibility rules.
    store.dispatch({
      type: 'workflows/fetch/fulfilled',
      payload: opts.workflows,
      meta: { arg: undefined, requestId: 'test', requestStatus: 'fulfilled' },
    });
  }
  if (opts.hideStarterSection) {
    store.dispatch(setHideStarterSection(true));
  }
  return {
    store,
    ...render(
      <Provider store={store}>
        <StarterWorkflowsSection />
      </Provider>
    ),
  };
}

describe('<StarterWorkflowsSection>', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders one card per template when the catalog returns four entries', async () => {
    const four = ['ru-1', 'ru-2', 'ru-3', 'ru-4'].map(makeTemplate);
    (workflowsApi.listStarterTemplates as ReturnType<typeof vi.fn>).mockResolvedValueOnce(four);
    renderSection({});
    await waitFor(() => {
      for (const t of four) {
        expect(screen.getByTestId(`starter-workflow-card-${t.template_id}`)).toBeInTheDocument();
      }
    });
  });

  it('renders the "all added" empty-catalog message when catalog is [] and workspace is NOT empty', async () => {
    (workflowsApi.listStarterTemplates as ReturnType<typeof vi.fn>).mockResolvedValueOnce([]);
    renderSection({ workflows: [makeWorkflow('wf-a')] });
    await waitFor(() => {
      expect(screen.getByTestId('workflows-starter-empty-catalog')).toBeInTheDocument();
    });
  });

  it('respects hideStarterSection when the workspace has ≥1 workflow', async () => {
    (workflowsApi.listStarterTemplates as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
      makeTemplate('ru-1'),
    ]);
    renderSection({ workflows: [makeWorkflow('wf-a')], hideStarterSection: true });
    // Section renders nothing — give the fetch a tick to settle and
    // then assert the wrapper is absent.
    await new Promise(r => setTimeout(r, 50));
    expect(screen.queryByTestId('workflows-starter-section')).not.toBeInTheDocument();
  });

  it('ignores hideStarterSection on a fresh workspace (FR-1.2.6)', async () => {
    (workflowsApi.listStarterTemplates as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
      makeTemplate('ru-1'),
    ]);
    renderSection({ workflows: [], hideStarterSection: true });
    await waitFor(() => {
      expect(screen.getByTestId('workflows-starter-section')).toBeInTheDocument();
      expect(screen.getByTestId('starter-workflow-card-ru-1')).toBeInTheDocument();
    });
    // Hide link is suppressed on the empty workspace.
    expect(screen.queryByTestId('workflows-starter-hide')).not.toBeInTheDocument();
  });

  it('shows the Hide link when the workspace is non-empty', async () => {
    (workflowsApi.listStarterTemplates as ReturnType<typeof vi.fn>).mockResolvedValueOnce([
      makeTemplate('ru-1'),
    ]);
    renderSection({ workflows: [makeWorkflow('wf-a')] });
    await waitFor(() => {
      expect(screen.getByTestId('workflows-starter-hide')).toBeInTheDocument();
    });
  });
});
