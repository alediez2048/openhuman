/**
 * Action row at the bottom of the proposal preview cards. Three
 * buttons: Discard (tertiary text), Save (paused) (ghost), Save &
 * Enable (primary). Tab order matches macOS dialog convention —
 * tertiary first, primary last.
 */
import { useT } from '../../../../lib/i18n/I18nContext';

export type ActionState = 'pending' | 'saving' | 'error';

interface Props {
  state: ActionState;
  errorMessage?: string | null;
  canSaveEnable: boolean;
  onDiscard: () => void;
  onSavePaused: () => void;
  onSaveAndEnable: () => void;
  onRetry: () => void;
}

export function ActionRow({
  state,
  errorMessage,
  canSaveEnable,
  onDiscard,
  onSavePaused,
  onSaveAndEnable,
  onRetry,
}: Props) {
  const { t } = useT();
  const busy = state === 'saving';

  if (state === 'error') {
    return (
      <div className="mt-3 flex items-center justify-between gap-3 text-xs">
        <span role="alert" className="text-coral-600 dark:text-coral-400">
          {t('workflows.preview.couldnt_save').replace('{reason}', errorMessage ?? 'unknown error')}
        </span>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={onDiscard}
            className="px-3 py-1.5 text-stone-600 dark:text-neutral-400 hover:text-stone-900 dark:hover:text-neutral-100 font-medium">
            {t('workflows.preview.discard')}
          </button>
          <button
            type="button"
            onClick={onRetry}
            className="px-3 py-1.5 rounded-lg bg-primary-600 text-white hover:bg-primary-700 font-medium">
            {t('workflows.preview.retry')}
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="mt-3 flex items-center justify-between gap-3">
      <button
        type="button"
        onClick={onDiscard}
        disabled={busy}
        className="px-3 py-1.5 text-xs text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 disabled:opacity-50 font-medium">
        {t('workflows.preview.discard')}
      </button>
      <div className="flex items-center gap-2">
        <button
          type="button"
          onClick={onSavePaused}
          disabled={busy}
          className="px-3 py-1.5 text-xs rounded-lg border border-primary-300 dark:border-primary-700 text-primary-700 dark:text-primary-300 hover:bg-primary-50 dark:hover:bg-primary-950 disabled:opacity-50 font-medium">
          {busy ? t('workflows.preview.saving') : t('workflows.preview.save_paused')}
        </button>
        <button
          type="button"
          onClick={onSaveAndEnable}
          disabled={busy || !canSaveEnable}
          className="px-3 py-1.5 text-xs rounded-lg bg-primary-600 text-white hover:bg-primary-700 disabled:opacity-50 disabled:cursor-not-allowed font-medium">
          {t('workflows.preview.save_enable')}
        </button>
      </div>
    </div>
  );
}
