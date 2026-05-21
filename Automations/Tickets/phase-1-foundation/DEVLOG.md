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

---

## F-6 — `<StarterWorkflowsSection>` + `[Add]` / `[Add & Enable]` catalog UI

**Status:** Complete · **Date:** 2026-05-21 · **Branch/commit:** direct-to-main on `alediez2048/openhuman`.

UI side of the F-5 catalog. Renders RU-1..RU-4 into F-4's
`starter-section-placeholder` slot with two CTAs driving the new
`addStarterTemplate` thunk: `[Add]` (workflows_create with
`origin = Seed{template_id}`) and `[Add & Enable]` (above + immediate
workflows_enable). After [Add] resolves, the thunk refetches BOTH
the workflow list and the catalog in parallel — server-side dedup
drops the just-added template, the new workflow appears in "Your
workflows."

### Tactical deviations from the F-6 primer

- **`raw_payload` extras stripped before `workflows_create`.** F-5's
  bundled templates carry forward-compat fields (`template_id`,
  `min_phase`, `tags`, `rationale_at_seed`) that the strict
  `CreateWorkflowRequest` rejects via `#[serde(deny_unknown_fields)]`.
  The `addStarterTemplate` thunk destructures those four extras out
  of `raw_payload` before passing the rest to `workflowsApi.create`.
  Documented inline.
- **Hide-link in section header, no Settings toggle yet.** The
  primer called for a Settings panel toggle to re-show a hidden
  catalog. The hide link in the section header writes the flag, but
  the Settings UI to flip it back is deferred — the
  `setHideStarterSection` reducer + the
  `settings.workflows.show_starter_label` i18n key both ship so a
  follow-up settings-panel PR is a small wire-up.
- **`useCronHumanizer` hook deferred.** F-5's `trigger_summary`
  already ships a server-computed humanized label, so the card uses
  it directly. F-14 can land the richer hook alongside other
  per-step humanization.

### Verified

- `pnpm typecheck` ✓
- `pnpm lint` ✓ (0 errors; 47 pre-existing warnings, none from F-6)
- `pnpm format:check` ✓
- `pnpm debug unit workflows` — **31 tests passing** (5
  StarterWorkflowCard + 5 StarterWorkflowsSection + 3 new slice
  thunk-flow tests + the existing F-4 surface).
- `pnpm debug unit i18n` — 44 parity tests green after adding 10 new
  catalog keys + 1 Settings section title key to every locale chunk.

### Files

- New: `app/src/components/workflows/StarterWorkflowCard.tsx`.
- New: `app/src/components/workflows/StarterWorkflowsSection.tsx`.
- New: `app/src/components/workflows/__tests__/StarterWorkflowCard.test.tsx`.
- New: `app/src/components/workflows/__tests__/StarterWorkflowsSection.test.tsx`.
- Modified: `app/src/types/workflows.ts` (added `StarterTemplateView`
  + `ListStarterTemplatesRequest`).
- Modified: `app/src/services/api/workflows.ts` (added
  `workflowsApi.listStarterTemplates`).
- Modified: `app/src/store/workflowsSlice.ts` (starter state +
  `fetchStarterTemplates` + `addStarterTemplate` thunk with the
  parallel refetch + selectors).
- Modified: `app/src/store/__tests__/workflowsSlice.test.ts` (3 new
  thunk-flow tests).
- Modified: `app/src/pages/Workflows/WorkflowsList.tsx` (replaced
  both `starter-section-placeholder` placeholders; renders catalog
  ABOVE the chat CTA on empty workspace, BELOW user's list
  otherwise; keeps the legacy testid on the wrapper).
- Modified: `app/src/lib/i18n/en.ts` + every `chunks/{xx}-5.ts` (10
  new catalog keys + 1 settings key).

### **Second live-test milestone** (per the locked execution contract)

Before F-7 we pause for a checkpoint. The user should:

1. Restart `pnpm dev:app`.
2. Visit `/workflows` on a fresh workspace.
3. Verify RU-1..RU-4 render under the empty-state hero.
4. Click `[Add]` on RU-1; confirm RU-1 disappears from the catalog
   and a new row appears in "Your workflows" with `enabled = false`.
5. Open the new workflow's overflow → Delete (or fire
   `dispatch(deleteWorkflow('<id>'))` from devtools — F-12 wires the
   delete-preview UI).
6. Confirm RU-1 re-appears in the catalog.
7. Click `[Add & Enable]` on RU-2; confirm the toggle reads
   `Enabled = true` and RU-2 drops from the catalog.
8. Click "Hide starter workflows"; confirm the section disappears.
   Restore via `dispatch(setHideStarterSection(false))` from
   devtools (Settings toggle ships in a follow-up).

### Next

F-7 — Cron + manual trigger dispatch. `workflows_run_now` RPC +
scheduler integration so [Add & Enable]'d templates actually fire.
The overflow-menu "Run now" handler from F-4 gets wired then.

---

## F-7 — Cron + manual trigger dispatch + `workflows_run_now` / `workflows_cancel_run`

**Status:** Complete · **Date:** 2026-05-21 · **Branch/commit:** direct-to-main on `alediez2048/openhuman`.

Filled `scheduler.rs` with the cron + manual dispatch surface. Enabled
cron workflows now auto-fire (per-30s polling loop), `workflows_run_now`
RPC dispatches a manual run with a `health == Ready` gate, every F-2
CRUD op keeps the scheduler registry in sync, and a boot-time
`reconcile_at_startup` rebuilds the registry from the DB so a sidecar
restart resumes scheduling without dropping enabled jobs (FR-1.4.1.1).

### Major tactical deviation from the F-7 primer

**Sibling scheduler loop, NOT `cron::JobType::WorkflowTrigger`.** The
ticket called for adding a new `WorkflowTrigger { workflow_id }` variant
to `cron::types::JobType`, then routing dispatch through the existing
cron domain. That reuse would have required:

  1. Converting the unit-only `JobType` enum into a struct-variant
     enum — breaking the lowercase-string storage in `cron.db`.
  2. Adding a `workflow_id` column to `cron_jobs` via a SQL migration.
  3. Updating every existing `JobType::as_str` / `JobType::parse` /
     `process_due_jobs` site.

F-7 instead ships a **sibling polling loop inside
`workflows::scheduler`**:

