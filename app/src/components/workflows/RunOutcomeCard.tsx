/**
 * T-2 (Phase 2.5 Trust UX): per-run outcome card.
 *
 * Replaces the previous "Succeeded — [agent narrative blob]" rendering
 * with a structured card that lists every concrete side effect the run
 * produced. Each `DeliveryReceipt` from T-1 becomes one plain-English
 * row with a deep link to the resulting artifact (Gmail, Slack,
 * Calendar, etc.).
 *
 * Two surface modes:
 *   - Succeeded with receipts → "What this run did" section listing
 *     each side effect; agent narrative collapsed behind a disclosure.
 *   - Failed → "Why this run failed" with the step error message.
 *     (T-4 will replace the raw error with a structured
 *     `FailureReason` + fix-it action; T-2 ships the visual shape
 *     so T-4 is a drop-in extension.)
 *
 * Conservative-success warning: if the run succeeded AND was granted
 * action connections (allowed_connections non-empty) AND produced zero
 * receipts, render a yellow subtitle. F-21 should prevent this from
 * happening (it flips such runs to Failed) — the subtitle is the belt
 * for those suspenders.
 */
import { useState } from 'react';
import { useT } from '../../lib/i18n/I18nContext';
import { invoke } from '@tauri-apps/api/core';
import type {
  DeliveryReceipt,
  Run,
  RunStep,
  RunStatus,
  Workflow,
} from '../../types/workflows';

interface Props {
  run: Run;
  steps: RunStep[];
  /** Used for the conservative-success zero-receipt warning. */
  workflow?: Workflow;
}

/**
 * All `DeliveryReceipt`s across every step, in chronological order.
 * Phase 1 workflows are single-node so this is typically `steps[0].delivery_receipts`,
 * but a multi-node Phase 2 chain accumulates receipts across steps.
 */
function collectReceipts(steps: RunStep[]): DeliveryReceipt[] {
  const out: DeliveryReceipt[] = [];
  for (const step of steps) {
    if (step.delivery_receipts && step.delivery_receipts.length > 0) {
      out.push(...step.delivery_receipts);
    }
  }
  return out;
}

/**
 * The agent's final narrative text, pulled out of the last step's
 * `output_json.text`. Carried for the "Agent's notes" disclosure —
 * useful context when the structured receipts don't tell the full
 * story, but no longer the headline.
 */
function readNarrative(steps: RunStep[]): string {
  for (let i = steps.length - 1; i >= 0; i--) {
    const raw = steps[i]?.output_json;
    if (!raw) continue;
    try {
      const parsed = JSON.parse(raw) as { text?: string };
      if (typeof parsed.text === 'string') return parsed.text.trim();
    } catch {
      // Some node kinds (tool_call, http_request) store non-`{text}`
      // shapes here. Skip — the narrative disclosure is best-effort.
    }
  }
  return '';
}

function statusIcon(status: RunStatus): string {
  switch (status) {
    case 'succeeded':
      return '✅';
    case 'failed':
      return '❌';
    case 'cancelled':
      return '⊘';
    case 'running':
    case 'pending':
      return '⏳';
    case 'timed_out':
      return '⏱';
    default:
      return '•';
  }
}

function statusLabelKey(status: RunStatus): string {
  switch (status) {
    case 'succeeded':
      return 'workflows.outcome.succeeded';
    case 'failed':
      return 'workflows.outcome.failed';
    case 'cancelled':
      return 'workflows.outcome.cancelled';
    case 'running':
    case 'pending':
      return 'workflows.outcome.in_progress';
    case 'timed_out':
      return 'workflows.outcome.timed_out';
    default:
      return 'workflows.outcome.in_progress';
  }
}

interface ReceiptRowProps {
  receipt: DeliveryReceipt;
  t: (key: string) => string;
}

/**
 * Icon + primary line + optional secondary + optional [Open in X] link.
 * One row per receipt. Click [Open] dispatches via the Tauri opener so
 * the link opens in the default browser, not inside the embedded webview.
 */
