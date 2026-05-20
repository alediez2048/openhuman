/**
 * Manage-MCP-server modal (P0-6b follow-up).
 *
 * Opens when the user clicks an MCP tile in the Hub. Shows the server name
 * + status and exposes a Remove action wired to the new
 * `connections_mcp_remove` RPC. The aggregator picks up the deletion on
 * the next `connections_list` refresh — same hot-reload contract as Add.
 *
 * For now this is intentionally lean: name + remove. Full per-server
 * config edit (endpoint, args, env) is filed as P0-6b.edit — would need
 * either a new RPC that returns the McpServerConfig by name, or carrying
 * the full config in the aggregator's ConnectionView (which currently
 * only carries the slug).
 */
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { connectionsApi } from '../../../services/api/connectionsApi';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch } from '../../../store/hooks';
import type { ConnectionStatus } from '../../../types/connections';

interface Props {
  serverId: string;
  displayName: string;
  status: ConnectionStatus;
  onClose: () => void;
}

export default function McpManageModal({ serverId, displayName, status, onClose }: Props) {
  const dispatch = useAppDispatch();
  const [removing, setRemoving] = useState(false);
  const [error, setError] = useState<string | null>(null);
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

  const onRemove = async () => {
    if (!window.confirm(`Remove the "${displayName}" MCP server? This cannot be undone.`)) return;
    setError(null);
    setRemoving(true);
    try {
      await connectionsApi.mcpRemove(serverId);
      void dispatch(fetchConnections());
      onClose();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRemoving(false);
    }
  };

  return createPortal(
    <div
      ref={backdropRef}
      onClick={onBackdropClick}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6"
      data-testid="mcp-manage-modal">
      <div className="w-full max-w-md bg-white dark:bg-neutral-900 rounded-2xl shadow-large p-5">
        <h2 className="text-base font-display font-semibold text-stone-900 dark:text-neutral-100 mb-1">
          {displayName}
        </h2>
        <p className="text-xs text-stone-500 dark:text-neutral-400 mb-4">
          MCP server registered in <code>config.mcp_client.servers</code>. Status:{' '}
          <span
            className={
              status.kind === 'connected'
                ? 'text-sage-700 dark:text-sage-400'
                : status.kind === 'error'
                  ? 'text-coral-600'
                  : 'text-stone-500 dark:text-neutral-400'
            }>
            {status.kind === 'error' ? `Error — ${status.reason}` : status.kind}
          </span>
          .
        </p>

        <div className="mb-4 px-3 py-2 text-xs text-stone-600 dark:text-neutral-400 bg-stone-50 dark:bg-neutral-800 rounded-lg">
          Full per-server config edit (endpoint, args, env) lives in{' '}
          <code>~/.openhuman/config.toml</code> for now. Removing this server here updates that file
          and unregisters the entry on the next aggregator refresh.
        </div>

        {error ? (
          <div
            className="mb-3 px-3 py-2 text-xs text-coral-700 bg-coral-50 border border-coral-200 rounded-lg"
            role="alert">
            {error}
          </div>
        ) : null}

        <div className="flex items-center justify-between pt-2">
          <button
            type="button"
            onClick={onRemove}
            disabled={removing}
            className="px-3 py-1.5 text-sm text-coral-600 hover:bg-coral-50 dark:hover:bg-coral-950/30 rounded-md disabled:opacity-50"
            data-testid="mcp-manage-remove">
            {removing ? 'Removing…' : 'Remove server'}
          </button>
          <button
            type="button"
            onClick={onClose}
            disabled={removing}
            className="px-3.5 py-1.5 text-sm text-stone-700 dark:text-neutral-300 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md disabled:opacity-50">
            Close
          </button>
        </div>
      </div>
    </div>,
    document.body
  );
}
