/**
 * `<WorkflowStatePreview>` — compact toggle / run-now confirmation
 * card for F-12's `workflow_propose_enable` / `_disable` /
 * `_run_now` payloads. Single primary button. Health gating
 * (`payload.enabled === false`) disables the Apply button + surfaces
 * the rationale verbatim.
 *
 * Click handlers invoke the matching mutating RPC directly per
 * ADR-010.
 */
import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { workflowsApi } from '../../../services/api/workflows';
import type { WorkflowStateProposal } from '../../../types/workflows';
import { DiscardedStub } from './internal/DiscardedStub';
import { ProposalHeader } from './internal/ProposalHeader';

type State = 'pending' | 'applying' | 'applied' | 'discarded' | 'error';

interface Props {
  proposal: WorkflowStateProposal;
  workflowName?: string;
}

function actionLabelKey(action: WorkflowStateProposal['action']): string {
  switch (action) {
    case 'enable':
      return 'workflows.preview.enable_action';
    case 'disable':
      return 'workflows.preview.disable_action';
    case 'run_now':
      return 'workflows.preview.run_now_action';
  }
}

function headerKey(action: WorkflowStateProposal['action']): string {
  switch (action) {
    case 'enable':
      return 'workflows.preview.enable_header';
    case 'disable':
      return 'workflows.preview.disable_header';
    case 'run_now':
      return 'workflows.preview.run_now_header';
  }
}

function iconFor(action: WorkflowStateProposal['action']): string {
  switch (action) {
    case 'enable':
      return '⏵';
    case 'disable':
      return '⏸';
    case 'run_now':
      return '▶';
  }
}

export function WorkflowStatePreview({ proposal, workflowName }: Props) {
  const { t } = useT();
  const [state, setState] = useState<State>('pending');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const handleApply = async () => {
    if (!proposal.enabled) return;
    setState('applying');
    setErrorMessage(null);
    console.debug(
      '[workflows-ui] state_preview_apply_clicked wf=%s action=%s',
      proposal.workflow_id,
      proposal.action
    );
    try {
      switch (proposal.action) {
        case 'enable':
          await workflowsApi.enable(proposal.workflow_id);
          break;
        case 'disable':
          await workflowsApi.disable(proposal.workflow_id);
          break;
        case 'run_now':
          await workflowsApi.runNow(proposal.workflow_id);
          break;
      }
      setState('applied');
    } catch (err) {
      const message = (err as Error | undefined)?.message ?? 'unknown error';
      console.error('[workflows-ui] state_preview_apply_failed message=%s', message, err);
      setErrorMessage(message);
      setState('error');
    }
  };

  if (state === 'discarded') {
    return <DiscardedStub onUndo={() => setState('pending')} />;
  }
  if (state === 'applied') {
    return (
      <div
        role="status"
        aria-live="polite"
        className="bg-white dark:bg-neutral-900 rounded-2xl border border-stone-200 dark:border-neutral-700 border-l-4 border-l-sage-500 p-3 max-w-[560px]">
        <p className="text-sm font-medium text-stone-900 dark:text-neutral-100">
          ✓ {t(actionLabelKey(proposal.action))} applied
        </p>
      </div>
    );
  }

  const busy = state === 'applying';
  const headerName = workflowName ?? proposal.workflow_id;
  return (
    <div
      data-testid="workflow-state-preview"
      role="region"
      aria-busy={busy}
      aria-label={`${proposal.action} ${headerName}`}
      className="bg-white dark:bg-neutral-900 rounded-2xl shadow-subtle border border-stone-200 dark:border-neutral-700 p-3 max-w-[560px]">
      <ProposalHeader
        icon={iconFor(proposal.action)}
        name={t(headerKey(proposal.action)).replace('{name}', headerName)}
      />
      {proposal.rationale.length > 0 && (
        <p className="mt-2 text-xs text-stone-600 dark:text-neutral-400">
          {proposal.rationale.join(' ')}
        </p>
      )}
      {state === 'error' && (
        <p role="alert" className="mt-2 text-xs text-coral-600 dark:text-coral-400">
          {t('workflows.preview.couldnt_save').replace('{reason}', errorMessage ?? 'unknown')}
        </p>
      )}
      <div className="mt-3 flex items-center justify-between gap-3">
        <button
          type="button"
          onClick={() => setState('discarded')}
          disabled={busy}
          className="px-3 py-1.5 text-xs text-stone-500 hover:text-stone-700 disabled:opacity-50 font-medium">
          {t('workflows.preview.cancel')}
        </button>
        <button
          type="button"
          onClick={handleApply}
          disabled={busy || !proposal.enabled}
          title={proposal.enabled ? undefined : (proposal.rationale[0] ?? 'Action blocked')}
          className="px-3 py-1.5 text-xs rounded-lg bg-primary-600 text-white hover:bg-primary-700 disabled:opacity-50 disabled:cursor-not-allowed font-medium">
          {t(actionLabelKey(proposal.action))}
        </button>
      </div>
    </div>
  );
}
