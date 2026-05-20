/**
 * Generic HTTP section of the Connections Hub.
 *
 * The escape-hatch connection type (ADR-005). Fully wired in P0-5: list,
 * create, test (stubbed probe), delete. Edit reopens the modal.
 *
 * P0-3 RPCs are the contract: `connections_generic_http_create/_update/
 * _delete`, `connections_test`.
 */
import { useState } from 'react';

import { connectionsApi } from '../../../services/api/connectionsApi';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch } from '../../../store/hooks';
import type { ConnectionView, GenericHttpConnection } from '../../../types/connections';
import ConnectionCard from '../ConnectionCard';
import SectionHeader from '../SectionHeader';
import GenericHttpEditModal from './GenericHttpEditModal';

interface Props {
  items: ConnectionView[];
}

type ModalState = { open: false } | { open: true; existing: GenericHttpConnection | null };

export default function GenericHttpSection({ items }: Props) {
  const dispatch = useAppDispatch();
  const [modal, setModal] = useState<ModalState>({ open: false });
  const [busyId, setBusyId] = useState<string | null>(null);
  const [toast, setToast] = useState<{ kind: 'ok' | 'err'; text: string } | null>(null);

  const refresh = () => dispatch(fetchConnections());

  const onTest = async (id: string) => {
    setBusyId(id);
    setToast(null);
    try {
      const r = await connectionsApi.test(id);
      setToast({
        kind: r.ok ? 'ok' : 'err',
        text: r.ok
          ? (r.error ?? 'Probe ok (P0-3 stub).')
          : (r.error ?? `Probe failed${r.status ? ` (HTTP ${r.status})` : ''}`),
      });
    } catch (e) {
      setToast({ kind: 'err', text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusyId(null);
    }
  };

  const onDelete = async (id: string, name: string) => {
    if (!window.confirm(`Delete "${name}"? This cannot be undone.`)) return;
    setBusyId(id);
    try {
      await connectionsApi.deleteGenericHttp(id);
      refresh();
    } catch (e) {
      setToast({ kind: 'err', text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusyId(null);
    }
  };

  const cta = (
    <button
      type="button"
      onClick={() => setModal({ open: true, existing: null })}
      className="px-3 py-1.5 text-xs font-medium text-white bg-primary-500 hover:bg-primary-600 rounded-lg"
      data-testid="generic-http-add-button">
      + Add HTTP connection
    </button>
  );

  return (
    <section data-testid="connections-section-generic-http">
      <SectionHeader
        title="Generic HTTP Endpoints"
        count={items.length}
        subtitle="Any REST API — bridge to n8n, Zapier, IFTTT, or your own services"
        cta={cta}
      />

      {items.length === 0 ? (
        <div className="text-sm text-stone-500 dark:text-neutral-400 px-3.5 py-4 bg-stone-50 dark:bg-neutral-800 rounded-xl">
          No HTTP connections yet. Add one to wire up any REST endpoint or external automation
          platform webhook.
        </div>
      ) : (
        <div className="space-y-2">
          {items.map((c, i) => {
            const connectionId =
              c.ref.type === 'generic_http' ? c.ref.connection_id : `unknown-${i}`;
            const busy = busyId === connectionId;
            return (
              <ConnectionCard
                key={connectionId}
                name={c.display_name}
                subtitle={c.mechanism_label}
                status={c.status}
                testId={`connection-card-generic-http-${connectionId}`}
                actions={
                  <div className="flex items-center gap-2">
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => onTest(connectionId)}
                      className="px-2.5 py-1 text-xs text-stone-700 dark:text-neutral-200 hover:bg-stone-100 dark:hover:bg-neutral-800 rounded-md disabled:opacity-50">
                      {busy ? 'Testing…' : 'Test'}
                    </button>
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => onDelete(connectionId, c.display_name)}
                      className="px-2.5 py-1 text-xs text-coral-600 hover:bg-coral-50 dark:hover:bg-coral-950/30 rounded-md disabled:opacity-50"
                      data-testid={`generic-http-delete-${connectionId}`}>
                      Delete
                    </button>
                  </div>
                }
              />
            );
          })}
        </div>
      )}

      {toast ? (
        <div
          className={`mt-2 px-3 py-2 text-xs rounded-lg ${
            toast.kind === 'ok'
              ? 'text-sage-700 bg-sage-50 border border-sage-200'
              : 'text-coral-700 bg-coral-50 border border-coral-200'
          }`}
          role="status">
          {toast.text}
        </div>
      ) : null}

      <GenericHttpEditModal
        mode={modal.open && modal.existing ? { kind: 'edit', existing: modal.existing } : 'create'}
        open={modal.open}
        onClose={() => setModal({ open: false })}
        onSaved={refresh}
      />
    </section>
  );
}
