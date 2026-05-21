# Phase 1 — Workflows Foundation DEVLOG

## F-1 — `workflows/` Rust Domain Skeleton + Types + Migrations

**Status:** Complete · **Date:** 2026-05-20 · **Branch/commit:** direct-to-main on `alediez2048/openhuman`.

Landed the `src/openhuman/workflows/` scaffold with the full Phase 1 type
universe, `workflows.db` + three SQLite migrations, the 11 `Workflow*`
DomainEvent variants on the event bus, and empty stubs for every file
F-2..F-15 will fill in. 21 new tests (16 types round-trip + 5 store
migration) all green; `cargo check` clean on both manifests; zero new
clippy hits on the workflows module.

### Tactical deviations from the F-1 primer

- **Store pattern.** Ticket suggested a long-lived `WorkflowsStore`
  struct holding a `rusqlite::Connection`. Codebase precedent
  (`connections::store::with_connection`, `cron::store::with_connection`)
  uses an ephemeral closure pattern keyed off `&Config`. F-1 follows the
  established convention — each caller opens a connection, SQLite
  file-level locking handles concurrency, no `Arc<Mutex<_>>` needed in
  F-3's subscriber or F-8's executor.

- **`ProposalValidationError` field rename.** Spec showed
  `UnsupportedNodeKind { kind: NodeKind, phase }` with
  `#[serde(tag = "kind")]`. The field name collides with the Serde
  internal-tag name. Renamed the field to `node_kind` and switched the
  tag to `"type"` (matching `ConnectionRef`, `Trigger`, `WorkflowOrigin`,
  `WorkflowHealth`). F-11 consumers reference `.node_kind` instead of
  `.kind`.

- **DomainEvent payloads use opaque JSON.** Following the
  `ConnectionAdded { connection_ref_json: serde_json::Value }`
  precedent, the new `Workflow*` events carry `origin_json`,
  `health_json`, `status_json`, `reason_json`,
  `attempted_trigger_source_json`. Subscribers deserialise into the
  typed shape from `workflows::types`. Keeps the event bus free of
  cross-domain type imports.

### Verified

- `cargo check --manifest-path Cargo.toml` ✓
- `cargo check --manifest-path app/src-tauri/Cargo.toml` ✓
- `cargo fmt --check` ✓
- `cargo clippy --manifest-path Cargo.toml -p openhuman` ✓ (zero new
  hits on `src/openhuman/workflows/`)
- `pnpm test:rust workflows` — 21 passed, 0 failed.

### Files

- New: `src/openhuman/workflows/{mod,types,store,ops,scheduler,executor,proposer,validator,agent_tools,bus,rpc,schemas,health}.rs`
- New: `src/openhuman/workflows/migrations/{001_init_workflows,002_runs,003_run_steps}.sql`
- New: `src/openhuman/workflows/{types_tests,store_tests}.rs`
- New: `src/openhuman/workflows/templates/.gitkeep`
- Modified: `src/openhuman/mod.rs` (added `pub mod workflows;`)
- Modified: `src/core/all.rs` (wired `all_workflows_*_controllers` /
  `_schemas` — empty in F-1, populated by F-2 onwards)
- Modified: `src/core/event_bus/events.rs` (11 new `Workflow*` variants
  + `domain()` match extension)

### Next

F-2 — Workflows CRUD RPCs + `WorkflowOrigin` discriminator wiring. Hard
depends on F-1.

---

## F-2 — Workflows CRUD RPCs + `WorkflowOrigin` Discriminator

**Status:** Complete · **Date:** 2026-05-20 · **Branch/commit:** direct-to-main on `alediez2048/openhuman`.

Filled the F-1 stubs with the seven Phase 1 CRUD operations:
`workflows_list`, `workflows_get`, `workflows_create`,
`workflows_update`, `workflows_delete`, `workflows_enable`,
`workflows_disable`. Each mutating op publishes the matching
`DomainEvent::Workflow*` event. Origin discriminator (UserChat /
UserForm / Seed{template_id} / Imported) round-trips end-to-end, with
`Imported` rejected at create time (no importer ships in Phase 1).

15 new ops tests + 21 F-1 tests + RPC handler wiring = full
`workflows::*` suite at **36 tests, all green**.

### Tactical deviations from the F-2 primer

- **UUIDv4 instead of UUIDv7.** Ticket called for UUIDv7. The workspace
  `uuid` crate only enables the `v4` feature, and the established
  codebase convention (cron, agent sessions, etc.) uses
  `Uuid::new_v4()`. At Phase 1 scale (O(10s) of workflows per user) the
  v7 index-locality benefit doesn't matter. If Phase 2+ surfaces a real
  need, we can add the `v7` feature to `Cargo.toml` and migrate the
  generator without touching persisted ids.
