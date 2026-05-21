/**
 * Vitest unit tests for `<WorkflowHealthBadge>`.
 *
 * Each variant of `WorkflowHealth` renders with the right aria label
 * and the testid the WorkflowCard depends on.
 */
import { render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import type { WorkflowHealth } from '../../../types/workflows';
import WorkflowHealthBadge from '../WorkflowHealthBadge';

describe('<WorkflowHealthBadge>', () => {
  it('renders the Ready state', () => {
    render(<WorkflowHealthBadge health={{ type: 'ready' } as WorkflowHealth} />);
    expect(screen.getByTestId('workflow-health-badge-ready')).toBeInTheDocument();
    expect(screen.getByLabelText('Ready')).toBeInTheDocument();
  });

  it('renders the NeedsConnections state with a tooltip listing missing refs', () => {
    render(
      <WorkflowHealthBadge
        health={{ type: 'needs_connections', missing: [{ type: 'composio', toolkit_id: 'gmail' }] }}
      />
    );
    const badge = screen.getByTestId('workflow-health-badge-needs-connections');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveAttribute('title', expect.stringContaining('gmail'));
  });

  it('renders the LastRunFailed state with the failure reason as title', () => {
    render(
      <WorkflowHealthBadge
        health={{ type: 'last_run_failed', run_id: 'r-1', reason: 'timeout after 300s' }}
      />
    );
    const badge = screen.getByTestId('workflow-health-badge-last-run-failed');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveAttribute('title', 'timeout after 300s');
  });

  it('renders the SessionExpired state with the connection label', () => {
    render(
      <WorkflowHealthBadge
        health={{
          type: 'session_expired',
          connection: { type: 'webview', provider: 'linkedin', account_id: 'acct-1' },
        }}
      />
    );
    const badge = screen.getByTestId('workflow-health-badge-session-expired');
    expect(badge).toBeInTheDocument();
    expect(badge).toHaveAttribute('title', expect.stringContaining('linkedin'));
  });
});