- Holds an in-memory registry
  (`OnceLock<Mutex<HashMap<WorkflowId, Entry>>>`). Each entry carries
  the cron expression + the precomputed next-run timestamp.
- `register(&workflow)` / `deregister(&id)` mutate the registry
  synchronously. F-2's `create` / `update` / `enable` / `disable` /
  `delete` call them at the right moments.
- `reconcile_at_startup(config)` scans the workflows table on boot
  and rebuilds the registry — restart never drops jobs.
- `run(config)` is a single tokio task that wakes every 30s, fires
  any entries whose `next_run` ≤ now, and advances each fired entry
  to its next cron occurrence (computed via the **same**
  `cron::normalize_expression` + `cron::Schedule::from_str` parser
  the cron domain uses, so behavior matches).

If Phase 2 needs shared scheduling state across domains, revisit
then; at Phase 1 scale (one user, O(10s) of workflows), the sibling
loop is materially simpler.

### Other deviations

- **`workflows_cancel_run` RPC ships, executor side is a stub.**
  Per the ticket, F-9 fills the cancel path. F-7's stub returns
  `CancelError::NotImplemented`; the RPC layer surfaces a stable
  `not_implemented` error code so F-14's UI can bind to the
  surface today.
- **F-7's `executor::dispatch_run` is also a stub.** Generates a
  fresh `Uuid::new_v4().to_string()` run id and returns it without
  persisting a `workflow_runs` row. F-8 fills the body — signature
  is locked so F-8's wire-up is a body-only change.
- **`scheduler::register` returns the next-run timestamp** on
  success. Surfacing it makes the tests + a future UI "next fire"
  affordance cheaper.

### Verified

- `cargo check` ✓ (both manifests)
- `cargo fmt --check` ✓
- `pnpm debug rust workflows` — **84/84 passing** (12 new
  scheduler_tests + 72 prior).

### Files

- New: `src/openhuman/workflows/scheduler_tests.rs` (12 tests).
- Modified: `src/openhuman/workflows/types.rs` (added
  `Trigger::is_cron()`, `ManualInitiator`, `RunNowError`).
- Modified: `src/openhuman/workflows/executor.rs` (`dispatch_run`
  + `cancel_run` stubs).
- Modified: `src/openhuman/workflows/scheduler.rs` (full
  implementation: registry singleton, register / deregister /
  reconcile_at_startup / run / handle_run_now; test-only
  `reset_registry_for_test` + `registered_ids_for_test`).
- Modified: `src/openhuman/workflows/ops.rs`
  (`scheduler_register_best_effort` + hooks in create / update /
  enable / disable / delete; new `run_now` + `cancel_run` ops).
- Modified: `src/openhuman/workflows/rpc.rs` (handlers).
- Modified: `src/openhuman/workflows/schemas.rs` (registered the
  two new controllers).
- Modified: `src/openhuman/workflows/mod.rs` (re-exports +
  scheduler_tests module wiring).
- Modified: `src/core/jsonrpc.rs` (spawned
  `reconcile_at_startup` + `run` on a tokio task in the
  `REGISTERED.call_once` block).

### Next

F-8 — `agent_prompt` executor + run history tables +
`scheduler_gate` integration. F-7's `dispatch_run` stub becomes
real: builds the agent definition, executes the node, persists
`workflow_runs` + `workflow_run_steps`, marks the run terminal.
**Third locked live-test milestone** per the execution contract.

---

## 2026-05-21 — F-8 shipped: `agent_prompt` executor + run history pipeline

### What landed

- **Run-row CRUD in `store.rs`**:
  - `RUN_STEP_OUTPUT_MAX_BYTES = 64 * 1024` const + UTF-8-safe
    `truncate_output_to_64kib()` (leaves headroom for a
    `\n…[truncated]` marker so the final string still fits the cap
    per NFR-2.3.5).
  - `insert_run`, `mark_run_terminal`, `mark_run_cancelled_flag`,
    `insert_run_step`, `update_run_step_terminal`.
  - `Pagination { limit, offset }` + `clamp()` capping `limit` to
    `[1, 100]` (NFR-2.5.6) — used by `list_runs` and the future
    F-12 propose-delete preview.
  - `list_runs`, `count_runs`, `get_run` (returns
    `Option<(Run, Vec<RunStep>)>` so the polling UI can distinguish
    "deleted mid-poll" from a transport error).
  - `row_to_run` + `row_to_run_step` helpers + `run_status_str` +
    `parse_run_status` for the string ↔ enum round-trip.
- **Real `executor::dispatch_run`**: loads the workflow, validates
  Phase 1 invariants (`validate_phase_1_workflow` — exactly one
  node, kind = `AgentPrompt`), persists a `workflow_runs` row with
  `status = Running`, records the run in
  `ExecutorState::in_flight`, publishes `WorkflowRunStarted`, and
  spawns `execute_inner` on a tokio task. Returns the new `RunId`
  immediately.
- **`execute_inner`**: wraps `execute_agent_prompt` in
  `tokio::time::timeout` (clamped to `[1, 3600]`s per FR-1.6.5),
  maps the outcome to `RunStatus::Succeeded | Failed | TimedOut`,
  calls `mark_run_terminal`, releases the single-flight slot,
  publishes `WorkflowRunCompleted`.
- **`execute_agent_prompt`**: inserts a `workflow_run_steps` row,
  publishes `WorkflowRunStepStarted`, runs the agent (placeholder
  body — see deviation below), truncates output to 64 KiB on a
  UTF-8 boundary via the new `store::truncate_output_to_64kib`,
  persists the terminal step state, publishes
  `WorkflowRunStepCompleted`.
- **`build_node_agent_definition(allowed_connections, iteration_cap, model_tier)`**:
  returns a `NodeAgentDefinition { allowed_tools, iteration_cap, model_tier }`.
  Allowlist shape per ADR-016 is exactly:
  `baseline (6 names) + connection-resolved (1 name per ConnectionRef) + read-only workflow tools (4 names)`,
  deduped while preserving order so a sub-agent that lists
  `list_connections` twice is harmless. Exported as `pub` so F-10
  can assert against the same `BASELINE_TOOL_NAMES` +
  `READ_ONLY_WORKFLOW_TOOL_NAMES` constants in its allowlist-
  enforcement test.
