/**
 * Saved-state terminal stub the proposal card morphs into after a
 * successful Save. Sage left border, compact (~56px), and shows the
 * View → link into /workflows. The conversational follow-up message
 * lives in the parent agent message bubble (F-15 hero-flow wires
 * that piece via the synthetic user-message hook).
 */
import { useT } from '../../../../lib/i18n/I18nContext';

interface Props {
  name: string;
  mode: 'paused' | 'enabled';
  workflowId: string | null;
  onEnableNow?: () => void;
  busy?: boolean;
}

export function SavedStub({ name, mode, workflowId, onEnableNow, busy }: Props) {
  const { t } = useT();
  const header =
    mode === 'enabled'
      ? t('workflows.preview.saved_enabled').replace('{name}', name)
      : t('workflows.preview.saved_paused').replace('{name}', name);
  return (
    <div
      role="status"
      aria-live="polite"
      className="bg-white dark:bg-neutral-900 rounded-2xl border border-stone-200 dark:border-neutral-700 border-l-4 border-l-sage-500 p-3 max-w-[560px] flex items-center justify-between gap-3 motion-safe:animate-fadeIn">
      <div className="min-w-0">
        <p className="text-sm font-medium text-stone-900 dark:text-neutral-100 truncate">
          {header}
        </p>
        <p className="text-xs text-stone-500 dark:text-neutral-400 mt-0.5">
          {mode === 'paused' ? 'Paused' : 'Enabled'} ·{' '}
          {workflowId && (
            <a href={`#/workflows`} className="text-primary-600 hover:underline">
              {t('workflows.preview.view_workflow')}
            </a>
          )}
        </p>
      </div>
      {mode === 'paused' && onEnableNow && (
        <button
          type="button"
          onClick={onEnableNow}
          disabled={busy}
          className="text-xs px-3 py-1.5 rounded-lg border border-primary-300 text-primary-700 hover:bg-primary-50 disabled:opacity-50 font-medium whitespace-nowrap">
          {t('workflows.preview.enable_now')}
        </button>
      )}
    </div>
  );
}
