# ADR-003: Separate SQLite databases per domain (`workflows.db`, `connections.db`)

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 0, 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

OpenHuman already has per-domain SQLite files (`cron.db`, `memory.db`, etc.). The new Workflows and Connections domains need persistence. The choice is whether to extend an existing database (most plausibly `cron.db` since the cron scheduler is already involved with workflow firing), share a single new global db, or follow the established convention and create separate files per domain.

This maps to `requirements.md §8` OQ-3.

## Decision

We will ship **two separate SQLite database files**: `workflows.db` (Phase 1, owned by `src/openhuman/workflows/store.rs`) and `connections.db` (Phase 0, owned by `src/openhuman/connections/store.rs`). Each has its own `schema_migrations` table and advances independently. Cross-domain references are **soft string ids** — e.g., `workflows.HttpRequestConfig.connection_id` references `connections.generic_http_connections.id` by string only, with no FK constraint across databases.

## Alternatives considered

**Extend `cron.db`.** The cron scheduler already knows about workflow triggers via `cron::JobType::WorkflowTrigger { workflow_id }`. Putting workflow rows next to cron-job rows would simplify joins on next-fire-time. Rejected because `cron.db` is owned by the cron domain; co-locating violates the "one domain, one store" convention in `CLAUDE.md`'s domain layout rule, and it would create a hard FK from cron's domain into workflows that we can't break later.

**Single shared `openhuman.db`.** All tables in one file with internal namespacing. Rejected because it couples migration timing across domains — a Phase 0 connections migration would block on a Phase 1 workflows migration, and vice versa. Per-domain files let each domain own its release cadence and rollback story independently.

## Consequences

### Positive
- Matches existing convention; no surprise for contributors who already know how other domains persist.
- Phase 0 can ship `connections.db` and merge before `workflows.db` even exists — no schema coordination.
- Migrations are scoped per domain — a Phase 1 schema change can't accidentally break Phase 0.

### Negative
- No SQL joins across `workflows.workflows` and `connections.generic_http_connections`. Cross-domain reads happen in Rust application code via the soft-id lookup (one extra query per row).
- Backup/restore must enumerate the per-domain files — there's no single "OpenHuman state" blob.

### Neutral
- Future domains (e.g., a Phase 3 templates marketplace) follow the same pattern: their own `.db` file in `~/.openhuman/`.

## Implementation notes

- `src/openhuman/workflows/store.rs` — owns `workflows.db` connection pool.
- `src/openhuman/connections/store.rs` — owns `connections.db` connection pool.
- Migrations: `workflows/migrations/{001_init_workflows,002_runs,003_run_steps}.sql`; `connections/migrations/001_init_generic_http.sql`.
- Per `systemsdesign.md §2.4` — no `workspace_state` table needed (catalog model is stateless).

## Related ADRs

- ADR-008 (Templates as read-only catalog) — relies on the stateless-catalog pattern enabled by per-domain dbs.
- ADR-017 (Workflow health computed-and-persisted field) — lives in `workflows.db`.
