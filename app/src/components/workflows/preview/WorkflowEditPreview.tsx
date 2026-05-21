/**
 * `<WorkflowEditPreview>` — diff-card rich-message for F-12's
 * `workflow_propose_update` payloads.
 *
 * Renders the `diff_summary` bullets the server computed (via
 * `workflows::diff::workflow_diff`). Each bullet starts with a stable
 * verb prefix the human eye recognises as a diff ("Renamed", "Changed
 * cron schedule", "Added", "Removed"); we map the leading verb to a
 * `+` / `-` / `±` gutter so the card reads like a true diff without
 * the developer-tool tells of field-path labels.
 *
 * Click [Apply changes] calls `workflows_update` directly per
 * ADR-010. Click [Cancel] discards the preview.
 */
import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import { workflowsApi } from '../../../services/api/workflows';
import type { WorkflowEditProposal, WorkflowPatch } from '../../../types/workflows';
import { DiscardedStub } from './internal/DiscardedStub';
import { ProposalHeader } from './internal/ProposalHeader';

type State = 'pending' | 'applying' | 'applied' | 'discarded' | 'error';

interface Props {
  proposal: WorkflowEditProposal;
}

function diffGutter(bullet: string): string {
  if (/^Removed |^Cleared /.test(bullet)) return '-';
  if (/^Added /.test(bullet)) return '+';
  if (/^(Renamed|Changed|Rewrote)/.test(bullet)) return '±';
  return ' ';
}

function proposedToPatch(proposed: WorkflowEditProposal['proposed']): WorkflowPatch {
  return {
    name: proposed.name,
    description: proposed.description ?? null,
    trigger: proposed.trigger,
    nodes: proposed.nodes,
    edges: proposed.edges,
    settings: proposed.settings,
  };
}

export function WorkflowEditPreview({ proposal }: Props) {
  const { t } = useT();
  const [state, setState] = useState<State>('pending');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);

  const noChanges = proposal.diff_summary.length === 0;

  const handleApply = async () => {
    setState('applying');
    setErrorMessage(null);
    console.debug(
      '[workflows-ui] edit_preview_apply_clicked wf=%s diff_count=%d',
      proposal.workflow_id,
      proposal.diff_summary.length
    );
    try {
      await workflowsApi.update({
        id: proposal.workflow_id,
        patches: proposedToPatch(proposal.proposed),
      });
      setState('applied');
    } catch (err) {
      const message = (err as Error | undefined)?.message ?? 'unknown error';
      console.error('[workflows-ui] edit_preview_apply_failed message=%s', message, err);
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
          ✓ Changes applied — “{proposal.proposed.name}”
        </p>
      </div>
    );
  }

  const busy = state === 'applying';
  return (
    <div
      data-testid="workflow-edit-preview"
      role="region"
      aria-busy={busy}
      aria-label={`Proposed edit: ${proposal.proposed.name}`}
      className="bg-white dark:bg-neutral-900 rounded-2xl shadow-subtle border border-primary-100 dark:border-primary-900 p-4 max-w-[560px]">
      <ProposalHeader
        icon="✏️"
        name={t('workflows.preview.edit_header').replace('{name}', proposal.proposed.name)}
      />
      <div className="mt-3 space-y-1">
        {noChanges ? (
          <p className="text-xs text-stone-500 dark:text-neutral-400 italic">
            {t('workflows.preview.no_changes')}
          </p>
        ) : (
          proposal.diff_summary.map((bullet, i) => {
            const gutter = diffGutter(bullet);
            const gutterClass =
              gutter === '+'
                ? 'text-sage-700'
                : gutter === '-'
                  ? 'text-coral-700'
                  : 'text-stone-500';
            return (
              <div key={i} className="flex items-start gap-2 text-xs">
                <span
                  aria-hidden
                  className={`font-mono w-4 text-center select-none ${gutterClass}`}>
                  {gutter}
                </span>
                <span className="text-stone-700 dark:text-neutral-300 flex-1">{bullet}</span>
              </div>
            );
          })
        )}
      </div>
      {state === 'error' && (
        <p role="alert" className="mt-3 text-xs text-coral-600 dark:text-coral-400">
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
          disabled={busy || noChanges}
          className="px-3 py-1.5 text-xs rounded-lg bg-primary-600 text-white hover:bg-primary-700 disabled:opacity-50 font-medium">
          {busy ? t('workflows.preview.saving') : t('workflows.preview.apply_changes')}
        </button>
      </div>
    </div>
  );
}
