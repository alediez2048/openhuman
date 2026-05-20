# Workflows & Automations — Tech Stack

**Status:** Draft — Phase 1 design locked (incl. agent-driven creation + seeded templates), Phase 1 blockers all resolved.
**Companion docs:** [`prd.md`](./prd.md), [`requirements.md`](./requirements.md), [`systemsdesign.md`](./systemsdesign.md)

> Pinned versions and dependency choices. Everything inherits OpenHuman's existing toolchain unless explicitly listed. Each ticket primer reads this doc.

---

## 1. Existing Toolchain (inherited; do NOT change)

| Layer | Tool | Version | Source |
|---|---|---|---|
| OS targets | Windows / macOS / Linux desktop | — | `CLAUDE.md §Runtime scope` |
| Rust | rustc | 1.93.0 (pinned via `rust-toolchain.toml`) | repo root |
| Rust components | `rustfmt`, `clippy` | matches toolchain | repo root |
| Node | Node.js | ≥ 24 | `app/package.json#engines` |
| Package manager | pnpm | 10.10.0 | root `package.json#packageManager` |
| Frontend framework | React | 19.1 | `app/package.json` |
| Frontend bundler | Vite | 8 | `app/package.json` |
| Desktop host | Tauri | 2.10 (vendored CEF-aware fork) | `app/src-tauri/vendor/tauri-cef/` |
| Webview runtime | CEF (Chromium Embedded Framework) | 146.0.9 | vendored |
| State management | Redux Toolkit + redux-persist | per `app/package.json` | existing |
| Styling | Tailwind 3 | `app/tailwind.config.js` | existing |
| Routing | react-router-dom 7 | HashRouter | existing |
| Frontend i18n | custom — `app/src/lib/i18n/` | — | existing |
| Test (frontend) | Vitest 4 + Testing Library | per `app/package.json` | existing |
| Test (Rust) | `cargo test` + mock backend (`scripts/test-rust-with-mock.sh`) | — | existing |
| E2E | WebdriverIO 9 + tauri-driver (Linux) / Appium Mac2 (macOS) | per `app/package.json` | existing |
| Rust SQLite | `rusqlite` | per `Cargo.toml` (same as `cron`) | existing |
| Rust async runtime | `tokio` | existing | existing |
| Rust HTTP client | `reqwest` | existing | existing |
| Event bus | in-tree (`src/core/event_bus/`) | — | existing |
| RPC transport | JSON-RPC over HTTP + `coreRpcClient` | — | existing |
| Agent runtime | in-tree (`src/openhuman/agent/`) | — | existing |
| Agent tool registry | in-tree (`src/openhuman/tools/`) | — | existing |
| Agent prompts | bundled (`src/openhuman/agent/prompts/`) | — | existing |
| Webhook tunnel | `src/openhuman/webhooks/` (HMAC-verified) | — | existing |
| Secrets storage | `src/openhuman/security/secrets` | — | existing |

**Bullet rules** carried from `CLAUDE.md`:

- ❌ No dynamic imports in production `app/src` code.
- ❌ No new JS injection into CEF webview accounts.
- ❌ No new standalone `*.rs` at `src/openhuman/` root — new code goes in `src/openhuman/workflows/` (Phase 1+) and `src/openhuman/connections/` (Phase 0).

---

## 2. New Dependencies — Phase 0 (Connections Hub)

**None.**

Phase 0 is a frontend refactor + a new aggregator domain using SQLite (already in workspace). No new crates or npm packages.

- Rust: `rusqlite`, `serde`, `serde_json`, `uuid` (existing) cover the new `generic_http_connections` table and the aggregator.
- Frontend: existing Tailwind / Redux / icon library / i18n stack covers the unified page.

Intentional — Phase 0 ships as a small, easily-reviewable refactor that pre-empts the Workflows feature.

---

## 3. New Dependencies — Phase 1 (Foundation + agent creation + seeded templates)

**None.**

- Rust: `rusqlite`, `serde`, `serde_json`, `uuid`, the existing `cron` crate, `tokio`, `reqwest`, `tracing` (existing). Plus the existing agent runtime + tools registry + prompt bundling — those provide the hero-flow infrastructure with zero new crates.
- Frontend: existing Tailwind tokens + Redux Toolkit cover the list / enable-toggle / empty / form-fallback UI.

The "no new dependency" property holds even with the agent-driven creation + seeded templates: the agent's tool plumbing, the prompt-bundling pipeline, the SQLite + JSON column storage — all already in tree.

