/**
 * `<WorkflowProposalPreview>` — the hero rich-message component
 * (F-14). Renders a `WorkflowProposal` emitted by F-12's
 * `workflow_propose_create` agent tool and hosts the
 * [Discard] / [Save (paused)] / [Save & Enable] action row that
 * directly invokes the matching mutating RPCs on click
 * (ADR-010 + ADR-012: click is the single mutation boundary).
 *
 * State machine: pending → saving → saved / error / discarded.
 * `saved` and `discarded` are terminal in the chat thread (chat
 * history is immutable — re-opening shows the terminal state).
 *
 * Per `Automations/Artifacts/designs/workflow-proposal-preview.md`
 * — locked design source. Visual decisions intentionally match the
 * artifact byte-for-byte; UI deviations should file a follow-up
 * against the artifact rather than tweak in-flight.
 */
import { useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import type { ConnectionRef } from '../../../types/connections';
import type { Workflow, WorkflowProposal } from '../../../types/workflows';
import { useWorkflowProposalActions } from './hooks/useWorkflowProposalActions';
import { ActionRow, type ActionState } from './internal/ActionRow';
import { ConnectionChips } from './internal/ConnectionChips';
import { DetailsPanel } from './internal/DetailsPanel';
import { DiscardedStub } from './internal/DiscardedStub';
import { MissingConnectionsBanner } from './internal/MissingConnectionsBanner';
import { ProposalHeader } from './internal/ProposalHeader';
import { SavedStub } from './internal/SavedStub';
import { TriggerLine } from './internal/TriggerLine';

type ViewState = 'pending' | 'saving' | 'saved' | 'error' | 'discarded';

export interface WorkflowProposalPreviewProps {
  proposal: WorkflowProposal;
  threadId?: string;
  /**
   * Pre-computed missing-connection set. Optional — if omitted the
   * UI treats every required connection as available (good for
   * unit-test fixtures). Production callers compute this from the
   * live connections aggregator.
   */
  missingConnections?: ConnectionRef[];
  /**
   * Optional override for the initial state. Used by the chat-thread
   * re-renderer to surface the terminal `saved` / `discarded` state
   * on history re-open. Defaults to `pending`.
   */
  initialState?: ViewState;
  /** Optional override for the `[Manage connections →]` link click. */
  onManageConnections?: (missing: ConnectionRef[]) => void;
}

export function WorkflowProposalPreview({
  proposal,
  threadId,
  missingConnections,
  initialState,
  onManageConnections,
}: WorkflowProposalPreviewProps) {
  const { t } = useT();
  const actions = useWorkflowProposalActions(proposal, threadId);
  const [state, setState] = useState<ViewState>(initialState ?? 'pending');
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [savedMode, setSavedMode] = useState<'paused' | 'enabled' | null>(null);
  const [savedWorkflow, setSavedWorkflow] = useState<Workflow | null>(null);

  const missing = missingConnections ?? [];
  const canSaveEnable = missing.length === 0;

  const handleSave = async (enable: boolean) => {
    setState('saving');
    setErrorMessage(null);
    console.debug(
      '[workflows-ui] proposal_save_clicked enable=%s thread_id=%s',
      enable,
      threadId ?? 'none'
    );
    try {
      const workflow = enable ? await actions.saveAndEnable() : await actions.savePaused();
      setSavedWorkflow(workflow);
      setSavedMode(enable ? 'enabled' : 'paused');
      setState('saved');
    } catch (err) {
      const message = (err as Error | undefined)?.message ?? 'unknown error';
      console.error('[workflows-ui] proposal_save_failed message=%s', message, err);
      setErrorMessage(message);
      setState('error');
    }
  };

  const handleDiscard = () => {
    console.debug('[workflows-ui] proposal_discarded thread_id=%s', threadId ?? 'none');
    setState('discarded');
  };

  if (state === 'saved' && savedMode && savedWorkflow) {
    return (
      <SavedStub
        name={savedWorkflow.name}
        mode={savedMode}
        workflowId={savedWorkflow.id}
        onEnableNow={
          savedMode === 'paused'
            ? async () => {
                try {
                  const enabled = await actions.enableExisting(savedWorkflow.id);
                  setSavedWorkflow(enabled);
                  setSavedMode('enabled');
                } catch (err) {
                  console.error(
                    '[workflows-ui] saved_enable_now_failed wf=%s',
                    savedWorkflow.id,
                    err
                  );
                }
              }
            : undefined
        }
      />
    );
  }

  if (state === 'discarded') {
    return (
      <DiscardedStub
        onUndo={() => {
          console.debug('[workflows-ui] proposal_undo_discard');
          setState('pending');
        }}
      />
    );
  }

  const actionState: ActionState =
    state === 'saving' ? 'saving' : state === 'error' ? 'error' : 'pending';

  return (
    <div
      data-testid="workflow-proposal-preview"
      role="region"
      aria-busy={state === 'saving'}
      aria-label={proposal.name}
      className="bg-white dark:bg-neutral-900 rounded-2xl shadow-subtle border border-primary-100 dark:border-primary-900 p-4 max-w-[560px]">
      <ProposalHeader
        icon="⚡"
        name={proposal.name}
        confidence={proposal.confidence}
        description={proposal.description}
      />
      <TriggerLine trigger={proposal.trigger} />
      <ConnectionChips required={proposal.required_connections} missing={missing} />
      {missing.length > 0 && (
        <MissingConnectionsBanner
          missing={missing}
          onManage={onManageConnections ? () => onManageConnections(missing) : undefined}
        />
      )}
      <button
        type="button"
        onClick={() => setExpanded(v => !v)}
        aria-expanded={expanded}
        className="mt-3 text-xs text-primary-600 hover:text-primary-700 hover:underline flex items-center gap-1">
        <span aria-hidden>{expanded ? '⌃' : '⌄'}</span>
        {expanded ? t('workflows.preview.hide_details') : t('workflows.preview.show_details')}
      </button>
      {expanded && <DetailsPanel proposal={proposal} />}
      <ActionRow
        state={actionState}
        errorMessage={errorMessage}
        canSaveEnable={canSaveEnable}
        onDiscard={handleDiscard}
        onSavePaused={() => handleSave(false)}
        onSaveAndEnable={() => handleSave(true)}
        onRetry={() => handleSave(savedMode === 'enabled')}
      />
      {/* Live region for state announcements (NFR a11y). */}
      <span className="sr-only" aria-live="polite" role="status">
        {state === 'saving' && t('workflows.preview.saving')}
        {state === 'saved' && t('workflows.preview.saved_announce')}
        {state === 'error' && t('workflows.preview.error_announce')}
      </span>
    </div>
  );
}
