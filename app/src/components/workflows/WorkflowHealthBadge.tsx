/**
 * Health-state pill rendered next to a workflow's name.
 *
 * Three Phase 1 visual states (FR-1.2.3):
 *   - Ready              → sage   ✓
 *   - NeedsConnections   → amber  ⚠
 *   - LastRunFailed      → coral  ✗   (Phase 2 placeholder)
 *   - SessionExpired     → coral  ⚠   (Phase 2 placeholder)
 *
 * Pure component — takes a `WorkflowHealth` and renders. Tooltip text
 * lists missing connection labels when relevant.
 */
import { useT } from '../../lib/i18n/I18nContext';
import type { ConnectionRef } from '../../types/connections';
import type { WorkflowHealth } from '../../types/workflows';

function refLabel(c: ConnectionRef): string {
  switch (c.type) {
    case 'composio':
      return c.account_id ? `${c.toolkit_id} (${c.account_id})` : c.toolkit_id;
    case 'channel':
      return `${c.provider} → ${c.channel_id}`;
    case 'webview':
      return `${c.provider} (${c.account_id})`;
    case 'builtin':
      return c.integration;
    case 'mcp':
      return c.tool_name ? `${c.server_id} / ${c.tool_name}` : c.server_id;
    case 'generic_http':
      return `HTTP: ${c.connection_id}`;
  }
}

interface Props {
  health: WorkflowHealth;
}

export default function WorkflowHealthBadge({ health }: Props) {
  const { t } = useT();

  switch (health.type) {
    case 'ready':
      return (
        <span
          aria-label={t('workflows.health.ready')}
          data-testid="workflow-health-badge-ready"
          className="inline-flex items-center gap-1 text-[11px] font-medium text-sage-700 dark:text-sage-400">
          <span className="w-1.5 h-1.5 rounded-full bg-sage-500" aria-hidden />
          {t('workflows.health.ready')}
        </span>
      );

    case 'needs_connections': {
      const labels = health.missing.map(refLabel).join(', ');
      const aria = `${t('workflows.health.needs_connections')}: ${labels}`;
      return (
        <span
          aria-label={aria}
          title={aria}
          data-testid="workflow-health-badge-needs-connections"
          className="inline-flex items-center gap-1 text-[11px] font-medium text-amber-700 dark:text-amber-400">
          <span className="w-1.5 h-1.5 rounded-full bg-amber-500" aria-hidden />
          {t('workflows.health.needs_connections')}
        </span>
      );
    }

    case 'last_run_failed':
      return (
        <span
          aria-label={t('workflows.health.last_run_failed')}
          title={health.reason}
          data-testid="workflow-health-badge-last-run-failed"
          className="inline-flex items-center gap-1 text-[11px] font-medium text-coral-600">
          <span className="w-1.5 h-1.5 rounded-full bg-coral-500" aria-hidden />
          {t('workflows.health.last_run_failed')}
        </span>
      );

    case 'session_expired':
      return (
        <span
          aria-label={t('workflows.health.session_expired')}
          title={refLabel(health.connection)}
          data-testid="workflow-health-badge-session-expired"
          className="inline-flex items-center gap-1 text-[11px] font-medium text-coral-600">
          <span className="w-1.5 h-1.5 rounded-full bg-coral-500" aria-hidden />
          {t('workflows.health.session_expired')}
        </span>
      );
  }
}

/** Exported for `WorkflowEnableToggle` so the tooltip and the badge stay in sync. */
export function missingConnectionLabels(health: WorkflowHealth): string {
  if (health.type === 'needs_connections') {
    return health.missing.map(refLabel).join(', ');
  }
  if (health.type === 'session_expired') {
    return refLabel(health.connection);
  }
  if (health.type === 'last_run_failed') {
    return health.reason;
  }
  return '';
}
