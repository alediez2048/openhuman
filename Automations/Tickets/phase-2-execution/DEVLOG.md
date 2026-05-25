# Phase 2 — Execution Depth: DEVLOG

Closure log for the F2-1..F2-16 sequence. Every entry references the
commit SHA that landed it on `main`.

---

## Status: 🟢 Core ticket sequence shipped (F2-1..F2-16). E2E specs deferred to F2-16b.

All 16 Phase 2 tickets executed against the locked OQ-7 / OQ-21 / OQ-22
decisions in `Automations/requirements.md §8`. The Phase 2 surface is
live end-to-end inside the core: every trigger (`webhook` /
`composio_event` / `channel_message`) and every node kind (`tool_call`
/ `http_request` / `channel_message` / `condition` / `delay`) is
reachable from chat-driven drafting all the way through the executor.
`CURRENT_PHASE = 2` on all the drafter-facing constants.

## Shipped tickets

| Ticket | SHA | What landed |
|---|---|---|
| F2-1 | `488d11be` | Phase 2 scaffold — `NodeConfig` variants + validator arms + `dispatch_node` router so every kind is reachable end-to-end behind a `NotImplementedYet` body |
| F2-2 | `50d1096a` | Multi-node execution — Kahn's-algorithm topological sort + `NodeContext` templating (OQ-7) + per-node `RunStep` persistence |
| F2-3 | `82ac7505` | `tool_call` node — `arguments_template` JSON walker + tool-registry dispatch + `CURRENT_PHASE = 2` in the executor |
| F2-4 | `fe75007c` | `http_request` node — `ResponseCapture { BodyAndStatus, StatusOnly, JsonPath }` + secret-safe send path against a `GenericHttp` connection |
| F2-5 | `5f2a61e2` | `channel_message` node — unified send through `channels::controllers` |
| F2-6 | `bab80580` | `condition` node — `left op right` predicate (`eq` / `not_eq` / `contains` / `matches`) + reachability-restricted branch walk |
| F2-7 | `f9ae5ff4` | `delay` node — in-process `tokio::sleep`; persistent resume across core restarts deferred to **F2-7b** |
| F2-8 | `a3f22d1f` | Per-node `retry_policy` + workflow-level `on_error` (Halt / Continue) + exponential backoff |
| F2-9 | `a25ebdc6` | `webhook` trigger plumbing — executor `dispatch_run_with_payload`, router `register_workflow`, ops lifecycle hooks; boot-time reconcile deferred to **F2-9b** |
| F2-10 | `fe92d405` | `composio_event` trigger — `ComposioEventSubscriber` + `list_workflows_matching_composio_event` store query + validator |
| F2-11 | `3812775a` | `channel_message` trigger — `ChannelMessageSubscriber` + `MessageFilter { contains, direct_only, from_user, regex }` evaluator; `DomainEvent::ChannelMessageReceived` extended with `is_direct: bool` |
| F2-12 | `bd7b2553` | RU-5..RU-9 Phase 2 starter templates exercising every new trigger + chain shape; round-trip + validator coverage |
| F2-13 | `9a8076eb` | `workflow_builder.md` prompt rewritten — flipped the Phase 2 "do NOT emit yet" reference into the active surface, added a Phase 2 worked example (channel_message → classify → condition → channel_message) |
| F2-14 | `4b7965c7` | 30-day soft-delete + `retention::run_purge_sweep` (`now_provider` injection point for tests) + `workflows_restore` RPC + `ListFilter.include_deleted` |
| F2-15 | `6585660b` | `Trigger::Cron.active_hours` enforcement — scheduler resolves "now" in the trigger's `tz` (chrono-tz) and drops out-of-window ticks with `SkippedReason::OutsideActiveHours` |
| F2-16 | *(this commit)* | Phase 2 closure — `CURRENT_PHASE = 2` flipped in `ops.rs` + `propose_create.rs` + `propose_update.rs`; 7 Phase 2 capability entries added to `about_app::catalog`; DEVLOG + README closure |

## Deferred follow-ups

Per the MVP-per-ticket convention, three follow-up tickets carry the
remaining scope so the Phase 2 thread closes cleanly:

- **F2-7b** — `delay` node persistent resume across core restarts. F2-7
  shipped an in-process `tokio::sleep`; longer delays survive only if
  the core stays up. Persistent resume needs the run-row schema to
  carry a `resume_at` cursor + the orphan-recovery sweep to refire
  scheduled resumes on boot.
- **F2-9b** — webhook boot reconcile + proposer `setup_instructions`
  + `target_path` regex validation. F2-9 registers webhooks on
  `create/update/enable`, but a fresh core boot doesn't re-register
  existing rows. The reconcile loop should walk every enabled
  `Trigger::Webhook` row at startup and re-register against the live
  router. The drafter also needs to surface the registered URL +
  HMAC secret in `setup_instructions`.
