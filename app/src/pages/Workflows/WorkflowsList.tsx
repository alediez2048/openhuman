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
import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import StarterWorkflowsSection from '../../components/workflows/StarterWorkflowsSection';
import WorkflowCard from '../../components/workflows/WorkflowCard';
import WorkflowEmptyState from '../../components/workflows/WorkflowEmptyState';
import { useT } from '../../lib/i18n/I18nContext';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import {
  fetchWorkflows,
  selectHideStarterSection,
  selectWorkflows,
  selectWorkflowsError,
  selectWorkflowsLoadStatus,
  setHideStarterSection,
} from '../../store/workflowsSlice';
import type { Workflow } from '../../types/workflows';

type StatusFilter = 'all' | 'ready' | 'attention';
type SortKey = 'name' | 'last_run' | 'created';

function applyFilterAndSort(
  workflows: Workflow[],
  search: string,
  status: StatusFilter,
  sort: SortKey
): Workflow[] {
  const needle = search.trim().toLowerCase();
  const filtered = workflows.filter(w => {
    if (status === 'ready' && w.health.type !== 'ready') return false;
    if (status === 'attention' && w.health.type === 'ready') return false;
    if (needle) {
      const haystack = `${w.name} ${w.description ?? ''}`.toLowerCase();
      if (!haystack.includes(needle)) return false;
    }
    return true;
  });
  // Stable sort. Date keys descend (newest first); name ascends.
  const sorted = [...filtered];
  if (sort === 'name') {
    sorted.sort((a, b) => a.name.localeCompare(b.name));
  } else if (sort === 'last_run') {
    sorted.sort((a, b) => {
      const aTs = a.last_run_at ? Date.parse(a.last_run_at) : 0;
      const bTs = b.last_run_at ? Date.parse(b.last_run_at) : 0;
      return bTs - aTs;
    });
  } else {
    // 'created' — newest first.
    sorted.sort((a, b) => Date.parse(b.created_at) - Date.parse(a.created_at));
  }
  return sorted;
}

