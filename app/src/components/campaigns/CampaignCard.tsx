/**
 * One row in the `CampaignsList` page (F4-11).
 *
 * Mirrors `WorkflowCard`'s visual idiom — name + status pill + entity
 * binding + throttle subtitle + overflow menu. Pause/Resume/Archive
 * thunks dispatch optimistically; Detail view lives at
 * `/campaigns/:id` (F4-12).
 */
import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';

import { useT } from '../../lib/i18n/I18nContext';
import {
  archiveCampaign,
  pauseCampaign,
  resumeCampaign,
  selectCampaignPending,
} from '../../store/campaignsSlice';
import { useAppDispatch, useAppSelector } from '../../store/hooks';
import type { Campaign } from '../../types/campaigns';

function formatThrottle(c: Campaign): string | null {
  if (!c.throttle) return null;
  const max = c.throttle.max_per_window;
  const window = c.throttle.window.type;
  const label = window === 'per_day' ? '/day' : window === 'per_hour' ? '/hr' : '/min';
  return `${max}${label}`;
}

function formatBinding(c: Campaign): string {
  const b = c.entity_binding;
  if (b.type === 'google_sheet') return `Sheets · ${b.range}`;
  return `Attio · ${b.object_type}`;
}

function statusPillClass(status: Campaign['status']): string {
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

interface CampaignCardProps {
  campaign: Campaign;
}

export default function CampaignCard({ campaign }: CampaignCardProps) {
  const { t } = useT();
  const dispatch = useAppDispatch();
  const navigate = useNavigate();
  const pending = useAppSelector(selectCampaignPending(campaign.id));
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement | null>(null);

  // Click-outside dismiss for the overflow menu.
  useEffect(() => {
    if (!menuOpen) return;
    function onDocClick(e: MouseEvent) {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    }
    document.addEventListener('mousedown', onDocClick);
    return () => document.removeEventListener('mousedown', onDocClick);
  }, [menuOpen]);

  const onPause = () => {
    setMenuOpen(false);
    void dispatch(pauseCampaign(campaign.id));
  };
  const onResume = () => {
    setMenuOpen(false);
    void dispatch(resumeCampaign(campaign.id));
  };
  const onArchive = () => {
    setMenuOpen(false);
    void dispatch(archiveCampaign(campaign.id));
  };
  const onView = () => {
    setMenuOpen(false);
    navigate(`/campaigns/${campaign.id}`);
  };

  const throttle = formatThrottle(campaign);
  const binding = formatBinding(campaign);
  const canPause = campaign.status === 'active';
  const canResume = campaign.status === 'paused' || campaign.status === 'draft';
  const canArchive = campaign.status === 'wound_down' || campaign.status === 'paused';

  return (
    <article
      data-testid={`campaign-card-${campaign.id}`}
      className="bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-800 rounded-xl p-3.5 shadow-subtle hover:shadow-soft transition-shadow">
      <div className="flex items-start justify-between gap-2">
        <button
          type="button"
          onClick={onView}
          className="flex-1 text-left min-w-0"
          data-testid={`campaign-card-name-${campaign.id}`}>
          <h3 className="text-sm font-medium text-stone-900 dark:text-neutral-100 truncate">
            {campaign.name}
          </h3>
          {campaign.description ? (
            <p className="mt-0.5 text-xs text-stone-500 dark:text-neutral-400 line-clamp-2">
              {campaign.description}
            </p>
          ) : null}
        </button>
        <div className="flex items-center gap-1.5 shrink-0">
          <span
            className={`px-2 py-0.5 rounded-full text-[10px] font-medium ${statusPillClass(
              campaign.status
            )}`}
            data-testid={`campaign-card-status-${campaign.id}`}>
            {t(`campaigns.status.${campaign.status}`)}
          </span>
          <div className="relative" ref={menuRef}>
            <button
              type="button"
              onClick={() => setMenuOpen(o => !o)}
              aria-label={t('campaigns.card.more_actions')}
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              disabled={pending}
              data-testid={`campaign-card-menu-${campaign.id}`}
              className="px-1.5 py-1 text-xs text-stone-500 dark:text-neutral-400 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md disabled:opacity-40">
              ⋯
            </button>
            {menuOpen ? (
              <div
                role="menu"
                className="absolute right-0 mt-1 z-10 min-w-[140px] bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-700 rounded-lg shadow-soft text-xs">
                <button
                  type="button"
                  role="menuitem"
                  onClick={onView}
                  className="block w-full text-left px-3 py-1.5 hover:bg-stone-50 dark:hover:bg-neutral-800">
                  {t('campaigns.card.view_detail')}
                </button>
                {canPause ? (
                  <button
                    type="button"
                    role="menuitem"
                    onClick={onPause}
                    data-testid={`campaign-card-pause-${campaign.id}`}
                    className="block w-full text-left px-3 py-1.5 hover:bg-stone-50 dark:hover:bg-neutral-800">
                    {t('campaigns.card.pause')}
                  </button>
                ) : null}
                {canResume ? (
                  <button
                    type="button"
                    role="menuitem"
                    onClick={onResume}
                    data-testid={`campaign-card-resume-${campaign.id}`}
                    className="block w-full text-left px-3 py-1.5 hover:bg-stone-50 dark:hover:bg-neutral-800">
                    {t('campaigns.card.resume')}
                  </button>
                ) : null}
                {canArchive ? (
                  <button
                    type="button"
                    role="menuitem"
                    onClick={onArchive}
                    data-testid={`campaign-card-archive-${campaign.id}`}
                    className="block w-full text-left px-3 py-1.5 text-coral-600 hover:bg-coral-50 dark:hover:bg-coral-950/40">
                    {t('campaigns.card.archive')}
                  </button>
                ) : null}
              </div>
            ) : null}
          </div>
        </div>
      </div>
      <div className="mt-2 flex items-center gap-2 text-[11px] text-stone-500 dark:text-neutral-400">
        <span className="px-1.5 py-0.5 rounded bg-stone-100 dark:bg-neutral-800">{binding}</span>
        {throttle ? (
          <span className="px-1.5 py-0.5 rounded bg-stone-100 dark:bg-neutral-800">
            {throttle}
          </span>
        ) : null}
        <span className="ml-auto">
          {t(`campaigns.card.policy.${campaign.approval_policy.kind}`)}
        </span>
      </div>
    </article>
  );
}
