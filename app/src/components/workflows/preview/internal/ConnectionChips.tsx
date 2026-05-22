/**
 * Pill-shaped chips listing the proposal's required connections.
 * Missing ones (per the parallel `missing` list) render with an
 * amber `⚠` prefix + amber border. Healthy ones render plain.
 *
 * Type-led "Connections · …" label rather than a leading emoji
 * (`🔌` previously). The pills themselves carry the semantic;
 * a decorative icon ahead of them read as kitsch on the saved-
 * workflow admin surface. The `⚠` on missing pills stays — it's
 * a load-bearing signal, not decoration.
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
    <div
      className="flex items-baseline gap-1.5 mt-1.5 flex-wrap"
      aria-label="Required connections">
      <span className="text-xs font-medium text-stone-500 dark:text-neutral-500">Connections</span>
      <span className="text-xs text-stone-300 dark:text-neutral-600" aria-hidden>
        ·
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
