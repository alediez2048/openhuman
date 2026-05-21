/**
 * One row in the `WorkflowsList` page. Activation surface first — the
 * enable/disable toggle is the primary action; everything else is
 * supporting context (FR-1.2.3).
 *
 * Overflow menu (Edit / Run now / Delete) is stubbed in F-4: clicks
 * emit a `console.debug` placeholder. F-7 wires "Run now" to
 * `workflows_run_now`, F-14 wires Edit through the proposal-preview
 * flow, and F-12 wires Delete through `workflow_propose_delete`.
 */
import { useEffect, useRef, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
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
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

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

  const handleOverflow = (action: 'edit' | 'run_now' | 'delete') => {
    setMenuOpen(false);
    // F-7 / F-12 / F-14 wire these. F-4 ships a placeholder.
    console.debug(`[workflows-ui] overflow_clicked action=${action} id=${workflow.id}`);
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
              className="w-full text-left px-3 py-1.5 text-xs text-stone-700 dark:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800">
              {t('workflows.edit')}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => handleOverflow('run_now')}
              className="w-full text-left px-3 py-1.5 text-xs text-stone-700 dark:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800">
              {t('workflows.run_now')}
            </button>
            <button
              type="button"
              role="menuitem"
              onClick={() => handleOverflow('delete')}
              className="w-full text-left px-3 py-1.5 text-xs text-coral-600 hover:bg-stone-100 dark:hover:bg-neutral-800">
              {t('workflows.delete')}
            </button>
          </div>
        )}
      </div>
    </div>
  );
}
