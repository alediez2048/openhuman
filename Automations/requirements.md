# Workflows & Automations — Requirements

**Status:** Draft — Phase 1 design fully locked (post-grilling).
**Companion docs:** [`prd.md`](./prd.md), [`systemsdesign.md`](./systemsdesign.md), [`techstack.md`](./techstack.md)

> Concrete functional + non-functional requirements derived from the brainstorm + the grilling session. Each requirement is phase-tagged. Phase 1 is fully scoped; later-phase requirements are placeholders refined during their own brainstorm.

---

## 1. Functional Requirements

### 1.1 Entity model — Phase 1
- **FR-1.1.1** A `Workflow` is a user-named, persisted entity composed of a `Trigger`, one or more `Node`s, zero or more `Edge`s.
- **FR-1.1.2** A workflow has an `enabled` state (default `false` on create).
- **FR-1.1.3** A workflow stores `created_at`, `updated_at`, `last_run_at` timestamps.
- **FR-1.1.4** A workflow has a stable `id` (UUIDv7 — sortable).
- **FR-1.1.5** A workflow row carries a `schema_version` column for forward-compat migrations.
- **FR-1.1.6** A workflow row carries an `origin` discriminator: `UserChat` (built via agent in chat), `UserForm` (Phase 1 form fallback), `Seed { template_id }` (added from the starter catalog), `Imported` (Phase 3+).
- **FR-1.1.7** A workflow row carries a `health` field of type `WorkflowHealth`. Variants:
    - `Ready` — all referenced connections exist and are connected; the workflow is fireable.
    - `NeedsConnections { missing: Vec<ConnectionRef> }` — one or more referenced connections are absent. Cron does not fire; `manual` Run-Now is blocked. The list-view card shows ⚠️ with the missing connection labels.
    - `LastRunFailed { run_id, reason }` (Phase 2+) — last run failed; informational, doesn't block firing.
    - `SessionExpired { connection }` (Phase 2+) — a webview-account session has expired; blocks firing of any node using that connection.
- **FR-1.1.8** `health` is recomputed on every `ConnectionAdded` / `ConnectionRemoved` event (subscriber in `workflows/bus.rs`). It is also recomputed on workflow create and update. It is not recomputed mid-run.

