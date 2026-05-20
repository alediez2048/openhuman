/**
 * Inline modal that hosts a CEF webview for "Connect a browser account"
 * directly from the Connections Hub — no redirect to the chat sidebar.
 *
 * Reuses the existing `<WebviewHost>` component so we inherit:
 *   - the bounds-measurement + native-window glue,
 *   - the loading / timeout / retry phases,
 *   - the cookie-persistence semantics (sessions stay warm across closes).
 *
 * Lifecycle:
 *   1. Modal mounts → if no account already exists for this provider in the
 *      accounts slice, generate a new accountId + dispatch `addAccount`.
 *   2. `<WebviewHost>` mounts inside the modal body → `openWebviewAccount`
 *      fires → CEF child webview docks to the body's bounding rect.
 *   3. User signs in (OAuth / QR / etc.) → cookies persist → next aggregator
 *      refresh picks up the new login (`detect_webview_logins`).
 *   4. User closes the modal → `<WebviewHost>` unmounts → `hideWebviewAccount`
 *      moves the native view off-screen but **does not** purge cookies.
 *
 * Cookie purging (a true "disconnect") is a separate flow filed as **P0-5c.b**.
 */
import { useEffect, useMemo, useRef } from 'react';
import { createPortal } from 'react-dom';

import { addAccount } from '../../../store/accountsSlice';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch, useAppSelector } from '../../../store/hooks';
import type { Account, AccountProvider } from '../../../types/accounts';
import WebviewHost from '../../accounts/WebviewHost';

const PROVIDER_LABELS: Record<AccountProvider, string> = {
  whatsapp: 'WhatsApp',
  wechat: 'WeChat',
  telegram: 'Telegram',
  linkedin: 'LinkedIn',
  slack: 'Slack',
  discord: 'Discord',
  'google-meet': 'Google Meet',
  zoom: 'Zoom',
  browserscan: 'BrowserScan',
  twitter: 'X (Twitter)',
  instagram: 'Instagram',
  messenger: 'Messenger',
};

function makeAccountId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === 'function') return c.randomUUID();
  return `acct-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

interface Props {
  provider: AccountProvider;
  onClose: () => void;
}

export default function BrowserAccountConnectModal({ provider, onClose }: Props) {
  const dispatch = useAppDispatch();
  const accounts = useAppSelector(s => s.accounts.accounts);
  const order = useAppSelector(s => s.accounts.order);

  // Reuse an existing account for this provider if present; otherwise mint a
  // new id. Keeping the id stable across modal opens preserves the warm-reopen
  // path inside `<WebviewHost>`.
  const accountId = useMemo(() => {
    for (const id of order) {
      const a = accounts[id];
      if (a && a.provider === provider) return id;
    }
    return makeAccountId();
  }, [accounts, order, provider]);

  // Register the account on first mount (no-op if it already exists — the
  // slice's `addAccount` reducer is idempotent on id).
  useEffect(() => {
    const existing = accounts[accountId];
    if (existing) return;
    const acct: Account = {
      id: accountId,
      provider,
      label: PROVIDER_LABELS[provider],
      createdAt: new Date().toISOString(),
      status: 'pending',
    };
    dispatch(addAccount(acct));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [accountId, provider]);

  // Poll the aggregator while the modal is open so the Hub's status badge
  // flips to "Connected" as soon as CEF flushes the new session cookies to
  // disk. The cookie probe reads through `mode=ro&immutable=1` so it only
  // sees what's on disk — and CEF batches cookie writes in memory before
  // flushing (typically every few seconds after a new value is set).
  //
  // On unmount, schedule one extra delayed fetch so a sign-in completed
  // right before the user clicks Close still gets picked up after CEF's
  // pending flush lands. `hideWebviewAccount` (called by WebviewHost
  // unmount) doesn't synchronously flush cookies — without the delay, the
  // post-close fetch would race the flush and miss the just-completed
  // login.
  useEffect(() => {
    const POLL_MS = 4_000;
    const CLOSE_DELAY_MS = 2_500;
    const intervalId = window.setInterval(() => {
      void dispatch(fetchConnections());
    }, POLL_MS);
    return () => {
      window.clearInterval(intervalId);
      window.setTimeout(() => {
        void dispatch(fetchConnections());
      }, CLOSE_DELAY_MS);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Esc closes.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    document.addEventListener('keydown', onKey);
    return () => document.removeEventListener('keydown', onKey);
  }, [onClose]);

  const backdropRef = useRef<HTMLDivElement | null>(null);
  const onBackdropClick = (e: React.MouseEvent<HTMLDivElement>) => {
    if (e.target === backdropRef.current) onClose();
  };

  const label = PROVIDER_LABELS[provider];

  return createPortal(
    <div
      ref={backdropRef}
      onClick={onBackdropClick}
      className="fixed inset-0 z-50 bg-black/40 flex items-center justify-center p-6"
      data-testid="browser-account-connect-modal">
      <div className="bg-white dark:bg-neutral-900 w-[90vw] h-[85vh] max-w-5xl rounded-2xl shadow-soft border border-stone-200 dark:border-neutral-700 flex flex-col overflow-hidden">
        <header className="flex items-center justify-between px-4 py-3 border-b border-stone-200 dark:border-neutral-700">
          <h2 className="text-sm font-semibold text-stone-900 dark:text-neutral-100">
            Sign in to {label}
          </h2>
          <button
            type="button"
            onClick={onClose}
            className="px-2.5 py-1 text-xs text-stone-700 dark:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md"
            data-testid="browser-account-connect-close">
            Close
          </button>
        </header>
        <div className="flex-1 min-h-0 relative">
          <WebviewHost accountId={accountId} provider={provider} />
        </div>
      </div>
    </div>,
    document.body
  );
}
