# Phase 0 — Connections Hub · DEVLOG

**Status:** Shipped (local branch `feat/connections-domain-skeleton`).
**Date completed:** 2026-05-19.
**Charter:** [`ADR-006-connections-hub-as-phase-0.md`](../../ADRs/ADR-006-connections-hub-as-phase-0.md).
**Acceptance criteria:** [`requirements.md §4`](../../requirements.md).

Phase 0 unified six previously-scattered connection mechanisms (Composio
toolkits, native chat channels, CEF-hosted browser accounts, backend-proxied
built-in integrations, MCP servers, and user-defined Generic HTTP endpoints)
into a single `/connections` surface backed by a new Rust domain. It is the
foundation that Phase 1's `workflows` domain reads from when proposing,
validating, and gating workflow drafts.

---

## Ticket-by-ticket summary

| Ticket | Commit | Subject |
|---|---|---|
| P0-1 | `b79b009d` | Scaffold `connections/` Rust domain (types, store, migrations, ops, rpc, schemas, bus stub). |
| P0-2 | `2480e2c4` | `connections_list` RPC + aggregator over 6 collectors (5 stubbed, `generic_http` wired). |
| P0-3 | `10e19926` | Generic HTTP CRUD + ChaCha20-Poly1305 secret integration + `connections_test` stub. |
| P0-4 | `659e8687` | `/skills` → `/connections` route rename + bottom-tab rename + `/channels` deep-link redirect. |
| P0-5 | `8bc4366c` | Hub UI: 6 section components + search + filter chips + Generic HTTP modal wired end-to-end. |
| P0-6 | `39888629` | Wire `collect_builtin` and `collect_mcp`; sections render real data on a fresh workspace. |
| P0-7 | _this branch_ | Capability catalog entries + `connections-hub.spec.ts` E2E + this DEVLOG. |

---

## Final Phase 0 surface

### RPC surface (`src/openhuman/connections/`)

| Method | Purpose |
|---|---|
| `openhuman.connections_list` | Unified aggregator-driven snapshot. Optional `kind_filter: ConnectionKind[]` and case-insensitive `search` substring against `display_name`. |
| `openhuman.connections_generic_http_create` | Insert a `GenericHttpConnection` row; encrypts the cleartext credential into `SecretRef { ciphertext }` via `SecretStore::encrypt`. |
| `openhuman.connections_generic_http_update` | Partial update; rotating `auth_credential = Some(_)` mints a new ciphertext and drops the old one. |
| `openhuman.connections_generic_http_delete` | Idempotent delete; publishes `ConnectionRemoved`. |
| `openhuman.connections_test` | Best-effort probe. Stubbed in P0-3 — returns `ok = true` for existing rows; real HTTP HEAD probe deferred as `P0-3a`. |

All five exposed via controller-only registration (`schemas.rs`); no branches
added under `src/core/cli.rs` or `src/core/jsonrpc.rs`.

### Event-bus additions (`src/core/event_bus/events.rs`)

| Variant | Payload | Publisher |
|---|---|---|
| `ConnectionAdded` | `connection_ref_json: serde_json::Value` | `ops::create_generic_http` (P0-3). |
| `ConnectionUpdated` | same | reserved — emitted once we mutate live mechanism state (Phase 1 health subscriber). |
| `ConnectionRemoved` | same | `ops::delete_generic_http` (P0-3). |

All three route to the new `"connection"` domain in `DomainEvent::domain()`.
Workflow-health recomputation (`ADR-017`) consumes these in Phase 1.

### UI surface (`app/src/`)

| Route | Component | Notes |
|---|---|---|
| `/connections` | `pages/Connections.tsx` → `components/connections/ConnectionsHub.tsx` | Canonical hub. |
| `/skills` | `<Navigate to="/connections" replace />` | Legacy → redirect. |
| `/channels` | `<Navigate to="/connections#channels" replace />` | Deep-link to the Channels section. |
| Bottom-tab | `BottomTabBar.tsx::'connections'` | Renamed from `'skills'` in P0-4. |

Hub sections (six, in canonical order):

1. `<ComposioSection>` — read-only cards (per-mechanism CRUD reuse deferred as `P0-5a`).
2. `<ChannelsSection>` — read-only cards (`P0-5b`).
3. `<WebviewAccountsSection>` — read-only cards (`P0-5c`).
4. `<BuiltinIntegrationsSection>` — real data from `collect_builtin`; status derived from `integrations::build_client` (`P0-6`). Per-account toggle deferred as `P0-6a`.
5. `<McpServersSection>` — real data from `McpServerRegistry::from_config` (`P0-6`). Restart / enable-disable deferred as `P0-6b`.
6. `<GenericHttpSection>` + `<GenericHttpEditModal>` — fully interactive: add / edit / test / delete (`P0-5`).

URL state for search (`?search=…`) and filter chips (`?kind=…`) handled via
`useSearchParams`. `#channels` anchor triggers scroll-into-view on mount.

