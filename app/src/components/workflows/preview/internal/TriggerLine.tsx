/**
 * Compact "when this fires" line under the proposal header.
 * Routes the typed [`Trigger`] through `useCronHumanizer`.
 *
 * Type-led label ("Trigger · …") rather than a leading emoji — the
 * cosmetic ⏰ read as kitsch on the saved-workflow admin surface
 * and didn't add semantic value the label couldn't carry. Same
 * treatment now applies in the proposal preview for consistency.
 */
import type { Trigger } from '../../../../types/workflows';
import { useCronHumanizer } from '../hooks/useCronHumanizer';

interface Props {
  trigger: Trigger;
}

export function TriggerLine({ trigger }: Props) {
  const label = useCronHumanizer(trigger);
  return (
    <div className="text-xs text-stone-600 dark:text-neutral-400 mt-2">
      <span className="font-medium text-stone-500 dark:text-neutral-500">Trigger</span>
      <span className="mx-1.5 text-stone-300 dark:text-neutral-600" aria-hidden>
        ·
      </span>
      <span>{label}</span>
    </div>
  );
}
