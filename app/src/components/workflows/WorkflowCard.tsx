/**
 * One row in the `WorkflowsList` page. Activation surface first — the
 * enable/disable toggle is the primary action; everything else is
 * supporting context (FR-1.2.3).
 *
 * Overflow menu actions (post-F-15):
 *   - **Run now** → `workflowsApi.runNow(id)`. Disabled until
 *     `workflow.health.type === 'ready'`; the server is the final
 *     gate (returns `health_blocked` on race) but we keep the
 *     button greyed out for honest UX.
 *   - **Delete** → `deleteWorkflow` thunk + refresh the starter
 *     catalog so a deleted Seed-origin row re-appears in the
 *     catalog (matches the F-15 catalog E2E semantics).
 *   - **Edit** → surfaces an inline "describe the change in chat"
 *     message. The full edit flow lives on the
 *     `workflow_propose_update` → `<WorkflowEditPreview>` chat
 *     path that's pending the Phase 1.5 chat-runtime protocol
 *     extension.
 *
 * Inline `actionMessage` shows the outcome (or error) directly on
 * the card — keeps the user focused without bouncing through a
 * global toast surface.
 */
import { useEffect, useRef, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { workflowsApi } from '../../services/api/workflows';
import { useAppDispatch } from '../../store/hooks';
import { deleteWorkflow, fetchStarterTemplates, fetchWorkflows } from '../../store/workflowsSlice';
import type { Trigger, Workflow } from '../../types/workflows';
import WorkflowEnableToggle from './WorkflowEnableToggle';
import WorkflowHealthBadge from './WorkflowHealthBadge';

function summarizeTrigger(trigger: Trigger, t: (key: string) => string): string {
  switch (trigger.type) {
    case 'cron':
      return trigger.tz ? `${trigger.expr} (${trigger.tz})` : trigger.expr;
    case 'manual':
      return t('workflows.card.runs_on_demand');
    case 'webhook':
      return `Webhook → ${trigger.target_path}`;
    case 'composio_event':
      return `${trigger.toolkit}: ${trigger.trigger_id}`;
    case 'channel_message':
      return `${trigger.provider} message`;
    default:
      return 'Custom trigger';
  }
}

function summarizeSteps(stepCount: number, t: (key: string) => string): string {
  const key = stepCount === 1 ? 'workflows.card.step_count_one' : 'workflows.card.step_count_other';
  return t(key).replace('{count}', String(stepCount));
}

function summarizeLastRun(
  lastRunAt: string | null | undefined,
  t: (key: string) => string
): string {
  if (!lastRunAt) return t('workflows.card.never_run');
  const date = new Date(lastRunAt);
  return `${t('workflows.card.last_run')}: ${date.toLocaleString()}`;
}

interface Props {
  workflow: Workflow;
}

export default function WorkflowCard({ workflow }: Props) {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const [menuOpen, setMenuOpen] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // Clear the inline action message after a short delay so the
  // card doesn't accumulate stale toasts.
  useEffect(() => {
    if (!actionMessage) return;
    const t = window.setTimeout(() => setActionMessage(null), 4000);
    return () => window.clearTimeout(t);
  }, [actionMessage]);

  // Close the overflow menu on outside click + Esc.
  useEffect(() => {
    if (!menuOpen) return;
    const onPointer = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenuOpen(false);
    };
    window.addEventListener('mousedown', onPointer);
    window.addEventListener('keydown', onKey);
    return () => {
      window.removeEventListener('mousedown', onPointer);
      window.removeEventListener('keydown', onKey);
    };
  }, [menuOpen]);

  const handleRunNow = async () => {
    setBusy(true);
    setActionMessage(null);
    console.debug('[workflows-ui] run_now_clicked id=%s', workflow.id);
    try {
      const runId = await workflowsApi.runNow(workflow.id);
      setActionMessage(`Run started (${runId.slice(0, 8)}…)`);
    } catch (err) {
      const message = (err as Error | undefined)?.message ?? 'unknown error';
      console.error('[workflows-ui] run_now_failed id=%s message=%s', workflow.id, message, err);
      // The server returns `health_blocked: {...}` when health !=
      // Ready. Surface a friendly inline message instead of the
      // raw error.
      if (message.includes('health_blocked')) {
        setActionMessage('Cannot run — connect the missing services first.');
      } else if (message.includes('not_found')) {
        setActionMessage('Workflow no longer exists.');
      } else {
        setActionMessage(`Run failed: ${message}`);
      }
    } finally {
      setBusy(false);
    }
  };

  const isSeedOrigin = workflow.origin.type === 'seed';

  const handleDelete = async () => {
    setBusy(true);
    setActionMessage(null);
    console.debug(
      '[workflows-ui] delete_clicked id=%s origin=%s',
      workflow.id,
      workflow.origin.type
    );
    try {
      await dispatch(deleteWorkflow(workflow.id)).unwrap();
      // Refresh the starter-catalog so a Seed-origin row's
      // template re-appears in the catalog automatically (the
      // F-5 list_starter_templates server-side dedupes against
      // existing Seed{template_id} workflows — drop the row +
      // the template reappears).
      void dispatch(fetchStarterTemplates());
      void dispatch(fetchWorkflows(undefined));
    } catch (err) {
      const message = (err as Error | undefined)?.message ?? 'unknown error';
      console.error('[workflows-ui] delete_failed id=%s message=%s', workflow.id, message, err);
      setActionMessage(`Delete failed: ${message}`);
      setBusy(false);
    }
    // No `setBusy(false)` on success — the card unmounts.
  };

  const handleEdit = () => {
    // Edit lives on the chat-driven propose path (F-12's
    // `workflow_propose_update` → `<WorkflowEditPreview>`). The
    // chat-runtime protocol extension that renders the preview
    // inside `AgentMessageBubble` is deferred to Phase 1.5.
    setActionMessage('Edit lands in chat — say what you want to change.');
  };

  const handleOverflow = (action: 'edit' | 'run_now' | 'delete') => {
    setMenuOpen(false);
    if (action === 'run_now') {
      void handleRunNow();
    } else if (action === 'delete') {
      void handleDelete();
    } else {
      handleEdit();
    }
  };

  return (
    <div
      data-testid={`workflow-card-${workflow.id}`}
      className="bg-white dark:bg-neutral-900 rounded-2xl shadow-subtle border border-stone-200 dark:border-neutral-700 p-4 flex items-center gap-4">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2 flex-wrap">
          <h3
            className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate"
            title={workflow.name}>
            {workflow.name}
          </h3>
          <WorkflowHealthBadge health={workflow.health} />
        </div>
        <div className="text-xs text-stone-500 dark:text-neutral-400 mt-1 truncate">
          {summarizeTrigger(workflow.trigger, t)} · {summarizeSteps(workflow.nodes.length, t)}
        </div>
        <div className="text-[11px] text-stone-400 dark:text-neutral-500 mt-0.5 truncate">
          {summarizeLastRun(workflow.last_run_at ?? null, t)}
        </div>
        {actionMessage && (
          <div
            role="status"
            aria-live="polite"
            className="text-[11px] text-primary-700 dark:text-primary-300 mt-1 truncate">
            {actionMessage}
          </div>
        )}
      </div>

      <WorkflowEnableToggle
        workflowId={workflow.id}
        enabled={workflow.enabled}
        health={workflow.health}
      />

      <div className="relative" ref={menuRef}>
        <button
          type="button"
          aria-label="More actions"
          aria-haspopup="menu"
          aria-expanded={menuOpen}
          onClick={() => setMenuOpen(v => !v)}
          data-testid={`workflow-card-overflow-${workflow.id}`}
          className="p-1.5 text-stone-500 dark:text-neutral-400 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md focus:outline-none focus:ring-2 focus:ring-primary-500">
          <svg className="w-4 h-4" fill="currentColor" viewBox="0 0 20 20">
            <path d="M10 6a2 2 0 110-4 2 2 0 010 4zM10 12a2 2 0 110-4 2 2 0 010 4zM10 18a2 2 0 110-4 2 2 0 010 4z" />
          </svg>
        </button>
        {menuOpen && (
          <div
            role="menu"
            data-testid={`workflow-card-menu-${workflow.id}`}
            className="absolute right-0 top-full mt-1 min-w-[140px] rounded-lg border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-900 shadow-strong py-1 z-20">
            <button
              type="button"
              role="menuitem"
              onClick={() => handleOverflow('edit')}
              disabled={busy}
              className="w-full text-left px-3 py-1.5 text-xs text-stone-700 dark:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800 disabled:opacity-50">
              {t('workflows.edit')}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => handleOverflow('run_now')}
              disabled={busy || workflow.health.type !== 'ready'}
              title={
                workflow.health.type !== 'ready' ? 'Connect the missing services first.' : undefined
              }
              className="w-full text-left px-3 py-1.5 text-xs text-stone-700 dark:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800 disabled:opacity-50 disabled:cursor-not-allowed">
              {t('workflows.run_now')}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => handleOverflow('delete')}
              disabled={busy}
              title={
                isSeedOrigin
                  ? 'Removes the workflow and returns its template to the starter section.'
                  : undefined
              }
              className={`w-full text-left px-3 py-1.5 text-xs hover:bg-stone-100 dark:hover:bg-neutral-800 disabled:opacity-50 ${
                isSeedOrigin ? 'text-stone-700 dark:text-neutral-200' : 'text-coral-600'
              }`}>
              {isSeedOrigin ? t('workflows.move_to_starter') : t('workflows.delete')}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
