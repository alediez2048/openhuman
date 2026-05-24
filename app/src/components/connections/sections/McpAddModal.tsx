/**
 * Inline "Add MCP server" modal (P0-6b).
 *
 * Two transport tabs:
 *
 * - **HTTP** — for hosted MCP servers (Linear, Notion). Captures the
 *   endpoint URL + optional bearer token. The most common case.
 * - **Stdio** — for local MCP servers (`npx @modelcontextprotocol/server-…`).
 *   Captures the command, args (one per line), and env vars (KEY=VALUE per
 *   line). Auth is always None for stdio — credentials go in env vars.
 *
 * Submit calls `connectionsApi.mcpAdd(...)` which mutates
 * `config.mcp_client.servers` in TOML on the Rust side and persists. On
 * success the modal closes and `fetchConnections()` re-runs so the MCP
 * tile flips to Connected.
 */
import { useEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

import { connectionsApi } from '../../../services/api/connectionsApi';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch } from '../../../store/hooks';
import type { McpAddAuth, McpAddRequest } from '../../../types/connections';

type Transport = 'http' | 'stdio';
type HttpAuthKind = 'none' | 'bearer_token';

interface Props {
  onClose: () => void;
}

function parseEnvLines(raw: string): Array<[string, string]> {
  return raw
    .split('\n')
    .map(line => line.trim())
    .filter(line => line.length > 0 && !line.startsWith('#'))
    .map(line => {
      const idx = line.indexOf('=');
      if (idx === -1) return [line, ''] as [string, string];
      return [line.slice(0, idx).trim(), line.slice(idx + 1)] as [string, string];
    })
    .filter(([k]) => k.length > 0);
}

function parseArgLines(raw: string): string[] {
  return raw
    .split('\n')
    .map(line => line.trim())
    .filter(line => line.length > 0);
}