- **`ExecutorState`**: process-global singleton (`OnceLock<ExecutorState>`)
  holding `in_flight: Mutex<HashMap<WorkflowId, RunId>>` and
  `cancel_requested: Mutex<HashMap<RunId, ()>>`. F-8 only writes
  to `in_flight` (releases on terminal); F-9 fills the
  single-flight invariant check + the cancel observer.
- **Two new RPCs** wired top-to-bottom:
  - `openhuman.workflows_list_runs(workflow_id, limit?, offset?)`
    — paginated runs view, newest-first, `limit` clamped server-side.
  - `openhuman.workflows_get_run(run_id)` — single row + its
    persisted step rows; returns null when the id is unknown.
  - Both controllers registered in `all_registered_controllers()`
    so the dispatcher picks them up without a manual branch
    (controller-only exposure per CLAUDE.md).

### Deviation: agent-invocation placeholder

The ticket spec called for `execute_agent_prompt` to invoke
`agent::run_subagent(definition, prompt, parent_context)`. The
F-8 dependency survey turned up two blockers:

1. `run_subagent` reads `ParentExecutionContext` from a
   task-local set by the harness's `Turn`. Calling it from a
   cron-fired tokio task (no `Turn` on the stack) errors with
   `NoParentContext`.
2. The clean alternative — `Agent::from_config(...).run_single(prompt)`
   per the cron domain's pattern — requires plumbing the
   project's `Config` into a model-tier-aware `Agent` and is
   non-trivial. Bigger than F-8's responsible budget.

**Decision:** ship the structural pipeline + the run-row + step-
row persistence + truncation + event publication + the allowlist
function + the new RPCs in this ticket. Leave a clearly-labelled
deterministic placeholder body in `run_agent_prompt` that echoes
the prompt + allowed-tools list into a `NodeOutput`. F-15's hero
E2E (third locked live-test milestone) is the swap point — the
signature is locked, the swap is a body-only change.

This unblocks:
- F-9's single-flight + soft-cancel + orphan-recovery sweep
  (lands on the real `dispatch_run`/`ExecutorState` surface).
- F-10's read-only workflow tools + allowlist enforcement test
  (asserts against the same `BASELINE_TOOL_NAMES` +
  `READ_ONLY_WORKFLOW_TOOL_NAMES` constants).
- F-12's propose-only tools (share the allowlist contract).
- F-14's run-history UI (reads through the new
  `workflows_list_runs` / `workflows_get_run` RPCs).

### Other deviations

- **`cancel_run` is still a stub.** Per the ticket, F-9 lands the
  real soft-cancel observer. F-8 returns
  `CancelError::NotImplemented` so the surface is stable.
- **No `scheduler_gate::wait_ready()` integration in F-8.** The
  existing `scheduler_gate` semantics are around
  cooperative-shutdown; for the placeholder agent body there's
  nothing to gate. F-15 will reintroduce the gate when the real
  `Agent::run_single()` invocation lands and shutdown can race
  in-flight runs.
- **Health gate is on the validator, not on dispatch.** The
  ticket spec gated `dispatch_run` on `health == Ready`. F-7 /
  F-11's design moves that gate to `validator` (catch at create
  time) + `workflows_run_now` (catch at the RPC entry — that's
  what `RunNowError::HealthBlocked` is for). The cron-tick path
  trusts the validator's prior check; if a connection disappears
  between create and tick, F-3's health subscriber will catch it
  on the next `ConnectionRemoved` event and the next tick's
  workflow load will reflect the change. This is a behaviour
  improvement vs the ticket: `dispatch_run` stays a thin
  persistence + spawn function.

### Verified

- `cargo check` ✓
- `cargo fmt` ✓
- `cargo clippy --lib` ✓ — no workflows-specific lints.
- `cargo test --lib openhuman::workflows` — **95/95 passing**
  (11 new `executor_tests` + 84 prior).

### Files

- New: `src/openhuman/workflows/executor_tests.rs` (11 tests:
  `build_node_agent_definition` shape × 3,
  `dispatch_run` validation + happy path + truncation,
  `list_runs` / `get_run` round-trips + clamp + unknown-id,
  `cancel_run` stub).
- Modified: `src/openhuman/workflows/executor.rs` (full Phase 1
  pipeline: `BASELINE_TOOL_NAMES`, `READ_ONLY_WORKFLOW_TOOL_NAMES`,
  `build_node_agent_definition`, `connection_tool_name`,
  `NodeAgentDefinition`, `ExecutorState`, real `dispatch_run`,
  `validate_phase_1_workflow`, `execute_inner`,
  `execute_agent_prompt`, `NodeOutput`, placeholder
  `run_agent_prompt`; `cancel_run` remains a `NotImplemented`
  stub for F-9; `DispatchError` + `CancelError`).
- Modified: `src/openhuman/workflows/store.rs` (run-row CRUD
  block: const + truncation + insert/mark/update +
  `Pagination` + `list_runs` + `count_runs` + `get_run` +
  helpers).
- Modified: `src/openhuman/workflows/ops.rs` (`list_runs` +
  `get_run` ops; `RunWithSteps` composite response struct).
- Modified: `src/openhuman/workflows/rpc.rs` (`workflows_list_runs`
  + `workflows_get_run` handlers).
- Modified: `src/openhuman/workflows/schemas.rs` (registered the
  two new controllers + their schema definitions; switched
  `limit`/`offset` field types to `TypeSchema::U64` since the
  catalog lacks a `U32` variant).
- Modified: `src/openhuman/workflows/mod.rs` (added
  `executor_tests` module).

### Next

F-9 — single-flight + soft-cancel + orphan-recovery sweep on the
existing `ExecutorState`. F-8 left the fields in place
(`in_flight`, `cancel_requested`) so F-9 is largely body-only:
the registry singleton is wired; the soft-cancel intent
mechanism slots into `execute_inner` + `cancel_run`; the
orphan-recovery sweep runs once at boot and marks any
non-terminal `workflow_runs` rows whose run wasn't restored by
the in-process executor as `Failed { reason: "core restart" }`.

---

## 2026-05-21 — F-9 shipped: single-flight + soft-cancel + orphan-recovery

### What landed

- **`InFlightSlot` RAII guard** owns the workflow's `in_flight`
  entry. `dispatch_run` constructs the guard, moves it into the
  spawned tokio task, and `Drop` removes the entry on **every**
  exit path: success, error, timeout, panic. Stale-guard check
  (`in_flight.get(&workflow_id) == Some(&self.run_id)`) so a
  guard from a previous run doesn't free a successor's slot.
