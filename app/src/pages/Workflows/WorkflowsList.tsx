/**
 * `/workflows` route — the Phase 1 list view.
 *
 * Renders "Your workflows" (rows from `workflows_list`) followed by a
 * placeholder for the Starter workflows section that F-5 / F-6 fill.
 * Empty state surfaces the chat-driven creation hero CTA (FR-1.2.6).
 *
 * See `Automations/systemsdesign.md §9` and `Automations/Tickets/
 * phase-1-foundation/F-4.md`.
 */
import { useEffect } from 'react';

import StarterWorkflowsSection from '../../components/workflows/StarterWorkflowsSection';
import WorkflowCard from '../../components/workflows/WorkflowCard';
import WorkflowEmptyState from '../../components/workflows/WorkflowEmptyState';
import { useT } from '../../lib/i18n/I18nContext';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import {
  fetchWorkflows,
  selectWorkflows,
  selectWorkflowsError,
  selectWorkflowsLoadStatus,
} from '../../store/workflowsSlice';

export default function WorkflowsList() {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const workflows = useAppSelector(selectWorkflows);
  const loadStatus = useAppSelector(selectWorkflowsLoadStatus);
  const error = useAppSelector(selectWorkflowsError);

  useEffect(() => {
    void dispatch(fetchWorkflows());
  }, [dispatch]);

  const hasWorkflows = workflows.length > 0;
  const isLoading = loadStatus === 'loading' && workflows.length === 0;

  return (
    <div data-testid="workflows-page-root" className="min-h-full p-4 pt-6 max-w-3xl mx-auto">
      <header className="mb-5">
        <h1 className="text-2xl font-display font-bold text-stone-900 dark:text-neutral-100">
          {t('nav.workflows')}
        </h1>
      </header>

      {loadStatus === 'error' ? (
        <div className="mb-4 px-3.5 py-3 text-sm text-coral-700 bg-coral-50 border border-coral-200 rounded-xl">
          {t('workflows.list_error')}
          {error ? `: ${error}` : ''}
          <button
            type="button"
            onClick={() => void dispatch(fetchWorkflows())}
            className="ml-3 underline text-coral-700 hover:text-coral-900"
            data-testid="workflows-list-retry">
            {t('workflows.list_retry')}
          </button>
        </div>
      ) : null}

      {isLoading ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          {t('common.loading')}
        </div>
      ) : null}

      {!isLoading && !hasWorkflows ? (
        <>
          {/* Empty workspace: the catalog is the hero (FR-1.2.6),
              rendered ABOVE the empty-state CTA. Keep the
              starter-section-placeholder testid on the section
              wrapper so existing tests continue to pass. */}
          <div data-testid="starter-section-placeholder">
            <StarterWorkflowsSection />
          </div>
          <div className="mt-2">
            <WorkflowEmptyState />
          </div>
        </>
      ) : null}

      {hasWorkflows ? (
        <>
          <section className="mt-2">
            <h2 className="text-sm font-medium text-stone-700 dark:text-neutral-300 mb-2">
              {t('workflows.your_workflows')}
            </h2>
            <div className="space-y-2" data-testid="workflows-list">
              {workflows.map(w => (
                <WorkflowCard key={w.id} workflow={w} />
              ))}
            </div>
          </section>
          <div data-testid="starter-section-placeholder" className="mt-8">
            <StarterWorkflowsSection />
          </div>
        </>
      ) : null}
    </div>
  );
}
