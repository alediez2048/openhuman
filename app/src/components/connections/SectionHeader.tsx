/**
 * Shared header for each section of the Connections Hub.
 *
 * Renders the section title, a count badge, an optional CTA slot, and an
 * optional collapse toggle. Keep this tiny — sections themselves own their
 * card grid.
 */
import { type ReactNode } from 'react';

interface SectionHeaderProps {
  title: string;
  /** Number of cards rendered in the section body. */
  count: number;
  /** Optional CTA slot (e.g. "+ Add HTTP service"). Rendered right-aligned. */
  cta?: ReactNode;
  /** Optional descriptive subtitle (single line). */
  subtitle?: string;
}

export default function SectionHeader({ title, count, cta, subtitle }: SectionHeaderProps) {
  return (
    <div className="flex items-baseline justify-between mb-2.5 mt-5 first:mt-0">
      <div className="flex items-baseline gap-2 min-w-0">
        <h2 className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate">
          {title}
        </h2>
        <span className="text-xs text-stone-500 dark:text-neutral-400 tabular-nums">{count}</span>
        {subtitle ? (
          <span className="text-xs text-stone-400 dark:text-neutral-500 truncate">
            · {subtitle}
          </span>
        ) : null}
      </div>
      {cta ? <div className="flex-shrink-0">{cta}</div> : null}
    </div>
  );
}