- **Empty `nodes` rejected at create.** F-2 primer's edge-case list flagged
  this as "the validator (F-11) enforces the same for proposals." Shipped
  the runtime check in `ops::create` and `ops::update` as well — F-11's
  semantic validator runs further upstream, but a direct RPC client can
  still bypass it, and an empty-`nodes` workflow is meaningless. Both
  layers catch the bug.
- **Idempotent enable/disable.** Toggling to the already-current state
  is a no-op AND skips the event publish, so subscribers don't see
  redundant transitions. F-3's health recompute subscriber will rely on
  this when it reasons about which events actually changed state.

### Verified

- `cargo check --manifest-path Cargo.toml` ✓
- `cargo check --manifest-path app/src-tauri/Cargo.toml` ✓
- `cargo fmt --check` ✓
- `pnpm test:rust workflows` — 36 passed, 0 failed (21 F-1 + 15 F-2).

### Files

- New: `src/openhuman/workflows/ops_tests.rs` (15 tests covering
  create/get/list/update/enable/disable/delete + event-bus emissions +
  idempotent no-op paths)
- Modified: `src/openhuman/workflows/types.rs` (added
  `CreateWorkflowRequest`, `UpdateWorkflowRequest`, `WorkflowPatch`,
  `ListFilter`, `HealthFilter` with `#[serde(deny_unknown_fields)]`)
- Modified: `src/openhuman/workflows/store.rs` (added
  `insert_workflow`, `get_workflow`, `list_workflows`,
  `update_workflow`, `set_enabled`, `delete_workflow`,
  `list_seed_origins` + JSON-blob encoding helpers)
- Modified: `src/openhuman/workflows/ops.rs` (filled the 7 operations)
- Modified: `src/openhuman/workflows/rpc.rs` (7 thin handlers
  delegating to ops)
- Modified: `src/openhuman/workflows/schemas.rs` (registered the 7
  controllers with full schemas)
- Modified: `src/openhuman/workflows/health.rs` (added stub
  `recompute(&Workflow, &()) -> WorkflowHealth::Ready` — F-3 replaces
  the body and widens the snapshot type)
- Modified: `src/openhuman/workflows/mod.rs` (re-exports the new types,
  wires `ops_tests`)

### Next

F-3 — `WorkflowHealth` recomputation subscriber on `ConnectionAdded` /
`ConnectionRemoved` / `ConnectionUpdated`. Per the locked execution
contract, F-3 is on the TDD-first side.

---

## F-3 — `WorkflowHealth` recomputation subscriber

**Status:** Complete · **Date:** 2026-05-21 · **Branch/commit:** direct-to-main on `alediez2048/openhuman`.

Replaced the F-2 health stub with a real walker that honours the
Phase 0 honest-connection truth table. Workflows now flip
`Ready ↔ NeedsConnections` automatically when any connection mechanism
fires a `ConnectionAdded` / `ConnectionRemoved` / `ConnectionUpdated`
event. Bounded UPDATE per affected workflow per event (one LIKE pre-
filter on `nodes_json`, then a second-pass filter through
`referenced_connections` to drop false positives). Idempotent — same
value reads skip the UPDATE and the bus publish.

Per the locked contract, F-3 is TDD-first. **24 new tests** landed
before the implementation passed: 12 in `health_tests.rs` (every
truth-table branch + the < 50 ms NFR-2.1.5 budget) and 8 in
`bus_tests.rs` (no-op / transition / bounded / false-positive /
unparseable-payload / unknown-event paths). Plus 4 new
`store_tests.rs` cases for `list_workflows_referencing` +
`set_health`. Total workflows suite: **60/60 passing** (21 F-1 +
15 F-2 + 24 F-3).

### Tactical deviations from the F-3 primer

- **`ConnectionsSnapshot` newtype** instead of a bare
  `Vec<ConnectionView>`. Centralises the Phase 0 honest-connection
  truth table in one method (`is_connected`) so the workflows domain
  doesn't reimplement the `requireVerification` rules everywhere they
  matter. The newtype also lets the snapshot ship `empty()` cleanly
  for the aggregator-failed fallback in `ops::create` / `ops::update`
  and `bus::recompute_for_ref`.
- **Aggregator-failure fallback.** Every recompute call has to deal
  with `aggregator::list_all` possibly erroring (network blip during
  Composio fan-out, etc.). Falling back to `ConnectionsSnapshot::empty`
  means the workflow gets marked `NeedsConnections { missing: refs }`
  rather than crashing or holding stale state — and F-3's own
  subscriber will fix it on the next event, so the false-negative
  window is bounded. Logged at warn level for ops visibility.