---

## 4. New Dependencies — Phase 2 (Execution expansion)

**None** in the happy path.

- **`http_request` node** → existing `reqwest`. Respects existing proxy + TLS settings.
- **`webhook` trigger** → existing `src/openhuman/webhooks/` tunnel + HMAC. New: a `TunnelRegistration::Workflow { workflow_id }` enum variant.
- **`composio_event` trigger** → existing `DomainEvent::ComposioTriggerReceived` event-bus path.
- **`channel_message` trigger** → existing channels-domain inbound stream.
- **`tool_call` node** → existing `tools::registry`.
- **`channel_message` node** → existing `channel_send_message`.
- **`condition` / `delay` nodes** → pure logic.

> **Internal decision (not OQ-tracked):** the cron-trigger reuse mode. Phase 1 reuses `cron`'s existing scheduler by registering each cron-triggered workflow as a soft-id-bearing entry. If that leaks too much surface from `cron::ops` during Phase 1, fall back to a sibling scheduler loop using the same `cron` crate. Decision deferred to the first Phase 1 ticket.

---

## 5. New Dependencies — Phase 3 (Visual canvas, deferred)

| Package | Purpose | Cost | License |
|---|---|---|---|
| `@xyflow/react` (formerly `reactflow`) | Node-graph canvas, edges, drag-to-connect, run-time node highlighting | ~80 KB gzipped | MIT |

Maintained, widely used (n8n itself), no native deps, no peer-dep conflicts with React 19.

**Rejected alternatives:** `react-flow-renderer` (deprecated), `mermaid` / Excalidraw (visualization-only), build-from-scratch (huge scope).

Only purchased if Phase 3 is pursued. The hero product story does not require this.

---

## 6. External-platform interop — Stack

OQ-9 = C. n8n / Zapier / IFTTT / Make served by:

| Direction | Mechanism | Phase | New deps |
|---|---|---|---|
| External → OpenHuman (inbound trigger) | Existing `webhooks/` tunnel + new `TunnelRegistration::Workflow` variant | 2 | None |
| OpenHuman → External (outbound action) | `http_request` node + Phase 0's `GenericHttpConnection` row + existing `reqwest` client | 2 | None |
| Credential storage | `security/secrets` (existing) | 0 | None |

Zero new dependencies for the full external-platform interop story.

Phase 4 (deferred indefinitely) would add platform-specific apps. No deps to pin here.

---

## 7. Persistence

### 7.1 Engine
SQLite via `rusqlite`. Matches existing `cron` and `subconscious` domains.

### 7.2 Location
**Decision (locked, OQ-3 = A):** Two separate database files, one per domain.

| File | Owned by | Phase introduced | Tables |
|---|---|---|---|
| `${OPENHUMAN_WORKSPACE}/connections.db` | `src/openhuman/connections/` | Phase 0 | `generic_http_connections`, `schema_migrations` |
| `${OPENHUMAN_WORKSPACE}/workflows.db` | `src/openhuman/workflows/` | Phase 1 | `workflows` (incl. `health`, `origin` columns), `workflow_runs`, `workflow_run_steps`, `schema_migrations` |

Cross-domain references use soft string ids:
- `workflows.HttpRequestConfig.connection_id` → `connections.generic_http_connections.id`
- `cron::JobType::WorkflowTrigger { workflow_id }` → `workflows.workflows.id`

**No `workspace_state` table.** The grilling resolved that templates ship as a read-only catalog (OQ-12), so no auto-seed watermark is needed. The catalog query is stateless: `workflows_list_starter_templates` reads in-repo JSON at request time, filters by `min_phase`, and dedupes against the user's existing `Workflow.origin = Seed { template_id }` rows.

### 7.3 Schema design
JSON columns (`trigger_json`, `nodes_json`, `edges_json`, `settings_json`) hold structured sub-types. Avoids 6-table FK explosion. Forward-compatible with new node kinds. Top-level columns indexed for list-view queries.

Run tables ARE normalized — queried by workflow, status, or time range.

`origin` column on `workflows`: `TEXT` with values `user_chat`, `user_form`, `seed:<template_id>`, `imported`. Used by the UI to show the "Starter" badge and by analytics to track adoption of each creation path.

### 7.4 Migrations

**`src/openhuman/connections/migrations/`** (`connections.db`):
- `001_init_generic_http.sql` — Phase 0.