### 1.2 List / browse / activation — Phase 1
- **FR-1.2.1** The user can see a list of all workflows from the `/workflows` route (dedicated 8th bottom-tab between Connections and Intelligence).
- **FR-1.2.2** The `/workflows` page has two top sections:
    - **Your workflows** — rows from the `workflows` table, including `origin = Seed { template_id }` entries the user has added.
    - **Starter workflows** — read-only catalog of templates the user has *not* yet added. Each catalog card is loaded from `workflows_list_starter_templates` (which reads `include_str!`-bundled JSON at request time). Already-added templates do not appear here (deduplicated by `template_id` against the user's `workflows` rows).
- **FR-1.2.3** Each "Your workflows" list row shows: name, **prominent enable/disable toggle as the primary action**, health badge (✓ Ready / ⚠️ Needs X / ❌ Failed), trigger summary, step summary, last run status, next run timestamp (if cron). Edit / Run-now / Delete are in an overflow menu.
- **FR-1.2.4** Toggle behavior: when `health = Ready`, click flips `enabled` immediately via `workflows_enable` / `workflows_disable`. When `health != Ready`, the toggle is disabled with a tooltip directing the user to fix the underlying cause (e.g., "Connect Twitter in /connections").
- **FR-1.2.5** Each "Starter workflows" catalog card shows: name, description, required connections, [Add to my workflows] (creates as `enabled = false`), [Add & Enable] (creates as `enabled = false` then immediately calls `workflows_enable`).
- **FR-1.2.6** When the user has zero workflows AND has no preference to hide starter workflows, the empty state simply renders the Starter workflows section prominently. Primary CTA in the empty state: **"Ask OpenHuman to build a workflow"** (opens chat with a starter prompt placeholder). Secondary: the starter catalog. Tertiary, low-emphasis: "Create manually" (form fallback).
- **FR-1.2.7** Filter chips above the list: All / Running / Paused / Needs setup (health != Ready) / Failed-last-run. Search by name or step content.
- **FR-1.2.8** The "Starter workflows" section has a "Hide starter workflows" preference (per-user, persisted in Redux slice). Hidden users can re-show it from a settings toggle.

### 1.3 Create / edit / delete — Phase 1
- **FR-1.3.1** Three creation paths exist in Phase 1, in declared priority order:
    1. **Chat (hero path)** — user describes the workflow in chat; agent calls `workflow_propose_create`; tool returns a `WorkflowProposal`; agent emits a `<WorkflowProposalPreview>` rich component; user clicks [Save (paused)] or [Save & Enable]; UI calls `workflows_create` RPC directly (the agent has no mutating tools — see §1.13).
    2. **Add from Starter workflows catalog** — user clicks [Add to my workflows] or [Add & Enable] on a catalog card; UI calls `workflows_create` with the template payload and `origin = Seed { template_id }`.
    3. **Form fallback** — basic create-form (name + description + trigger + one `agent_prompt` node). Lowest-emphasis, for power users.
- **FR-1.3.2** Editing follows the same propose-then-click contract as create. User says "rename my retweet workflow"; agent calls `workflow_propose_update`; tool returns a diff payload; agent emits a `<WorkflowEditPreview>` showing before/after; user clicks [Apply] or [Discard]; UI calls `workflows_update`. There is no canvas editor in Phase 1 (Phase 3, deferred).
- **FR-1.3.3** Edits do not affect in-flight runs.
- **FR-1.3.4** Delete uses the propose-then-click contract: `workflow_propose_delete` emits a `<WorkflowDeletePreview>` showing what'll be removed; user clicks [Delete] or [Cancel]; UI calls `workflows_delete`. **Phase 1 hard-deletes** the workflow row immediately and all its `workflow_runs` + `workflow_run_steps` rows via SQLite cascade. The 30-day soft-delete + retention sweep moves to Phase 2 (filed against OQ-5).
- **FR-1.3.5** Direct (non-chat) actions in `/workflows` (toggle, Run Now from overflow menu, Delete from overflow menu with browser confirm) call the same RPCs directly. They do not require chat or proposal previews because the user's intent is unambiguous (button-press, not generated by an LLM).

### 1.4 Triggers

#### 1.4.1 Phase 1 triggers
- **FR-1.4.1.1** `cron` — cron expression + optional timezone + optional active-hours window. Reuses `cron/scheduler.rs`. When `tz` is `None`, defaults to UTC (documented in the trigger config UI).
- **FR-1.4.1.2** `manual` — fires only via the `workflows_run_now` RPC. Always available regardless of `enabled`.
- **FR-1.4.1.3** A workflow with `enabled = false` or `health != Ready` does **not** fire on `cron`. `manual` Run-Now is blocked when `health != Ready` (toggle is disabled).
- **FR-1.4.1.4** Cron-scheduled runs missed during downtime are **not** replayed. Documented in the trigger config UI.

#### 1.4.2 Phase 2 triggers
- **FR-1.4.2.1** `webhook` — inbound HTTP via the `webhooks/` tunnel. Extends `TunnelRegistration` with a `Workflow { workflow_id }` variant. HMAC-verified. **Inbound bridge for n8n / Zapier / IFTTT / Make and any other webhook source.**
- **FR-1.4.2.2** `composio_event` — subscribes to `DomainEvent::ComposioTriggerReceived`.
- **FR-1.4.2.3** `channel_message` — fires on inbound chat-channel messages matching an optional filter.

### 1.5 Node kinds

#### 1.5.1 Phase 1 node kinds
- **FR-1.5.1.1** `agent_prompt` — runs the OpenHuman agent with a user-authored prompt, an `iteration_cap` (default 10), an optional `model_tier` override, and an `allowed_connections` allowlist drawn from the Phase 0 Connections Hub.

#### 1.5.2 Phase 2 node kinds
- **FR-1.5.2.1** `tool_call` — invoke a single named tool from `tools::registry`.
- **FR-1.5.2.2** `http_request` — generic REST. **Outbound bridge to n8n / Zapier / IFTTT / Make and any other REST service.**
- **FR-1.5.2.3** `channel_message`, **FR-1.5.2.4** `condition`, **FR-1.5.2.5** `delay`.

#### 1.5.3 Reserved node kinds (Phase 3+)
- **FR-1.5.3.1** `transform`, `await_human_approval`, `fan_out`. Declared in the `NodeKind` enum from Phase 1 day one.

### 1.6 Execution & runs — Phase 1
- **FR-1.6.1** A `Run` captures: `id` (UUIDv7), `workflow_id`, `trigger_source`, `status`, `started_at`, `completed_at`, `error`.
- **FR-1.6.2** A `RunStep` row captures per-node execution.
- **FR-1.6.3** Runs are visible from the workflow detail view, newest first, paginated.
- **FR-1.6.4** A failing node halts the run unless `on_error = Continue` (Phase 2; Phase 1 hard-codes `Halt`).
- **FR-1.6.5** A run is bounded by a per-workflow wall-clock timeout (default 5 min, max 1 hour). Hard-kill on timeout → status `TimedOut`.
- **FR-1.6.6** Runs execute through `scheduler_gate`.
- **FR-1.6.7** "Run Now" is synchronous only to the point of run-row creation; execution is async, observable via `workflows_get_run`.
- **FR-1.6.8** **Concurrent runs of the same workflow: single-flight.** At most one run is in flight per workflow at any moment. New triggers that arrive while a run is in flight are **dropped** (not queued). Each drop publishes a `DomainEvent::WorkflowRunSkipped { workflow_id, reason: AlreadyRunning, attempted_trigger_source }`. UI shows a transient toast on manual Run-Now drops; cron drops show in run history as a "skipped" row.
- **FR-1.6.9** Cancellation (`workflows_cancel_run`) is **soft**: the cancelled flag is set; the current node completes (its LLM call is not aborted); subsequent nodes are skipped; run status becomes `Cancelled`. The in-flight registry slot is released so a new trigger can fire.
- **FR-1.6.10** On core startup, an orphan-recovery sweep marks every `workflow_runs` row with `status = Running` as `Failed { reason: CoreCrashed }`.

### 1.7 Visual editor (canvas) — Phase 3, deferred
- **FR-1.7.1..FR-1.7.6** *(unchanged from prior draft; preserved for forward-compatibility)*

### 1.8 Starter workflows catalog — Phase 1
> Templates are a **read-only catalog**, not auto-inserted into the user's workflow table. The user explicitly adds entries.

- **FR-1.8.1** Templates ship at `src/openhuman/workflows/templates/*.json`, embedded at build time via `include_str!`. Each file is a full `WorkflowProposal` JSON document with an additional `template_id`, `min_phase`, and `description`.
- **FR-1.8.2** `workflows_list_starter_templates` RPC returns templates whose `min_phase <= current_phase` AND whose `template_id` is *not* already in the user's `workflows.origin = Seed { template_id }` set.
- **FR-1.8.3** Adding a template (`workflows_create` with `origin = Seed { template_id }`) does not consume the template — but it removes the card from the visible catalog (because of FR-1.8.2's deduplication). Deleting the resulting workflow restores the card to the catalog (the row no longer exists, so the dedup check passes).
- **FR-1.8.4** Phase 1 ships RU-1 through RU-4. Phase 2 ships RU-5 through RU-9 (which require Phase 2 node kinds / triggers).
- **FR-1.8.5** No `workspace_state` table. No `workflows_seeded_at_v*` watermark. The catalog is stateless at the OpenHuman level.

### 1.9 Connections Hub — Phase 0 (prerequisite)
- **FR-1.9.1..FR-1.9.7** *(unchanged from prior draft)*

### 1.10 Generic HTTP connections — Phase 0
- **FR-1.10.1..FR-1.10.4** *(unchanged from prior draft)*

### 1.11 External-platform interoperability — Phase 2
- **FR-1.11.1..FR-1.11.5** *(unchanged from prior draft)*

### 1.12 RPC + Agent Tool surfaces — phase-tagged

> RPCs are called by the frontend UI. Agent tools are called by the chat agent inside `tools::registry`. **All mutations live in the RPC surface; agent tools are read-only or propose-only.**

#### 1.12.1 RPCs (UI-callable)

| Method | Phase | Purpose |
|---|---|---|
| `connections_list` | 0 | Unified list across all 6 mechanisms. |
| `connections_generic_http_create` | 0 | Create a Generic HTTP connection. |
| `connections_generic_http_update` | 0 | Update one. |
| `connections_generic_http_delete` | 0 | Delete one. |
| `connections_test` | 0 | Connectivity probe. |
| `workflows_list` | 1 | Paginated list (user's workflows). |
| `workflows_get` | 1 | Fetch one by id. |
| `workflows_create` | 1 | Create from a fully-formed Workflow JSON. **All chat-driven Saves call this directly from the UI.** Called by the form fallback and the Starter-catalog [Add] buttons too. |
| `workflows_update` | 1 | Update fields. Called by the UI's [Apply] click on an edit preview. |
| `workflows_delete` | 1 | Soft-delete. Called by the UI's [Delete] click on a delete preview. |
| `workflows_enable` / `workflows_disable` | 1 | Toggle `enabled`. Called by direct toggles in `/workflows` and by chat-driven preview clicks. |
| `workflows_run_now` | 1 | Manual trigger. Called by overflow-menu and by chat-driven preview clicks. |
| `workflows_cancel_run` | 1 | Cancel an in-flight run (soft). |
| `workflows_list_runs` | 1 | Paginated runs. |
| `workflows_get_run` | 1 | Run details with all `RunStep`s. |
| `workflows_list_starter_templates` | 1 | Returns templates whose `min_phase <= current_phase` and that the user hasn't already added. |

#### 1.12.2 Agent tools (chat-agent-callable)

| Tool | Phase | Surface | Purpose |
|---|---|---|---|
| `workflow_list` | 1 | Read | Returns the user's workflows so the chat agent can answer "what workflows do I have?". |
| `workflow_get` | 1 | Read | Returns one workflow JSON so the agent can reason about an edit. |
| `workflows_list_runs` | 1 | Read | Last-N runs for the agent to summarize. |
| `workflows_get_run` | 1 | Read | Run detail for the agent to summarize. |
| `workflow_propose_create` | 1 | Propose | Returns a `WorkflowProposal` for "I want to build…". |
| `workflow_propose_update` | 1 | Propose | Returns a diff payload for "rename / reschedule / add a step…". |
| `workflow_propose_delete` | 1 | Propose | Returns a destruction-preview payload. |
| `workflow_propose_enable` / `workflow_propose_disable` | 1 | Propose | Returns a state-toggle preview payload. |
| `workflow_propose_run_now` | 1 | Propose | Returns a Run-Now preview ("Run X now? Estimated cost: $0.01."). |

**There are no mutating agent tools.** Mutation is owned by the UI via button clicks on the preview components emitted by the propose tools.

### 1.13 Agent-driven workflow creation — Phase 1 (hero)
> Pinned post-grilling. The agent has read + propose only; the UI owns mutation via button clicks.

- **FR-1.13.1** The chat agent has the read and propose tools listed in §1.12.2. It has no mutating workflow tools.
- **FR-1.13.2** `workflow_propose_create` runs through `proposer.rs`, a **dedicated drafting sub-agent** with a restricted toolset (the four read-only tools + `connections_list` + a synthetic `emit_proposal` tool). The drafting sub-agent has its own `iteration_cap` (default 6).
- **FR-1.13.3** The drafting sub-agent's system prompt comes from `src/openhuman/agent/prompts/workflow_builder.md` and includes:
    - The valid `Trigger` and `NodeKind` variants for the current phase.
    - A **summary** of the user's currently-connected mechanisms inlined into the prompt (e.g., "You have Composio: gmail, slack, linear. Webview: linkedin, twitter. No Generic HTTP."). The agent can call `connections_list` for richer detail. *(Hybrid discovery; locked.)*
    - Schema reminders + JSON shape requirements for `WorkflowProposal`.
    - A worked example of a propose-then-click exchange.
    - **The webhook escape hatch pattern:** when the user's trigger source isn't in the catalog, propose a `Webhook` trigger and populate `setup_instructions` with a paragraph explaining how to wire the platform up to the generated tunnel URL.
- **FR-1.13.4** `WorkflowProposal` JSON shape (returned by `emit_proposal` and `workflow_propose_create`):
    ```
    { name, description, trigger, nodes[], edges[], settings,
      rationale: string[],           // bullet-by-bullet, shown in the preview
      required_connections: ConnectionRef[],
      missing_connections: ConnectionRef[], // subset of required that the user lacks
      setup_instructions: string?,   // present when external setup is needed
      confidence: "high" | "medium" | "low" }
    ```
- **FR-1.13.5** The chat agent emits the proposal as a `<WorkflowProposalPreview>` rich component carrying the full payload. The component renders Save (paused) / Save & Enable / Discard buttons. Clicks invoke `workflows_create` directly from the UI — **the agent is not involved in the commit step**. After click, the UI posts a synthetic *"Saved as <name>."* user message into the chat so the agent has continuity on its next turn.
- **FR-1.13.6** Editing follows the same flow: `workflow_propose_update` returns a `WorkflowEditProposal { workflow_id, current, proposed, diff_summary, rationale }`. Component renders [Apply] / [Discard] buttons. Click invokes `workflows_update`.
- **FR-1.13.7** Deletion: `workflow_propose_delete` returns `WorkflowDeletePreview { workflow_id, name, run_count, retention_days }`. Component shows the destructive action with [Delete] / [Cancel] buttons. Click invokes `workflows_delete`.
- **FR-1.13.8** State toggles + Run-Now have proposal variants for chat-driven invocation but use the same `workflows_enable` / `workflows_disable` / `workflows_run_now` RPCs as direct UI actions.
- **FR-1.13.9** **Proposal validation + retry.** Every `WorkflowProposal` returned by the drafting sub-agent goes through a validator before being returned to the chat agent:
    - JSON schema parse.
    - `required_connections` ⊆ `connections_list` output (no hallucinated connections).
    - `nodes[].kind` ∈ allowed-for-current-phase set.
    - `cron` expressions parse via the `cron` crate.
    - Edge integrity (no edges referencing nonexistent node ids).
    - No `agent_prompt.allowed_connections` references unknown to `connections_list`.

  On any failure, the wrapper re-invokes the drafting sub-agent with a `ProposalValidationError` appended to the system prompt. **Up to 3 total attempts** (1 original + 2 retries). After 3 failures, surface a structured error to the chat agent: *"Drafting failed after 3 attempts. Last error: <details>."*

- **FR-1.13.10** `ProposalValidationError` variants: `JsonParse(reason)`, `UnknownConnection { ref, candidates }`, `UnsupportedNodeKind { kind, phase }`, `InvalidCron { expr, parse_error }`, `EdgeIntegrity { from, to, reason }`, `MissingRequiredField { field }`.
- **FR-1.13.11** Telemetry counters track: proposals emitted, validation failures (by `ProposalValidationError` variant), Saves clicked, Discards clicked, Save-and-Enable vs. Save-paused split. Counts only — never proposal content or user descriptions.

---

## 2. Non-functional Requirements

### 2.1 Performance
- **NFR-2.1.1** The `/workflows` list view must render with `N=100` workflows in under 200 ms (cold) and 50 ms (warm cache).
- **NFR-2.1.2** A workflow run's overhead beyond node execution must be < 200 ms.
- **NFR-2.1.3** The `/connections` page renders all 6 sections populated in under 250 ms.
- **NFR-2.1.4** `workflow_propose_*` returns within **30 s** in the happy path (single drafting attempt) and within **90 s** in the worst case (3 retry attempts). The chat shows a "Thinking…" indicator throughout.
- **NFR-2.1.5** Validation passes (`ProposalValidationError`-free check) on a typical proposal run in under 50 ms.

### 2.2 Reliability
- **NFR-2.2.1** Workflow definitions are persisted before any RPC returns 200.
- **NFR-2.2.2** A core crash mid-run marks the run `Failed { reason: CoreCrashed }` on next startup (orphan sweep — FR-1.6.10).
- **NFR-2.2.3** Cron-scheduled runs missed during downtime are NOT replayed.
- **NFR-2.2.4** Inbound webhook triggers honor existing tunnel HMAC verification.
- **NFR-2.2.5** `connections_list` deduplication of `workflows_list_starter_templates` is consistent — a template added is no longer in the catalog response within one RPC round-trip.

### 2.3 Security
- **NFR-2.3.1** A workflow inherits the credentials of the user who created it.
- **NFR-2.3.2** Secrets stay in `security/secrets` — **never** serialized into workflow definitions, run records, run-step outputs, or log lines.
- **NFR-2.3.3** `agent_prompt` nodes invoking webview-account actions are gated by existing CDP-only injection rules.
- **NFR-2.3.4** Outbound `http_request` nodes respect existing proxy settings.
- **NFR-2.3.5** Run-step output is truncated to 64 KiB before persistence.
- **NFR-2.3.6** **Mutation surface is closed for agents.** The chat agent has no mutating workflow tools. All mutations require an explicit UI button click backed by a direct RPC. The harness does not need to validate confirmation tokens or text affirmatives — the absence of mutating tools is the security boundary.
- **NFR-2.3.7** **`agent_prompt` sub-agent tool allowlist** (per Q9 of the grilling session):
    - Baseline tools (memory, web_search, time, etc.).
    - The node's `allowed_connections` (filtered against the user's actual connections at run-time).
    - The four read-only workflow tools: `workflow_list`, `workflow_get`, `workflows_list_runs`, `workflows_get_run`.
    - **Excluded:** every `workflow_propose_*`, every mutating workflow surface, every agent tool not on the baseline list.

    Implementation: `executor::build_node_agent_definition(allowed_connections)` returns the exact `AgentDefinition.allowed_tools` list. Tested in `executor_tests.rs` against a known-good allowlist.

### 2.4 Observability
- **NFR-2.4.1** Every state transition publishes a `DomainEvent` on the event bus. New variants in Phase 1: `WorkflowDefined`, `WorkflowUpdated`, `WorkflowDeleted`, `WorkflowEnabled`, `WorkflowDisabled`, `WorkflowHealthChanged`, `WorkflowRunStarted`, `WorkflowRunStepStarted`, `WorkflowRunStepCompleted`, `WorkflowRunCompleted`, `WorkflowRunSkipped { workflow_id, reason, attempted_trigger_source }`.
- **NFR-2.4.2** Logs use grep-friendly prefixes: `[workflows]`, `[workflows-run]`, `[workflows-rpc]`, `[connections]`, `[workflows-proposer]` (drafting-agent activity), `[workflows-validator]` (proposal validator).
- **NFR-2.4.3** A failed run records the offending node id, exception summary, stack trace tail. No secret values, no agent prompt body, no external response bodies.
- **NFR-2.4.4** Sentry breadcrumbs include workflow id, run id, offending node id. Never prompt content.
- **NFR-2.4.5** `WorkflowRunSkipped` events surface in the workflow's run-history view as collapsed "Skipped (already running)" entries, distinct from real runs.

### 2.5 Compatibility & convention
- **NFR-2.5.1** All new Rust code lives in `src/openhuman/workflows/` and `src/openhuman/connections/`.
- **NFR-2.5.2** RPC methods follow `openhuman.<domain>_<verb>`; agent tools follow `workflow_<verb>` (singular).
- **NFR-2.5.3** Controllers register via `schemas.rs`. No new branches in `src/core/cli.rs` or `src/core/jsonrpc.rs`.
- **NFR-2.5.4** Frontend code lives under `app/src/pages/Workflows/`, `app/src/pages/Connections/`, `app/src/components/{workflows,connections}/`. No dynamic imports.
- **NFR-2.5.5** Capability catalog (`src/openhuman/about_app/`) updated per phase.
- **NFR-2.5.6** The workflow-builder prompt lives at `src/openhuman/agent/prompts/workflow_builder.md`, bundled per existing prompt-resource convention.
- **NFR-2.5.7** The `<WorkflowProposalPreview>`, `<WorkflowEditPreview>`, `<WorkflowDeletePreview>` rich-message components live in `app/src/components/workflows/preview/` and are registered with the chat-runtime message-renderer per existing rich-message convention.

### 2.6 Testing
- **NFR-2.6.1** Coverage on changed lines ≥ 80%.
- **NFR-2.6.2** Each phase ships Vitest unit tests, cargo unit + integration tests, ≥1 E2E spec.
- **NFR-2.6.3** Phase 1 hero E2E spec: user opens chat → describes a workflow → drafting agent proposes → preview component renders with payload → user clicks [Save & Enable] → UI calls `workflows_create` then `workflows_enable` → workflow appears in `/workflows` enabled → next cron tick (or immediate `manual` re-run) fires the run → run completes → output visible in run history.
- **NFR-2.6.4** Phase 1 catalog E2E spec: user opens `/workflows` on a fresh workspace → starter section shows RU-1..RU-4 → user clicks [Add] on RU-1 → row appears in "Your workflows" → catalog re-renders without RU-1 → user deletes RU-1 → catalog re-shows RU-1.
- **NFR-2.6.5** Phase 1 unit tests cover:
    - Proposal validator: each `ProposalValidationError` variant has a test case.
    - `executor::build_node_agent_definition`: returns expected allowlist (no propose tools, no mutating tools).
    - `executor` in-flight registry: parallel triggers drop with `WorkflowRunSkipped`.
    - Health recomputation on `ConnectionAdded` / `ConnectionRemoved`.
    - Orphan-recovery sweep on simulated mid-run crash.
- **NFR-2.6.6** Phase 1 cargo integration tests: full `workflows_create` round-trip including `WorkflowProposal` validator path through `proposer::draft_with_retries` with a mock LLM that intentionally fails once then succeeds.

### 2.7 i18n
- **NFR-2.7.1** All user-visible strings via `useT()` / `app/src/lib/i18n/en.ts`.
- **NFR-2.7.2** New nav label `nav.workflows: 'Workflows'`. `nav.connections` preserved.
- **NFR-2.7.3** Starter template `name` + `description` fields are translatable. Phase 1 ships English-only with `// translate later` markers; locale chunks updated in subsequent PRs.
- **NFR-2.7.4** Workflow-builder prompt is English-only in Phase 1 (the chat agent translates its outputs to the user's language as usual). The validator's error messages are English-only (consumed only by the drafting sub-agent).
- **NFR-2.7.5** `<WorkflowProposalPreview>` strings (buttons, labels) are translatable.

---

## 3. Constraints (inherited from `CLAUDE.md`)

- Desktop-only runtime.
- Rust core is the authoritative business-logic layer.
- No JavaScript injection into CEF webview accounts beyond what's already grandfathered.
- Pre-merge: Prettier, ESLint, `tsc --noEmit`, `cargo fmt`, `cargo check`.
- Files ≤ ~500 lines preferred.
- No new top-level `*.rs` files at `src/openhuman/` root.

---

## 4. Acceptance Criteria — Phase 0 (Connections Hub)
*(unchanged from prior draft — see history)*

---

## 5. Acceptance Criteria — Phase 1

> Per-ticket DoD checklists in `Tickets/phase-1-foundation/F-*.md`. High-level:

- [ ] `cargo check`, `cargo fmt --check`, `pnpm typecheck`, `pnpm lint`, `pnpm test`, `pnpm test:rust` all pass.
- [ ] `/workflows` route is reachable via the new dedicated bottom-tab.
- [ ] **Hero flow (NFR-2.6.3) passes end-to-end.**
- [ ] **Catalog flow (NFR-2.6.4) passes end-to-end.**
- [ ] All Phase 1 RPCs round-trip (`workflows_*` and `workflows_list_starter_templates`).
- [ ] All Phase 1 agent tools (`workflow_list`, `workflow_get`, `workflow_propose_*`, the two read-only run tools) round-trip and return validated outputs.
- [ ] **No mutating agent tools are registered** — verified by a unit test asserting `tools::registry::list_tools()` contains no `workflow_create_from_proposal` / `workflow_create` / `workflow_update` / `workflow_delete` / etc.
- [ ] `agent_prompt` sub-agent tool allowlist matches NFR-2.3.7's spec — verified by `executor_tests.rs`.
- [ ] Concurrent run drop semantics (FR-1.6.8) and `WorkflowRunSkipped` event publication tested.
- [ ] Orphan-recovery sweep marks crashed runs `Failed { reason: CoreCrashed }`.
- [ ] Workflow `origin` discriminator correctly set in each creation path (chat → `UserChat`, catalog → `Seed { template_id }`, form → `UserForm`).
- [ ] Workflow `health` field correctly set on create and recomputed on `ConnectionAdded` / `ConnectionRemoved` events.
- [ ] Proposal validator covers all `ProposalValidationError` variants (NFR-2.6.5).
- [ ] `proposer::draft_with_retries` retries up to 3 times with structured error feedback.
- [ ] At least one Vitest test per new component + the rich-message preview components (`<WorkflowProposalPreview>`, `<WorkflowEditPreview>`, `<WorkflowDeletePreview>`).
- [ ] Phase 1 ships the workflow-builder prompt at `src/openhuman/agent/prompts/workflow_builder.md`.
- [ ] Phase 1 ships templates `ru-1-founder-morning-digest.json` through `ru-4-jira-sprint-retro.json`.

---

## 6. Acceptance Criteria — Phase 2 (Execution expansion)

> Filled in during Phase 2 brainstorm. Covers `webhook` / `composio_event` / `channel_message` triggers, the Phase 2 node kinds, additional starter templates (RU-5..RU-9), and the workflow-builder prompt's expanded taxonomy.

---

## 7. Acceptance Criteria — Phase 3 (Visual canvas, deferred)

> Only filled in if Phase 3 is greenlit.

---

## 8. Open Questions

| # | Question | Status | Notes |
|---|----------|--------|-------|
| OQ-1 | Nav placement: 8th bottom-tab vs settings sub-tab? | ✅ **Resolved: A** | Dedicated 8th bottom-tab. |
| OQ-2 | Phase 1 PR scope? | ✅ **Resolved: B+** | Foundation + minimum execution + agent-driven creation + starter catalog. |
| OQ-3 | Storage: own SQLite db file vs extend cron.db? | ✅ **Resolved: A** | Separate `workflows.db` + `connections.db`. |
| OQ-4 | Phase 2 trigger types beyond webhook + composio + channel_message? | 🟡 Open — Phase 2 spec | |
| OQ-5 | Run-history retention default? | 🟡 Open — Phase 2 spec | Lean: 30 days. |
| OQ-6 | Bottom-tab icon: which Heroicon? | ✅ **Resolved: `BoltIcon`** | Closest semantic fit (automation = bolt of energy). Rejected: `ArrowPathRoundedSquareIcon` (too loop-y), `RectangleStackIcon` (too list-y). |
| OQ-7 | Node inter-step data passing: literal, templating, or expressions? | 🟡 Open — Phase 2 spec | Lean: `{{node.<id>.output.<jsonpath>}}` templating. |
| OQ-8 | Templates: in-repo JSON via `include_str!` vs fetched from backend? | ✅ **Resolved: in-repo JSON** | |
| OQ-9 | Positioning vs n8n / Zapier / IFTTT / Make? | ✅ **Resolved: C** | Hybrid: native engine + external platforms as connections. |
| OQ-10 | Connections Hub sequencing? | ✅ **Resolved: B** | Phase 0 sub-project, ships first. |
| OQ-11 | Primary workflow-creation path? | ✅ **Resolved: chat** | Agent-driven via `workflow_propose_*` + UI button clicks. |
| OQ-12 | Seeded templates: where in the phase plan? | ✅ **Resolved: Phase 1 catalog** | Read-only catalog, not auto-seeded. |
| OQ-13 | Agent connection discovery in drafting sub-agent? | ✅ **Resolved: hybrid** | Summary inlined in prompt + `connections_list` tool fallback. |
| OQ-14 | Confirmation mechanism for chat-driven mutations? | ✅ **Resolved: interactive buttons** | `<WorkflowProposalPreview>` component with Save/Discard buttons. No text matching. |
| OQ-15 | Save behavior when proposed workflow references missing connections? | ✅ **Resolved: save with health flag** | Save persists with `health: NeedsConnections { missing }`. Toggle disabled until resolved. |
| OQ-16 | Mutation execution path? | ✅ **Resolved: UI-direct RPC** | Agent has zero mutating tools. UI calls `workflows_*` RPC on button clicks. |
| OQ-17 | Out-of-catalog request handling (e.g., heart-rate trigger)? | ✅ **Resolved: webhook escape hatch** | Propose `Webhook` trigger + populate `setup_instructions`. |
| OQ-18 | Concurrent runs of the same workflow? | ✅ **Resolved: single-flight, drop overlaps** | `WorkflowRunSkipped` event on drops. Soft cancellation. Orphan sweep on boot. |
| OQ-19 | Drafting sub-agent failure recovery? | ✅ **Resolved: bounded auto-retry (3 attempts)** | `ProposalValidationError` feedback per retry. 90s worst-case NFR. |
| OQ-20 | `agent_prompt` sub-agent tool allowlist? | ✅ **Resolved: baseline + allowed_connections + 4 read-only workflow tools** | No propose, no mutations. |
