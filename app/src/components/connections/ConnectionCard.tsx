/**
 * Generic connection card used by every section in the Connections Hub.
 *
 * Section components pass `name`, `subtitle`, `status`, and optional `actions`
 * slot. The card stays visually identical across all six mechanisms to give
 * the page a unified rhythm.
 */
import { type ReactNode } from 'react';

import type { ConnectionStatus } from '../../types/connections';

interface ConnectionCardProps {
  name: string;
  subtitle?: string;
  status: ConnectionStatus;
  /** Right-aligned action slot (buttons, toggles, etc.). */
  actions?: ReactNode;
  /** Stable identifier for analytics + tests. */
  testId?: string;
}

function StatusBadge({ status }: { status: ConnectionStatus }) {
  switch (status.kind) {
    case 'connected':
      return (
        <span
          className="inline-flex items-center gap-1 text-xs text-sage-700 dark:text-sage-400"
          title="Connected">
          <span className="w-1.5 h-1.5 rounded-full bg-sage-500" />
          Connected
        </span>
      );
    case 'not_connected':
      return (
        <span
          className="inline-flex items-center gap-1 text-xs text-stone-500 dark:text-neutral-400"
          title="Not connected">
          <span className="w-1.5 h-1.5 rounded-full bg-stone-400" />
          Not connected
        </span>
      );
    case 'disabled':
      return (
        <span
          className="inline-flex items-center gap-1 text-xs text-stone-500 dark:text-neutral-400"
          title="Disabled">
          <span className="w-1.5 h-1.5 rounded-full bg-stone-300" />
          Disabled
        </span>
      );
    case 'error':
      return (
        <span
          className="inline-flex items-center gap-1 text-xs text-coral-600"
          title={status.reason}>
          <span className="w-1.5 h-1.5 rounded-full bg-coral-500" />
          Error
        </span>
      );
  }
}

export default function ConnectionCard({
  name,
  subtitle,
  status,
  actions,
  testId,
}: ConnectionCardProps) {
  return (
    <div
      className="flex items-center justify-between gap-3 px-3.5 py-3 bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-700 rounded-xl shadow-subtle hover:shadow-soft transition-shadow"
      data-testid={testId}>
      <div className="min-w-0 flex-1">
        <div className="text-sm font-medium text-stone-900 dark:text-neutral-100 truncate">
          {name}
        </div>
        <div className="mt-0.5 flex items-center gap-2 min-w-0">
          <StatusBadge status={status} />
          {subtitle ? (
            <span className="text-xs text-stone-500 dark:text-neutral-400 truncate">
              · {subtitle}
            </span>
          ) : null}
        </div>
      </div>
      {actions ? <div className="flex-shrink-0">{actions}</div> : null}
    </div>
  );
}
