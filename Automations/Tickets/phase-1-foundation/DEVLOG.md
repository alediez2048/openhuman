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
