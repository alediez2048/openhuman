/**
 * Create / edit form for Generic HTTP connections.
 *
 * The cleartext credential is only ever in memory inside this component (and
 * the in-flight RPC). On submit, the backend encrypts it via security/secrets
 * and persists the resulting enc2:<hex> blob — see ops.rs.
 */
import { useEffect, useState } from 'react';

import { connectionsApi } from '../../../services/api/connectionsApi';
import type {
  AuthKind,
  CreateGenericHttpRequest,
  GenericHttpConnection,
  UpdateGenericHttpRequest,
} from '../../../types/connections';

type Mode = 'create' | { kind: 'edit'; existing: GenericHttpConnection };

interface Props {
  mode: Mode;
  open: boolean;
  onClose: () => void;
  onSaved: () => void;
  /** Optional "Test" action shown in edit mode. Receives the connection id. */
  onTest?: (id: string) => void | Promise<void>;
  /** Optional "Delete" action shown in edit mode. Receives the connection id. */
  onDelete?: (id: string) => void | Promise<void>;
  /** Optional inline status banner pushed by the section after onTest. */
  toast?: { kind: 'ok' | 'err'; text: string } | null;
}

type AuthKindTag = AuthKind['kind'];

const AUTH_KIND_OPTIONS: { value: AuthKindTag; label: string }[] = [
  { value: 'none', label: 'None' },
  { value: 'bearer', label: 'Bearer token' },
  { value: 'basic', label: 'Basic auth' },
  { value: 'api_key_header', label: 'API key (header)' },
  { value: 'query_param', label: 'API key (query param)' },
];

function buildAuthKind(tag: AuthKindTag, paramName: string): AuthKind {
  switch (tag) {
    case 'none':
      return { kind: 'none' };
    case 'bearer':
      return { kind: 'bearer' };
    case 'basic':
      return { kind: 'basic' };
    case 'api_key_header':
      return { kind: 'api_key_header', name: paramName || 'X-API-Key' };
    case 'query_param':
      return { kind: 'query_param', name: paramName || 'api_key' };
  }
}

