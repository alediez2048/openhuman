/**
 * Built-in integration detail modal.
 *
 * Built-in integrations (Twilio, Apify, Google Places, Parallel, Seltz,
 * Stock Prices) are backend-proxied — there's no local toggle or per-account
 * credential to manage here. The modal shows the integration's purpose +
 * status, with a clear note about how it's actually gated.
 *
 * Per-account toggle / credential rotation lands in **P0-6a** once the
 * backend exposes a matching account-management surface.
 */
import { useEffect, useRef } from 'react';
import { createPortal } from 'react-dom';
import { useNavigate } from 'react-router-dom';

import type { ConnectionStatus } from '../../../types/connections';

interface Props {
  integrationId: string;
  displayName: string;
  description: string;
  status: ConnectionStatus;
  onClose: () => void;
}

export default function BuiltinDetailModal({
  integrationId,
  displayName,
  description,
  status,
  onClose,
}: Props) {
  const navigate = useNavigate();
  const backdropRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === backdropRef.current) onClose();
  };

  const statusLabel =
    status.kind === 'connected'
      ? 'Connected'
      : status.kind === 'not_connected'
        ? 'Not connected — sign in to OpenHuman to enable'
        : status.kind === 'error'
          ? `Error — ${status.reason}`
          : status.kind;

  return createPortal(
    <div
      ref={backdropRef}
      onClick={onBackdropClick}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6"
      data-testid={`builtin-detail-modal-${integrationId}`}>
      <div className="w-full max-w-md bg-white dark:bg-neutral-900 rounded-2xl shadow-large p-5">
        <h2 className="text-base font-display font-semibold text-stone-900 dark:text-neutral-100 mb-1">
          {displayName}
        </h2>
        <p className="text-xs text-stone-500 dark:text-neutral-400 mb-4">{description}</p>

        <div className="mb-3 px-3 py-2 text-xs rounded-lg bg-stone-50 dark:bg-neutral-800">
          <div className="font-medium text-stone-700 dark:text-neutral-300 mb-1">Status</div>
          <div className="text-stone-600 dark:text-neutral-400">{statusLabel}</div>
        </div>

        <div className="mb-4 px-3 py-2 text-xs text-stone-600 dark:text-neutral-400 bg-stone-50 dark:bg-neutral-800 rounded-lg">
          Built-in integrations are <strong>backend-proxied</strong>: OpenHuman's backend handles
          the third-party API call, including billing, rate limits, and credential storage. There's
          no per-account credential to manage here — availability is controlled by your OpenHuman
          account.
        </div>

        <div className="flex items-center justify-end gap-2 pt-2">
          <button
            type="button"
            onClick={() => {
              onClose();
              navigate('/intelligence');
            }}
            className="px-3 py-1.5 text-sm text-stone-700 dark:text-neutral-300 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md">
            Open intelligence settings
          </button>
          <button
            type="button"
            onClick={onClose}
            className="px-3.5 py-1.5 text-sm font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg">
            Close
          </button>
        </div>
      </div>
    </div>,
    document.body
  );
}
