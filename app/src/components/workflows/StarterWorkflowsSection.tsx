/**
 * Catalog section rendered on `/workflows`. Reads
 * `workflows_list_starter_templates` via `fetchStarterTemplates` and
 * renders one [`StarterWorkflowCard`] per template.
 *
 * Visibility rules (FR-1.2.6):
 *   - On a fresh workspace (`workflows.length === 0`), the section is
 *     ALWAYS shown — it's the empty-state hero, so the
 *     `hideStarterSection` preference is intentionally ignored.
 *   - With ≥1 workflow, the section honors `hideStarterSection`. A
 *     "Hide starter workflows" link in the section header flips the
 *     preference; users restore it from Settings (F-6.e wires the
 *     toggle).
 *   - When the catalog returns `[]` AND `workflows.length > 0`, render
 *     the "All starter workflows added" empty-catalog message.
 */
import { useEffect } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import {
  fetchStarterTemplates,
  selectHideStarterSection,
  selectStarterError,
  selectStarterLoadStatus,
  selectStarterTemplates,
  selectWorkflows,
  setHideStarterSection,
} from '../../store/workflowsSlice';
import StarterWorkflowCard from './StarterWorkflowCard';

export default function StarterWorkflowsSection() {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const templates = useAppSelector(selectStarterTemplates);
  const loadStatus = useAppSelector(selectStarterLoadStatus);
  const error = useAppSelector(selectStarterError);
  const workflows = useAppSelector(selectWorkflows);
  const hideStarterSection = useAppSelector(selectHideStarterSection);

  useEffect(() => {
    void dispatch(fetchStarterTemplates());
  }, [dispatch]);

  const isEmptyWorkspace = workflows.length === 0;
  // Per FR-1.2.6, the hide preference is overridden on an empty
  // workspace so the catalog can carry the empty-state hero load.
  const hidden = hideStarterSection && !isEmptyWorkspace;
  if (hidden) {
    return null;
  }

  const hasTemplates = templates.length > 0;
  const showEmptyCatalogMessage = loadStatus === 'success' && !hasTemplates && !isEmptyWorkspace;

  return (
    <section
      data-testid="workflows-starter-section"
      className="mt-2"
      aria-label={t('workflows.starter_workflows')}>
      <header className="flex items-center justify-between mb-2">
        <div>
          <h2 className="text-sm font-medium text-stone-700 dark:text-neutral-300">
            {t('workflows.starter_workflows')}
          </h2>
          <p className="text-xs text-stone-500 dark:text-neutral-400 mt-0.5">
            {t('workflows.starter_subtitle')}
          </p>
        </div>
        {!isEmptyWorkspace ? (
          <button
            type="button"
            onClick={() => dispatch(setHideStarterSection(true))}
            data-testid="workflows-starter-hide"
            className="text-[11px] text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 underline">
            {t('workflows.hide_starter')}
          </button>
        ) : null}
      </header>

      {loadStatus === 'error' ? (
        <div className="mb-3 px-3.5 py-2.5 text-xs text-coral-700 bg-coral-50 border border-coral-200 rounded-lg">
          {t('workflows.starter_load_error')}
          {error ? `: ${error}` : ''}
        </div>
      ) : null}

      {showEmptyCatalogMessage ? (
        <div
          data-testid="workflows-starter-empty-catalog"
          className="text-xs text-stone-500 dark:text-neutral-400 px-3.5 py-3 bg-stone-50 dark:bg-neutral-800/40 rounded-xl">
          {t('workflows.starter_empty_catalog')}
        </div>
      ) : null}

      {hasTemplates ? (
        <div className="grid grid-cols-1 md:grid-cols-2 gap-3" data-testid="workflows-starter-grid">
          {templates.map(template => (
            <StarterWorkflowCard key={template.template_id} template={template} />
          ))}
        </div>
      ) : null}
    </section>
  );
}
