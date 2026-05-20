# ADR-018: `Workflow.origin` discriminator tracks creation provenance

**Status:** Accepted
**Date:** 2026-05-19
**Phase:** 1
**Companion:** [`prd.md`](../prd.md) · [`requirements.md`](../requirements.md) · [`systemsdesign.md`](../systemsdesign.md)

## Context

Workflows can be created via three Phase 1 paths (chat, starter catalog, form fallback) and a Phase 3+ Import path. Several systems need to know *how* a workflow came to exist: the starter catalog needs to dedup against already-added templates, the UI shows a "Starter" badge on cards seeded from the catalog, and telemetry distinguishes adoption patterns ("chat-driven creation vs. catalog adoption").

Storing this as a free-form string ("via chat") would be fragile. Not storing it at all forces every dedup and analytics use case to infer it from other signals.

## Decision

`Workflow` carries an **`origin: WorkflowOrigin` discriminated enum**, persisted in the `workflows.origin` text column:

```rust
pub enum WorkflowOrigin {
    UserChat,                          // built via the chat agent (hero path)
    UserForm,                          // built via the Phase 1 fallback form
    Seed { template_id: String },      // added from the Starter workflows catalog
    Imported,                          // Phase 3+
}
```

The `Seed { template_id }` variant carries the `template_id` so the catalog query (FR-1.8.2) can dedup against the user's existing seed rows.

## Alternatives considered

**Free-form string ("chat", "catalog:ru-1-…").** Cheaper to add new variants but no compile-time exhaustiveness; analytics queries become string-prefix matches. Rejected because the type-safe enum costs nothing extra and prevents typos like `"chat "` vs. `"chat"`.

**No provenance tracking.** Infer creation path from other signals (e.g., `created_at` proximity to a known catalog-add event, presence of a `template_id` in the workflow's name). Rejected because (a) the inference is fragile, and (b) the catalog's dedup-on-add requires knowing the `template_id` cleanly — without `origin`, the catalog can't reliably hide already-added templates.

## Consequences

### Positive
- Catalog dedup is a clean `WHERE origin->>'template_id' = $1` query.
- UI can render a "Starter" badge on cards where `origin = Seed { … }`.
- Telemetry segmentation (`requirements.md §1.13.11`) — chat-vs.-catalog-vs.-form creation splits — is trivially queryable.
- Adding `Imported` for Phase 3 is a non-breaking enum extension.

### Negative
- The persisted JSON for `Seed { template_id }` is slightly larger than a simple discriminator tag. Negligible.
- The frontend `WorkflowOrigin` type must stay in sync with the Rust enum — covered by the controller-schema contract per `CLAUDE.md`.

### Neutral
- `UserForm` exists today (FR-1.3.1.3) at low emphasis; if the form fallback is removed in a future phase, the variant remains for historical rows.

## Implementation notes

- `WorkflowOrigin` enum in `src/openhuman/workflows/types.rs`.
- Persisted as JSON discriminator in the `workflows.origin` text column (`systemsdesign.md §2.4`).
- Set on every creation path:
    - Chat: `origin = UserChat` set in `workflows_create` when called from preview-component click handlers.
    - Catalog: `origin = Seed { template_id }` set when the catalog [Add] button creates the workflow.
    - Form: `origin = UserForm` set when the form fallback creates the workflow.
- Phase 1 DoD: each creation path is tested to set `origin` correctly.

## Related ADRs

- ADR-008 (Starter templates as read-only catalog) — relies on `Seed { template_id }` for dedup.
- ADR-007 (Chat as primary creation path) — produces `UserChat` rows.
