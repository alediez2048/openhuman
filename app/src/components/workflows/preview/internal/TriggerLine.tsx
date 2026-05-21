/**
 * Compact "when this fires" line under the proposal header.
 * Routes the typed [`Trigger`] through `useCronHumanizer`.
 */
import type { Trigger } from '../../../../types/workflows';
import { useCronHumanizer } from '../hooks/useCronHumanizer';

interface Props {
  trigger: Trigger;
}

export function TriggerLine({ trigger }: Props) {
  const label = useCronHumanizer(trigger);
  return (
    <div className="text-xs text-stone-600 dark:text-neutral-400 mt-2 flex items-center gap-1.5">
      <span aria-hidden className="select-none">
        ⏰
      </span>
      <span>{label}</span>
    </div>
  );
}