export default function GenericHttpEditModal({
  mode,
  open,
  onClose,
  onSaved,
  onTest,
  onDelete,
  toast,
}: Props) {
  const isEdit = typeof mode === 'object' && mode.kind === 'edit';
  const existing = isEdit ? (mode as Extract<Mode, { kind: 'edit' }>).existing : null;

  const [name, setName] = useState(existing?.name ?? '');
  const [baseUrl, setBaseUrl] = useState(existing?.base_url ?? '');
  const [authTag, setAuthTag] = useState<AuthKindTag>(existing?.auth_kind.kind ?? 'none');
  const [authParamName, setAuthParamName] = useState<string>(
    existing &&
      (existing.auth_kind.kind === 'api_key_header' || existing.auth_kind.kind === 'query_param')
      ? existing.auth_kind.name
      : ''
  );
  const [credential, setCredential] = useState('');
  // Default masked (security floor); click Show to reveal so the user
  // can verify what they pasted. Resets on each open of the modal.
  const [showCredential, setShowCredential] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  // Reset on open to avoid stale state between sessions.
  useEffect(() => {
    if (!open) return;
    setName(existing?.name ?? '');
    setBaseUrl(existing?.base_url ?? '');
    setAuthTag(existing?.auth_kind.kind ?? 'none');
    setAuthParamName(
      existing &&
        (existing.auth_kind.kind === 'api_key_header' || existing.auth_kind.kind === 'query_param')
        ? existing.auth_kind.name
        : ''
    );
    setCredential('');
    setShowCredential(false);
    setError(null);
    setSubmitting(false);
  }, [open, existing]);

  if (!open) return null;

  const needsParamName = authTag === 'api_key_header' || authTag === 'query_param';
  const needsCredential = authTag !== 'none';

  const validateAndBuildAuth = (): AuthKind => buildAuthKind(authTag, authParamName);

  const onSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError(null);

    if (!name.trim()) {
      setError('Name is required.');
      return;
    }
    if (!baseUrl.startsWith('http://') && !baseUrl.startsWith('https://')) {
      setError('Base URL must start with http:// or https://');
      return;
    }

    setSubmitting(true);
    try {
      const auth = validateAndBuildAuth();
      const auth_credential = needsCredential && credential.trim() ? { secret: credential } : null;

      if (isEdit && existing) {
        const req: UpdateGenericHttpRequest = {
          name,
          base_url: baseUrl,
          auth_kind: auth,
          auth_credential,
        };
        await connectionsApi.updateGenericHttp(existing.id, req);
      } else {
        if (needsCredential && !auth_credential) {
          setError('A credential is required for this auth type.');
          setSubmitting(false);
          return;
        }
        const req: CreateGenericHttpRequest = {
          name,
          base_url: baseUrl,
          auth_kind: auth,
          auth_credential,
          default_headers: existing?.default_headers ?? [],
        };
        await connectionsApi.createGenericHttp(req);
      }
      onSaved();
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40"
      data-testid="generic-http-edit-modal">
      <form
        onSubmit={onSubmit}
        className="w-full max-w-md bg-white dark:bg-neutral-900 rounded-2xl shadow-large p-5">
        <h2 className="text-base font-display font-semibold text-stone-900 dark:text-neutral-100 mb-3">
          {isEdit ? 'Edit HTTP connection' : 'Add HTTP connection'}
        </h2>

        <label className="block mb-3">
          <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
            Name
          </span>
          <input
            type="text"
            value={name}
            onChange={e => setName(e.target.value)}
            placeholder="my-zapier-hook"
            className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="generic-http-modal-name"
          />
        </label>

        <label className="block mb-3">
          <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
            Base URL
          </span>
          <input
            type="url"
            value={baseUrl}
            onChange={e => setBaseUrl(e.target.value)}
            placeholder="https://hooks.zapier.com/hooks/catch/12345/abc"
            className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="generic-http-modal-base-url"
          />
        </label>

        <label className="block mb-3">
          <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
            Authentication
          </span>
          <select
            value={authTag}
            onChange={e => setAuthTag(e.target.value as AuthKindTag)}
            className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
            data-testid="generic-http-modal-auth-kind">
            {AUTH_KIND_OPTIONS.map(opt => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        </label>

        {needsParamName ? (
          <label className="block mb-3">
            <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
              {authTag === 'api_key_header' ? 'Header name' : 'Query parameter name'}
            </span>
            <input
              type="text"
              value={authParamName}
              onChange={e => setAuthParamName(e.target.value)}
              placeholder={authTag === 'api_key_header' ? 'X-API-Key' : 'api_key'}
              className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500"
            />
          </label>
        ) : null}

        {needsCredential ? (
          <label className="block mb-3">
            <div className="flex items-center justify-between">
              <span className="text-xs uppercase tracking-wide font-semibold text-stone-500 dark:text-neutral-400">
                Credential {isEdit && '(leave blank to keep existing)'}
              </span>
              <button
                type="button"
                onClick={() => setShowCredential(prev => !prev)}
                className="text-xs font-medium text-primary-600 hover:text-primary-700 dark:text-primary-400 dark:hover:text-primary-300"
                data-testid="generic-http-modal-credential-toggle"
              >
                {showCredential ? 'Hide' : 'Show'}
              </button>
            </div>
            <input
              type={showCredential ? 'text' : 'password'}
              value={credential}
              onChange={e => setCredential(e.target.value)}
              placeholder={isEdit ? '••••••••' : 'token'}
              autoComplete="new-password"
              className="mt-1 w-full px-3 py-2 text-sm bg-white dark:bg-neutral-800 border border-stone-300 dark:border-neutral-600 rounded-lg focus:outline-none focus:ring-2 focus:ring-primary-500 font-mono"
              data-testid="generic-http-modal-credential"
            />
          </label>
        ) : null}

        {error ? (
          <div className="mb-3 px-3 py-2 text-xs text-coral-700 bg-coral-50 border border-coral-200 rounded-lg">
            {error}
          </div>
        ) : null}

        {toast ? (
          <div
            className={`mb-3 px-3 py-2 text-xs rounded-lg ${
              toast.kind === 'ok'
                ? 'text-sage-700 bg-sage-50 border border-sage-200'
                : 'text-coral-700 bg-coral-50 border border-coral-200'
            }`}
            role="status">
            {toast.text}
          </div>
        ) : null}

        <div className="flex items-center justify-between gap-2 pt-2">
          <div className="flex items-center gap-2">
            {isEdit && existing && onTest ? (
              <button
                type="button"
                onClick={() => onTest(existing.id)}
                disabled={submitting}
                className="px-3 py-1.5 text-sm text-stone-700 dark:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md disabled:opacity-50"
                data-testid="generic-http-modal-test">
                Test
              </button>
            ) : null}
            {isEdit && existing && onDelete ? (
              <button
                type="button"
                onClick={() => onDelete(existing.id)}
                disabled={submitting}
                className="px-3 py-1.5 text-sm text-coral-600 hover:bg-coral-50 dark:hover:bg-coral-950/30 rounded-md disabled:opacity-50"
                data-testid="generic-http-modal-delete">
                Delete
              </button>
            ) : null}
          </div>
          <div className="flex items-center gap-2">
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
              data-testid="generic-http-modal-save">
              {submitting ? 'Saving…' : isEdit ? 'Save changes' : 'Add connection'}
            </button>
          </div>
        </div>
      </form>
    </div>
  );
}