function ReceiptRow({ receipt, t }: ReceiptRowProps) {
  const { icon, primary, openLabel } = describeReceipt(receipt, t);

  const handleOpen = () => {
    if (!receipt.link) return;
    // `plugin:opener|open_url` is the Tauri opener plugin's command.
    // Best-effort: failures (e.g. malformed URL) get logged but don't
    // crash the surrounding card.
    invoke('plugin:opener|open_url', { url: receipt.link }).catch((err: unknown) => {
      console.warn('[workflows-ui] receipt open_url failed', receipt.link, err);
    });
  };

  return (
    <div
      data-testid="receipt-row"
      className="flex items-start gap-3 py-2 px-3 rounded-md hover:bg-stone-50 dark:hover:bg-stone-800/30"
    >
      <span className="text-lg leading-none mt-0.5" aria-hidden="true">
        {icon}
      </span>
      <div className="flex-1 min-w-0">
        <div className="text-sm text-stone-900 dark:text-stone-100 truncate">{primary}</div>
        {receipt.message_id && (
          <div className="text-xs text-stone-500 dark:text-stone-400 truncate font-mono">
            {receipt.message_id}
          </div>
        )}
      </div>
      {receipt.link && (
        <button
          type="button"
          onClick={handleOpen}
          className="text-xs px-2 py-1 rounded text-primary-700 dark:text-primary-300 hover:bg-primary-50 dark:hover:bg-primary-900/30 whitespace-nowrap"
        >
          {openLabel} →
        </button>
      )}
    </div>
  );
}

/**
 * Map a `DeliveryReceipt` to its UI surface: emoji icon, plain-English
 * primary line, and [Open in X] button label (when there's a deep link).
 * Pure function — no hooks, no side effects — so it's easily testable.
 */
function describeReceipt(
  receipt: DeliveryReceipt,
  t: (key: string) => string
): { icon: string; primary: string; openLabel: string } {
  const kind = receipt.side_effect_kind;
  const recipient = receipt.recipient ?? t('workflows.outcome.unknown_recipient');
  switch (kind.kind) {
    case 'email_sent':
      return {
        icon: '📧',
        primary: t('workflows.outcome.email_sent_to').replace('{recipient}', recipient),
        openLabel: t('workflows.outcome.open_in_gmail'),
      };
    case 'message_posted':
      return {
        icon: '💬',
        primary: t('workflows.outcome.message_posted_in')
          .replace('{provider}', humanProvider(kind.provider))
          .replace('{recipient}', recipient),
        openLabel: t('workflows.outcome.open_message'),
      };
    case 'file_created':
      return {
        icon: '📄',
        primary: t('workflows.outcome.file_created_in')
          .replace('{provider}', humanProvider(kind.provider))
          .replace('{title}', recipient),
        openLabel: t('workflows.outcome.open_file'),
      };
    case 'record_created':
      return {
        icon: '🗂',
        primary: t('workflows.outcome.record_created_in')
          .replace('{provider}', humanProvider(kind.provider))
          .replace('{name}', recipient),
        openLabel: t('workflows.outcome.open_record'),
      };
    case 'record_updated':
      return {
        icon: '✏️',
        primary: t('workflows.outcome.record_updated_in')
          .replace('{provider}', humanProvider(kind.provider))
          .replace('{name}', recipient),
        openLabel: t('workflows.outcome.open_record'),
      };
    case 'calendar_event_created':
      return {
        icon: '📅',
        primary: t('workflows.outcome.calendar_event_created').replace('{title}', recipient),
        openLabel: t('workflows.outcome.open_calendar'),
      };
    case 'other':
      return {
        icon: '⚙️',
        primary: t('workflows.outcome.other_action')
          .replace('{verb}', kind.verb)
          .replace('{tool}', receipt.tool),
        openLabel: t('workflows.outcome.open_link'),
      };
  }
}

function humanProvider(slug: string): string {
  // Title-case the provider slug for display. "slack" → "Slack",
  // "googlecalendar" → "Googlecalendar" (good enough; a curated map
  // can come later).
  if (!slug) return slug;
  return slug.charAt(0).toUpperCase() + slug.slice(1);
}

function relativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  const now = Date.now();
  const diffMs = Math.max(0, now - then);
  if (diffMs < 60_000) return 'just now';
  const diffMin = Math.floor(diffMs / 60_000);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDay = Math.floor(diffHr / 24);
  return `${diffDay}d ago`;
}

