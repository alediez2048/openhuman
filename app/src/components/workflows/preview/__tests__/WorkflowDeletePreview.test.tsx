import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

import enDict from '../../../../lib/i18n/en';
import { workflowsApi } from '../../../../services/api/workflows';
import type { WorkflowDeletePreview as DeletePayload } from '../../../../types/workflows';
import { WorkflowDeletePreview } from '../WorkflowDeletePreview';

vi.mock('../../../../lib/i18n/I18nContext', () => ({
  useT: () => ({ t: (k: string) => (enDict as Record<string, string>)[k] ?? k }),
}));
vi.mock('../../../../services/api/workflows', () => ({ workflowsApi: { delete: vi.fn() } }));
const deleteMock = workflowsApi.delete as unknown as ReturnType<typeof vi.fn>;

const preview: DeletePayload = {
  workflow_id: 'wf-1',
  name: 'Morning digest',
  run_count: 14,
  retention_days: 30,
};

beforeEach(() => deleteMock.mockReset());

describe('<WorkflowDeletePreview>', () => {
  it('renders the run_count and 30-day retention copy', () => {
    render(<WorkflowDeletePreview preview={preview} />);
    expect(screen.getByText(/14 past runs/)).toBeInTheDocument();
    expect(screen.getByText(/30 days/)).toBeInTheDocument();
  });

  it('renders the "no past runs" copy when run_count is 0', () => {
    render(<WorkflowDeletePreview preview={{ ...preview, run_count: 0 }} />);
    expect(screen.getByText(/no past runs/)).toBeInTheDocument();
  });

  it('Delete calls workflowsApi.delete with the workflow id', async () => {
    deleteMock.mockResolvedValueOnce(true);
    render(<WorkflowDeletePreview preview={preview} />);
    fireEvent.click(screen.getByText('Delete'));
    await waitFor(() => expect(deleteMock).toHaveBeenCalledWith('wf-1'));
  });

  it('Cancel transitions to discarded stub', () => {
    render(<WorkflowDeletePreview preview={preview} />);
    fireEvent.click(screen.getByText('Cancel'));
    expect(screen.getByText(/Discarded — Undo/)).toBeInTheDocument();
  });
});
