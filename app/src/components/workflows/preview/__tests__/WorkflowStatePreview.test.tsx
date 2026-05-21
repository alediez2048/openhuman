import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import enDict from '../../../../lib/i18n/en';
import { workflowsApi } from '../../../../services/api/workflows';
import type { WorkflowStateProposal } from '../../../../types/workflows';
import { WorkflowStatePreview } from '../WorkflowStatePreview';

vi.mock('../../../../lib/i18n/I18nContext', () => ({
  useT: () => ({ t: (k: string) => (enDict as Record<string, string>)[k] ?? k }),
}));
vi.mock('../../../../services/api/workflows', () => ({
  workflowsApi: { enable: vi.fn(), disable: vi.fn(), runNow: vi.fn() },
}));
const enableMock = workflowsApi.enable as unknown as ReturnType<typeof vi.fn>;
const disableMock = workflowsApi.disable as unknown as ReturnType<typeof vi.fn>;
const runNowMock = workflowsApi.runNow as unknown as ReturnType<typeof vi.fn>;

function proposal(action: WorkflowStateProposal['action'], enabled = true): WorkflowStateProposal {
  return {
    workflow_id: 'wf-1',
    action,
    rationale:
      action === 'run_now' && enabled
        ? ['Estimated time: 4.2s.']
        : !enabled
          ? ['Cannot run: missing connections [gmail].']
          : [],
    enabled,
  };
}

beforeEach(() => {
  enableMock.mockReset();
  disableMock.mockReset();
  runNowMock.mockReset();
});

describe('<WorkflowStatePreview>', () => {
  it('action=enable click calls workflowsApi.enable', async () => {
    enableMock.mockResolvedValueOnce({});
    render(<WorkflowStatePreview proposal={proposal('enable')} workflowName="A" />);
    fireEvent.click(screen.getByText('Enable'));
    await waitFor(() => expect(enableMock).toHaveBeenCalledWith('wf-1'));
  });

  it('action=disable click calls workflowsApi.disable', async () => {
    disableMock.mockResolvedValueOnce({});
    render(<WorkflowStatePreview proposal={proposal('disable')} workflowName="A" />);
    fireEvent.click(screen.getByText('Disable'));
    await waitFor(() => expect(disableMock).toHaveBeenCalledWith('wf-1'));
  });

  it('action=run_now (healthy) click calls workflowsApi.runNow', async () => {
    runNowMock.mockResolvedValueOnce('run-1');
    render(<WorkflowStatePreview proposal={proposal('run_now')} workflowName="A" />);
    fireEvent.click(screen.getByText('Run now'));
    await waitFor(() => expect(runNowMock).toHaveBeenCalledWith('wf-1'));
  });

  it('action=run_now (health-blocked) disables the Apply button + shows rationale', () => {
    render(<WorkflowStatePreview proposal={proposal('run_now', false)} workflowName="A" />);
    expect(screen.getByText('Run now')).toBeDisabled();
    expect(screen.getByText(/Cannot run/)).toBeInTheDocument();
    expect(runNowMock).not.toHaveBeenCalled();
  });

  it('Cancel transitions to discarded stub', () => {
    render(<WorkflowStatePreview proposal={proposal('enable')} workflowName="A" />);
    fireEvent.click(screen.getByText('Cancel'));
    expect(screen.getByText(/Discarded — Undo/)).toBeInTheDocument();
  });
});