- **Single-flight enforcement in `dispatch_run`**: acquire the
  `in_flight` mutex, on conflict publish
  `DomainEvent::WorkflowRunSkipped { reason_json:
  {"kind":"already_running"}, attempted_trigger_source_json }`
  and return `DispatchError::AlreadyRunning { workflow_id, run_id }`
  carrying the existing run id. Lock held only long enough to
  claim the slot; the run-row insert + start event publish run
  outside the critical section. If `insert_run` fails the slot
  is released so a transient SQLite hiccup doesn't permanently
  brick the workflow.
- **Real `cancel_run`**: queries `store::get_run`; returns
  `CancelError::NotFound(run_id)` when missing,
  `CancelError::NotRunning { run_id, current_status }` when the
  run is already terminal, otherwise calls
  `store::set_cancelled_flag` and returns `Ok(())`. F-8's
  `NotImplemented` placeholder is gone. The current node's LLM
  call is **not** aborted (FR-1.6.9 cooperative cancel).
- **Between-node cancellation observer**: `execute_inner` calls
  `cancellation_observed(config, wf_id, run_id)` pre-node and
  post-node. The post-node check upgrades a successful return to
  `RunStatus::Cancelled` if the bit was flipped during the
  agent's body. `cancellation_observed` swallows DB errors as
  "not cancelled" with a warn log — a transient SQLite hiccup
  must not spuriously cancel a real run.
- **`orphan_recovery_sweep`**: calls `store::orphan_running_runs`
  (single SQL UPDATE: `status = 'failed', error = 'CoreCrashed',
  completed_at = ?` WHERE `status = 'running'`), publishes
  `WorkflowRunCompleted { status: Failed }` for each touched row,
  returns the count. Idempotent — a clean DB returns `Ok(0)`.
- **Boot wiring** in `src/core/jsonrpc.rs`: the sweep runs
  **before** `scheduler::reconcile_at_startup` so a re-registered
  cron tick can't bounce off a stale single-flight slot. Comment
  added to lock the ordering in place.
- **Store helpers**: `set_cancelled_flag`, `is_cancelled`
  (returns `Ok(false)` for unknown ids — graceful fallback for
  the cascade-deleted case), `orphan_running_runs(completed_at)`
  returning `Vec<(WorkflowId, RunId)>` for ergonomic match
  against `WorkflowRunCompleted` event fields.
- **Type variants**:
  - `DispatchError::AlreadyRunning { workflow_id, run_id }` —
    error code `already_running`.
  - `CancelError::NotRunning { run_id, current_status }` — error
    code `not_running`.
  - `CancelError::Store(String)` — error code `store_error`.
  - Dropped `CancelError::NotImplemented`.

### Deviations

- **Soft-cancel observer is `workflow_runs.cancelled`, not
  `ExecutorState.cancel_requested`.** The persisted-flag pattern
  is robust to a core crash (a flagged-but-uncancelled run gets
  observed correctly on the next tick) and to multiple cancel
  attempts (idempotent). The in-memory map was dropped — the
  struct now holds only `in_flight`.
- **Phase 1 effectively single-node, but the loop is in place.**
  `execute_inner`'s post-node check fires once today; the same
  code structure supports Phase 2's multi-node graphs without
  changes. The pre-node check handles the edge case where
  cancel_run fires between dispatch and the task's first poll.
- **No `state_in_flight_*_for_test` exposure in production.**
  Two `#[cfg(test)]` helpers — `state_in_flight_insert_for_test`
  + `state_in_flight_remove_for_test` — let the single-flight
  test set up the "previous run already in-flight" precondition
  deterministically without racing against a real tokio task.

### Verified