**`src/openhuman/workflows/migrations/`** (`workflows.db`):
- `001_init_workflows.sql` — Phase 1: `workflows`, `schema_migrations`.
- `002_runs.sql` — Phase 1: `workflow_runs`.
- `003_run_steps.sql` — Phase 1: `workflow_run_steps`.

Idempotent (CREATE TABLE IF NOT EXISTS). Each domain advances its own `schema_migrations` row independently.

### 7.5 Starter workflows catalog storage
Templates ship at `src/openhuman/workflows/templates/*.json`, embedded via `include_str!` (OQ-8 = in-repo JSON). The `workflows_list_starter_templates` RPC reads + filters them at request time — no caching, no DB-resident copies.

Each template has a `min_phase` field. Templates whose `min_phase > current_phase` are filtered out (RU-5..RU-9 require Phase 2 node kinds, so they don't appear in the Phase 1 catalog).

The catalog is a **read-only catalog** (OQ-12 = catalog model): clicking [Add to my workflows] calls `workflows_create` with `origin = Seed { template_id }`. The template's `template_id` lives in the resulting `Workflow.origin` and is what the catalog query uses for deduplication.

---

## 8. Agent runtime — Workflow creation tooling

Inherits the existing OpenHuman agent infrastructure with two additions, both file-only (no new deps):

1. **New prompt bundle** — `src/openhuman/agent/prompts/workflow_builder.md`. Loaded into the drafting sub-agent's system prompt. Includes Phase-aware schema reminders (trigger and node-kind variants change per phase), the user's connected mechanisms (queried at prompt-render time), a worked example, and the always-preview-before-commit rule.
2. **New tool registrations** — `src/openhuman/workflows/agent_tools.rs` exports `WorkflowProposeTool`, `WorkflowCreateFromProposalTool`, etc. via the existing `tools::Tool` trait. Registered in `tools::ops::all_tools_with_runtime()` behind a `workflows` feature gate (always on).

The drafting sub-agent runs in the same `agent::run_subagent` harness. Tool allowlist is restricted to read tools + `emit_proposal`. Iteration cap (default 6) is set per-call.

---

## 9. Model Tier / LLM Use

`AgentPrompt` nodes use the agent's existing model selection. Override via `AgentPromptConfig.model_tier`:

- `low` — local Ollama / LM Studio.
- `medium` — default (backend-routed).
- `high` — premium tier.

The drafting sub-agent for `workflow_propose` uses `medium` by default (fast, predictable schema output). Templates' `agent_prompt` nodes default to `medium`. Workflow-builder prompt is structured to keep tokens minimal.

Inherits OpenHuman's existing inference routing (`src/openhuman/inference/`).

---

## 10. Observability

- **Logging:** `tracing` / `log` with prefixes `[workflows]`, `[workflows-run]`, `[workflows-rpc]`, `[workflows-agent]`, `[connections]`, `[connections-rpc]`.
- **Sentry:** existing DSN. Tag `feature=workflows`. Never log prompt body, secret values, external response bodies.
- **Event bus:** `systemsdesign.md §8`.
- **Frontend debug:** `debug` package + `console.debug('[workflows] …')`.
- **Telemetry events** (anonymous, opt-out via `OPENHUMAN_ANALYTICS_ENABLED=false`):
    - `workflow_proposal_emitted` — counts each preview component the chat agent renders.
    - `workflow_proposal_validation_error` — by `ProposalValidationError` variant. Drives prompt tuning.
    - `workflow_proposal_save_clicked` / `_discard_clicked` / `_save_and_enable_clicked` — acceptance split.
    - `workflow_created` — by `origin` (`UserChat` / `UserForm` / `Seed { template_id }`).
    - `workflow_enabled` / `workflow_disabled` — toggle adoption.
    - `workflow_run_completed` / `_failed` / `_skipped` — reliability.
    - `starter_template_added` — by `template_id`. Replaces the old `seed_template_activated` (no auto-seed any more).

---

## 11. Testing Stack

Inherits OpenHuman's strategy — see `gitbooks/developing/testing-strategy.md` and `gitbooks/developing/e2e-testing.md`.

- **Mock backend** (`scripts/mock-api-server.mjs`) reused for Vitest + Rust tests. Phase 2 adds endpoints simulating external platforms.
- **Coverage gate:** ≥ 80% on changed lines.
- **Pre-merge:** Prettier, ESLint, `tsc --noEmit`, `cargo fmt`, `cargo check`.
- **E2E adders per phase:**
    - Phase 0: `app/test/e2e/specs/connections-hub.spec.ts`
    - Phase 1:
        - `app/test/e2e/specs/workflows-agent-creation.spec.ts` (the hero flow)
        - `app/test/e2e/specs/workflows-seeded.spec.ts` (seed insertion + activation)
    - Phase 2:
        - `app/test/e2e/specs/workflows-webhook.spec.ts` (RU-8 inbound + outbound)

---

## 12. Frontend Bundle Impact

| Phase | Δ bundle (gzip est.) | Notes |
|---|---|---|
| Phase 0 | ≈ +4 KB | New Connections components (mostly imports of existing per-mechanism UI; no duplication). |
| Phase 1 | ≈ +6 KB | Workflow components + slice + service + seeded-templates UI + proposal-preview component. |
| Phase 2 | ≈ +5 KB | Phase 2 trigger / node-kind config forms + run-history detail expansion. |
| Phase 3 (deferred) | **≈ +85 KB** | `@xyflow/react`. Only loaded on the workflow detail route (code-split). |

Audited via `vite --report` per phase.

---

## 13. Versioning & Compatibility

- Workflow JSON in DB is versioned via `schema_version` on each row. On startup, in-place migration upgrades rows. Older app builds reading a newer DB schema refuse to load workflows and surface a banner; the rest of the app continues working.
- `connections.db` Phase 0 schema is `v1` from day one.
- Starter-template catalog is stateless — no watermark, no `workspace_state` table. New templates land as files; the catalog query (`workflows_list_starter_templates`) dedupes against the user's existing `Workflow.origin = Seed { template_id }` rows on every call. See ADR-008.
- No backwards-compat shims for un-shipped Phase 2/3 schema bumps — code evolves freely until each phase merges.

---

## 14. Pinned Versions to Add Later

- Phase 3 (only if pursued): `@xyflow/react@^12`.

---

## 15. Risk Register (tech-stack-specific)

| Risk | Mitigation |
|---|---|
| Drafting sub-agent produces invalid JSON | `validator::validate` runs after every `emit_proposal`. On failure, `proposer::draft_with_retries` re-invokes the drafting sub-agent with the structured `ProposalValidationError` appended to the system prompt. Up to 3 total attempts. Final failure surfaces a clear error to the chat agent. |
| Agent silently mutates state | **Structurally impossible** post-grilling (OQ-16). The agent has zero mutating workflow tools. Mutations are owned by RPC handlers, reached only via UI button clicks on `<WorkflowProposalPreview>` components. No confirmation-token validation logic needed because the agent surface is closed. |
| Catalog adds a template twice for the same user | `workflows_list_starter_templates` query dedupes against `Workflow.origin = Seed { template_id }`. Phase 1 unit test covers double-Add (calls `workflows_create` with same template, asserts second call doesn't cause issues; UI's catalog excludes the already-added template on next render). |
| `@xyflow/react` major breaking change between Phase 3 spec and merge | Pin to a specific major; defer feature work that depends on un-released APIs. |
| `cron` scheduler reuse leaks too much surface | Phase 1 fallback is a sibling scheduler loop using the same `cron` crate. |
| `webhook` tunnel HMAC regressions | Phase 2 e2e covers happy path + tampered signature. |
| Generic HTTP connection misconfiguration leaks credentials in logs | NFR-2.3.5 mandates output truncation; secret refs never serialize. |
| Workflow proposes a connection the user doesn't have | The proposal carries `missing_connections: Vec<ConnectionRef>` and the workflow saves with `health: NeedsConnections { missing }` (OQ-15). The list-view card surfaces missing connections; the toggle is disabled until they resolve. `bus.rs` subscriber on `ConnectionAdded` recomputes health automatically. |
| Concurrent triggers double-fire a non-idempotent workflow | **Single-flight invariant** (OQ-18). `executor::ExecutorState::in_flight: HashMap<WorkflowId, RunId>` ensures at most one run per workflow. Overlapping triggers publish `WorkflowRunSkipped` and are dropped. |
| `agent_prompt` sub-agent reaches mutating tools | **Structurally impossible** (OQ-20 / NFR-2.3.7). `executor::build_node_agent_definition` returns an allowlist precisely matching baseline + allowed_connections + 4 read-only workflow tools. A unit test asserts the returned list contains zero `workflow_propose_*` and zero mutating entries. |