- **F2-16b** — Hero E2E (`workflows-phase-2-hero.spec.ts`) +
  Catalog E2E (`workflows-phase-2-catalog.spec.ts`). The Rust core +
  validator + executor are fully tested at the unit + integration
  level (345 workflows tests pass, including 100+ added during this
  Phase 2 sequence). The WDIO/Appium E2E specs need a mock LLM that
  emits a deterministic multi-node proposal + a tunnel-shaped
  webhook receiver — both worth landing but neither blocks the
  closure of the F2-1..F2-15 implementation work.

## ADR drift audit

| ADR | Status | Notes |
|---|---|---|
| ADR-003 (separate SQLite DBs) | ✅ Conformant | F2-14's migration 004 lives in `workflows.db` only; no leakage into other DB files |
| ADR-014 (single-flight + orphan recovery) | ✅ Conformant | F2-10 / F2-11 fan-out paths both rely on `executor::dispatch_run_with_payload`'s single-flight gate; `AlreadyRunning` errors are debug-logged rather than re-tried by the subscriber |
| ADR-015 (drafter retry budget) | ✅ Conformant | `DEFAULT_MAX_ATTEMPTS = 3` preserved through Phase 2 prompt rewrite |
| ADR-016 (drafter tool allowlist) | ✅ Conformant | `[list_connections, workflow_list, emit_proposal]` unchanged |
| ADR-017 (`WorkflowHealth` computed field) | ✅ Conformant | F-3 health-recompute subscriber unchanged; F2-14 soft-delete adds a `deleted_at IS NULL` filter to the recompute query so deleted rows can't transition |
| ADR-018 (`WorkflowOrigin` discriminator) | ✅ Conformant | F2-14 `restore` republishes `WorkflowDefined` with the original `origin_json` |
| ADR-019 (`ProposalValidationError` variants) | ⚠️ Documented drift | F2-11 + F2-15 surface trigger-shape failures through `InvalidNodeConfig { node_id: "trigger" }` rather than minting a new `InvalidTrigger` variant. Decision: trigger and per-node config failures share a single error class because the drafter's retry prompt format is identical — split when there's a UI need to render them differently. |
| OQ-7 (templating) | ✅ Conformant | Single-token references preserve JSON type (`{{trigger.x}}` → number stays number); multi-token interpolation always stringifies |
| OQ-21 (retry shape) | ✅ Conformant | Backoff: `initial_ms × 2^(attempt-1)` capped at `max_ms`; bounds enforced at validator time (`max_attempts ∈ [1,5]`, `initial_ms ∈ [100, 10000]`, `max_ms ≤ 60000`) |
| OQ-22 (trigger payload exposure) | ⚠️ Partial — 256 KB cap not enforced | The cap was specified in the requirements but the executor currently passes the full payload through. Truncation lives as part of **F2-9b** (webhook hardening) since the same code path needs to enforce the cap for every triggered run, not just webhook. |
| Condition operator surface | ⚠️ Documented drift | F2-6 ships `eq` / `not_eq` / `contains` / `matches` (regex). Numeric comparisons (`gt` / `lt` / `gte` / `lte`) + boolean composition (`and` / `or` / `not`) deferred until a concrete use case appears — text ops cover RU-6 + every Phase 2 starter template. |

## Phase 2 metrics

- **Code**: +5,500 LoC across the workflows domain, +850 LoC across event_bus / core wiring, +5 starter templates (RU-5..RU-9).
- **Tests**: +100 unit tests added across F2-1..F2-15. Full workflows + about_app suite: **345 / 345 passing**.
- **Migrations**: +1 (`004_workflow_soft_delete.sql`).
- **DomainEvent variants**: +3 (`WorkflowRunStepRetried`, `WorkflowPurged`, `is_direct` field on `ChannelMessageReceived`).
- **Capability entries**: +7 in `about_app::catalog` so `list_capabilities()` returns the Phase 2 surface verbatim.

## Next work

Per the F2-16 ticket and the post-Phase-2 plan in `STATE.md`:

1. **F2-16b — Phase 2 E2E specs**. Land the two WDIO specs against a
   mock-LLM + mock-webhook-receiver harness.
2. **F2-7b — Delay persistent resume**. Schema + boot-loop refire.
3. **F2-9b — Webhook boot reconcile + payload-cap enforcement**.
4. **Phase 3 (Browser Agent)**. Ticket drafts already at
   `Automations/Tickets/phase-3-browser-agent/`.

Phase 5 (`entity_tags` business-entity rollup) and Phase 6 (proactive
agent) remain placeholder — earliest start after Phase 3 has been live
2–4 months per the autonomous-business-agent vision in
`Automations/STATE.md`.
