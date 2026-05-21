/**
 * Empty-state hero rendered when `workflows_list` returns `[]`.
 *
 * Per FR-1.2.6, the chat-driven creation path is the hero: a prominent
 * "Ask OpenHuman to build a workflow" CTA navigates to `/chat`. F-14
 * wires the full proposal-preview flow that lands when the user
 * describes a workflow in chat.
 *
 * The starter-section placeholder below the hero is the insertion
 * point F-5 / F-6 use for the catalog.
 */
import { useNavigate } from 'react-router-dom';

import { useT } from '../../lib/i18n/I18nContext';

export default function WorkflowEmptyState() {
  const { t } = useT();
  const navigate = useNavigate();

  return (
    <div className="flex flex-col items-center text-center px-6 py-12">
      <div
        className="mb-4 w-16 h-16 rounded-2xl bg-primary-50 dark:bg-primary-950/40 flex items-center justify-center text-primary-500"
        aria-hidden>
        <svg className="w-9 h-9" fill="none" stroke="currentColor" viewBox="0 0 24 24">
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            strokeWidth={1.5}
            d="M13 10V3L4 14h7v7l9-11h-7z"
          />
        </svg>
      </div>
      <h2
        data-testid="workflows-empty-title"
        className="text-lg font-semibold text-stone-900 dark:text-neutral-100">
        {t('workflows.empty_title')}
      </h2>
      <p className="mt-2 max-w-md text-sm text-stone-500 dark:text-neutral-400">
        {t('workflows.empty_subtitle')}
      </p>
      <button
        type="button"
        onClick={() => navigate('/chat')}
        data-testid="workflows-empty-cta"
        className="mt-5 px-4 py-2 text-sm font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg shadow-soft transition-colors focus:outline-none focus:ring-2 focus:ring-primary-500">
        {t('workflows.empty_cta')}
      </button>
    </div>
  );
}