- **`set_health` updates only `health` + `updated_at`.** The bus
  subscriber must not churn unrelated fields; a dedicated targeted
  UPDATE keeps the bounded-work contract tight.
- **Forward transition (NeedsConnections → Ready) not unit-tested.**
  The production `recompute_for_ref` calls `aggregator::list_all`,
  which runs through real per-mechanism collectors. Mocking it is
  out of scope for F-3; F-15's hero E2E walks the full forward path
  against a real connection in a live build. The reverse transition
  (Ready → NeedsConnections against an empty aggregator) IS unit-
  tested, plus we drive the subscriber's `handle()` directly with
  synthetic events.

### Verified

- `cargo check` ✓ (both manifests)
- `cargo fmt --check` ✓
- `pnpm test:rust workflows` — 60 passed, 0 failed.
- `recompute_is_fast_enough_for_phase_one_workflows` test asserts
  recompute runs in < 50 ms (NFR-2.1.5).

### Files

- New: `src/openhuman/workflows/health_tests.rs` (12 tests)
- New: `src/openhuman/workflows/bus_tests.rs` (8 tests)
- Modified: `src/openhuman/workflows/health.rs` — replaced the
  F-2 stub with the real walker + `ConnectionsSnapshot` newtype +
  helpers (`referenced_connections`, `missing_against`,
  `requires_verification`).
- Modified: `src/openhuman/workflows/bus.rs` — filled with
  `WorkflowHealthRecomputeSubscriber` + `recompute_for_ref` +
  `register_health_recompute_subscriber` boot helper.
- Modified: `src/openhuman/workflows/store.rs` — added
  `list_workflows_referencing` (LIKE pre-filter keyed on JSON
  fragments per `ConnectionRef` variant) + `set_health` +
  `escape_like` + `json_fragment_for` helpers. 4 new
  `store_tests.rs` cases.
- Modified: `src/openhuman/workflows/ops.rs` — `create` / `update`
  now build a real `ConnectionsSnapshot` from `aggregator::list_all`
  before calling `health::recompute`.
- Modified: `src/openhuman/workflows/mod.rs` — wired `bus_tests` +
  `health_tests`.
- Modified: `src/core/jsonrpc.rs` — registered
  `WorkflowHealthRecomputeSubscriber` alongside the other domain
  subscribers in the boot path.

### Next

F-4 — `/workflows` route + bottom-tab nav + `WorkflowsList` +
`WorkflowCard`. **First milestone live-test checkpoint** per the
locked execution contract: after F-4 we pause, run the app, and
verify the route renders + the empty-state CTA is wired before
moving on to F-5.

---

## F-4 — `/workflows` route + bottom-tab + list view + empty state

**Status:** Complete · **Date:** 2026-05-21 · **Branch/commit:** direct-to-main on `alediez2048/openhuman`.

First user-visible Phase 1 surface. New 8th bottom-tab between
**Connections** and **Intelligence** (ADR-001 / OQ-1=A); `/workflows`
route; `<WorkflowsList>` page renders rows from `workflows_list` with
`<WorkflowCard>` (activation-first layout — prominent
enable/disable toggle as the primary action per FR-1.2.3);
`<WorkflowHealthBadge>` covers all four states; empty state surfaces
the chat-driven creation hero CTA per FR-1.2.6. Adds
`workflowsSlice` + `workflowsApi` client + full i18n key set across
every locale.

### Tactical deviations from the F-4 primer

- **i18n keys live in `chunks/{xx}-5.ts`, not just `xx.ts`.** The
  primer said "add `nav.workflows` to `en.ts` + `// translate later`
  to every other locale." The repo's i18n coverage tests
  (`src/lib/i18n/__tests__/coverage.test.ts` +
  `I18nContext.test.tsx`) enforce key parity between
  `en.ts` ↔ `en-*.ts` chunks AND every non-English locale against
  English. Updating only the flat `xx.ts` files broke both tests.
  Added the workflow keys to `en-5.ts` AND to every `{xx}-5.ts`
  chunk (10 non-English locales) with the English value + a
  `// translate later` block-comment marker.

- **Page subdirectory mirrors `pages/Channels.tsx` vs
  `pages/Workflows/WorkflowsList.tsx`.** The primer suggested
  `pages/Workflows/WorkflowsList.tsx`; we kept the subdirectory so
  F-5 / F-6 / F-14 / F-15 have a natural place to land their pages
  (`StarterWorkflowsSection`, `WorkflowProposalPreview`, etc.) without
  collision with `pages/Channels.tsx`-style flat files.

