/**
 * One catalog tile rendered by [`StarterWorkflowsSection`]. Surfaces a
 * starter template (RU-1..RU-4 from F-5) with two CTAs that drive the
 * `addStarterTemplate` thunk:
 *
 *   - `[Add]`        → workflows_create with origin = Seed{template_id}
 *   - `[Add & Enable]` → above + workflows_enable
 *
 * Required-connection pills surface every `ConnectionRef` the template
 * needs. Missing refs get an amber `⚠ Needs` prefix + a tooltip with
 * the canonical label.
 */
import { useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import { addStarterTemplate, selectStarterPending } from '../../store/workflowsSlice';
import type { ConnectionRef } from '../../types/connections';
import type { StarterTemplateView } from '../../types/workflows';

function refLabel(c: ConnectionRef): string {
  switch (c.type) {
    case 'composio':
      return c.account_id ? `${c.toolkit_id} (${c.account_id})` : c.toolkit_id;
    case 'channel':
      return c.channel_id ? `${c.provider} → ${c.channel_id}` : c.provider;
    case 'webview':
      return c.account_id ? `${c.provider} (${c.account_id})` : c.provider;
    case 'builtin':
      return c.integration;
    case 'mcp':
      return c.tool_name ? `${c.server_id} / ${c.tool_name}` : c.server_id;
    case 'generic_http':
      return `HTTP: ${c.connection_id}`;
  }
}

function refKey(c: ConnectionRef): string {
  switch (c.type) {
    case 'composio':
      return `composio:${c.toolkit_id}:${c.account_id ?? ''}`;
    case 'channel':
      return `channel:${c.provider}:${c.channel_id ?? ''}`;
    case 'webview':
      return `webview:${c.provider}:${c.account_id ?? ''}`;
    case 'builtin':
      return `builtin:${c.integration}`;
    case 'mcp':
      return `mcp:${c.server_id}:${c.tool_name ?? ''}`;
    case 'generic_http':
      return `generic_http:${c.connection_id}`;
  }
}

interface Props {
  template: StarterTemplateView;
}

export default function StarterWorkflowCard({ template }: Props) {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const pending = useAppSelector(selectStarterPending(template.template_id));
  const [intent, setIntent] = useState<'add' | 'add_enable' | null>(null);

  const missingKeys = new Set(template.missing_connections.map(refKey));

  const onAdd = (enableImmediately: boolean) => {
    if (pending) return;
    setIntent(enableImmediately ? 'add_enable' : 'add');
    void dispatch(addStarterTemplate({ template, enableImmediately }));
  };

  return (
    <div
      data-testid={`starter-workflow-card-${template.template_id}`}
      className="bg-white dark:bg-neutral-900 rounded-2xl shadow-subtle border border-stone-200 dark:border-neutral-700 p-4 flex flex-col gap-3">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <h3
            className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate"
            title={template.name}>
            {template.name}
          </h3>
          <p className="mt-1 text-xs text-stone-500 dark:text-neutral-400 line-clamp-3">
            {template.description}
          </p>
        </div>
      </div>

      <div className="text-[11px] text-stone-500 dark:text-neutral-400">
        ⏰ {template.trigger_summary}
      </div>

      <div
        className="flex flex-wrap gap-1"
        data-testid={`starter-workflow-pills-${template.template_id}`}>
        {template.required_connections.map(ref => {
          const missing = missingKeys.has(refKey(ref));
          const label = refLabel(ref);
          return (
            <span
              key={refKey(ref)}
              title={
                missing ? `${t('workflows.starter_card.missing_connections')}: ${label}` : label
              }
              data-missing={missing ? 'true' : 'false'}
              className={
                'inline-flex items-center gap-1 px-2 py-0.5 text-[10px] rounded-full border ' +
                (missing
                  ? 'text-amber-700 dark:text-amber-400 border-amber-300 bg-amber-50 dark:bg-amber-950/30'
                  : 'text-stone-600 dark:text-neutral-300 border-stone-300 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800/40')
              }>
              {missing ? '⚠ ' : ''}
              {label}
            </span>
          );
        })}
      </div>

      <div className="flex justify-end gap-2 mt-1">
        <button
          type="button"
          onClick={() => onAdd(false)}
          disabled={pending}
          data-testid={`starter-workflow-add-${template.template_id}`}
          className="px-3 py-1.5 text-xs font-medium text-primary-700 dark:text-primary-300 hover:bg-primary-50 dark:hover:bg-primary-950/30 border border-primary-300 dark:border-primary-800 rounded-lg transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-60 disabled:cursor-not-allowed">
          {pending && intent === 'add'
            ? t('workflows.starter_adding')
            : t('workflows.add_to_my_workflows')}
        </button>
        <button
          type="button"
          onClick={() => onAdd(true)}
          disabled={pending}
          data-testid={`starter-workflow-add-enable-${template.template_id}`}
          className="px-3 py-1.5 text-xs font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg shadow-soft transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500 disabled:opacity-60 disabled:cursor-not-allowed">
          {pending && intent === 'add_enable'
            ? t('workflows.starter_adding')
            : t('workflows.add_and_enable')}
        </button>
      </div>
    </div>
  );
}
