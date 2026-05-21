import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import enDict from '../../../../lib/i18n/en';
import { workflowsApi } from '../../../../services/api/workflows';
import type { Workflow, WorkflowEditProposal } from '../../../../types/workflows';
import { WorkflowEditPreview } from '../WorkflowEditPreview';

vi.mock('../../../../lib/i18n/I18nContext', () => ({
  useT: () => ({ t: (k: string) => (enDict as Record<string, string>)[k] ?? k }),
}));
vi.mock('../../../../services/api/workflows', () => ({ workflowsApi: { update: vi.fn() } }));
const updateMock = workflowsApi.update as unknown as ReturnType<typeof vi.fn>;

const baseRow: Workflow = {
  id: 'wf-1',
  schema_version: 1,
  name: 'Morning digest',
  description: null,
  enabled: false,
  origin: { type: 'user_chat' },
  health: { type: 'ready' },
  trigger: { type: 'cron', expr: '0 8 * * 1-5' },
  nodes: [
    {
      id: 'n1',
      kind: 'agent_prompt',
      config: { kind: 'agent_prompt', prompt: 'do', iteration_cap: 12 },
    },
  ],
  edges: [],
  settings: { timeout_secs: 300, on_error: 'halt' },
  created_at: '2026-05-21T00:00:00Z',
  updated_at: '2026-05-21T00:00:00Z',
};

const proposal: WorkflowEditProposal = {
  workflow_id: baseRow.id,
  current: baseRow,
  proposed: { ...baseRow, trigger: { type: 'cron', expr: '0 9 * * 1-5' } },
  diff_summary: ['Changed cron schedule from `0 8 * * 1-5` to `0 9 * * 1-5`.'],
  rationale: [],
};

beforeEach(() => updateMock.mockReset());

describe('<WorkflowEditPreview>', () => {
  it('renders the diff bullets with a ± gutter for Changed entries', () => {
    render(<WorkflowEditPreview proposal={proposal} />);
    expect(screen.getByText(/Changed cron schedule/)).toBeInTheDocument();
  });

  it('renders Apply enabled when there is at least one diff bullet', () => {
    render(<WorkflowEditPreview proposal={proposal} />);
    expect(screen.getByText('Apply changes')).not.toBeDisabled();
  });

  it('Apply calls workflowsApi.update with the proposed shape as patches', async () => {
    updateMock.mockResolvedValueOnce(proposal.proposed);
    render(<WorkflowEditPreview proposal={proposal} />);
    fireEvent.click(screen.getByText('Apply changes'));
    await waitFor(() => expect(updateMock).toHaveBeenCalledTimes(1));
    expect(updateMock.mock.calls[0]![0]!.id).toBe('wf-1');
    expect(updateMock.mock.calls[0]![0]!.patches.trigger).toEqual({
      type: 'cron',
      expr: '0 9 * * 1-5',
    });
  });

  it('disables Apply when diff_summary is empty', () => {
    render(<WorkflowEditPreview proposal={{ ...proposal, diff_summary: [] }} />);
    expect(screen.getByText('Apply changes')).toBeDisabled();
    expect(screen.getByText('No changes proposed.')).toBeInTheDocument();
  });

  it('Cancel transitions to discarded stub', () => {
    render(<WorkflowEditPreview proposal={proposal} />);
    fireEvent.click(screen.getByText('Cancel'));
    expect(screen.getByText(/Discarded — Undo/)).toBeInTheDocument();
  });

  it('surfaces an alert on update failure', async () => {
    updateMock.mockRejectedValueOnce(new Error('boom'));
    render(<WorkflowEditPreview proposal={proposal} />);
    fireEvent.click(screen.getByText('Apply changes'));
    await waitFor(() => expect(screen.getByRole('alert')).toBeInTheDocument());
    expect(screen.getByRole('alert')).toHaveTextContent(/Couldn’t save/);
  });
});
