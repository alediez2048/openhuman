/**
 * `<WorkflowDeletePreview>` — coral confirmation card for F-12's
 * `workflow_propose_delete` payloads. Click [Delete] triggers
 * `workflows_delete` directly (ADR-010 single-mutation-boundary).
 */
import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { workflowsApi } from '../../../services/api/workflows';
import type { WorkflowDeletePreview as DeletePreviewPayload } from '../../../types/workflows';
import { DiscardedStub } from './internal/DiscardedStub';
import { ProposalHeader } from './internal/ProposalHeader';

type State = 'pending' | 'deleting' | 'deleted' | 'discarded' | 'error';

interface Props {
  preview: DeletePreviewPayload;
}

export function WorkflowDeletePreview({ preview }: Props) {
  const { t } = useT();
  const [state, setState] = useState<State>('pending');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const handleDelete = async () => {
    setState('deleting');
    setErrorMessage(null);
    console.debug('[workflows-ui] delete_preview_confirm_clicked wf=%s', preview.workflow_id);
    try {
      await workflowsApi.delete(preview.workflow_id);
      setState('deleted');
    } catch (err) {
      const message = (err as Error | undefined)?.message ?? 'unknown error';
      console.error('[workflows-ui] delete_preview_failed message=%s', message, err);
      setErrorMessage(message);
      setState('error');
    }
  };

  if (state === 'discarded') {
    return <DiscardedStub onUndo={() => setState('pending')} />;
  }
  if (state === 'deleted') {
    return (
      <div
        role="status"
        aria-live="polite"
        className="bg-white dark:bg-neutral-900 rounded-2xl border border-stone-200 dark:border-neutral-700 border-l-4 border-l-coral-500 p-3 max-w-[560px]">
        <p className="text-sm font-medium text-stone-900 dark:text-neutral-100">
          🗑 Deleted “{preview.name}”
        </p>
      </div>
    );
  }

  const busy = state === 'deleting';
  const body =
    preview.run_count === 0
      ? t('workflows.preview.delete_no_runs').replace('{days}', String(preview.retention_days))
      : t('workflows.preview.delete_subtitle')
          .replace('{count}', String(preview.run_count))
          .replace('{days}', String(preview.retention_days));

  return (
    <div
      data-testid="workflow-delete-preview"
      role="region"
      aria-busy={busy}
      aria-label={`Delete ${preview.name}`}
      className="bg-white dark:bg-neutral-900 rounded-2xl shadow-subtle border border-stone-200 dark:border-neutral-700 border-l-4 border-l-coral-500 p-4 max-w-[560px]">
      <ProposalHeader
        icon="🗑"
        name={t('workflows.preview.delete_header').replace('{name}', preview.name)}
      />
      <p className="mt-3 text-xs text-stone-700 dark:text-neutral-300">{body}</p>
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
          onClick={handleDelete}
          disabled={busy}
          className="px-3 py-1.5 text-xs rounded-lg bg-coral-600 text-white hover:bg-coral-700 disabled:opacity-50 font-medium">
          {t('workflows.preview.delete_action')}
        </button>
      </div>
    </div>
  );
}
