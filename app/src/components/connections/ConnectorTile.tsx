/**
 * Shared tile used by every section of the Connections Hub.
 *
 * Single visual contract across Composio / Channels / Browser / Built-in /
 * MCP / Generic HTTP sections so the page reads as one unified surface:
 *
 *   ┌──────────────────┐
 *   │      [icon]      │
 *   │      Title       │
 *   │   ● Connected    │
 *   └──────────────────┘
 *
 * Per-card actions (Test / Delete / Re-login / Configure) live inside the
 * modal that opens on click — the tile itself stays uniform so the catalog
 * scan feels consistent regardless of mechanism.
 */
import { type ReactNode } from 'react';

import type { ConnectionStatus, Verification } from '../../types/connections';

interface ConnectorTileProps {
  name: string;
  /** Branded logo, SVG, or emoji rendered above the title. */
  icon: ReactNode;
  status: ConnectionStatus;
  /**
   * Last probe outcome from this core session. When present, **overrides**
   * `status` for the visible pill — verification is strictly more
   * authoritative than "row exists in DB". `null`/undefined falls back to
   * the section's mechanism-specific status semantics.
   */
  verification?: Verification | null;
  /**
   * When `true`, a Connected `status` is downgraded to "Configured" unless
   * we have a real probe outcome to back it up. Used by HTTP/MCP/Channels
   * sections where the `status` field only means "row exists in DB" or
   * "registry membership," not "actually responds." Composio / Webview /
   * Built-in pass `false` (the default) because their status is already
   * derived from authoritative evidence (Composio API, cookie probe,
   * session token).
   */
  requireVerification?: boolean;
  /** Optional one-liner under the status pill. Most sections omit. */
  description?: string;
  /** Click handler; `undefined` makes the tile non-interactive. */
  onClick?: () => void;
  /** When true, render as a disabled affordance with reduced opacity. */
  disabled?: boolean;
  /** Tooltip shown on hover (browser-native). */
  title?: string;
  testId?: string;
}

function StatusPill({
  status,
  verification,
  requireVerification,
}: {
  status: ConnectionStatus;
  verification?: Verification | null;
  requireVerification: boolean;
}) {
  // Verification is strictly more authoritative than the binary status —
  // if we actually pinged the service, we report what happened.
  if (verification) {
    if (verification.result.kind === 'live') {
      return (
        <span
          className="inline-flex items-center gap-1 text-[11px] text-sage-700 dark:text-sage-400"
          title={`Verified ${new Date(verification.last_probed_at).toLocaleString()}`}>
          <span className="w-1.5 h-1.5 rounded-full bg-sage-500" />
          Verified
        </span>
      );
    }
    return (
      <span
        className="inline-flex items-center gap-1 text-[11px] text-coral-600"
        title={verification.result.reason}>
        <span className="w-1.5 h-1.5 rounded-full bg-coral-500" />
        Probe failed
      </span>
    );
  }
  switch (status.kind) {
    case 'connected':
      // For mechanisms whose status is already authoritative (Composio,
      // Webview cookie probe, Built-in session token), Connected stays
      // green. For HTTP/MCP/Channels we downgrade to "Configured" until
      // a probe runs — `status: Connected` alone only means "row exists
      // in DB / registry."
      if (requireVerification) {
        return (
          <span
            className="inline-flex items-center gap-1 text-[11px] text-stone-500 dark:text-neutral-400"
            title="Configured but never verified — open the manage modal and run Test to probe.">
            <span className="w-1.5 h-1.5 rounded-full bg-stone-400" />
            Configured
          </span>
        );
      }
      return (
        <span className="inline-flex items-center gap-1 text-[11px] text-sage-700 dark:text-sage-400">
          <span className="w-1.5 h-1.5 rounded-full bg-sage-500" />
          Connected
        </span>
      );
    case 'error':
      return (
        <span
          className="inline-flex items-center gap-1 text-[11px] text-coral-600"
          title={status.reason}>
          <span className="w-1.5 h-1.5 rounded-full bg-coral-500" />
          Error
        </span>
      );
    case 'disabled':
      return (
        <span className="inline-flex items-center gap-1 text-[11px] text-stone-500 dark:text-neutral-400">
          <span className="w-1.5 h-1.5 rounded-full bg-stone-300" />
          Disabled
        </span>
      );
    case 'not_connected':
      return (
        <span className="inline-flex items-center gap-1 text-[11px] text-stone-400 dark:text-neutral-500">
          Connect
        </span>
      );
  }
}

export default function ConnectorTile({
  name,
  icon,
  status,
  verification,
  requireVerification = false,
  description,
  onClick,
  disabled,
  title,
  testId,
}: ConnectorTileProps) {
  const interactive = onClick != null && !disabled;
  const className =
    'flex flex-col items-center justify-center gap-1.5 px-2 py-3 bg-white dark:bg-neutral-900 border border-stone-200 dark:border-neutral-700 rounded-xl shadow-subtle text-center transition-all ' +
    (interactive
      ? 'hover:shadow-soft hover:border-primary-300 dark:hover:border-primary-700 focus:outline-none focus:ring-2 focus:ring-primary-500 cursor-pointer'
      : 'opacity-70 cursor-default');

  return (
    <button
      type="button"
      onClick={onClick}
      disabled={disabled || !onClick}
      title={title}
      data-testid={testId}
      className={className}>
      <div className="mb-0.5 flex items-center justify-center h-9">{icon}</div>
      <div className="text-xs font-medium text-stone-900 dark:text-neutral-100 truncate max-w-full">
        {name}
      </div>
      <StatusPill
        status={status}
        verification={verification}
        requireVerification={requireVerification}
      />
      {description ? (
        <div className="text-[10px] text-stone-500 dark:text-neutral-400 truncate max-w-full">
          {description}
        </div>
      ) : null}
    </button>
  );
}

/**
 * "+ Add custom" tile shown at the end of a catalog grid. Lighter chrome so
 * it reads as an action affordance, not a connector.
 */
export function AddCustomTile({
  label,
  onClick,
  testId,
}: {
  label: string;
  onClick: () => void;
  testId?: string;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-testid={testId}
      className="flex flex-col items-center justify-center gap-1.5 px-2 py-3 bg-stone-50 dark:bg-neutral-800/40 border border-dashed border-stone-300 dark:border-neutral-700 rounded-xl text-center transition-all hover:bg-stone-100 dark:hover:bg-neutral-800 hover:border-primary-400 focus:outline-none focus:ring-2 focus:ring-primary-500 cursor-pointer">
      <div className="mb-0.5 flex items-center justify-center h-9 text-stone-500 dark:text-neutral-400">
        <svg
          className="h-6 w-6"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.8"
          strokeLinecap="round">
          <path d="M12 5v14M5 12h14" />
        </svg>
      </div>
      <div className="text-xs font-medium text-stone-700 dark:text-neutral-300 truncate max-w-full">
        {label}
      </div>
      <div className="text-[11px] text-stone-400 dark:text-neutral-500">Add custom</div>
    </button>
  );
}
