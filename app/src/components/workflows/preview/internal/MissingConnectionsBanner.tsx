/**
 * Amber banner rendered above the action row when the proposal
 * references connections the user hasn't set up. Save (paused)
 * remains enabled — the workflow saves with
 * `health = NeedsConnections { missing }` and lives paused until
 * the user connects them. Save & Enable is disabled (workflow
 * can't actually fire yet).
 */
import { useT } from '../../../../lib/i18n/I18nContext';
import type { ConnectionRef } from '../../../../types/connections';
import { useConnectionMeta } from '../hooks/useConnectionMeta';

interface Props {
  missing: ConnectionRef[];
  onManage?: () => void;
}

export function MissingConnectionsBanner({ missing, onManage }: Props) {
  const { t } = useT();
  const metas = useConnectionMeta(missing);
  if (missing.length === 0) return null;
  const names = metas.map(m => m.label).join(', ');
  const banner = t('workflows.preview.missing_connections_banner').replace('{names}', names);
  return (
    <div
      role="alert"
      className="mt-3 px-3 py-2 rounded-lg border-l-4 border-amber-400 bg-amber-50 text-amber-800 text-xs flex items-center justify-between gap-2">
      <span>
        <span aria-hidden className="mr-1">
          ⚠
        </span>
        {banner}
      </span>
      {onManage && (
        <button
          type="button"
          onClick={onManage}
          className="text-amber-800 hover:text-amber-900 hover:underline font-medium whitespace-nowrap">
          {t('workflows.preview.manage_connections')}
        </button>
      )}
    </div>
  );
}
