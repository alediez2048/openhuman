/**
 * Workflow enable/disable toggle — the primary action on
 * [`WorkflowCard`] (FR-1.2.3).
 *
 * Disabled when `health.type !== 'ready'`: the backend RPC stays
 * permissive but the UI is the gate per FR-1.2.4 / ADR-011. The
 * disabled-tooltip lists the missing connections so the user knows
 * exactly what to fix.
 */
import { useT } from '../../lib/i18n/I18nContext';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import { disableWorkflow, enableWorkflow, selectWorkflowPending } from '../../store/workflowsSlice';
import type { WorkflowHealth, WorkflowId } from '../../types/workflows';
import { missingConnectionLabels } from './WorkflowHealthBadge';

interface Props {
  workflowId: WorkflowId;
  enabled: boolean;
  health: WorkflowHealth;
}

export default function WorkflowEnableToggle({ workflowId, enabled, health }: Props) {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const pending = useAppSelector(selectWorkflowPending(workflowId));

  const healthy = health.type === 'ready';
  // Allow disabling a Ready-but-currently-enabled workflow even though
  // health is unhealthy on subsequent re-toggles. The gate only blocks
  // the off → on transition.
  const blocked = !enabled && !healthy;
  const isBusy = pending;

  const labelOn = t('workflows.disable');
  const labelOff = t('workflows.enable');
  const label = enabled ? labelOn : labelOff;

  const disabledTooltip = blocked
    ? t('workflows.toggle_disabled_health').replace('{reason}', missingConnectionLabels(health))
    : undefined;

  const onClick = () => {
    if (blocked || isBusy) return;
    if (enabled) {
      void dispatch(disableWorkflow(workflowId));
    } else {
      void dispatch(enableWorkflow(workflowId));
    }
  };

  const disabled = blocked || isBusy;

  return (
    <button
      type="button"
      role="switch"
      aria-checked={enabled}
      aria-disabled={disabled}
      aria-label={label}
      disabled={disabled}
      title={disabledTooltip}
      onClick={onClick}
      data-testid={`workflow-enable-toggle-${workflowId}`}
      className={
        'relative inline-flex h-6 w-11 flex-none items-center rounded-full transition-colors ' +
        'focus:outline-none focus:ring-2 focus:ring-primary-500 ' +
        (enabled ? 'bg-primary-500' : 'bg-stone-300 dark:bg-neutral-700') +
        (disabled ? ' opacity-60 cursor-not-allowed' : ' cursor-pointer')
      }>
      <span
        className={
          'inline-block h-5 w-5 transform rounded-full bg-white shadow transition-transform ' +
          (enabled ? 'translate-x-5' : 'translate-x-0.5')
        }
        aria-hidden
      />
    </button>
  );
}