- **Enable toggle "off-only block" on unhealthy workflows.** Primer
  said `aria-disabled` when `health !== Ready`. We refined: the
  off→on transition is blocked when health isn't Ready, but the
  on→off transition stays enabled. Otherwise a user couldn't
  disable a workflow whose health just degraded — they'd be stuck
  with it firing on cron. Captured in `WorkflowEnableToggle`'s
  `blocked = !enabled && !healthy` check + a Vitest case for the
  "disable an enabled-but-unhealthy workflow" path.

- **Overflow menu items are stubs in F-4.** Edit → F-14
  (proposal-preview), Run now → F-7 (`workflows_run_now`), Delete
  → F-12 (`workflow_propose_delete`). Clicks emit `console.debug`
  placeholders so the wiring is visible in devtools without
  pretending to do work.

- **`hideStarterSection` lives in `workflowsSlice` from day one**
  (persisted via `whitelist: ['hideStarterSection']` in
  `store/index.ts`). F-5 / F-6 read/write it; landing it now avoids
  a follow-up slice migration.

### Verified

- `pnpm typecheck` ✓
- `pnpm lint` ✓ (0 errors; 47 pre-existing warnings, none from
  F-4 code)
- `pnpm format:check` ✓
- `pnpm test` — 2 701 passed / 1 failed / 3 skipped. The single
  failure is `src/test/mockApiCore.portSelection.test.ts` — a
  pre-existing test-infrastructure flake where port 5005 doesn't
  release between vitest runs; unrelated to F-4. All 18 F-4 tests
  + 5 BottomTabBar tests + 44 i18n parity tests pass.

### Files

- New TS types: `app/src/types/workflows.ts` (mirrors
  `src/openhuman/workflows/types.rs`).
- New RPC client: `app/src/services/api/workflows.ts` (list / get /
  create / update / delete / enable / disable). Envelope-unwrap
  helper mirrors `connectionsApi.ts`.
- New Redux slice: `app/src/store/workflowsSlice.ts`. Thunks for
  fetch / enable / disable / delete; per-id `pending` map; selectors;
  `setHideStarterSection` action.
- New page: `app/src/pages/Workflows/WorkflowsList.tsx`.
- New components in `app/src/components/workflows/`:
  `WorkflowCard.tsx`, `WorkflowEnableToggle.tsx`,
  `WorkflowHealthBadge.tsx`, `WorkflowEmptyState.tsx`.
- Modified: `app/src/AppRoutes.tsx` (registered `/workflows`
  route).
- Modified: `app/src/components/BottomTabBar.tsx` (inserted
  workflows tab between connections and intelligence).
- Modified: `app/src/store/index.ts` (wired
  `workflowsSlice` + persist config for `hideStarterSection`).
- Modified: `app/src/lib/i18n/en.ts` + 11 chunks
  (`en-5.ts`, `ar-5.ts`, …, `zh-CN-5.ts`) with `nav.workflows`
  + 23 page-content keys.
- New tests: `WorkflowCard.test.tsx`, `WorkflowHealthBadge.test.tsx`,
  `WorkflowsList.test.tsx`, `workflowsSlice.test.ts` + new
  assertion in `BottomTabBar.test.tsx`.

### **First live-test milestone** (per locked execution contract)

After F-4 we pause for a checkpoint. The user should:
1. Restart `pnpm dev:app`.
2. Confirm the Workflows tab appears in the bottom bar between
   Connections and Intelligence.
3. Click it → `/workflows` renders the empty state.
4. Click the "Ask OpenHuman to build a workflow" CTA → navigates to
   `/chat`.
5. (Optional) Inject a workflow via the dev console (
   `await window.__OPENHUMAN_STORE__.dispatch(...)`) and verify the
   row + toggle render. F-15 will provide the proper hero E2E once
   the chat-driven creation path lands in F-14.

### Next

F-5 — Starter templates catalog. Bundles RU-1..RU-4 JSON +
`workflows_list_starter_templates` RPC. F-4's
`data-testid="starter-section-placeholder"` is the insertion point.

---

## F-5 — Starter templates catalog + RU-1..RU-4 + `workflows_list_starter_templates`

**Status:** Complete · **Date:** 2026-05-21 · **Branch/commit:** direct-to-main on `alediez2048/openhuman`.

Bundled four RU-* starter templates into the binary via `include_str!`
(ADR-004 / ADR-008) and exposed them through a read-only
`workflows_list_starter_templates` RPC that filters by `min_phase`,
dedupes against the user's existing `Seed { template_id }` workflows,
and computes `missing_connections` server-side from the live
aggregator snapshot.