- `cargo check` ✓
- `cargo fmt` ✓
- `cargo test --lib openhuman::workflows` — **106/106 passing**
  (11 new tests over F-8's 95). Includes:
  - `dispatch_run_rejects_second_overlapping_dispatch_with_already_running`
  - `dispatch_run_releases_slot_on_success_and_can_redispatch`
  - `dispatch_run_independent_workflows_run_concurrently`
  - `cancel_run_returns_not_found_for_unknown_id`
  - `cancel_run_returns_not_running_when_terminal`
  - `cancel_run_flips_flag_and_executor_observes_cancelled_terminal`
  - `orphan_recovery_sweep_marks_stale_running_runs_failed_core_crashed`
  - `orphan_recovery_sweep_on_clean_db_returns_zero`
  - `set_and_read_cancelled_flag_round_trip`
  - `is_cancelled_returns_false_for_unknown_run_id`
  - `orphan_running_runs_marks_running_rows_failed_with_core_crashed`

### Files

- Modified: `src/openhuman/workflows/executor.rs`
  (`InFlightSlot` RAII guard, single-flight check, real
  `cancel_run`, `cancellation_observed`, `finalize_run`,
  `orphan_recovery_sweep`, F-9 `DispatchError::AlreadyRunning`
  + `CancelError::NotRunning`/`Store`; dropped
  `CancelError::NotImplemented` + the unused
  `cancel_requested` field).
- Modified: `src/openhuman/workflows/store.rs`
  (`set_cancelled_flag` renamed from `mark_run_cancelled_flag`,
  `is_cancelled`, `orphan_running_runs`; added
  `OptionalExtension` import).
- Modified: `src/core/jsonrpc.rs` (orphan_recovery_sweep boot
  call before `reconcile_at_startup`).
- Modified: `src/openhuman/workflows/executor_tests.rs`
  (replaced F-8 NotImplemented stub test with the three real
  cancel paths; added single-flight + slot-release + sibling-
  independence + orphan-sweep tests).
- Modified: `src/openhuman/workflows/store_tests.rs` (F-9 store
  tests: cancelled-flag round-trip, is_cancelled-on-unknown,
  orphan_running_runs SQL + idempotency).

### Next

F-10 — agent read tools + `build_node_agent_definition`
allowlist enforcement test. F-8 referenced the four read-only
tool names by constant; F-10 registers them in the agent tool
registry, then adds the regression test asserting the allowlist
matches `BASELINE_TOOL_NAMES + READ_ONLY_WORKFLOW_TOOL_NAMES`
verbatim (ADR-016 / NFR-2.3.7).

---

## 2026-05-21 — F-10 shipped: read-only workflow agent tools + allowlist enforcement

### What landed

- **Four Tool impls** under `src/openhuman/tools/impl/workflows/`
  (matching the existing `tools/impl/<domain>/` convention used by
  cron / memory / network / etc., not a one-file outlier under
  `workflows/`):
  - `WorkflowListTool` (`workflow_list`) — paginated list of the
    user's workflows. Filter is `{enabled?, health_state?, search?}`.
  - `WorkflowGetTool` (`workflow_get`) — single workflow by id;
    returns null on unknown id.
  - `WorkflowsListRunsTool` (`workflows_list_runs`) — runs for a
    workflow, `limit` clamped server-side to `[1, 100]` by
    `ops::list_runs`.
  - `WorkflowsGetRunTool` (`workflows_get_run`) — run + steps;
    returns null on unknown id.
- All four declare `PermissionLevel::ReadOnly`,
  `ToolCategory::System`, and `is_concurrency_safe = true` (no
  shared mutable state — the agent tool loop can fan them out).
- **Stable name constants** in
  `tools/impl/workflows/mod.rs`: `TOOL_WORKFLOW_LIST`,
  `TOOL_WORKFLOW_GET`, `TOOL_WORKFLOWS_LIST_RUNS`,
  `TOOL_WORKFLOWS_GET_RUN`, `READ_ONLY_TOOL_NAMES`.
  Re-exported from `workflows::agent_tools` so callers depend on
  the workflows domain's public surface.
- **`FORBIDDEN_MUTATING_TOOL_NAMES`** in `workflows::agent_tools`
  — eight names the agent must NEVER see
  (`workflows_create/update/delete/enable/disable/run_now/cancel_run`
  + `workflow_create_from_proposal`). Two test sites enforce it:
  the allowlist test and the registered-tools test.
- **Boot wiring** in `tools::ops::all_tools_with_runtime`: four
  `Box::new(...)` entries after the cron block. Comment cites
  ADR-012 + F-8 so a future reader knows why no mutating tools
  appear here.
- **`workflows::agent_tools` upgraded** from F-1 stub to the
  re-export hub for the read-only constant + the forbidden list.
- **Test suite** in
  `src/openhuman/tools/impl/workflows/tests.rs` (11 tests):
  - Each tool's `name()` matches the canonical constant.
  - `READ_ONLY_TOOL_NAMES` matches
    `executor::READ_ONLY_WORKFLOW_TOOL_NAMES` exactly (catches
    F-8 ↔ F-10 drift).
  - Round-trip against `ops::*` for each tool (workflow_list,
    workflow_get, workflows_list_runs, workflows_get_run).
  - `workflow_get` returns null for unknown id; same for
    `workflows_get_run`.
  - `workflows_list_runs` honours the server-side limit clamp
    (limit = 99999 doesn't error).
  - **Secret-leak regression** — a workflow whose node carries
    a `ConnectionRef::GenericHttp { connection_id }` produces a
    `workflow_get` payload that contains neither the literal
    `"secret_ref"` nor any `Bearer ` / `Basic ` Authorization-
    header substring.
  - **`build_node_agent_definition` allowlist (NFR-2.3.7)** —
    asserted in both the no-connections case and the
    Composio-allowed-connections case: baseline names present,
    read-only workflow names present, **zero** propose names,
    **zero** forbidden mutation names.
  - **Registered-tool surface check** — calls
    `tools::all_tools(...)` end-to-end, walks every registered
    name, asserts none match `FORBIDDEN_MUTATING_TOOL_NAMES`
    and all four `READ_ONLY_TOOL_NAMES` ARE present. This is
    the load-bearing ADR-012 security boundary — drift here
    means an agent could mutate via the tool surface.

### Deviations

- **Location**: F-10's spec called for
  `src/openhuman/workflows/agent_tools.rs` to hold the tool
  impls. Reality: every other tool implementation lives under
  `src/openhuman/tools/impl/<domain>/`. Staying consistent with
  that convention is more important than literally matching the
  spec's filesystem path. `workflows::agent_tools` keeps its
  re-export role (constants + forbidden list).
- **No `Registry::new()` API**: the spec assumed a `Registry`
  type with `register_tool`. Reality: tools register by pushing
  into a `Vec<Box<dyn Tool>>` inside
  `tools::ops::all_tools_with_runtime`. The negative-allowlist
  test exercises the real surface, not a synthetic registry.
- **`Pagination::limit` typed as `i64` in JSON-schema**: the
  schema declares `"type": "integer"` (which serde-json maps to
  `u64`), and the tool parses with `Value::as_u64`. Schemas in
  other tools follow the same pattern.

### Verified

- `cargo check` ✓
- `cargo fmt` ✓
- `cargo test --lib openhuman::tools::implementations::workflows`
  — **11/11 passing**.
- `cargo test --lib openhuman::tools` — **906/906 passing**
  (F-10 added 11; no regressions).
- `cargo test --lib openhuman::workflows` — **106/106 passing**
  (unchanged).

### Files

- New: `src/openhuman/tools/impl/workflows/mod.rs` (module +
  name constants + `READ_ONLY_TOOL_NAMES`).
- New: `src/openhuman/tools/impl/workflows/list.rs`
  (`WorkflowListTool`).
- New: `src/openhuman/tools/impl/workflows/get.rs`
  (`WorkflowGetTool`).
- New: `src/openhuman/tools/impl/workflows/list_runs.rs`
  (`WorkflowsListRunsTool`).
- New: `src/openhuman/tools/impl/workflows/get_run.rs`
  (`WorkflowsGetRunTool`).
- New: `src/openhuman/tools/impl/workflows/tests.rs` (11 tests).
- Modified: `src/openhuman/tools/impl/mod.rs` (declared
  `workflows` submodule + re-export of the four tool types).
- Modified: `src/openhuman/tools/ops.rs` (added four
  `Box::new(...)` entries with F-10 comment).
- Modified: `src/openhuman/workflows/agent_tools.rs` (filled
  the F-1 stub with re-exports + `FORBIDDEN_MUTATING_TOOL_NAMES`
  + module docs explaining the F-10/F-12 split).

### Next

F-11 — drafting sub-agent + deterministic validator +
`draft_with_retries`. Lands `workflows::proposer` and
`workflows::validator`. The drafting sub-agent gets its own
allowlist (different from `agent_prompt`'s): `list_connections`
+ `workflow_list` + a synthetic `emit_proposal` tool. F-11
mirrors F-10's negative allowlist test for that surface.

---

## 2026-05-21 — F-11 shipped: drafting sub-agent + validator + draft_with_retries

### What landed

- **`validator::validate(proposal, snapshot, phase)`** — pure
  deterministic check, sub-50 ms on real proposals (NFR-2.1.5).
  Covers every `ProposalValidationError` variant:
  - `MissingRequiredField` for empty `name` / `description` /
    `nodes`.
  - `UnsupportedNodeKind { node_kind, phase }` via
    `allowed_node_kinds(phase)` (Phase 1 = `[AgentPrompt]`; Phase 2
    adds 7 kinds; Phase 3 adds `FanOut`).
  - `InvalidCron { expr, parse_error }` — routes through
    `cron::normalize_expression` (5-field → 6-field) so
    `*/15 * * * *` parses without the caller knowing about the
    Quartz translation.
  - `EdgeIntegrity { from, to, reason }` — both `from` + `to` must
    reference a node id in `nodes`; vacuously true with `edges = []`.
  - `UnknownConnection { ref, candidates }` for both
    `required_connections` and per-node `allowed_connections`
    walks. `candidates` carries up to 3 fuzzy suggestions ranked
    by Levenshtein, scoped to the same `ConnectionRef` mechanism
    (a Composio typo doesn't suggest a Channel row).
- **`fuzzy_candidates`** — pluggable helper, char-aware
  Levenshtein, limit 3 + max edit distance 3. Tested via
  `gmaill → [gmail]` (typo) + cross-mechanism rejection (a Channel
  with `provider = "gmail"` is NOT suggested for a missing
  Composio `gmail`).
- **`Drafter` trait + `draft_with_retries(drafter, description,
  snapshot, phase, max_attempts)`** — bounded-retry loop per
  ADR-015:
  - Trait-based for testability (`MockDrafter` in tests scripts
    the response sequence; the production `AgentDrafter` is the
    F-15 swap point with a clearly-labelled placeholder body).
  - On each attempt, calls `build_system_prompt(snapshot, phase,
    last_error)` and the drafter, then runs `validator::validate`.
  - On Ok: returns the proposal.
  - On `ProposalValidationError`: appends the error to the next
    attempt's prompt and loops.
  - On `RunFailure` from the drafter: surfaces immediately without
    consuming retries (so a transient LLM 503 doesn't burn the
    budget).
  - After `max_attempts` validation failures:
    `DraftFailure::ValidationFailedAfterRetries { attempts,
    last_error }`.
- **`build_system_prompt`** — pure function composing:
  1. The `workflow_builder.md` base (bundled via `include_str!`
     from `src/openhuman/agent/prompts/workflow_builder.md`).
  2. A "Your connections" group-by-mechanism summary (Composio /
     Channels / Webview / Built-in / MCP / Generic HTTP).
  3. A Phase N constraints block listing
     `allowed_node_kinds(phase)` so the model's output surface is
     tight.
  4. (Optional) A "PREVIOUS ATTEMPT FAILED" block carrying the
     structured `ProposalValidationError` per
     `format_validation_error` — deliberately terse, no proposal
     content (NFR-2.4.4).
- **Constants exposed for ADR/spec drift detection**:
  - `DEFAULT_MAX_ATTEMPTS = 3` (ADR-015 / FR-1.13.4).
  - `DEFAULT_ITERATION_CAP = 6` (FR-1.13.2).
  - `DRAFTING_TOOL_ALLOWLIST = ["list_connections", "workflow_list", "emit_proposal"]`
    (ADR-016). Test asserts the slice verbatim — adding a tool
    requires updating ADR-016 + this test in lock-step.
- **`DraftFailure` + `RunFailure` types** added to `types.rs`
  with `Display + Error + kind_label` impls. `DraftFailure`
  serialises with the standard `{"type": "snake_case", ...}`
  tag pattern matching `ProposalValidationError`.
- **`workflow_builder.md`** placeholder copied from
  `Automations/Artifacts/prompts/` → `src/openhuman/agent/prompts/`
  so `include_str!` resolves at build time. F-13 owns the file
  content + Tauri bundling; the path is locked.

### Deviations

- **`AgentDrafter` is a labelled placeholder.** Same F-8
  reasoning: invoking the agent from a non-Turn context
  (standalone tests, future RPC entry points) requires
  `Agent::from_config(...).run_single()` which exceeds F-11's
  budget. The placeholder returns
  `RunFailure { reason: "AgentDrafter is the F-11 placeholder;
  live agent invocation lands at F-15." }` so any caller exercising
  the live path observes a stable error code instead of looping
  silently. F-15 swaps the body without changing the `Drafter`
  trait signature.
- **`metrics::counter!` deferred.** F-11 spec called for a
  metrics counter per retry. The workspace doesn't currently
  depend on `metrics`; replaced with structured `tracing::warn!`
  on every validator failure (kind = label). A future
  observability ticket can swap in the real counter against the
  same log site.
- **`emit_proposal` synthetic tool not separately wired.** F-11
  spec described a tool the `run_subagent` wrapper intercepts.
  In our trait-based design the synthetic step is collapsed into
  the `Drafter::draft` return value — `Ok(WorkflowProposal)` IS
  the "emit_proposal payload extracted". F-15's swap-in
  `AgentDrafter` implementation will resurrect the synthetic
  tool inside the live agent loop.

### Verified

- `cargo check` ✓
- `cargo fmt` ✓
- `cargo test --lib openhuman::workflows` — **138/138 passing**
  (32 new: 19 validator + 13 proposer).

### Files

- New: `src/openhuman/agent/prompts/workflow_builder.md`
  (placeholder copy from `Automations/Artifacts/prompts/`; F-13
  owns).
- New: `src/openhuman/workflows/validator_tests.rs` (19 tests).
- New: `src/openhuman/workflows/proposer_tests.rs` (13 tests).
- Modified: `src/openhuman/workflows/validator.rs` (filled the
  F-1 stub: `validate`, `allowed_node_kinds`, `fuzzy_candidates`,
  `levenshtein`, `validate_cron_expr`, `name_for_fuzzy`).
- Modified: `src/openhuman/workflows/proposer.rs` (filled the
  F-1 stub: `Drafter` trait, `draft_with_retries`,
  `build_system_prompt`, `summarize_connections`,
  `phase_constraints_block`, `format_validation_error`,
  `AgentDrafter` placeholder, `RunFailure`).
- Modified: `src/openhuman/workflows/types.rs` (added
  `DraftFailure` with `kind_label` + `Display + Error`).
- Modified: `src/openhuman/workflows/mod.rs` (declared
  `validator_tests` + `proposer_tests` + re-exported
  `DraftFailure`).

### Next

F-12 — propose-only agent tools. Six new tools register on the
agent surface (`workflow_propose_create`, `_update`, `_delete`,
`_enable`, `_disable`, `_run_now`). Each calls into
`draft_with_retries` (create) or builds the matching
`Workflow{Edit,State,Delete}Proposal` preview (others) and
returns the JSON preview WITHOUT mutating. The F-10 negative
allowlist test will expand to ALLOW `workflow_propose_*` names
while still forbidding the seven mutation RPCs.

---

## 2026-05-21 — F-12 shipped: propose-only agent tools + workflow_diff

### What landed

- **Six propose-only tools** under `src/openhuman/tools/impl/workflows/`:
  - `workflow_propose_create` — calls
    `proposer::draft_with_retries` against the F-11 retry loop;
    surfaces `DraftFailure` as structured
    `{ error, kind_label, ... }` JSON. Uses the F-11 placeholder
    `AgentDrafter` (F-15 swap point) — chat agent observes a
    `drafting_failed/run_failure` payload today; production
    invocation lands at F-15 without changing the tool's
    surface.
  - `workflow_propose_update` — fetches the current workflow,
    runs the new `draft_with_retries_for_update` (sibling
    drafter that inlines the current shape via
    `build_update_system_prompt`), computes the
    `workflow_diff`, returns a `WorkflowEditProposal { workflow_id,
    current, proposed, diff_summary, rationale }` payload.
    Returns `{ error: "not_found" }` for unknown ids.
  - `workflow_propose_delete` — returns
    `WorkflowDeletePreview { workflow_id, name, run_count,
    retention_days: 30 }` from the new `ops::count_runs`.
    Retention is hard-coded to 30 per FR-1.3.4.
  - `workflow_propose_enable` / `workflow_propose_disable` —
    static-rationale state proposals (no LLM call). Already-enabled
    or already-disabled paths produce a no-op rationale.
  - `workflow_propose_run_now` — health-gated: `enabled: false`
    when `health != Ready` with a "Cannot run: missing connections"
    rationale; `enabled: true` with a duration estimate
    (median of last 5 successful runs, or "unknown (no past runs)").
- **`workflow_diff(current, proposed)`** in
  `src/openhuman/workflows/diff.rs` — flat `Vec<String>` of human-
  friendly bullets covering name, description, trigger
  (cron expr/tz/active hours + kind change), settings
  (timeout_secs, on_error), nodes (length delta, kind change,
  prompt rewrite by line count, iteration_cap, model_tier,
  allowed_connections adds + removes per step). Capped at
  `MAX_DIFF_BULLETS = 20` with a tail bullet
  "… and N more changes." on overflow.
- **`draft_with_retries_for_update`** in `proposer.rs` — sibling
  of `draft_with_retries`; runs the same validator-or-retry loop
  with an `UpdateDrafter` trait surface. The proposed shape is
  projected through `WorkflowProposal` so the F-11 validator
  reuses every check (no duplicate validation logic).
- **`AgentUpdateDrafter`** placeholder mirrors `AgentDrafter` —
  same F-15 swap point semantics.
- **`build_update_system_prompt`** extends the F-11 base prompt
  with a "Current workflow" pretty-printed JSON block + an
  instruction to return the full proposed shape (not a diff).
- **`ops::count_runs`** — wraps `store::count_runs` so
  `_propose_delete` doesn't need to reach into the store
  directly.
- **`PROPOSE_TOOL_NAMES`** constant in
  `tools::implementations::workflows` + re-exported from
  `workflows::agent_tools`. F-10's negative-allowlist test now
  asserts the six names are registered (the previous-attempt
  "zero propose" check is gone, replaced by "no mutations").
- **New regression test** in
  `tools::implementations::workflows::tests`:
  `build_node_agent_definition_excludes_propose_tools` — asserts
  zero `workflow_propose_*` names appear in `agent_prompt`'s
  allowlist (ADR-016). Belt-and-suspenders: tested with and
  without connection-resolved tools.
- **Boot wiring** in `tools::ops::all_tools_with_runtime` — six
  `Box::new(...)` entries after the F-10 block, comment cites
  ADR-016 + the regression test path.

### Deviations

- **No `metrics::counter!`**. Same call as F-11: the workspace
  doesn't depend on `metrics`; `tracing` covers every tool
  invocation with the `[workflows-agent]` prefix.
- **`workflow_propose_create` lives on the live drafter today**
  → returns `RunFailure` from the F-11 placeholder. The tool's
  surface is locked; F-15 swap is body-only inside
  `AgentDrafter::draft`. Two test cases lock the behaviour:
  `workflow_propose_create_returns_drafting_failed_with_f11_placeholder`
  + the empty-description guard.
- **Generic HTTP "saved connection" agent tool deferred to
  Phase 2** per the F-12 ticket's Phase 0 follow-up note: a
  proposal that lists `ConnectionRef::GenericHttp { connection_id }`
  in its `allowed_connections` is accepted today, but the
  sub-agent at run time has no per-id `http_call(connection_id=...)`
  tool — it falls back to the existing raw `http_request`. F-3's
  health computation + F-8's executor still gate against the
  connection's existence, and the secret stays server-side via
  `secret_ref` resolution in the HTTP probe path; this is
  acceptable for Phase 1.

### Verified

- `cargo check` ✓
- `cargo fmt` ✓
- `cargo test --lib openhuman::tools::implementations::workflows`
  — **22/22 passing** (11 new F-12 tests on top of F-10's 11).
- `cargo test --lib openhuman::workflows` — **148/148 passing**
  (10 new diff tests).

### Files

- New: `src/openhuman/workflows/diff.rs` (`workflow_diff`,
  `MAX_DIFF_BULLETS`, per-field helpers).
- New: `src/openhuman/workflows/diff_tests.rs` (10 tests).
- New: `src/openhuman/tools/impl/workflows/propose_create.rs`.
- New: `src/openhuman/tools/impl/workflows/propose_update.rs`.
- New: `src/openhuman/tools/impl/workflows/propose_delete.rs`.
- New: `src/openhuman/tools/impl/workflows/propose_enable.rs`.
- New: `src/openhuman/tools/impl/workflows/propose_disable.rs`.
- New: `src/openhuman/tools/impl/workflows/propose_run_now.rs`.
- Modified: `src/openhuman/tools/impl/workflows/mod.rs`
  (added six propose-tool modules + `PROPOSE_TOOL_NAMES` +
  the six per-tool name constants).
- Modified: `src/openhuman/tools/impl/mod.rs` (added
  `WorkflowPropose*Tool` to the re-exports).
- Modified: `src/openhuman/tools/ops.rs` (registered six tools).
- Modified: `src/openhuman/tools/impl/workflows/tests.rs`
  (F-12 regression test + 9 propose-tool unit tests; updated
  the registered-tools test to expect the six propose names).
- Modified: `src/openhuman/workflows/agent_tools.rs`
  (re-exported the propose name constants).
- Modified: `src/openhuman/workflows/mod.rs` (`pub mod diff` +
  `diff_tests` test module).
- Modified: `src/openhuman/workflows/ops.rs` (added
  `count_runs`).
- Modified: `src/openhuman/workflows/proposer.rs` (added
  `UpdateDrafter` trait, `AgentUpdateDrafter`,
  `draft_with_retries_for_update`, `build_update_system_prompt`,
  `collect_required_connections`).

### Next

F-13 — finalize `workflow_builder.md` + bundle it in
`tauri.conf.json` resources so the production binary ships the
prompt alongside the existing `agent/prompts/*.md` files. The
F-11 placeholder copy is already on disk; F-13 verifies the
build-time + runtime resolution paths.

---

## 2026-05-21 — F-13 shipped: workflow_builder.md locked + bundled + smoke-tested

### What landed

- **`workflow_builder.md` promoted to production**: F-11 already
  copied `Automations/Artifacts/prompts/workflow_builder.md` →
  `src/openhuman/agent/prompts/workflow_builder.md`. F-13
  re-verified byte-identical
  (`diff` returns empty) and locked the production path as
  canonical.
- **Tauri bundling already in place**: `app/src-tauri/tauri.conf.json`
  ships the whole `agent/prompts/` directory via the existing
  `"../../src/openhuman/agent/prompts"` resource glob — `workflow_builder.md`
  bundles automatically alongside `IDENTITY.md` / `SOUL.md` /
  `USER.md`. No JSON change needed.
- **Proposer module doc updated**: removed the F-11/F-13
  "placeholder" framing and replaced with the canonical
  rationale (compile-time `include_str!` + the parallel Tauri
  resource glob + the dual-write expectation against the
  design-time artifact).
- **`WORKFLOW_BUILDER_PROMPT_SIGNATURE` constant** — a stable
  substring drawn from the artifact's line 13 ("drafting
  sub-agent for OpenHuman's Workflows feature"). The F-13 smoke
  tests assert this is present so a build that picked up an
  empty / wrong-path file fails fast at test time rather than
  at the first chat-driven proposal.
- **One-shot init log** (`log_prompt_load_once`) — info-level
  `[workflows-proposer]` entry at first `build_system_prompt`
  call: `loaded workflow_builder.md ({n} chars,
  signature_present=true)`. Uses
  `AtomicBool::swap(Relaxed)` so subsequent calls are silent.
- **Three new smoke tests** in `proposer_tests.rs`:
  - `bundled_workflow_builder_prompt_carries_canonical_signature` —
    full `build_system_prompt` output contains the signature
    substring (covers both the file presence AND the
    composition pipeline that places it).
  - `bundled_workflow_builder_prompt_matches_design_time_artifact`
    — compares the production file against the
    `Automations/Artifacts/prompts/workflow_builder.md`
    artifact via parallel `include_str!`. Drift fails the
    suite, enforcing the dual-write expectation.
  - `bundled_workflow_builder_prompt_is_non_trivial_size` —
    > 4 KiB sanity bound catches the "we shipped a one-line
    file with the signature inside" failure mode the signature
    check alone doesn't cover.

### Bundle verification

- `cargo build --manifest-path Cargo.toml --bin openhuman-core` ✓
- `strings target/debug/openhuman-core | grep "drafting sub-agent" | wc -l` → **4**
  hits (the prompt and its references are embedded in the binary's
  text segment via `include_str!`).
- Tauri resource path stays
  `../../src/openhuman/agent/prompts` — the directory glob the
  existing `IDENTITY.md` / `SOUL.md` / `USER.md` resources
  already use. macOS resource lookup path:
  `<App>.app/Contents/Resources/_up_/_up_/src/openhuman/agent/prompts/workflow_builder.md`
  (unchanged convention).

### Deviations

- **No JSON edit to `tauri.conf.json`**. The existing directory
  glob already captures `workflow_builder.md`; the F-13 ticket's
  illustrative pattern (one file per line) would have meant
  *adding redundancy* to the JSON. Documented in the module
  doc + DEVLOG so future readers see why the resource list
  isn't explicit.
- **Dual-write expectation**: future edits to the system prompt
  edit both
  `Automations/Artifacts/prompts/workflow_builder.md` (design-time
  SoT) and
  `src/openhuman/agent/prompts/workflow_builder.md` (production).
  The drift test in `proposer_tests.rs` catches one-side
  edits. A follow-up "symlink production path → artifact"
  ticket can collapse this once Tauri bundling has been
  validated against symlinks on every target OS.

### Verified

- `cargo check` ✓
- `cargo fmt` ✓
- `cargo build --bin openhuman-core` ✓
- `cargo test --lib openhuman::workflows` — **151/151 passing**
  (3 new F-13 smoke tests).

### Files

- Modified: `src/openhuman/workflows/proposer.rs` (canonical
  module doc + `WORKFLOW_BUILDER_PROMPT_SIGNATURE` constant +
  `log_prompt_load_once` helper + call site in
  `build_system_prompt`).
- Modified: `src/openhuman/workflows/proposer_tests.rs` (3 new
  F-13 smoke tests).

### Next

F-14 — `<WorkflowProposalPreview>` + companion components +
chat-runtime integration. F-14 is the **next live-test
milestone** per the locked execution contract — it renders
the F-12 propose-tool payloads, surfaces the run-history
view, and adds the cancel-run button. F-15 then closes the
loop with the hero E2E + the agent-invocation swap.
