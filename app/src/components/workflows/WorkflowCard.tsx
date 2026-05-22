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
import type { ConnectionRef } from '../../types/connections';
import type { Trigger, Workflow } from '../../types/workflows';
import { ConnectionChips } from './preview/internal/ConnectionChips';
import { TriggerLine } from './preview/internal/TriggerLine';
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

/**
 * Linear-style relative timestamp: "just now", "5m ago", "3h ago",
 * "2d ago", and full month-day for anything older than a week.
 * Used in the dense row layout where the full locale-formatted
 * date is too long to fit.
 */
function relativeTime(lastRunAt: string | null | undefined, t: (key: string) => string): string {
  if (!lastRunAt) return t('workflows.card.never_run');
  const then = new Date(lastRunAt).getTime();
  const now = Date.now();
  const diffMs = now - then;
  if (diffMs < 60_000) return 'just now';
  const diffMin = Math.floor(diffMs / 60_000);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < 7) return `${diffDay}d ago`;
  // > 1 week: explicit date, locale-aware short form (e.g. "May 14").
  return new Date(lastRunAt).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

interface Props {
  workflow: Workflow;
}

type DetailSection = 'prompt' | 'connections' | 'settings';

/**
 * Pull the user-authored agent prompt out of the workflow's first
 * `agent_prompt` node. Phase 1 ships single-node workflows; if a
 * later phase lands multi-node, this needs to surface every node.
 */
function readAgentPrompt(workflow: Workflow): string {
  for (const node of workflow.nodes) {
    if (node.config && node.config.kind === 'agent_prompt') {
      return node.config.prompt;
    }
  }
  return '';
}

/**
 * Pull `allowed_connections` from the workflow's first agent_prompt
 * node. Used as the source of truth for "what does this workflow
 * need". The list is what the user authored (or what the drafter
 * proposed and the user saved) — same shape the proposal preview
 * shows.
 */
function readRequiredConnections(workflow: Workflow): ConnectionRef[] {
  for (const node of workflow.nodes) {
    if (node.config && node.config.kind === 'agent_prompt') {
      return node.config.allowed_connections ?? [];
    }
  }
  return [];
}

/**
 * Health-derived "currently missing" subset of required_connections.
 * The proposal-preview side gets this from the drafter; on a saved
 * workflow the same info lives in `workflow.health` when the F-3
 * recompute decided some refs aren't connected.
 */
function readMissingConnections(workflow: Workflow): ConnectionRef[] {
  if (workflow.health.type === 'needs_connections') {
    return workflow.health.missing;
  }
  return [];
}

interface SettingsRow {
  k: string;
  v: string | number;
}

function buildSettingsRows(workflow: Workflow): SettingsRow[] {
  const rows: SettingsRow[] = [
    { k: 'timeout_secs', v: workflow.settings.timeout_secs },
    { k: 'on_error', v: workflow.settings.on_error },
  ];
  // Look at the first agent_prompt node for per-node tuning. Phase 1
  // workflows are single-node — multi-node listing would belong in
  // a Phase 2/3 detail view that knows about edges.
  for (const node of workflow.nodes) {
    if (node.config && node.config.kind === 'agent_prompt') {
      if (node.config.iteration_cap !== undefined) {
        rows.push({ k: 'iteration_cap', v: node.config.iteration_cap });
      }
      if (node.config.model_tier) {
        rows.push({ k: 'model_tier', v: node.config.model_tier });
      }
      break;
    }
  }
  return rows;
}