function workflowHasActionConnections(workflow: Workflow | undefined): boolean {
  if (!workflow) return false;
  for (const node of workflow.nodes) {
    if (node.config && node.config.kind === 'agent_prompt') {
      if ((node.config.allowed_connections?.length ?? 0) > 0) return true;
    }
  }
  return false;
}

export default function RunOutcomeCard({ run, steps, workflow }: Props) {
  const { t } = useT();
  const [narrativeOpen, setNarrativeOpen] = useState(false);

  const receipts = collectReceipts(steps);
  const narrative = readNarrative(steps);
  const stepError = steps.find((s) => s.error)?.error ?? run.error ?? null;
  const isFailed = run.status === 'failed' || run.status === 'timed_out';
  const isTerminal = run.status !== 'running' && run.status !== 'pending';

  // F-21 belt-suspender: Succeeded with action connections but zero
  // receipts is a regression signal. Render a warning subtitle so the
  // user notices even if F-21's flip-to-Failed somehow doesn't fire.
  const showZeroReceiptsWarning =
    run.status === 'succeeded' &&
    receipts.length === 0 &&
    workflowHasActionConnections(workflow);

  return (
    <div
      data-testid="run-outcome-card"
      className="rounded-lg border border-stone-200 dark:border-stone-700 bg-white dark:bg-stone-900 p-4 space-y-3"
    >
      {/* Header */}
      <div className="flex items-center gap-2">
        <span className="text-xl leading-none" aria-hidden="true">
          {statusIcon(run.status)}
        </span>
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium text-stone-900 dark:text-stone-100">
            {t(statusLabelKey(run.status))}
            <span className="text-stone-500 dark:text-stone-400 font-normal">
              {' · '}
              {relativeTime(run.completed_at ?? run.started_at)}
            </span>
          </div>
          {showZeroReceiptsWarning && (
            <div className="text-xs text-amber-700 dark:text-amber-400 mt-0.5">
              {t('workflows.outcome.zero_receipts_warning')}
            </div>
          )}
        </div>
      </div>

      {/* Side effects (success branch) */}
      {!isFailed && receipts.length > 0 && (
        <div>
          <div className="text-xs uppercase tracking-wide text-stone-500 dark:text-stone-400 mb-1 px-3">
            {t('workflows.outcome.what_happened')}
          </div>
          <div className="space-y-0.5">
            {receipts.map((receipt, idx) => (
              <ReceiptRow key={`${receipt.tool}-${idx}`} receipt={receipt} t={t} />
            ))}
          </div>
        </div>
      )}

      {/* Failure (failed branch) */}
      {isFailed && (
        <div>
          <div className="text-xs uppercase tracking-wide text-stone-500 dark:text-stone-400 mb-1 px-3">
            {t('workflows.outcome.why_failed')}
          </div>
          <div
            data-testid="run-outcome-failure"
            className="px-3 py-2 rounded bg-coral-50 dark:bg-coral-950/30 text-sm text-coral-900 dark:text-coral-200 font-mono whitespace-pre-wrap break-words"
          >
            {stepError ?? t('workflows.outcome.unknown_failure')}
          </div>
        </div>
      )}

      {/* Still in flight */}
      {!isTerminal && (
        <div className="px-3 py-2 text-sm text-stone-600 dark:text-stone-400">
          {t('workflows.outcome.running_subtitle')}
        </div>
      )}

      {/* Collapsed narrative — context only, not the headline */}
      {narrative && (
        <div className="pt-1">
          <button
            type="button"
            onClick={() => setNarrativeOpen((v) => !v)}
            className="text-xs text-stone-500 dark:text-stone-400 hover:text-stone-700 dark:hover:text-stone-200 flex items-center gap-1"
          >
            <span aria-hidden="true">{narrativeOpen ? '▾' : '▸'}</span>
            <span>
              {narrativeOpen
                ? t('workflows.outcome.hide_agent_notes')
                : t('workflows.outcome.show_agent_notes')}
            </span>
          </button>
          {narrativeOpen && (
            <div
              data-testid="run-outcome-narrative"
              className="mt-2 px-3 py-2 text-sm text-stone-700 dark:text-stone-300 whitespace-pre-wrap bg-stone-50 dark:bg-stone-800/50 rounded"
            >
              {narrative}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
