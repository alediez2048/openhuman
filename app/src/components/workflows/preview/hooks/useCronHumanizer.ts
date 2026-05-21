/**
 * Pure helper that converts a [`Trigger`] into a human-readable
 * "when this fires" string for the preview cards (F-14). No hook
 * dependencies (despite the `use*` name) — kept stable so consumers
 * can call it inline.
 *
 * Phase 1 supports `cron` + `manual`; the Phase 2 / 3 variants
 * fall back to a stable label (`On webhook trigger`, `On Composio
 * event`, `On channel message`) so the renderer never panics on
 * forward-compat payloads emitted by future drafting agents.
 *
 * We intentionally avoid pulling in `cronstrue` for Phase 1 — the
 * starter templates use a small, fixed set of expressions and the
 * deterministic table here is more predictable across locales than
 * `cronstrue`'s "Every weekday at 8 AM" output, which has subtle
 * pluralization issues in some chunks. A follow-up ticket can swap
 * in `cronstrue` if user feedback demands richer humanization.
 */
import type { Trigger } from '../../../../types/workflows';

const HOUR_RANGE_WEEKDAYS = /^0 (\d{1,2}) \* \* 1-5$/;
const HOUR_DAILY = /^0 (\d{1,2}) \* \* \*$/;
const EVERY_N_MINUTES = /^\*\/(\d{1,2}) \* \* \* \*$/;
const HOURLY_AT = /^(\d{1,2}) \* \* \* \*$/;

function hourLabel(hour: number): string {
  if (hour === 0) return '12am';
  if (hour < 12) return `${hour}am`;
  if (hour === 12) return '12pm';
  return `${hour - 12}pm`;
}

/**
 * Returns a human label for a single cron expression. Falls back to
 * `Cron: <expr>` when the shape isn't one of the canonical starter-
 * template forms.
 */
export function humanizeCron(expr: string): string {
  let m = expr.match(HOUR_RANGE_WEEKDAYS);
  if (m) return `Every weekday at ${hourLabel(parseInt(m[1]!, 10))}`;
  m = expr.match(HOUR_DAILY);
  if (m) return `Every day at ${hourLabel(parseInt(m[1]!, 10))}`;
  m = expr.match(EVERY_N_MINUTES);
  if (m) return `Every ${m[1]} minutes`;
  m = expr.match(HOURLY_AT);
  if (m) return `Hourly at :${m[1]!.padStart(2, '0')}`;
  return `Cron: ${expr}`;
}

/**
 * Hook-shaped façade so consumers can keep the
 * `useCronHumanizer(trigger)` call site, but the implementation is
 * deterministic + stateless. Returns the localised "when" label for
 * the preview's trigger line.
 */
export function useCronHumanizer(trigger: Trigger): string {
  switch (trigger.type) {
    case 'cron': {
      const base = humanizeCron(trigger.expr);
      return trigger.tz ? `${base} (${trigger.tz})` : base;
    }
    case 'manual':
      return 'Run on demand';
    case 'webhook':
      return `On webhook trigger → ${trigger.target_path}`;
    case 'composio_event':
      return `On ${trigger.toolkit} event: ${trigger.trigger_id}`;
    case 'channel_message':
      return `On ${trigger.provider} message`;
    default:
      return 'Custom trigger';
  }
}