export default function WorkflowCard({ workflow }: Props) {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const [menuOpen, setMenuOpen] = useState(false);
  const [actionMessage, setActionMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [openSection, setOpenSection] = useState<DetailSection | null>(null);
  const [promptCopied, setPromptCopied] = useState(false);
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

  const agentPrompt = readAgentPrompt(workflow);
  const requiredConnections = readRequiredConnections(workflow);
  const missingConnections = readMissingConnections(workflow);
  const settingsRows = buildSettingsRows(workflow);
  const promptFirstLine = agentPrompt.split('\n')[0] ?? '';
  const handleCopyPrompt = async () => {
    try {
      await navigator.clipboard.writeText(agentPrompt);
      setPromptCopied(true);
      window.setTimeout(() => setPromptCopied(false), 2000);
    } catch {
      // Browser clipboard API denied; the prompt is already
      // visible in the panel so the user can copy manually.
    }
  };

  return (
    <div
      data-testid={`workflow-card-${workflow.id}`}
      className="bg-white dark:bg-neutral-900 rounded-xl border border-stone-200 dark:border-neutral-700 hover:border-stone-300 dark:hover:border-neutral-600 transition-colors">
      {/* Linear-style single-line row. Click anywhere except the
          toggle / kebab toggles the expanded view. */}
      <div className="flex items-center gap-3 px-3 py-2.5">
        <button
          type="button"
          onClick={() => setExpanded(v => !v)}
          aria-expanded={expanded}
          aria-controls={`workflow-card-details-${workflow.id}`}
          data-testid={`workflow-card-toggle-details-${workflow.id}`}
          className="flex-1 min-w-0 flex items-center gap-3 text-left focus:outline-none focus:ring-2 focus:ring-primary-500 rounded-md py-0.5 -my-0.5">
          {/* Status dot — green for ready, amber for any other
              health state. Keeps a single load-bearing glyph; the
              text label sits in the WorkflowHealthBadge if the
              user expands. */}
          <span
            aria-hidden
            className={`flex-shrink-0 w-2 h-2 rounded-full ${
              workflow.health.type === 'ready'
                ? 'bg-sage-500'
                : workflow.health.type === 'last_run_failed'
                  ? 'bg-coral-500'
                  : 'bg-amber-500'
            }`}
            title={workflow.health.type}
          />
          <h3
            className="text-sm font-medium text-stone-900 dark:text-neutral-100 truncate"
            title={workflow.name}>
            {workflow.name}
          </h3>
          <span className="text-xs text-stone-400 dark:text-neutral-500 truncate hidden sm:inline">
            {summarizeTrigger(workflow.trigger, t)}
          </span>
          <span
            className="ml-auto text-xs text-stone-400 dark:text-neutral-500 whitespace-nowrap"
            title={
              workflow.last_run_at ? new Date(workflow.last_run_at).toLocaleString() : undefined
            }>
            {relativeTime(workflow.last_run_at, t)}
          </span>
        </button>

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
            className="p-1 text-stone-400 hover:text-stone-700 dark:text-neutral-500 dark:hover:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md focus:outline-none focus:ring-2 focus:ring-primary-500">
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
                  workflow.health.type !== 'ready'
                    ? 'Connect the missing services first.'
                    : undefined
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

      {actionMessage && (
        <div
          role="status"
          aria-live="polite"
          className="px-3 pb-2 text-[11px] text-primary-700 dark:text-primary-300 truncate -mt-1">
          {actionMessage}
        </div>
      )}

      {expanded && (
        <div
          id={`workflow-card-details-${workflow.id}`}
          className="border-t border-stone-100 dark:border-neutral-700 px-3 py-3">
          <div className="flex items-center gap-2 mb-2">
            <WorkflowHealthBadge health={workflow.health} />
            <span className="text-[11px] text-stone-400 dark:text-neutral-500">
              {summarizeSteps(workflow.nodes.length, t)}
            </span>
          </div>
          {workflow.description && (
            <p className="text-xs text-stone-600 dark:text-neutral-300 mb-2">
              {workflow.description}
            </p>
          )}
          <TriggerLine trigger={workflow.trigger} />
          <ConnectionChips required={requiredConnections} missing={missingConnections} />

          {/* Collapsible detail sections, mirroring the proposal-preview
              DetailsPanel layout: one expanded at a time, badge for
              count, chevron prefix. */}
          <div className="mt-3 border-t border-stone-100 dark:border-neutral-700">
            {(
              [
                {
                  key: 'prompt' as DetailSection,
                  label: t('workflows.preview.agent_prompt'),
                  badge: undefined,
                  hintInline: !openSection && promptFirstLine ? promptFirstLine : null,
                },
                {
                  key: 'connections' as DetailSection,
                  label: t('workflows.preview.required_connections'),
                  badge:
                    requiredConnections.length > 0 ? String(requiredConnections.length) : undefined,
                  hintInline: null,
                },
                {
                  key: 'settings' as DetailSection,
                  label: t('workflows.preview.settings'),
                  badge: undefined,
                  hintInline: null,
                },
              ] satisfies Array<{
                key: DetailSection;
                label: string;
                badge: string | undefined;
                hintInline: string | null;
              }>
            ).map(section => {
              const isOpen = openSection === section.key;
              return (
                <div
                  key={section.key}
                  className="border-b border-stone-100 dark:border-neutral-700 last:border-b-0">
                  <button
                    type="button"
                    onClick={() => setOpenSection(isOpen ? null : section.key)}
                    aria-expanded={isOpen}
                    aria-controls={`workflow-card-${workflow.id}-section-${section.key}`}
                    className="w-full flex items-center justify-between py-2 text-left text-xs font-medium text-stone-700 dark:text-neutral-200 hover:text-primary-700 dark:hover:text-primary-300">
                    <span className="flex items-center gap-2">
                      <span aria-hidden className="text-stone-400 dark:text-neutral-500">
                        {isOpen ? '⌃' : '▸'}
                      </span>
                      {section.label}
                      {section.key === 'prompt' && section.hintInline && (
                        <span className="text-stone-400 dark:text-neutral-500 truncate max-w-[200px] font-normal">
                          {section.hintInline}
                        </span>
                      )}
                    </span>
                    {section.badge && (
                      <span className="text-[11px] text-stone-500 dark:text-neutral-400 font-normal">
                        ({section.badge})
                      </span>
                    )}
                  </button>
                  {isOpen && section.key === 'prompt' && (
                    <div id={`workflow-card-${workflow.id}-section-prompt`} className="pb-3 pl-5">
                      <div className="relative">
                        <pre
                          className="font-mono text-[11px] bg-stone-50 dark:bg-neutral-800 text-stone-700 dark:text-neutral-200 p-2 rounded-md max-h-64 overflow-auto whitespace-pre-wrap break-words"
                          aria-label="Agent prompt">
                          {agentPrompt || (
                            <span className="italic text-stone-400">No prompt set.</span>
                          )}
                        </pre>
                        {agentPrompt && (
                          <button
                            type="button"
                            onClick={handleCopyPrompt}
                            className="absolute top-1 right-1 text-[10px] text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 bg-white/80 dark:bg-neutral-900/80 px-1.5 py-0.5 rounded">
                            {promptCopied
                              ? t('workflows.preview.copied')
                              : t('workflows.preview.copy_prompt')}
                          </button>
                        )}
                      </div>
                    </div>
                  )}
                  {isOpen && section.key === 'connections' && (
                    <div
                      id={`workflow-card-${workflow.id}-section-connections`}
                      className="pb-3 pl-5">
                      {requiredConnections.length === 0 ? (
                        <p className="text-xs text-stone-500 dark:text-neutral-400 italic">
                          No connections required.
                        </p>
                      ) : (
                        <ul className="text-xs text-stone-700 dark:text-neutral-300 space-y-1">
                          {requiredConnections.map((ref, i) => {
                            const isMissing = missingConnections.some(
                              m => JSON.stringify(m) === JSON.stringify(ref)
                            );
                            return (
                              <li key={i} className="flex items-center justify-between gap-2">
                                <span className="truncate">
                                  {ref.type === 'composio'
                                    ? `Composio · ${ref.toolkit_id}`
                                    : ref.type === 'channel'
                                      ? `Channel · ${ref.provider}`
                                      : ref.type === 'webview'
                                        ? `Webview · ${ref.provider}`
                                        : ref.type === 'builtin'
                                          ? `Built-in · ${ref.integration}`
                                          : ref.type === 'mcp'
                                            ? `MCP · ${ref.server_id}`
                                            : 'HTTP'}
                                </span>
                                <span
                                  className={
                                    isMissing
                                      ? 'text-amber-700 dark:text-amber-300 whitespace-nowrap'
                                      : 'text-sage-700 dark:text-sage-300 whitespace-nowrap'
                                  }>
                                  {isMissing ? '⚠ not connected' : '✓ connected'}
                                </span>
                              </li>
                            );
                          })}
                        </ul>
                      )}
                    </div>
                  )}
                  {isOpen && section.key === 'settings' && (
                    <div id={`workflow-card-${workflow.id}-section-settings`} className="pb-3 pl-5">
                      <table className="w-full text-xs">
                        <tbody>
                          {settingsRows.map(row => (
                            <tr
                              key={row.k}
                              className="border-t border-stone-100 dark:border-neutral-800 first:border-t-0">
                              <td className="py-1 font-mono text-stone-500 dark:text-neutral-400">
                                {row.k}
                              </td>
                              <td className="py-1 text-stone-700 dark:text-neutral-300 text-right">
                                {row.v}
                              </td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