export default function WorkflowsList() {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const navigate = useNavigate();
  const workflows = useAppSelector(selectWorkflows);
  const loadStatus = useAppSelector(selectWorkflowsLoadStatus);
  const error = useAppSelector(selectWorkflowsError);
  const hideStarterSection = useAppSelector(selectHideStarterSection);

  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');
  const [sort, setSort] = useState<SortKey>('last_run');

  useEffect(() => {
    void dispatch(fetchWorkflows());
  }, [dispatch]);

  const hasWorkflows = workflows.length > 0;
  const isLoading = loadStatus === 'loading' && workflows.length === 0;

  // Counts for the filter pills — computed on the full set so the
  // pill labels are stable when the user types in the search box.
  const counts = useMemo(() => {
    let ready = 0;
    let attention = 0;
    for (const w of workflows) {
      if (w.health.type === 'ready') ready += 1;
      else attention += 1;
    }
    return { all: workflows.length, ready, attention };
  }, [workflows]);

  const visibleWorkflows = useMemo(
    () => applyFilterAndSort(workflows, search, statusFilter, sort),
    [workflows, search, statusFilter, sort]
  );
  // When the user previously hid the catalog AND has at least one
  // workflow, the catalog section returns null. Render a compact
  // "Show starter workflows" toggle so they can un-hide without
  // visiting Settings.
  const showStarterToggle = hasWorkflows && hideStarterSection;

  return (
    <div data-testid="workflows-page-root" className="min-h-full p-4 pt-6 max-w-3xl mx-auto">
      <header className="mb-5 flex items-start justify-between gap-3">
        <h1 className="text-2xl font-display font-bold text-stone-900 dark:text-neutral-100">
          {t('nav.workflows')}
        </h1>
        {hasWorkflows ? (
          <button
            type="button"
            onClick={() => navigate('/chat')}
            data-testid="workflows-new-cta"
            className="px-3 py-1.5 text-xs font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg shadow-soft transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500 whitespace-nowrap">
            {t('workflows.empty_cta')}
          </button>
        ) : null}
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

            {/* Control bar — search, status pills, sort. Linear-style
                density: lets the user scan 20+ workflows without
                scrolling. Pills carry counts so the "Needs attention"
                filter telegraphs urgency before the user clicks it. */}
            <div className="mb-2 flex items-center gap-2 flex-wrap">
              <input
                type="search"
                value={search}
                onChange={e => setSearch(e.target.value)}
                placeholder={t('workflows.search_placeholder')}
                aria-label={t('workflows.search_placeholder')}
                data-testid="workflows-search"
                className="flex-1 min-w-[160px] px-3 py-1.5 text-xs bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500 placeholder:text-stone-400"
              />
              <div
                role="tablist"
                aria-label={t('workflows.filter_label')}
                className="flex items-center bg-stone-100 dark:bg-neutral-800 rounded-lg p-0.5 text-xs">
                {(
                  [
                    { key: 'all', label: t('workflows.filter_all'), count: counts.all },
                    { key: 'ready', label: t('workflows.filter_ready'), count: counts.ready },
                    {
                      key: 'attention',
                      label: t('workflows.filter_attention'),
                      count: counts.attention,
                    },
                  ] as Array<{ key: StatusFilter; label: string; count: number }>
                ).map(pill => {
                  const active = statusFilter === pill.key;
                  return (
                    <button
                      key={pill.key}
                      type="button"
                      role="tab"
                      aria-selected={active}
                      onClick={() => setStatusFilter(pill.key)}
                      data-testid={`workflows-filter-${pill.key}`}
                      className={`px-2.5 py-1 rounded-md font-medium transition-colors whitespace-nowrap ${
                        active
                          ? 'bg-white dark:bg-neutral-900 text-stone-900 dark:text-neutral-100 shadow-subtle'
                          : 'text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200'
                      }`}>
                      {pill.label}
                      <span
                        className={`ml-1.5 text-[10px] ${
                          active
                            ? 'text-stone-500 dark:text-neutral-400'
                            : 'text-stone-400 dark:text-neutral-500'
                        }`}>
                        {pill.count}
                      </span>
                    </button>
                  );
                })}
              </div>
              <label className="text-xs text-stone-500 dark:text-neutral-400 flex items-center gap-1.5 whitespace-nowrap">
                <span className="sr-only sm:not-sr-only">{t('workflows.sort_label')}</span>
                <select
                  value={sort}
                  onChange={e => setSort(e.target.value as SortKey)}
                  aria-label={t('workflows.sort_label')}
                  data-testid="workflows-sort"
                  className="bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-700 rounded-lg px-2 py-1 text-xs focus:outline-none focus:ring-2 focus:ring-primary-500">
                  <option value="last_run">{t('workflows.sort_last_run')}</option>
                  <option value="name">{t('workflows.sort_name')}</option>
                  <option value="created">{t('workflows.sort_created')}</option>
                </select>
              </label>
            </div>

            {visibleWorkflows.length === 0 ? (
              <div className="text-xs text-stone-500 dark:text-neutral-400 px-3 py-3 bg-stone-50 dark:bg-neutral-800 rounded-xl">
                {t('workflows.no_results')}
              </div>
            ) : (
              <div
                className="grid grid-cols-1 md:grid-cols-2 gap-2"
                data-testid="workflows-list">
                {visibleWorkflows.map(w => (
                  <WorkflowCard key={w.id} workflow={w} />
                ))}
              </div>
            )}
          </section>
          <div data-testid="starter-section-placeholder" className="mt-8">
            {showStarterToggle ? (
              <button
                type="button"
                onClick={() => dispatch(setHideStarterSection(false))}
                data-testid="workflows-starter-show"
                className="text-xs text-primary-600 hover:text-primary-700 hover:underline">
                {t('workflows.show_starter')}
              </button>
            ) : (
              <StarterWorkflowsSection />
            )}
          </div>
        </>
      ) : null}
    </div>
  );
}
