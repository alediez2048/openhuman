/**
 * Discarded-state terminal stub. Muted "Discarded — Undo" affordance.
 * The Undo button reverts the preview to `pending`; the parent owns
 * the 15s timeout (so the stub stays focused on rendering, not
 * timer management).
 */
import { useT } from '../../../../lib/i18n/I18nContext';

interface Props {
  onUndo: () => void;
}

export function DiscardedStub({ onUndo }: Props) {
  const { t } = useT();
  return (
    <div className="bg-stone-50 dark:bg-neutral-800 rounded-xl border border-stone-200 dark:border-neutral-700 px-3 py-2 max-w-[560px] flex items-center justify-between gap-3">
      <span className="text-xs text-stone-500 dark:text-neutral-400">
        {t('workflows.preview.discarded_undo')}
      </span>
      <button
        type="button"
        onClick={onUndo}
        className="text-xs text-primary-600 hover:text-primary-700 hover:underline font-medium">
        Undo
      </button>
    </div>
  );
}
