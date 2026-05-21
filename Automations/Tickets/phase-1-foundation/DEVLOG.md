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
