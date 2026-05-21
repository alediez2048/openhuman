# Workflows & Automations — Current State

**Last updated:** 2026-05-20
**Branch:** `main` on `alediez2048/openhuman` (the user's fork). Upstream `tinyhumansai/openhuman` not pushed to yet — this is private dev so far.

A fresh session should read this file first to know where the initiative stands.

---

## TL;DR

**Phase 0 (Connections Hub) is SHIPPED to `main`.** All 6 connection mechanisms (Composio, Channels, Browser, Built-in, MCP, Generic HTTP) are unified under `/connections` with a real verification model, inline add/manage modals, and an agent tool that gives the LLM the same view the UI shows.

**Phase 1 (Workflows Foundation) is the next ticket — `Automations/Tickets/phase-1-foundation/F-1.md`.**

---

## What's on `main` right now

### Backend (Rust, `src/openhuman/`)

- **`connections/` domain** — `types`, `store`, `ops`, `aggregator`, `rpc`, `schemas`, `bus`, `verification`. SQLite at `${workspace}/connections.db` with the `generic_http_connections` table. ChaCha20-Poly1305 secret storage via `security/secrets`.
- **Aggregator** — `connections::aggregator::list_all(config)` fans out across 6 collectors in parallel (`composio`, `channels`, `webview`, `builtin`, `mcp`, `generic_http`). Every collector reads through the home domain's public API — no parallel architecture. **Each collector rebuilds its source registry per call** (no Arc snapshots). 3s timeout on Composio (network); others are local.
- **Verification cache** (`connections/verification.rs`) — process-local `OnceLock<Mutex<HashMap>>` mapping `VerificationKey` → `Verification { last_probed_at, result: Live | Failed }`. Probe RPCs (`connections_test`, `connections_mcp_test`) write to it; aggregator reads from it via `verification::lookup`. **In-memory only** — resets on core restart.
- **MCP probes** — `ops::test_mcp_server` calls `McpServerRegistry::initialize(server_id)` under a 15s timeout, records into verification cache.
- **MCP add/remove** — `connections_mcp_add` / `connections_mcp_remove` mutate `config.mcp_client.servers` in TOML + persist via `config.save().await`. Aggregator picks up changes on next call (no restart).
- **Real HTTP probes (P0-3a done)** — `ops::test_generic_http` does `HEAD → OPTIONS → GET(Range: 0-0)` fallback chain via reqwest. Decrypts secret_ref just-in-time, applies AuthKind (Bearer/Basic/ApiKeyHeader/QueryParam). Records into verification cache.
- **MCP client compliance** — `mcp_client/client.rs` now sends `Accept: application/json, text/event-stream` on every POST (required by MCP Streamable HTTP spec; Higgsfield + the official `@modelcontextprotocol/sdk` server return 406 without it). Regression test in place.
- **Webview probes** — `webview_accounts/ops.rs::PROVIDERS` curated to {whatsapp, telegram, slack, discord, linkedin, twitter, instagram, messenger}. Gmail/Google Messages/Zoom removed (anti-automation / native-app redirects make them unusable in CEF). Telegram uses **IndexedDB-folder existence** instead of cookies (Telegram Web stores auth in IndexedDB).
- **Capability catalog** — `about_app/catalog.rs` has 3 new automation entries (`view_connections_hub`, `manage_generic_http_connection`, `test_connection`) plus repointed `skills.open_connections_hub`.

### Agent layer

- **`tools::implementations::network::connections::ConnectionsListTool`** — new agent tool `list_connections` that calls `aggregator::list_all`. Single source of truth across all 6 mechanisms. Markdown grouped by mechanism + machine-readable JSON. **Always registered** (no boot-time gate).
- **MCP tools rebuild registry per call** — `McpListServersTool`, `McpListToolsTool`, `McpCallTool` all hold `Arc<Config>` (not `Arc<McpServerRegistry>`). Each `execute()` calls `McpServerRegistry::from_config(&config)`. **Always registered** (no `!mcp_registry.is_empty()` gate). New servers added mid-session immediately visible without restart.
- **Orchestrator + planner prompts updated** — `agent/agents/orchestrator/prompt.md` + `agent.toml` and `agent/agents/planner/agent.toml` direct the agent to call `list_connections` first ("Authoritative source for what's connected"). `composio_list_connections` is documented as the Composio subset.

### Frontend (`app/src/`)

- **`/connections` page** — `ConnectionsHub.tsx` orchestrates 6 sections in a unified tile grid. Search + filter chips in URL state.
- **`<ConnectorTile>`** — shared tile component. Status pill logic: verification overrides status; `requireVerification` prop downgrades Connected → "Configured" for mechanisms where the status field is weak evidence (HTTP/MCP/Channels).
- **Per-section modals**:
  - `ComposioConnectModal` — reused from the legacy Skills page
  - `ChannelSetupModal` — per-channel auth mode pickers
  - `BrowserAccountConnectModal` — hosts the live `<WebviewHost>` inline so the user signs in directly in the Hub (no `/chat` detour). Polls fetchConnections every 4s + 2.5s after close to wait for CEF cookie flush.
  - `GenericHttpEditModal` — Test + Delete buttons inside the modal footer
  - `McpAddModal` — HTTP/Stdio tabs with form validation
  - `McpManageModal` — Test + Remove actions
  - `BuiltinDetailModal` — read-only info (no per-account toggle until backend surface lands)
- **Composio section** — full catalog overlay using `KNOWN_COMPOSIO_TOOLKITS` (~118 toolkits). Square tile grid + category filter chips (All / Chat / Productivity / Tools & Automation / Social / Platform).
- **`connectionsApi.ts`** — unwraps the `RpcOutcome::single_log` `{ result, logs }` envelope automatically. All 9 methods routed through this helper.

### Tests

- **Rust**: ~45 connections-domain tests, ~10 webview_accounts tests (including the IndexedDB Telegram probe), 16 mcp_client tests (including the Accept-header regression). All green.
- **Vitest**: 12 connections component tests + envelope-unwrap tests + redirect tests. Full app suite: 2683 passed / 3 skipped.
- **WDIO E2E**: `app/test/e2e/specs/connections-hub.spec.ts` (not yet run on a live build).

---

## Two pre-existing test failures (NOT ours)

These fail under `pnpm test:rust` and predate the branch:

1. **`agent::harness::session::turn::*`** — tests read the developer's real `~/.openhuman/` memory tree instead of an isolated tempdir. Test-isolation bug.
2. **`tools::network::polymarket::place_order_happy_path`** — mock-server contract drift. Passes under plain `cargo test`, fails under the `test:rust` mock wrapper.

Don't waste time investigating these for Phase 1 work.

---

## Deferred follow-ups (none blocking)

| Tag | What | Why parked |
|---|---|---|
| **P0-2d-channels-probes** | Telegram `getMe`, Discord WS, iMessage AppleScript probes | Verification framework is ready; just need per-provider impls |
| **P0-5c.flush** | Tauri IPC wrapping `CefCookieManager::FlushStore()` for instant browser-account status updates | Polling works; this would be a UX upgrade |
| **P0-6a** | Built-in per-account toggle UI | Backend has no per-account integration-enabled surface yet |
| **P0-6b.edit** | MCP edit (not just add/remove) — needs `connections_mcp_describe` RPC | Today users edit `config.toml` for endpoint/args/env changes |
| **Verification persistence** | Survive core restart | Currently in-memory only |
| **Auto-probe on Hub open** | Run probes once when user opens the page | Currently user clicks Test explicitly |
| **"Delegation Guide — Integrations" prompt block** | Still Composio-only; dynamic `list_connections` tool fixes worst case | Cosmetic — agent has the right tool now |
| **Generic HTTP "saved connection" agent tool** | Agent has raw `http_request` but no awareness of saved connections by id | Phase 2 work — the workflows engine needs this too |
| **`connections_test` happy-path test** | Currently smoke-tested against example.com offline | Needs a mock-based test |
| **WDIO E2E run** | Spec written, never run on a live build | Requires built Tauri bundle |

---

## Gotchas learned this session

- **`pnpm dev:app` reinstalls vendored tauri-cli** when Cargo.lock changes. First build after a Rust change takes 1–3 minutes. Pre-existing warnings in `vendor/tauri-cef/` are all harmless.
- **Rust changes require `pnpm dev:app` restart**. Vite HMR only handles frontend.
- **Agent prompts are captured at thread/session start.** After changing `orchestrator/prompt.md` or `agent.toml`, ALSO start a new chat thread, not just reload the page.
- **`RpcOutcome::single_log` wraps responses in `{ result, logs }`** — frontend api clients must unwrap. `connectionsApi.ts` has a helper; replicate the pattern for any new domain.
- **Aggregator collectors must rebuild source registries per call** — never hold `Arc<Registry>` snapshots in tools or services. The user-reported "no higgsfield mcp" bug was exactly this anti-pattern. See `connections/aggregator.rs::collect_mcp` and `tools/impl/network/mcp.rs::fresh_registry` as the reference pattern.
- **MCP HTTP clients MUST send `Accept: application/json, text/event-stream`** — spec-strict servers return 406 without it. Test exists in `mcp_client/client.rs::initialize_sends_required_accept_header`.
- **Telegram Web stores auth in IndexedDB**, not cookies. Pattern in `webview_accounts/ops.rs::Provider::indexeddb_origin`.
- **CEF cookies don't flush synchronously.** Inline browser-account modal polls `fetchConnections` every 4s while open + 2.5s after close. A proper Tauri IPC wrapping `FlushStore()` would be cleaner.
- **`X.com` login needs `arkoselabs.com`, `funcaptcha.com`, `gstatic.com`, `recaptcha.net`, `accounts.google.com`** in the CEF allowed-hosts list, or the page paints blank. Same class of bug for any provider that uses third-party CAPTCHA.

---

## What Phase 1 starts with

**File: `Automations/Tickets/phase-1-foundation/F-1.md`** — Workflows Rust domain scaffold (`workflows/` parallel to `connections/`).

Phase 1 builds the workflows model + chat-driven creation. The drafting sub-agent uses `list_connections` (already shipped) for hybrid connection discovery (ADR-009). The proposal preview design is in `Automations/Artifacts/designs/workflow-proposal-preview.md`.

Critical ADRs the next session should read before starting F-1:
- ADR-002 — Phase 1 scope (foundation + execution split)
- ADR-007 — chat as the primary creation path
- ADR-009 — hybrid connection discovery
- ADR-010 — button confirmation (no text matching)
- ADR-011 — missing-connections save with health flag
- ADR-017 — workflow-health computed field
- ADR-020 — WorkflowProposalPreview design synthesis
