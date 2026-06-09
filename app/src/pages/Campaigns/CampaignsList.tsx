/**
 * `/campaigns` route — F4-11 list view.
 *
 * Lists every campaign as a card grid. Filter chips by status,
 * search by name/description. Empty state directs to the chat
 * drafter ("Help me create a campaign"). Mirrors the
 * `/workflows` page idiom intentionally so a user moving between
 * surfaces feels at home.
 */
import { useEffect, useMemo, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import CampaignCard from '../../components/campaigns/CampaignCard';
import { useT } from '../../lib/i18n/I18nContext';
import {
  fetchCampaigns,
  selectCampaigns,
  selectCampaignsError,
  selectCampaignsLoadStatus,
} from '../../store/campaignsSlice';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import type { Campaign, CampaignStatus } from '../../types/campaigns';

type StatusFilter = 'all' | CampaignStatus;

function filterAndSort(campaigns: Campaign[], search: string, status: StatusFilter): Campaign[] {
  const needle = search.trim().toLowerCase();
  const out = campaigns.filter(c => {
    if (status !== 'all' && c.status !== status) return false;
    if (needle) {
      const hay = `${c.name} ${c.description ?? ''}`.toLowerCase();
      if (!hay.includes(needle)) return false;
    }
    return true;
  });
  // Newest-updated first; campaigns are coarse-grained so an
  // updated_at sort is sufficient (no per-list-row sort selector in v1).
  return [...out].sort((a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at));
}

const ALL_PILLS: { key: StatusFilter; labelKey: string }[] = [
  { key: 'all', labelKey: 'campaigns.filter.all' },
  { key: 'active', labelKey: 'campaigns.filter.active' },
  { key: 'paused', labelKey: 'campaigns.filter.paused' },
  { key: 'draft', labelKey: 'campaigns.filter.draft' },
  { key: 'archived', labelKey: 'campaigns.filter.archived' },
];

export default function CampaignsList() {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const navigate = useNavigate();
  const campaigns = useAppSelector(selectCampaigns);
  const loadStatus = useAppSelector(selectCampaignsLoadStatus);
  const error = useAppSelector(selectCampaignsError);

  const [search, setSearch] = useState('');
  const [statusFilter, setStatusFilter] = useState<StatusFilter>('all');

  useEffect(() => {
    void dispatch(fetchCampaigns());
  }, [dispatch]);

  const counts = useMemo(() => {
    const c: Record<StatusFilter, number> = {
      all: campaigns.length,
      active: 0,
      paused: 0,
      draft: 0,
      wound_down: 0,
      archived: 0,
    };
    for (const x of campaigns) c[x.status] += 1;
    return c;
  }, [campaigns]);

  const visible = useMemo(
    () => filterAndSort(campaigns, search, statusFilter),
    [campaigns, search, statusFilter]
  );

  const isLoading = loadStatus === 'loading' && campaigns.length === 0;
  const hasCampaigns = campaigns.length > 0;

  return (
    <div data-testid="campaigns-page-root" className="min-h-full p-4 pt-6 max-w-3xl mx-auto">
      <header className="mb-5 flex items-start justify-between gap-3">
        <h1 className="text-2xl font-display font-bold text-stone-900 dark:text-neutral-100">
          {t('nav.campaigns')}
        </h1>
        <button
          type="button"
          onClick={() => navigate('/chat')}
          data-testid="campaigns-new-cta"
          className="px-3 py-1.5 text-xs font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg shadow-soft transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500 whitespace-nowrap">
          {t('campaigns.new_cta')}
        </button>
      </header>

      {loadStatus === 'error' ? (
        <div className="mb-4 px-3.5 py-3 text-sm text-coral-700 bg-coral-50 border border-coral-200 rounded-xl">
          {t('campaigns.list_error')}
          {error ? `: ${error}` : ''}
          <button
            type="button"
            onClick={() => void dispatch(fetchCampaigns())}
            data-testid="campaigns-list-retry"
            className="ml-3 underline text-coral-700 hover:text-coral-900">
            {t('campaigns.list_retry')}
          </button>
        </div>
      ) : null}

      {isLoading ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          {t('common.loading')}
        </div>
      ) : null}

      {!isLoading && !hasCampaigns ? (
        <div
          className="text-center py-10 bg-stone-50 dark:bg-neutral-800 rounded-xl"
          data-testid="campaigns-empty-state">
          <p className="text-sm text-stone-600 dark:text-neutral-300 mb-3">
            {t('campaigns.empty_title')}
          </p>
          <p className="text-xs text-stone-500 dark:text-neutral-400 max-w-md mx-auto mb-4">
            {t('campaigns.empty_body')}
          </p>
          <button
            type="button"
            onClick={() => navigate('/chat')}
            className="px-3 py-1.5 text-xs font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg shadow-soft transition-colors">
            {t('campaigns.empty_cta')}
          </button>
        </div>
      ) : null}

      {hasCampaigns ? (
        <section className="mt-2">
          <div className="mb-2 flex items-center gap-2 flex-wrap">
            <input
              type="search"
              value={search}
              onChange={e => setSearch(e.target.value)}
              placeholder={t('campaigns.search_placeholder')}
              aria-label={t('campaigns.search_placeholder')}
              data-testid="campaigns-search"
              className="flex-1 min-w-[160px] px-3 py-1.5 text-xs bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-700 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500 placeholder:text-stone-400"
            />
            <div
              role="tablist"
              aria-label={t('campaigns.filter_label')}
              className="flex items-center bg-stone-100 dark:bg-neutral-800 rounded-lg p-0.5 text-xs">
              {ALL_PILLS.map(pill => {
                const active = statusFilter === pill.key;
                return (
                  <button
                    key={pill.key}
                    type="button"
                    role="tab"
                    aria-selected={active}
                    onClick={() => setStatusFilter(pill.key)}
                    data-testid={`campaigns-filter-${pill.key}`}
                    className={`px-2.5 py-1 rounded-md font-medium transition-colors whitespace-nowrap ${
                      active
                        ? 'bg-white dark:bg-neutral-900 text-stone-900 dark:text-neutral-100 shadow-subtle'
                        : 'text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200'
                    }`}>
                    {t(pill.labelKey)}
                    <span
                      className={`ml-1.5 text-[10px] ${
                        active
                          ? 'text-stone-500 dark:text-neutral-400'
                          : 'text-stone-400 dark:text-neutral-500'
                      }`}>
                      {counts[pill.key]}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>

          {visible.length === 0 ? (
            <div className="text-xs text-stone-500 dark:text-neutral-400 px-3 py-3 bg-stone-50 dark:bg-neutral-800 rounded-xl">
              {t('campaigns.no_results')}
            </div>
          ) : (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-2" data-testid="campaigns-list">
              {visible.map(c => (
                <CampaignCard key={c.id} campaign={c} />
              ))}
            </div>
          )}
        </section>
      ) : null}
    </div>
  );
}
