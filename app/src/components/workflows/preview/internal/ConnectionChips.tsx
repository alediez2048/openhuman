/**
 * Pill-shaped chips listing the proposal's required connections.
 * Missing ones (per the parallel `missing` list) render with an
 * amber `⚠` prefix + amber border.
 */
import type { ConnectionRef } from '../../../../types/connections';
import { useConnectionMeta } from '../hooks/useConnectionMeta';

interface Props {
  required: ConnectionRef[];
  missing: ConnectionRef[];
}

function isMissing(r: ConnectionRef, missing: ConnectionRef[]): boolean {
  return missing.some(m => JSON.stringify(m) === JSON.stringify(r));
}

export function ConnectionChips({ required, missing }: Props) {
  const metas = useConnectionMeta(required);
  if (required.length === 0) {
    return null;
  }
  return (
    <div className="flex items-center gap-1.5 mt-1.5 flex-wrap" aria-label="Required connections">
      <span aria-hidden className="text-xs text-stone-500 dark:text-neutral-500 select-none">
        🔌
      </span>
      {metas.map((m, i) => {
        const ref = required[i]!;
        const missingFlag = isMissing(ref, missing);
        const baseClasses =
          'inline-flex items-center gap-1 rounded-full text-[11px] font-medium px-2 py-0.5 transition-colors';
        const variantClasses = missingFlag
          ? 'bg-amber-50 text-amber-800 border border-amber-300'
          : 'bg-stone-100 text-stone-700 border border-stone-200 dark:bg-neutral-800 dark:text-neutral-200 dark:border-neutral-700';
        return (
          <span
            key={m.refKey}
            className={`${baseClasses} ${variantClasses}`}
            title={`${m.mechanism}: ${m.label}${missingFlag ? ' (not connected)' : ''}`}>
            {missingFlag && <span aria-hidden>⚠</span>}
            {m.label}
          </span>
        );
      })}
    </div>
  );
}