export default function McpAddModal({ onClose }: Props) {
  const dispatch = useAppDispatch();
  const [transport, setTransport] = useState<Transport>('http');
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');

  // HTTP fields
  const [endpoint, setEndpoint] = useState('');
  const [httpAuthKind, setHttpAuthKind] = useState<HttpAuthKind>('none');
  const [bearerToken, setBearerToken] = useState('');
  // Default masked (security floor); click Show to reveal so the user
  // can verify what they pasted before saving. Resets every time the
  // modal opens because state lives at component scope.
  const [showBearerToken, setShowBearerToken] = useState(false);

  // Stdio fields
  const [command, setCommand] = useState('');
  const [argsRaw, setArgsRaw] = useState('');
  const [envRaw, setEnvRaw] = useState('');

  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
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

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);
    if (!name.trim()) {
      setError('Name is required.');
      return;
    }
    if (transport === 'http' && !endpoint.trim()) {
      setError('Endpoint URL is required for HTTP transport.');
      return;
    }
    if (transport === 'stdio' && !command.trim()) {
      setError('Command is required for stdio transport.');
      return;
    }

    let auth: McpAddAuth = { kind: 'none' };
    if (transport === 'http' && httpAuthKind === 'bearer_token') {
      if (!bearerToken.trim()) {
        setError('Bearer token cannot be empty when "Bearer" auth is selected.');
        return;
      }
      auth = { kind: 'bearer_token', token: bearerToken };
    }

    const req: McpAddRequest =
      transport === 'http'
        ? {
            name: name.trim(),
            endpoint: endpoint.trim(),
            description: description.trim() || null,
            auth,
          }
        : {
            name: name.trim(),
            command: command.trim(),
            args: parseArgLines(argsRaw),
            env: parseEnvLines(envRaw),
            description: description.trim() || null,
            auth: { kind: 'none' },
          };

    setSubmitting(true);
    try {
      await connectionsApi.mcpAdd(req);
      void dispatch(fetchConnections());
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return createPortal(
    <div
      ref={backdropRef}
      onClick={onBackdropClick}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6"
      data-testid="mcp-add-modal">
      <form
        onSubmit={onSubmit}
        className="w-full max-w-lg bg-white dark:bg-neutral-900 rounded-2xl shadow-large p-5 max-h-[90vh] overflow-y-auto">
        <h2 className="text-base font-display font-semibold text-stone-900 dark:text-neutral-100 mb-3">
          Add MCP server
        </h2>

        <div className="mb-4 flex gap-2" role="tablist">
          <button
            type="button"
            role="tab"
            aria-selected={transport === 'http'}
            onClick={() => setTransport('http')}
            className={`px-3 py-1.5 text-xs rounded-lg ${
              transport === 'http'
                ? 'bg-primary-500 text-white'
                : 'bg-stone-100 dark:bg-neutral-800 text-stone-700 dark:text-neutral-300'
            }`}
            data-testid="mcp-add-transport-http">
            HTTP
          </button>
          <button
            type="button"
            role="tab"
            aria-selected={transport === 'stdio'}
            onClick={() => setTransport('stdio')}
            className={`px-3 py-1.5 text-xs rounded-lg ${
              transport === 'stdio'
                ? 'bg-primary-500 text-white'
                : 'bg-stone-100 dark:bg-neutral-800 text-stone-700 dark:text-neutral-300'
            }`}
            data-testid="mcp-add-transport-stdio">
            Stdio (local)
          </button>
        </div>

        <label className="block mb-3">
          <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
            Name
          </span>
          <input
            type="text"
            value={name}
            onChange={e => setName(e.target.value)}
            placeholder="linear"
            className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="mcp-add-name"
            autoFocus
          />
        </label>

        {transport === 'http' ? (
          <>
            <label className="block mb-3">
              <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
                Endpoint URL
              </span>
              <input
                type="url"
                value={endpoint}
                onChange={e => setEndpoint(e.target.value)}
                placeholder="https://mcp.linear.app/sse"
                className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="mcp-add-endpoint"
              />
            </label>
            <label className="block mb-3">
              <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
                Auth
              </span>
              <select
                value={httpAuthKind}
                onChange={e => setHttpAuthKind(e.target.value as HttpAuthKind)}
                className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500">
                <option value="none">None</option>
                <option value="bearer_token">Bearer token</option>
              </select>
            </label>
            {httpAuthKind === 'bearer_token' ? (
              <label className="block mb-3">
                <div className="flex items-center justify-between">
                  <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
                    Bearer token
                  </span>
                  <button
                    type="button"
                    onClick={() => setShowBearerToken(prev => !prev)}
                    className="text-xs font-medium text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300"
                    data-testid="mcp-add-bearer-token-toggle"
                  >
                    {showBearerToken ? 'Hide' : 'Show'}
                  </button>
                </div>
                <input
                  type={showBearerToken ? 'text' : 'password'}
                  value={bearerToken}
                  onChange={e => setBearerToken(e.target.value)}
                  placeholder="lin_oauth_…"
                  autoComplete="new-password"
                  className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500 font-mono"
                  data-testid="mcp-add-bearer-token"
                />
              </label>
            ) : null}
          </>
        ) : (
          <>
            <label className="block mb-3">
              <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
                Command
              </span>
              <input
                type="text"
                value={command}
                onChange={e => setCommand(e.target.value)}
                placeholder="npx"
                className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="mcp-add-command"
              />
            </label>
            <label className="block mb-3">
              <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
                Args (one per line)
              </span>
              <textarea
                value={argsRaw}
                onChange={e => setArgsRaw(e.target.value)}
                rows={3}
                placeholder={'-y\n@modelcontextprotocol/server-github'}
                className="mt-1 w-full px-3 py-2 text-sm font-mono bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="mcp-add-args"
              />
            </label>
            <label className="block mb-3">
              <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
                Environment variables (KEY=VALUE per line)
              </span>
              <textarea
                value={envRaw}
                onChange={e => setEnvRaw(e.target.value)}
                rows={3}
                placeholder={'GITHUB_PERSONAL_ACCESS_TOKEN=ghp_…'}
                className="mt-1 w-full px-3 py-2 text-sm font-mono bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
                data-testid="mcp-add-env"
              />
            </label>
          </>
        )}

        <label className="block mb-3">
          <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
            Description (optional)
          </span>
          <input
            type="text"
            value={description}
            onChange={e => setDescription(e.target.value)}
            placeholder="Linear issues + projects MCP"
            className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="mcp-add-description"
          />
        </label>

        {error ? (
          <div
            className="mb-3 px-3 py-2 text-xs text-coral-700 bg-coral-50 border border-coral-200 rounded-lg"
            role="alert">
            {error}
          </div>
        ) : null}

        <div className="flex items-center justify-end gap-2 pt-2">
          <button
            type="button"
            onClick={onClose}
            disabled={submitting}
            className="px-3 py-1.5 text-sm text-stone-600 hover:text-stone-900 dark:text-neutral-400 dark:hover:text-neutral-100 disabled:opacity-50">
            Cancel
          </button>
          <button
            type="submit"
            disabled={submitting}
            className="px-3.5 py-1.5 text-sm font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg disabled:opacity-60"
            data-testid="mcp-add-save">
            {submitting ? 'Adding…' : 'Add server'}
          </button>
        </div>
      </form>
    </div>,
    document.body
  );
}