### Persistence (`connections.db` under `OPENHUMAN_WORKSPACE`)

- `schema_migrations` — migration tracking table (idempotent re-open).
- `generic_http_connections` — `id` (UUIDv7), `name`, `base_url`, `auth_kind`
  (JSON), `secret_ref` (nullable JSON), `default_headers` (JSON), timestamps.

Credentials live in `security/secrets` as `enc2:<hex>` ChaCha20-Poly1305
blobs. The cleartext credential is never serialized to the DB, never logged,
and is dropped from memory immediately after encryption.

### Capability catalog (`src/openhuman/about_app/catalog.rs`)

Three new entries under `category: Automation`:

- `automation.view_connections_hub` — umbrella entry (Stable).
- `automation.manage_generic_http_connection` — CRUD entry (Stable, `LOCAL_CREDENTIALS` privacy).
- `automation.test_connection` — probe entry (Beta).

Updated the existing `skills.open_connections_hub` deep-link entry to point
at `/connections` and bumped its status from Beta → Stable.

---

## Tests landed

| Surface | Count | Location |
|---|---|---|
| Rust unit (connections domain) | 38 | `src/openhuman/connections/{types,store,ops,aggregator,rpc}_tests.rs` |
| Rust unit (about_app catalog) | +1 | `src/openhuman/about_app/catalog_tests.rs::connections_hub_phase_0_capabilities_are_registered` |
| Vitest (frontend) | 14 | `app/src/components/connections/__tests__/`, `app/src/services/api/__tests__/connectionsApi.test.ts`, `app/src/pages/__tests__/connections-redirect.test.tsx` |
| WDIO E2E | 1 spec, 6 scenarios | `app/test/e2e/specs/connections-hub.spec.ts` |

Total Vitest sweep at end of P0-6: **2680 passed / 3 skipped** (across the
whole app suite). `cargo test` connections suite clean.

---

## Deferred follow-ups (filed during Phase 0)

These were scoped out of Phase 0 deliberately — none of them block Phase 1.

| Tag | Description | Reason for deferral |
|---|---|---|
| **P0-2a** | Wire `collect_composio` against `composio::ops::list_connected_toolkits`. | Per-mechanism collector wiring split into separate PRs to keep P0-2 small. |
| **P0-2b** | Wire `collect_channels` against `channels` public read APIs. | Same. |
| **P0-2c** | Wire `collect_webview` against the Tauri-side webview account registry (needs a new read RPC). | Crosses the Tauri shell boundary; needs its own audit. |
| **P0-3a** | Replace the `connections_test` stub with a real HTTP HEAD/GET probe. | P0-3 shipped CRUD + status; live probe pulled to a follow-up to land secrets sooner. |
| **P0-4a** | Rename `app/src/components/skills/` → `app/src/components/connections-legacy/` (13+ import sites). | Pure rename, no behavioral change — bundled with a later cleanup commit. |
| **P0-5a** | Reuse the per-card Composio UI from the legacy Skills page in `<ComposioSection>` (live counts, manage account). | The legacy 990-line page (`ConnectionsLegacy.tsx`) still owns this surface; reuse means a careful extraction. |
| **P0-5b** | Reuse the per-card Channels UI (setup/edit) in `<ChannelsSection>`. | Same. |
| **P0-5c** | Reuse `webviewAccountService` for "Add account" / "Re-login" in `<WebviewAccountsSection>`. | Same. |
| **P0-5d** | Replace the URL-driven search/filter tests in `ConnectionsHub.test.tsx` with interactive `userEvent` tests. | React Router v7 + `useSearchParams` roundtrip has timing quirks under `userEvent.type`. |
| **P0-6a** | Per-account toggle + credential rotation for the six built-in integrations. | Built-ins are backend-proxied; toggle UI is meaningless until the backend exposes a per-account integration-enabled surface. |
| **P0-6b** | MCP restart / enable-disable / inline add-server. | The registry has no in-process "restart" verb today (HTTP clients are lazy, stdio per-call). Needs a lifecycle RPC + TOML mutation flow. |

Each will get its own ticket when work resumes, or be folded into Phase 1
where it touches workflow execution.

---

## Hand-off to Phase 1

**Phase 0 is the foundation Phase 1 builds against.** Specifically:

- The `connections_list` RPC becomes the source for the Phase 1 drafting
  sub-agent's hybrid connection-discovery prompt (`ADR-009`).
- `GenericHttpConnection` rows become the targets for Phase 2's
  `http_request` workflow node (`ADR-005`).
- `ConnectionAdded` / `ConnectionRemoved` / `ConnectionUpdated` events
  drive Phase 1's workflow-health recomputation subscriber (`ADR-017`).

Next ticket: [`Automations/Tickets/phase-1-foundation/F-1.md`](../phase-1-foundation/F-1.md).