### Templates shipped

| id | Trigger | Connections |
| --- | --- | --- |
| `ru-1-founder-morning-digest` | `0 8 * * 1-5` | Composio: gmail / linear / slack + Telegram channel |
| `ru-2-linkedin-engagement-queue` | `0 11 * * 1-5` | Webview: linkedin + Telegram channel |
| `ru-3-spotify-friday-five` | `0 17 * * 5` | Composio: spotify / discord |
| `ru-4-jira-sprint-retro` | `0 16 */14 * 5` | Composio: jira / notion |

### Tactical deviations from the F-5 primer

- **Templates parse as opaque JSON for `trigger` / `nodes` / `edges` /
  `settings`.** The artifact RU-1 file uses fields not in Phase 1's
  typed `Node` shape (per-node `name`, per-node `on_error`). Modelling
  `StarterTemplate.nodes` as `serde_json::Value` (instead of
  `Vec<Node>`) preserves those forward-compat fields losslessly on the
  `raw_payload` that F-6's [Add] button passes back to
  `workflows_create`. The strict typed fields the catalog DOES need
  (`template_id`, `name`, `description`, `required_connections`,
  `min_phase`) stay typed.
- **Empty `channel_id: ""` added to RU-1's Telegram entry.** The
  artifact spec at `Automations/Artifacts/templates/ru-1-...json` omits
  `channel_id` entirely, which the strict `ConnectionRef::Channel`
  parser rejects. Adding the empty string keeps the field present at
  parse time; the user picks a real channel on [Add] (per the F-5
  primer's design for RU-2..RU-4). Updated the bundled copy only —
  the design-time artifact stays untouched.
- **Cron normalization via `cron::normalize_expression`.** The `cron`
  crate uses Quartz-style 6-field expressions (with seconds). The
  templates use standard 5-field crontab. The shipping path routes
  both through `crate::openhuman::cron::normalize_expression`
  (production scheduler convention) which prepends `0` for the
  seconds field. Pinned the same normalizer in
  `templates_tests::every_template_has_a_parseable_cron_expression`
  so a future template can't ship a 5-field cron that breaks the
  validator.
- **Phase 1 degradation of RU-2** — the "true" RU-2 uses
  `await_human_approval` (Phase 3 node kind). The Phase 1 template
  queues drafts to Telegram for manual copy-paste; documented in
  the template's `rationale_at_seed` so the user understands the
  difference.
- **Hard-coded `CURRENT_PHASE = 1` in `ops.rs`.** F-15 will swap
  this for `about_app::current_phase()` when that surface lands;
  TODO comment in the file flags the follow-up.

### Verified

- `cargo check` ✓ (both manifests)
- `cargo fmt --check` ✓
- `pnpm test:rust workflows` — **72/72 passing** (21 F-1 + 15 F-2 +
  24 F-3 + 6 templates_tests + 6 ops_tests for the catalog).
- The bundled JSON survives `cargo build --bin openhuman-core`
  (verified via the templates_tests parse loop).

### Files

- New: `src/openhuman/workflows/templates/ru-{1,2,3,4}-*.json`
- New: `src/openhuman/workflows/templates/mod.rs` (`include_str!` +
  `all_bundled()` + `BUNDLED_JSON` + `raw_payload_for`).
- New: `src/openhuman/workflows/templates/README.md` (file-shape
  docs + new-template checklist).
- New: `src/openhuman/workflows/templates_tests.rs` (6 tests).
- Modified: `src/openhuman/workflows/types.rs` (new
  `StarterTemplate`, `StarterTemplateView`,
  `ListStarterTemplatesRequest`).
- Modified: `src/openhuman/workflows/ops.rs`
  (`list_starter_templates` + `build_view` +
  `summarize_trigger_value`).
- Modified: `src/openhuman/workflows/rpc.rs`
  (`workflows_list_starter_templates` handler).
- Modified: `src/openhuman/workflows/schemas.rs` (registered the
  new controller + schema).
- Modified: `src/openhuman/workflows/ops_tests.rs` (6 dedup +
  min_phase + missing_connections + raw_payload assertions).
- Modified: `src/openhuman/workflows/mod.rs`
  (`pub mod templates;` + test-module wiring + re-exports).

### Next

F-6 — `<StarterWorkflowsSection>` UI renders this catalog into F-4's
`data-testid="starter-section-placeholder"` slot, with `[Add]` /
`[Add & Enable]` buttons that call `workflows_create` with
`origin = Seed{template_id}` + the `raw_payload`. **Second locked
live-test milestone**.
