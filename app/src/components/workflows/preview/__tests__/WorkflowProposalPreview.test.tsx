import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import enDict from '../../../../lib/i18n/en';
import { workflowsApi } from '../../../../services/api/workflows';
import type { Workflow, WorkflowProposal } from '../../../../types/workflows';
import { WorkflowProposalPreview } from '../WorkflowProposalPreview';

// Use the real en.ts dictionary so the component's
// `t(key).replace('{placeholder}', value)` substitutions land. Tests
// that need a specific surface key still match via the resulting
// English copy.
vi.mock('../../../../lib/i18n/I18nContext', () => ({
  useT: () => ({ t: (k: string) => (enDict as Record<string, string>)[k] ?? k }),
}));

vi.mock('../../../../services/api/workflows', () => ({
  workflowsApi: {
    create: vi.fn(),
    enable: vi.fn(),
    // T-3 (Phase 2.5 Trust UX): pre-flight runs before saveAndEnable.
    // Default to passed=true so existing happy-path tests still flow;
    // the T-3-blocking test below overrides this with passed=false.
    preflight: vi.fn().mockResolvedValue({ passed: true, checks: [] }),
  },
}));

const createMock = workflowsApi.create as unknown as ReturnType<typeof vi.fn>;
const enableMock = workflowsApi.enable as unknown as ReturnType<typeof vi.fn>;
const preflightMock = workflowsApi.preflight as unknown as ReturnType<typeof vi.fn>;

const baseProposal: WorkflowProposal = {
  name: 'Morning digest',
  description: 'Send me a 7am summary',
  trigger: { type: 'cron', expr: '0 7 * * *', tz: 'UTC' },
  nodes: [
    {
      id: 'n1',
      kind: 'agent_prompt',
      config: { kind: 'agent_prompt', prompt: 'do the thing', iteration_cap: 12 },
    },
  ],
  edges: [],
  settings: { timeout_secs: 300, on_error: 'halt' },
  required_connections: [],
  rationale: ['Triage email', 'Group by sender'],
  confidence: 'high',
};

const createdRow: Workflow = {
  id: 'wf-1',
  schema_version: 1,
  name: baseProposal.name,
  description: baseProposal.description,
  enabled: false,
  origin: { type: 'user_chat' },
  health: { type: 'ready' },
  trigger: baseProposal.trigger,
  nodes: baseProposal.nodes,
  edges: baseProposal.edges,
  settings: baseProposal.settings,
  created_at: '2026-05-21T00:00:00Z',
  updated_at: '2026-05-21T00:00:00Z',
};

beforeEach(() => {
  createMock.mockReset();
  enableMock.mockReset();
  preflightMock.mockReset();
  // T-3: default to a pass result so existing tests flow through.
  preflightMock.mockResolvedValue({ passed: true, checks: [] });
});

describe('<WorkflowProposalPreview> — pending state', () => {
  it('renders name, description, trigger, action buttons', () => {
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    expect(screen.getByText('Morning digest')).toBeInTheDocument();
    expect(screen.getByText('Send me a 7am summary')).toBeInTheDocument();
    expect(screen.getByText(/Every day at 7am/)).toBeInTheDocument();
    expect(screen.getByText('Discard')).toBeInTheDocument();
    expect(screen.getByText('Save (paused)')).toBeInTheDocument();
    expect(screen.getByText('Save & Enable')).toBeInTheDocument();
  });

  it('exposes the confidence dot via aria-label', () => {
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    expect(screen.getByLabelText(/Confidence: high/)).toBeInTheDocument();
  });
});

describe('<WorkflowProposalPreview> — save flow', () => {
  it('Save (paused) calls create exactly once and does NOT call enable', async () => {
    createMock.mockResolvedValueOnce(createdRow);
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save (paused)'));
    await waitFor(() => expect(createMock).toHaveBeenCalledTimes(1));
    expect(enableMock).not.toHaveBeenCalled();
  });

  it('Save & Enable calls create then enable in that order', async () => {
    createMock.mockResolvedValueOnce(createdRow);
    enableMock.mockResolvedValueOnce({ ...createdRow, enabled: true });
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save & Enable'));
    await waitFor(() => expect(enableMock).toHaveBeenCalledWith('wf-1'));
    expect(createMock).toHaveBeenCalledTimes(1);
    expect(enableMock).toHaveBeenCalledTimes(1);
  });

  it('transitions to saved (paused) stub on success', async () => {
    createMock.mockResolvedValueOnce(createdRow);
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save (paused)'));
    await waitFor(() => expect(screen.getByText(/Saved as/)).toBeInTheDocument());
  });

  it('transitions to saved (enabled) stub after Save & Enable', async () => {
    createMock.mockResolvedValueOnce(createdRow);
    enableMock.mockResolvedValueOnce({ ...createdRow, enabled: true });
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save & Enable'));
    await waitFor(() => expect(screen.getByText(/Saved & enabled/)).toBeInTheDocument());
  });

  it('transitions to error state on create failure and offers retry', async () => {
    createMock.mockRejectedValueOnce(new Error('network'));
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save (paused)'));
    // The error surface renders both an `alert` and an
    // `aria-live` announcement; assert on the role=alert one
    // for specificity.
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent(/Couldn’t save/));
    expect(screen.getByText('Retry')).toBeInTheDocument();
  });
});

describe('<WorkflowProposalPreview> — discard flow', () => {
  it('Discard transitions to discarded stub with Undo affordance', () => {
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Discard'));
    expect(screen.getByText(/Discarded — Undo/)).toBeInTheDocument();
    expect(screen.getByText('Undo')).toBeInTheDocument();
  });

  it('Undo returns to pending with action row visible again', () => {
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Discard'));
    fireEvent.click(screen.getByText('Undo'));
    expect(screen.getByText('Save (paused)')).toBeInTheDocument();
  });
});

