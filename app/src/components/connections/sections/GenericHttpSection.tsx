/**
 * Generic HTTP section of the Connections Hub.
 *
 * Tile grid matching every other section. Each user-saved HTTP connection
 * renders as a tile; clicking opens `<GenericHttpEditModal>` in edit mode
 * with Test + Delete actions in the footer. The `+ Add custom` tile opens
 * the same modal in create mode.
 *
 * Generic HTTP is the **escape hatch** (ADR-005) — wire any REST API,
 * webhook (n8n / Zapier / Make / IFTTT), or custom internal service.
 * Featured templates ship in **Pass B**.
 */
import { useState } from 'react';

import { connectionsApi } from '../../../services/api/connectionsApi';
import { fetchConnections } from '../../../store/connectionsSlice';
import { useAppDispatch } from '../../../store/hooks';
import type { ConnectionView, GenericHttpConnection } from '../../../types/connections';
import { httpIcon } from '../connectorIcons';
import ConnectorTile, { AddCustomTile } from '../ConnectorTile';
import SectionHeader from '../SectionHeader';
import GenericHttpEditModal from './GenericHttpEditModal';

interface Props {
  items: ConnectionView[];
}

type ModalState = { open: false } | { open: true; existing: GenericHttpConnection | null };

export default function GenericHttpSection({ items }: Props) {
  const dispatch = useAppDispatch();
  const [modal, setModal] = useState<ModalState>({ open: false });
  const [toast, setToast] = useState<{ kind: 'ok' | 'err'; text: string } | null>(null);

  const refresh = () => dispatch(fetchConnections());

  const onTest = async (id: string) => {
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
    }
  };

  const onDelete = async (id: string) => {
    if (!window.confirm('Delete this HTTP connection? This cannot be undone.')) return;
    try {
      await connectionsApi.deleteGenericHttp(id);
      setModal({ open: false });
      setToast(null);
      refresh();
    } catch (e) {
      setToast({ kind: 'err', text: e instanceof Error ? e.message : String(e) });
    }
  };

  return (
    <section data-testid="connections-section-generic-http">
      <SectionHeader
        title="Generic HTTP Endpoints"
        count={items.length}
        subtitle="Escape hatch — any REST API, webhook, or external automation platform"
      />

      <div className="grid grid-cols-3 sm:grid-cols-4 md:grid-cols-5 gap-2">
        {items.map((c, i) => {
          const connectionId = c.ref.type === 'generic_http' ? c.ref.connection_id : `unknown-${i}`;
          return (
            <ConnectorTile
              key={connectionId}
              name={c.display_name}
              icon={httpIcon(null)}
              status={c.status}
              verification={c.verification}
              requireVerification
              onClick={async () => {
                setToast(null);
                // Fetch the real persisted row before opening the modal —
                // the aggregator's ConnectionView only carries name +
                // status, not the full auth_kind / base_url / headers
                // shape the modal needs. Constructing a stub here (the
                // pre-P0-3b behaviour) lied to the user by showing empty
                // form fields for a row that actually had data.
                try {
                  const real = await connectionsApi.getGenericHttp(connectionId);
                  if (!real) {
                    setToast({
                      kind: 'err',
                      text: 'Connection no longer exists. The list will refresh.',
                    });
                    refresh();
                    return;
                  }
                  setModal({ open: true, existing: real });
                } catch (e) {
                  setToast({ kind: 'err', text: e instanceof Error ? e.message : String(e) });
                }
              }}
              title={`${c.display_name} — open to test / edit / delete`}
              testId={`connection-card-generic-http-${connectionId}`}
            />
          );
        })}
        <AddCustomTile
          label="HTTP endpoint"
          onClick={() => {
            setModal({ open: true, existing: null });
            setToast(null);
          }}
          testId="generic-http-add-button"
        />
      </div>

      <GenericHttpEditModal
        mode={modal.open && modal.existing ? { kind: 'edit', existing: modal.existing } : 'create'}
        open={modal.open}
        onClose={() => {
          setModal({ open: false });
          setToast(null);
        }}
        onSaved={refresh}
        onTest={onTest}
        onDelete={onDelete}
        toast={toast}
      />
    </section>
  );
}
