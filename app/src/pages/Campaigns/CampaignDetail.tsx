/**
 * `/campaigns/:id` route — F4-12 detail view.
 *
 * Structured renderer: shows the persisted JSON shape directly
 * (per the ticket's "no lossy summarisation" invariant). Header
 * carries pause/resume + open-chat; main column has overview,
 * sub-workflows, approval badge, throttle gauge. Activity feed +
 * entity preview + cost widget are F4-12+ follow-ups (require
 * new RPC / socket plumbing).
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';

import { useT } from '../../lib/i18n/I18nContext';
import { campaignsApi } from '../../services/api/campaigns';
import { workflowsApi } from '../../services/api/workflows';
import {
  archiveCampaign,
  pauseCampaign,
  resumeCampaign,
  selectCampaignPending,
} from '../../store/campaignsSlice';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import type {
  ApprovalEntry,
  Campaign,
  CampaignStatus,
  ThrottleSnapshot,
} from '../../types/campaigns';
import type { Workflow } from '../../types/workflows';

function statusPillClass(status: CampaignStatus): string {
  switch (status) {
    case 'active':
      return 'bg-sage-100 text-sage-700 dark:bg-sage-900/40 dark:text-sage-200';
    case 'paused':
      return 'bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-200';
    case 'draft':
      return 'bg-stone-100 text-stone-700 dark:bg-neutral-800 dark:text-neutral-300';
    case 'wound_down':
      return 'bg-stone-100 text-stone-500 dark:bg-neutral-800 dark:text-neutral-400';
    case 'archived':
      return 'bg-stone-100 text-stone-400 dark:bg-neutral-800 dark:text-neutral-500';
  }
}

function formatBinding(c: Campaign): string {
  const b = c.entity_binding;
  if (b.type === 'google_sheet') return `Google Sheets · ${b.range}`;
  return `Attio · ${b.object_type}`;
}

function formatThrottle(c: Campaign): string | null {
  if (!c.throttle) return null;
  const w = c.throttle.window.type;
  const label = w === 'per_day' ? '/day' : w === 'per_hour' ? '/hour' : '/minute';
  return `${c.throttle.max_per_window}${label}`;
}

export default function CampaignDetail() {
  const { t } = useT();
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const dispatch = useAppDispatch();
  const pending = useAppSelector(selectCampaignPending(id ?? ''));

  const [campaign, setCampaign] = useState<Campaign | null>(null);
  const [throttleSnap, setThrottleSnap] = useState<ThrottleSnapshot | null>(null);
  const [subWorkflows, setSubWorkflows] = useState<Workflow[]>([]);
  const [pendingApprovals, setPendingApprovals] = useState<ApprovalEntry[]>([]);
  const [loadStatus, setLoadStatus] = useState<'idle' | 'loading' | 'success' | 'error'>('idle');
  const [error, setError] = useState<string | null>(null);
  const [expandedWorkflow, setExpandedWorkflow] = useState<string | null>(null);

  const reload = useCallback(async () => {
    if (!id) return;
    setLoadStatus('loading');
    setError(null);
    try {
      const [c, throttle, allWorkflows, approvals] = await Promise.all([
        campaignsApi.get(id),
        campaignsApi.throttleStatus(id).catch(() => null),
        workflowsApi.list({}).catch(() => [] as Workflow[]),
        campaignsApi.listPendingApprovals(id).catch(() => [] as ApprovalEntry[]),
      ]);
      if (!c) {
        setError(t('campaigns.detail.not_found'));
        setLoadStatus('error');
        return;
      }
      setCampaign(c);
      setThrottleSnap(throttle);
      setSubWorkflows(
        allWorkflows.filter(w => (w as Workflow & { campaign_id?: string }).campaign_id === id)
      );
      setPendingApprovals(approvals);
      setLoadStatus('success');
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setLoadStatus('error');
    }
  }, [id, t]);

  useEffect(() => {
    void reload();
  }, [reload]);

  const isLoading = loadStatus === 'loading' && !campaign;
  const canPause = campaign?.status === 'active';
  const canResume = campaign?.status === 'paused' || campaign?.status === 'draft';
  const canArchive = campaign?.status === 'wound_down' || campaign?.status === 'paused';

  const throttleDisplay = useMemo(() => {
    if (!throttleSnap) return null;
    return `${throttleSnap.consumed}/${throttleSnap.limit}`;
  }, [throttleSnap]);

  if (!id) {
    return (
      <div className="p-4 text-sm text-coral-600" data-testid="campaign-detail-no-id">
        {t('campaigns.detail.no_id')}
      </div>
    );
  }

  return (
    <div data-testid="campaign-detail-root" className="min-h-full p-4 pt-6 max-w-4xl mx-auto">
      <button
        type="button"
        onClick={() => navigate('/campaigns')}
        data-testid="campaign-detail-back"
        className="mb-3 text-xs text-stone-500 hover:text-stone-700 dark:text-neutral-400 dark:hover:text-neutral-200">
        ← {t('campaigns.detail.back')}
      </button>

      {isLoading ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          {t('common.loading')}
        </div>
      ) : null}

      {loadStatus === 'error' ? (
        <div className="px-3.5 py-3 text-sm text-coral-700 bg-coral-50 border border-coral-200 rounded-xl">
          {error ?? t('campaigns.detail.load_error')}
          <button
            type="button"
            onClick={() => void reload()}
            data-testid="campaign-detail-retry"
            className="ml-3 underline">
            {t('campaigns.list_retry')}
          </button>
        </div>
      ) : null}

      {campaign ? (
        <>
          {/* Header strip */}
          <header className="mb-5">
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <h1
                  className="text-2xl font-display font-bold text-stone-900 dark:text-neutral-100 truncate"
                  data-testid="campaign-detail-name">
                  {campaign.name}
                </h1>
                {campaign.description ? (
                  <p className="mt-1 text-sm text-stone-600 dark:text-neutral-400">
                    {campaign.description}
                  </p>
                ) : null}
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <span
                  className={`px-2.5 py-1 rounded-full text-xs font-medium ${statusPillClass(
                    campaign.status
                  )}`}
                  data-testid="campaign-detail-status">
                  {t(`campaigns.status.${campaign.status}`)}
                </span>
                {canPause ? (
                  <button
                    type="button"
                    onClick={() => void dispatch(pauseCampaign(campaign.id)).then(reload)}
                    disabled={pending}
                    data-testid="campaign-detail-pause"
                    className="px-2.5 py-1 text-xs font-medium text-amber-700 bg-amber-50 hover:bg-amber-100 rounded-lg disabled:opacity-40">
                    {t('campaigns.card.pause')}
                  </button>
                ) : null}
                {canResume ? (
                  <button
                    type="button"
                    onClick={() => void dispatch(resumeCampaign(campaign.id)).then(reload)}
                    disabled={pending}
                    data-testid="campaign-detail-resume"
                    className="px-2.5 py-1 text-xs font-medium text-sage-700 bg-sage-50 hover:bg-sage-100 rounded-lg disabled:opacity-40">
                    {t('campaigns.card.resume')}
                  </button>
                ) : null}
                {canArchive ? (
                  <button
                    type="button"
                    onClick={() => void dispatch(archiveCampaign(campaign.id)).then(reload)}
                    disabled={pending}
                    data-testid="campaign-detail-archive"
                    className="px-2.5 py-1 text-xs font-medium text-coral-700 hover:bg-coral-50 rounded-lg disabled:opacity-40">
                    {t('campaigns.card.archive')}
                  </button>
                ) : null}
                <button
                  type="button"
                  onClick={() => navigate('/chat')}
                  data-testid="campaign-detail-discuss"
                  className="px-2.5 py-1 text-xs font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg">
                  {t('campaigns.detail.discuss')}
                </button>
              </div>
            </div>
          </header>

          {/* Overview */}
          <section
            data-testid="campaign-detail-overview"
            className="mb-5 bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-800 rounded-xl p-4">
            <h2 className="text-sm font-medium text-stone-700 dark:text-neutral-300 mb-2">
              {t('campaigns.detail.overview_title')}
            </h2>
            <dl className="grid grid-cols-2 gap-x-4 gap-y-2 text-xs">
              <dt className="text-stone-500 dark:text-neutral-400">
                {t('campaigns.detail.entity_binding')}
              </dt>
              <dd className="text-stone-900 dark:text-neutral-100">{formatBinding(campaign)}</dd>
              <dt className="text-stone-500 dark:text-neutral-400">
                {t('campaigns.detail.approval_policy')}
              </dt>
              <dd className="text-stone-900 dark:text-neutral-100">
                {t(`campaigns.card.policy.${campaign.approval_policy.kind}`)}
              </dd>
              {campaign.throttle ? (
                <>
                  <dt className="text-stone-500 dark:text-neutral-400">
                    {t('campaigns.detail.throttle')}
                  </dt>
                  <dd className="text-stone-900 dark:text-neutral-100">
                    {formatThrottle(campaign)}
                    {throttleDisplay ? (
                      <span
                        className="ml-2 text-stone-500 dark:text-neutral-400"
                        data-testid="campaign-detail-throttle-snapshot">
                        ({throttleDisplay} {t('campaigns.detail.throttle_used')})
                      </span>
                    ) : null}
                  </dd>
                </>
              ) : null}
              {campaign.target_outcome ? (
                <>
                  <dt className="text-stone-500 dark:text-neutral-400">
                    {t('campaigns.detail.target_outcome')}
                  </dt>
                  <dd className="text-stone-900 dark:text-neutral-100">
                    {campaign.target_outcome.kind === 'count'
                      ? `${campaign.target_outcome.target} ${campaign.target_outcome.metric}`
                      : `${campaign.target_outcome.target}% ${campaign.target_outcome.metric}`}
                  </dd>
                </>
              ) : null}
              <dt className="text-stone-500 dark:text-neutral-400">
                {t('campaigns.detail.created_at')}
              </dt>
              <dd className="text-stone-900 dark:text-neutral-100">
                {new Date(campaign.created_at).toLocaleString()}
              </dd>
            </dl>
          </section>

          {/* Approval badge */}
          {pendingApprovals.length > 0 ? (
            <section
              data-testid="campaign-detail-approvals-badge"
              className="mb-5 bg-amber-50 dark:bg-amber-950/30 border border-amber-200 dark:border-amber-900 rounded-xl p-3 flex items-center justify-between gap-3">
              <span className="text-sm text-amber-800 dark:text-amber-200">
                {t('campaigns.detail.pending_approvals').replace(
                  '{count}',
                  String(pendingApprovals.length)
                )}
              </span>
              <button
                type="button"
                onClick={() => navigate(`/approvals?campaign=${campaign.id}`)}
                className="px-2.5 py-1 text-xs font-medium text-white bg-amber-600 hover:bg-amber-700 rounded-lg">
                {t('campaigns.detail.review_approvals')}
              </button>
            </section>
          ) : null}

          {/* Sub-workflows */}
          <section
            data-testid="campaign-detail-subworkflows"
            className="mb-5 bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-800 rounded-xl p-4">
            <h2 className="text-sm font-medium text-stone-700 dark:text-neutral-300 mb-2">
              {t('campaigns.detail.subworkflows_title').replace(
                '{count}',
                String(subWorkflows.length)
              )}
            </h2>
            {subWorkflows.length === 0 ? (
              <p className="text-xs text-stone-500 dark:text-neutral-400">
                {t('campaigns.detail.no_subworkflows')}
              </p>
            ) : (
              <ul className="space-y-2">
                {subWorkflows.map(w => {
                  const open = expandedWorkflow === w.id;
                  return (
                    <li
                      key={w.id}
                      className="border border-stone-200 dark:border-neutral-700 rounded-lg"
                      data-testid={`campaign-detail-subworkflow-${w.id}`}>
                      <button
                        type="button"
                        onClick={() => setExpandedWorkflow(open ? null : w.id)}
                        aria-expanded={open}
                        className="w-full flex items-center justify-between gap-3 px-3 py-2 text-left">
                        <div className="min-w-0">
                          <div className="text-sm font-medium text-stone-900 dark:text-neutral-100 truncate">
                            {w.name}
                          </div>
                          <div className="text-[11px] text-stone-500 dark:text-neutral-400">
                            {w.trigger.type} · {w.nodes.length}{' '}
                            {t('campaigns.detail.subworkflow_nodes')}
                          </div>
                        </div>
                        <span className="text-stone-400 text-xs">{open ? '▾' : '▸'}</span>
                      </button>
                      {open ? (
                        <div className="px-3 pb-3 text-xs text-stone-600 dark:text-neutral-300 border-t border-stone-100 dark:border-neutral-800">
                          {w.description ? <p className="mt-2">{w.description}</p> : null}
                          <ul className="mt-2 space-y-1">
                            {w.nodes.map(n => (
                              <li key={n.id} className="flex items-center gap-2">
                                <span className="px-1.5 py-0.5 rounded bg-stone-100 dark:bg-neutral-800 text-[10px]">
                                  {n.kind}
                                </span>
                                <span className="text-stone-500 dark:text-neutral-400 truncate">
                                  {n.id}
                                </span>
                              </li>
                            ))}
                          </ul>
                          <button
                            type="button"
                            onClick={() => navigate(`/workflows`)}
                            className="mt-2 text-[11px] text-primary-600 hover:text-primary-700 hover:underline">
                            {t('campaigns.detail.subworkflow_edit_link')}
                          </button>
                        </div>
                      ) : null}
                    </li>
                  );
                })}
              </ul>
            )}
          </section>

          {/* Provenance */}
          <footer
            className="text-[11px] text-stone-500 dark:text-neutral-500"
            data-testid="campaign-detail-provenance">
            {t('campaigns.detail.provenance').replace(
              '{date}',
              new Date(campaign.created_at).toLocaleDateString()
            )}
          </footer>
        </>
      ) : null}
    </div>
  );
}