describe('<WorkflowProposalPreview> — details disclosure', () => {
  it('toggles the details panel', () => {
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    expect(screen.queryByText('Rationale')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Show details'));
    expect(screen.getByText('Rationale')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Hide details'));
    expect(screen.queryByText('Rationale')).not.toBeInTheDocument();
  });

  it('with confidence === low, the rationale section is open by default inside the panel', () => {
    const lowProposal = { ...baseProposal, confidence: 'low' as const };
    render(<WorkflowProposalPreview proposal={lowProposal} />);
    fireEvent.click(screen.getByText('Show details'));
    // Rationale bullets render — confirming the radio defaults to
    // opening Rationale when confidence is low.
    expect(screen.getByText('Triage email')).toBeInTheDocument();
  });
});

describe('<WorkflowProposalPreview> — missing connections', () => {
  it('renders the missing-connections banner', () => {
    render(
      <WorkflowProposalPreview
        proposal={{
          ...baseProposal,
          required_connections: [{ type: 'composio', toolkit_id: 'gmail' }],
        }}
        missingConnections={[{ type: 'composio', toolkit_id: 'gmail' }]}
      />
    );
    expect(screen.getByRole('alert')).toBeInTheDocument();
    expect(screen.getByRole('alert').textContent).toContain('gmail');
  });

  it('disables Save & Enable when there are missing connections', () => {
    render(
      <WorkflowProposalPreview
        proposal={{
          ...baseProposal,
          required_connections: [{ type: 'composio', toolkit_id: 'gmail' }],
        }}
        missingConnections={[{ type: 'composio', toolkit_id: 'gmail' }]}
      />
    );
    const enableBtn = screen.getByText('Save & Enable');
    expect(enableBtn).toBeDisabled();
  });

  it('keeps Save (paused) enabled even with missing connections', () => {
    render(
      <WorkflowProposalPreview
        proposal={{
          ...baseProposal,
          required_connections: [{ type: 'composio', toolkit_id: 'gmail' }],
        }}
        missingConnections={[{ type: 'composio', toolkit_id: 'gmail' }]}
      />
    );
    expect(screen.getByText('Save (paused)')).not.toBeDisabled();
  });
});

describe('<WorkflowProposalPreview> — a11y', () => {
  it('sets aria-busy while saving and exposes the saving live-region', async () => {
    let resolve!: (w: Workflow) => void;
    createMock.mockImplementationOnce(
      () =>
        new Promise<Workflow>(r => {
          resolve = r;
        })
    );
    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save (paused)'));
    const region = screen.getByRole('region', { name: 'Morning digest' });
    expect(region).toHaveAttribute('aria-busy', 'true');
    resolve(createdRow);
  });
});

describe('<WorkflowProposalPreview> — T-3 pre-flight gating', () => {
  it('blocks Save & Enable when preflight returns passed=false', async () => {
    preflightMock.mockResolvedValue({
      passed: false,
      checks: [
        {
          kind: { kind: 'model_available', tier: 'claude-opus-4-7' },
          status: 'fail',
          detail:
            "Model `claude-opus-4-7` isn't in the OpenHuman backend's tier list.",
          fix_hint: 'Replace with one of: reasoning-v1, agentic-v1, …',
        },
      ],
    });
    createMock.mockResolvedValue(createdRow);
    enableMock.mockResolvedValue({ ...createdRow, enabled: true });

    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save & Enable'));

    // Banner renders with the failed-check detail + hint.
    await waitFor(() =>
      expect(screen.getByTestId('workflow-preflight-banner')).toBeInTheDocument()
    );
    expect(
      screen.getByText(/claude-opus-4-7.* isn't in the OpenHuman backend/i)
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Replace with one of: reasoning-v1, agentic-v1/i)
    ).toBeInTheDocument();
    // create + enable must NOT have been called — preflight blocked Save.
    expect(createMock).not.toHaveBeenCalled();
    expect(enableMock).not.toHaveBeenCalled();
  });

  it('Save (paused) bypasses preflight (lets user persist + fix later)', async () => {
    preflightMock.mockResolvedValue({
      passed: false,
      checks: [
        {
          kind: { kind: 'model_available', tier: 'claude-opus-4-7' },
          status: 'fail',
          detail: 'bad model',
          fix_hint: 'fix it',
        },
      ],
    });
    createMock.mockResolvedValue(createdRow);

    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save (paused)'));
    // SavedStub renders the workflow name in a "Saved as ..." line —
    // the absence of the proposal preview region is the signal that
    // we transitioned to the saved state.
    await waitFor(() =>
      expect(screen.queryByText('Save & Enable')).not.toBeInTheDocument()
    );
    expect(screen.getByText(/Saved as/)).toBeInTheDocument();
    // preflight should NOT have been called on the paused path.
    expect(preflightMock).not.toHaveBeenCalled();
    expect(createMock).toHaveBeenCalledTimes(1);
  });

  it('allows Save & Enable when preflight passes', async () => {
    preflightMock.mockResolvedValue({ passed: true, checks: [] });
    createMock.mockResolvedValue(createdRow);
    enableMock.mockResolvedValue({ ...createdRow, enabled: true });

    render(<WorkflowProposalPreview proposal={baseProposal} />);
    fireEvent.click(screen.getByText('Save & Enable'));
    await waitFor(() =>
      expect(screen.getByText(/Saved & enabled/)).toBeInTheDocument()
    );
    expect(preflightMock).toHaveBeenCalledTimes(1);
    expect(createMock).toHaveBeenCalledTimes(1);
    expect(enableMock).toHaveBeenCalledTimes(1);
  });
});
