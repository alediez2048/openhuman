# Workflows & Automations — Systems Design

**Status:** Draft — Phase 1 design fully locked (post-grilling).
**Companion docs:** [`prd.md`](./prd.md), [`requirements.md`](./requirements.md), [`techstack.md`](./techstack.md)

> Authoritative architecture for the new `workflows` and `connections` domains. Read by every ticket primer under "Files you should READ for context."

---

## 1. Domain Boundaries

### 1.1 `src/openhuman/connections/` — Phase 0
```
src/openhuman/connections/
  mod.rs, types.rs, store.rs, ops.rs, aggregator.rs, rpc.rs, schemas.rs, bus.rs
  migrations/001_init_generic_http.sql
```
*(unchanged from prior draft)*

### 1.2 `src/openhuman/workflows/` — Phase 1+
```
src/openhuman/workflows/
  mod.rs             // exports + controller registration
  types.rs           // Workflow, Trigger, Node, Edge, Run, RunStep, WorkflowHealth,
                     // WorkflowOrigin, WorkflowProposal, WorkflowEditProposal,
                     // WorkflowDeletePreview, ProposalValidationError
  store.rs           // SQLite persistence
  ops.rs             // CRUD + health recomputation + starter-catalog query
  scheduler.rs       // Phase 1: cron + manual; Phase 2: webhook/composio/channel_message
  executor.rs        // Run lifecycle, in_flight registry, soft-cancel, orphan sweep,
                     // build_node_agent_definition (per NFR-2.3.7)
  proposer.rs        // Drafting sub-agent + draft_with_retries
  validator.rs       // Schema + connections + node-kind + cron + edge-integrity checks
  agent_tools.rs     // workflow_list, workflow_get, workflows_list_runs,
                     // workflows_get_run, workflow_propose_create/update/delete/
                     // enable/disable/run_now — read + propose ONLY
  templates/         // *.json starter workflows (Phase 1: RU-1..RU-4)
  bus.rs             // ConnectionAdded subscriber for health recomputation
  rpc.rs             // workflows_* RPC handlers (mutations live here)
  schemas.rs         // controller registration
  migrations/        // 001_init_workflows.sql, 002_runs.sql, 003_run_steps.sql
```

**Important convention from the grilling:** `agent_tools.rs` contains **only read + propose tools**. No mutating tools exist on the agent surface. Every mutation is owned by `rpc.rs` and reached via UI button clicks.

No new top-level `*.rs` files at `src/openhuman/` root.

---

## 2. Data Model

### 2.1 Connection types (Phase 0)
*(unchanged from prior draft — `ConnectionRef` discriminated union, `GenericHttpConnection`, `ConnectionView`)*

### 2.2 Workflow types (Phase 1)

```rust
pub struct Workflow {
    pub id: WorkflowId,                     // UUIDv7
    pub schema_version: u32,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub origin: WorkflowOrigin,
    pub health: WorkflowHealth,             // computed; persisted for fast list-view reads
    pub trigger: Trigger,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub settings: WorkflowSettings,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
}

pub enum WorkflowOrigin {
    UserChat,                               // built via the chat agent (hero path)
    UserForm,                               // built via the Phase 1 fallback form
    Seed { template_id: String },           // added from the Starter workflows catalog
    Imported,                               // Phase 3+
}

/// Computed liveness of a workflow. Persisted for fast list-view filtering.
pub enum WorkflowHealth {
    Ready,
    NeedsConnections { missing: Vec<ConnectionRef> },
    // Phase 2+:
    LastRunFailed   { run_id: RunId, reason: String },
    SessionExpired  { connection: ConnectionRef },
}

pub enum Trigger {
    // Phase 1:
    Cron   { expr: String, tz: Option<String>, active_hours: Option<ActiveHours> },
    Manual,
    // Phase 2:
    Webhook        { tunnel_uuid: Uuid, target_path: String },
    ComposioEvent  { trigger_id: String, toolkit: String },
    ChannelMessage { provider: ChannelProvider, filter: Option<MessageFilter> },
}

pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub config: NodeConfig,
    pub position: Option<CanvasPosition>,   // Phase 3 (deferred)
    pub on_error: OnErrorPolicy,
}

pub enum NodeKind {
    AgentPrompt,        // Phase 1
    ToolCall,           // Phase 2
    HttpRequest,        // Phase 2
    ChannelMessage,     // Phase 2
    Condition,          // Phase 2
    Delay,              // Phase 2
    Transform,          // Phase 3+ (reserved)
    AwaitHumanApproval, // Phase 3+ (reserved)
    FanOut,             // Phase 3+ (reserved)
}

pub enum NodeConfig {
    AgentPrompt(AgentPromptConfig),         // Phase 1
    // ... Phase 2 variants
}

pub struct Run { /* unchanged from prior draft */ }
pub struct RunStep { /* unchanged from prior draft */ }
pub enum RunStatus { Pending, Running, Succeeded, Failed, Cancelled, TimedOut }
```

### 2.3 Proposal types (Phase 1 — agent creation)

```rust
/// Returned by workflow_propose_create. Pre-persistence draft.
pub struct WorkflowProposal {
    pub name: String,
    pub description: String,
    pub trigger: Trigger,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub settings: WorkflowSettings,
    pub rationale: Vec<String>,
    pub required_connections: Vec<ConnectionRef>,
    pub missing_connections: Vec<ConnectionRef>,
    pub setup_instructions: Option<String>, // populated for webhook-escape-hatch + missing-conn cases
    pub confidence: Confidence,             // High | Medium | Low
}

pub struct WorkflowEditProposal {
    pub workflow_id: WorkflowId,
    pub current: Workflow,
    pub proposed: Workflow,
    pub diff_summary: Vec<String>,          // bullet list shown in the preview
    pub rationale: Vec<String>,
}

pub struct WorkflowDeletePreview {
    pub workflow_id: WorkflowId,
    pub name: String,
    pub run_count: u32,
    pub retention_days: u32,
}

pub struct WorkflowStateProposal {
    pub workflow_id: WorkflowId,
    pub action: StateAction,                // Enable | Disable | RunNow
    pub rationale: Vec<String>,
}

/// Error path for the validator. Drives auto-retry in proposer::draft_with_retries.
pub enum ProposalValidationError {
    JsonParse           { reason: String },
    UnknownConnection   { r#ref: ConnectionRef, candidates: Vec<ConnectionRef> },
    UnsupportedNodeKind { kind: NodeKind, phase: u32 },
    InvalidCron         { expr: String, parse_error: String },
    EdgeIntegrity       { from: NodeId, to: NodeId, reason: String },
    MissingRequiredField{ field: &'static str },
}
```

### 2.4 Storage schemas (SQLite)

Two database files, one per domain. No `workspace_state` table (the grilling removed it — there's no need for an auto-seed watermark with the catalog model).

```
connections.db                                  (Phase 0)
├── generic_http_connections
└── schema_migrations

workflows.db                                    (Phase 1)
├── workflows                                   columns: id PK, schema_version,
│                                                        name, description,
│                                                        enabled (bool),
│                                                        origin (text — discriminator),
│                                                        health (json — discriminator + payload),
│                                                        trigger_json, nodes_json, edges_json, settings_json,
│                                                        created_at, updated_at, last_run_at
│   indexes: enabled, updated_at, last_run_at, health
├── workflow_runs
│   columns: id PK, workflow_id FK, trigger_source (json),
│            status, started_at, completed_at, error
│   indexes: workflow_id, status, started_at
├── workflow_run_steps
│   columns: id PK, run_id FK, node_id, status,
│            started_at, completed_at, output_json (≤ 64 KiB), error
│   indexes: run_id
└── schema_migrations
```

Cross-domain references use soft string ids:
- `workflows.HttpRequestConfig.connection_id` → `connections.generic_http_connections.id`
- `cron::JobType::WorkflowTrigger { workflow_id }` → `workflows.workflows.id`

---

## 3. Execution Pipeline

```
┌─────────────────┐    ┌──────────────────┐    ┌─────────────────────┐
│ Trigger source  │───▶│ workflows::      │───▶│ workflows::         │
│ cron / manual   │    │ scheduler        │    │ executor            │
│ (P1); +webhook  │    │ ::dispatch_run() │    │ ::run_workflow      │
│ +composio_event │    └──────────────────┘    │                     │
│ +channel_msg P2 │                            │  • check in_flight  │
└─────────────────┘                            │    HashMap          │
                                               │  • if present:      │
                                               │    publish          │
                                               │    WorkflowRunSkipped│
                                               │  • else: insert     │
                                               │    record + run     │
                                               └──────────┬──────────┘
                                                          ▼
                                                ┌─────────────────────┐
                                                │ Per-node executor   │
                                                │ ─ agent_prompt (P1) │
                                                │ ─ tool_call    (P2) │
                                                │ ─ http_request (P2) │
                                                │ ─ channel_msg  (P2) │
                                                │ ─ condition    (P2) │
                                                │ ─ delay        (P2) │
                                                └──────────┬──────────┘
                                                           ▼
                                                ┌─────────────────────┐
                                                │ Existing OpenHuman  │
                                                │ subsystems:         │
                                                │ agent (with curated │
                                                │   allowlist NFR-    │
                                                │   2.3.7), tools,    │
                                                │ composio, channels, │
                                                │ webhooks (out),     │
                                                │ security/secrets,   │
                                                │ scheduler_gate      │
                                                └─────────────────────┘
```

### 3.1 Trigger dispatch
- **Phase 1 — `cron`** — `workflows::scheduler` registers each enabled cron-triggered workflow with the existing cron subsystem via `cron::JobType::WorkflowTrigger { workflow_id }`. On tick, calls `executor::dispatch_run`.
- **Phase 1 — `manual`** — `workflows.run_now(workflow_id)` RPC calls `executor::dispatch_run` directly.
- **Phase 2 — `webhook`** — extends `webhooks::TunnelRegistration` with `Workflow { workflow_id }`.
- **Phase 2 — `composio_event`** — subscribe to `DomainEvent::ComposioTriggerReceived` in `workflows/bus.rs`.
- **Phase 2 — `channel_message`** — subscribe to `DomainEvent::ChannelMessageReceived`.

### 3.2 Executor — single-flight invariant + recovery

```rust
struct ExecutorState {
    in_flight: parking_lot::Mutex<HashMap<WorkflowId, RunId>>,
}

impl Executor {
    pub fn dispatch_run(&self, workflow_id: WorkflowId, source: TriggerSource)
        -> Result<RunId, DispatchError>
    {
        let mut in_flight = self.state.in_flight.lock();
        if let Some(existing) = in_flight.get(&workflow_id) {
            event_bus::publish(DomainEvent::WorkflowRunSkipped {
                workflow_id,
                reason: SkippedReason::AlreadyRunning,
                attempted_trigger_source: source,
            });
            return Err(DispatchError::AlreadyRunning { run_id: *existing });
        }
        let run_id = self.create_run_row(workflow_id, source);
        in_flight.insert(workflow_id, run_id);
        drop(in_flight);
        tokio::spawn(self.clone().execute(workflow_id, run_id));
        Ok(run_id)
    }

    async fn execute(self, workflow_id: WorkflowId, run_id: RunId) {
        let result = self.execute_inner(workflow_id, run_id).await;
        self.state.in_flight.lock().remove(&workflow_id);
        self.finalize_run(run_id, result).await;
    }
}
```

**Soft cancellation** (`workflows_cancel_run`): sets a flag on the run row; the current node completes naturally (its LLM call is not aborted); subsequent nodes are skipped; status becomes `Cancelled`; the `in_flight` slot is released.

**Orphan recovery** at core startup: a sweep marks every `workflow_runs` row with `status = Running` as `Failed { reason: CoreCrashed }`. Implemented in `executor::init` before any scheduler dispatch.

### 3.3 Per-node executors

**Phase 1 — `AgentPrompt`:**
1. `build_node_agent_definition(allowed_connections)` constructs an `AgentDefinition` whose `allowed_tools` is exactly:
    - baseline tools (memory, web_search, time, …),
    - the resolved tools for each `ConnectionRef` in `allowed_connections`,
    - the four read-only workflow tools (`workflow_list`, `workflow_get`, `workflows_list_runs`, `workflows_get_run`).
   Per NFR-2.3.7. **No propose tools, no mutating tools** — verified by a unit test asserting the returned allowlist contains zero of `workflow_propose_*`.
2. Run `agent::run_subagent(definition, prompt, parent_context)`.
3. Agent's final assistant message becomes the node's `output`. Truncated to 64 KiB before persistence (NFR-2.3.5).

**Phase 2 — other kinds:** *(unchanged from prior draft)*

### 3.4 Data flow between nodes
*(unchanged from prior draft — Phase 1 single node, Phase 2 linear chain literal substitution, Phase 3+ templating)*

---

## 4. Agent-Driven Workflow Creation — Phase 1 Hero Path

### 4.1 Flow diagram

```
        User                  Chat agent              Drafting          UI
                                                      sub-agent
         │                         │                       │              │
"every retweet…"                  │                       │              │
         │────────────────────────▶│                       │              │
         │                         │                       │              │
         │   workflow_propose_create(description)         │              │
         │                         │───────────────────────▶              │
         │                         │                       │              │
         │                         │     proposer.rs       │              │
         │                         │     draft_with_       │              │
         │                         │     retries           │              │
         │                         │      • iteration_cap 6│              │
         │                         │      • validate after │              │
         │                         │        emit_proposal  │              │
         │                         │      • up to 3 retries│              │
         │                         │        with Validation│              │
         │                         │        Error context  │              │
         │                         │                       │              │
         │                         │      WorkflowProposal │              │
         │                         │◀──────────────────────│              │
         │                         │                       │              │
         │  <WorkflowProposalPreview payload=…>            │              │
         │  rendered in chat thread                        │              │
         │◀────────────────────────│                       │              │
         │                         │                       │              │
   click [Save & Enable]           │                       │              │
         │────────────────────────────────────────────────────────────────▶
         │                         │                       │              │
         │                         │                       │   workflows_create(payload)
         │                         │                       │   workflows_enable(id)
         │                         │                       │              │
         │                         │                       │   { workflow_id, name }
         │                         │                       │              │
         │  Synthetic user msg:    │                       │              │
         │  "Saved as 'Retweet     │                       │              │
         │  → LinkedIn'."          │                       │              │
         │────────────────────────▶│                       │              │
         │                         │                       │              │
         │  Agent ack + next-step  │                       │              │
         │  suggestion             │                       │              │
         │◀────────────────────────│                       │              │
```

**Key invariant** (post-grilling, OQ-14 + OQ-16):
- The chat agent's only contribution to the mutation is **emitting the preview component**.
- The button click directly invokes the `workflows_create` (and optionally `workflows_enable`) RPC from the UI.
- The chat agent has **no `workflow_create_from_proposal` tool**, no confirmation token, no harness validation of affirmative text — the mutation surface is closed for agents by construction.

### 4.1.1 Which propose tools route through the drafting sub-agent?

Not every propose tool needs an LLM call. The drafting sub-agent (`proposer.rs`) is reserved for tools that genuinely require generative work; the simpler propose tools generate their payloads server-side without invoking the LLM:

| Agent tool | Path | Why |
|---|---|---|
| `workflow_propose_create` | Drafting sub-agent (LLM) | Needs to translate NL → structured `WorkflowProposal`. |
| `workflow_propose_update` | Drafting sub-agent (LLM) | Needs to translate "rename to X" / "change schedule to 9am" → diff payload. |
| `workflow_propose_enable` | Server-side, no LLM | Static `WorkflowStateProposal { workflow_id, action: Enable, rationale: ["This will resume the cron schedule. Next run …"] }`. |
| `workflow_propose_disable` | Server-side, no LLM | Same shape, action: `Disable`. Rationale states what stops firing. |
| `workflow_propose_run_now` | Server-side, no LLM | Same shape, action: `RunNow`. Rationale states the estimated cost / next-run impact. |
| `workflow_propose_delete` | Server-side, no LLM | Static `WorkflowDeletePreview { workflow_id, name, run_count, retention_days }`. Just a read against the DB. |

The four server-side propose tools are still gated by the same preview-component button-click pattern (ADR-010) — the user still clicks [Enable] / [Run Now] / [Delete] to commit. Removing the LLM call for these saves ~30s of latency per invocation, eliminates a class of validation failures, and keeps the `<WorkflowStatePreview>` / `<WorkflowDeletePreview>` payloads deterministic.

### 4.2 The drafting sub-agent (`proposer.rs`)

```rust
pub async fn draft_with_retries(
    description: &str,
    connections: &ConnectionsSnapshot,
    phase: u32,
    max_attempts: u32, // 3
) -> Result<WorkflowProposal, DraftFailure>
```

System prompt is loaded from `src/openhuman/agent/prompts/workflow_builder.md`. The prompt includes:

1. **Schema reminders** — the JSON shape of `WorkflowProposal`, exhaustive list of `Trigger` and `NodeKind` variants for the current `phase`.
2. **Hybrid connection summary** (OQ-13) — inline a tight summary of the user's connections, e.g.:
   ```
   You have these connections:
     • Composio: gmail (jad@…), slack, linear
     • Channel:  telegram
     • Webview:  linkedin, twitter
     • Built-in: twilio
   You can call `connections_list` for richer detail if needed.
   ```
3. **Webhook escape-hatch rule** (OQ-17) — *"When the user's trigger source isn't in the catalog (e.g., 'when my heart rate spikes'), propose a `Webhook` trigger and populate `setup_instructions` with a paragraph explaining how to wire the platform up to the tunnel URL."*
4. **Confirmation pattern reminder** — *"You return the proposal payload via `emit_proposal`. The user confirms in the UI by clicking a button. You do not persist anything."*
5. **One worked example** — full propose-then-click exchange showing the conversational shape.

**Tool allowlist** for the drafting sub-agent:
- `connections_list` (read).
- `workflow_list` (read — so the agent can detect "this is similar to an existing workflow X — should I update it instead?").
- `emit_proposal(payload: WorkflowProposal)` — synthetic tool that returns the payload to the wrapper.

Notably absent: `workflow_create_*`, any mutation, any propose tool (the drafting sub-agent doesn't recurse).

### 4.3 Confirmation contract — UI buttons, not text matching

The proposal payload is embedded in the chat message that renders `<WorkflowProposalPreview>`. The preview component carries:

```typescript
interface WorkflowProposalPreviewProps {
  proposal: WorkflowProposal;
  onSavePaused: (proposal: WorkflowProposal) => Promise<void>;
  onSaveAndEnable: (proposal: WorkflowProposal) => Promise<void>;
  onDiscard: () => void;
}
```

`onSavePaused` calls `workflowsClient.create(payload)`. `onSaveAndEnable` calls `workflowsClient.create(payload)` then `workflowsClient.enable(id)`. `onDiscard` removes nothing from the chat history — the preview just transitions to a "discarded" visual state.

After Save, the UI posts a synthetic user message into the chat — *"Saved as `<name>`."* — so the agent has context continuity on its next turn.

### 4.4 Validation + retry — `proposer::draft_with_retries`

```rust
pub async fn draft_with_retries(...) -> Result<WorkflowProposal, DraftFailure> {
    let mut last_error: Option<ProposalValidationError> = None;
    for attempt in 0..max_attempts {
        let prompt = build_system_prompt(connections, phase, &last_error);
        let proposal = run_drafting_subagent(description, prompt).await?;
        match validator::validate(&proposal, connections, phase) {
            Ok(()) => return Ok(proposal),
            Err(e) => {
                metrics::counter!("workflow_proposal_validation_error", "kind" => e.kind_label()).increment(1);
                last_error = Some(e);
            }
        }
    }
    Err(DraftFailure::ValidationFailedAfterRetries {
        attempts: max_attempts,
        last_error: last_error.unwrap(),
    })
}
```

The validator (`validator.rs`) checks:
- JSON deserializes into `WorkflowProposal`.
- `required_connections` ⊆ `connections_list` output.
- `nodes[].kind` ∈ phase-allowed set.
- `cron` expressions parse via the `cron` crate.
- Edge integrity (every edge's `from`/`to` references a node id that exists).
- `agent_prompt.allowed_connections` are known.

Validation passes are < 50ms (NFR-2.1.5). The retry attempt sees the failed proposal + the structured `ProposalValidationError` appended to the system prompt:

```
PREVIOUS ATTEMPT FAILED:
{ ProposalValidationError::UnknownConnection {
    ref: ConnectionRef::Composio { toolkit_id: "spotify-pro", … },
    candidates: [Composio { toolkit_id: "spotify", … }],
  } }
Fix the unknown connection. Pick from the listed candidates if the user's intent matches.
```

### 4.5 Webhook escape hatch — `setup_instructions`

When the user's trigger source isn't in the catalog, the drafting sub-agent proposes a `Webhook` trigger and populates `setup_instructions`. The preview renders the instructions as a callout above the buttons:

```
⚠️ Setup needed
   This workflow waits for HTTP POSTs to
       https://<tunnel-url>/<uuid>
   You'll need a service that can POST to that URL when the event
   you described occurs (e.g., a phone shortcut, an IFTTT applet,
   or a custom webhook integration).
```

Save still works — the workflow persists with `health: Ready` (Webhook triggers don't depend on connections) and waits for posts. The toggle is enabled — the user activates it once they've wired up the external source.

---

## 5. Starter Workflows Catalog — Phase 1

> Templates are a **read-only catalog at runtime**, not auto-inserted into the user's table. Per OQ-12 grilling decision.

### 5.1 Template files

`src/openhuman/workflows/templates/*.json` — embedded via `include_str!`. Each file:

```json
{
  "$schema": "../schemas/starter-template.v1.json",
  "template_id": "ru-1-founder-morning-digest",
  "min_phase": 1,
  "name": "Founder morning digest",
  "description": "Every weekday at 8am, …",
  "tags": ["productivity", "morning-routine"],
  "rationale_at_seed": [
    "Short, scannable bullets shown when the user hovers the catalog card.",
    "Mirrors the rationale field on a chat-drafted proposal."
  ],
  "trigger": { "type": "cron", "expr": "0 8 * * 1-5", "tz": "UTC" },
  "nodes": [ /* one agent_prompt node */ ],
  "edges": [],
  "settings": { "timeout_secs": 300 },
  "required_connections": [
    { "type": "composio", "toolkit_id": "gmail" },
    { "type": "composio", "toolkit_id": "linear" },
    { "type": "composio", "toolkit_id": "slack" },
    { "type": "channel",  "provider": "telegram" }
  ]
}
```

Phase 1 ships:
- `ru-1-founder-morning-digest.json`
- `ru-2-linkedin-engagement-queue.json`
- `ru-3-spotify-friday-five.json`
- `ru-4-jira-sprint-retro.json`

Phase 2 ships: RU-5..RU-9 (need Phase 2 node kinds / triggers).

### 5.2 Catalog query

`workflows_list_starter_templates` is a stateless RPC:

```rust
pub fn list_starter_templates(ctx: &Ctx, phase: u32) -> Vec<StarterTemplateView> {
    let all = include_str!-bundled templates;
    let user_seeded_ids: HashSet<String> = ctx.workflows_store()
        .list_seed_origins()
        .into_iter().collect();
    all.into_iter()
       .filter(|t| t.min_phase <= phase)
       .filter(|t| !user_seeded_ids.contains(&t.template_id))
       .map(StarterTemplateView::from)
       .collect()
}
```

No `workspace_state` table, no watermark, no migration. Pure read of in-repo JSON + a dedup check against the user's existing `Seed` origins.

### 5.3 Catalog → user-workflow promotion

[Add to my workflows] click on a catalog card calls `workflows_create(template_payload)` with `origin = Seed { template_id }`. The newly-created row has its own `id` (UUIDv7); the `template_id` is preserved in `origin` for analytics + dedup. The catalog query (FR-1.8.2) automatically excludes this template on the next call.

[Add & Enable] is the same call followed by `workflows_enable(id)`.

### 5.4 Health on first-add

A freshly-added Seed workflow inherits `health` from the standard recomputation: if the user has all `required_connections`, it's `Ready`; otherwise `NeedsConnections { missing }`. The "Add & Enable" button can still be clicked when the workflow will be `NeedsConnections` — the create succeeds, the enable call returns the workflow in a `enabled = true, health = NeedsConnections` state. The cron scheduler skips it. The UI surfaces the missing connection prominently. (This matches the Q3 grilling decision: don't block Save just because connections are missing.)

---

## 6. Integration With Existing Primitives
*(unchanged from prior draft — see history)*

---

## 7. RPC + Agent Tool Surfaces (Phase-tagged)

### 7.1 RPCs (UI-callable)

| Method | Phase | Owner | Mutating? |
|---|---|---|---|
| `connections_*` | 0 | `src/openhuman/connections/rpc.rs` | mixed |
| `workflows_list` | 1 | `src/openhuman/workflows/rpc.rs` | no |
| `workflows_get` | 1 | rpc.rs | no |
| `workflows_create` | 1 | rpc.rs | **yes (mutates)** |
| `workflows_update` | 1 | rpc.rs | **yes** |
| `workflows_delete` | 1 | rpc.rs | **yes** |
| `workflows_enable` / `workflows_disable` | 1 | rpc.rs | **yes** |
| `workflows_run_now` | 1 | rpc.rs | **yes** |
| `workflows_cancel_run` | 1 | rpc.rs | **yes** |
| `workflows_list_runs` | 1 | rpc.rs | no |
| `workflows_get_run` | 1 | rpc.rs | no |
| `workflows_list_starter_templates` | 1 | rpc.rs | no |

### 7.2 Agent tools (chat-agent-callable, registered in `tools::registry`)

| Tool | Phase | Owner | Mutating? |
|---|---|---|---|
| `workflow_list` | 1 | `src/openhuman/workflows/agent_tools.rs` | no |
| `workflow_get` | 1 | agent_tools.rs | no |
| `workflows_list_runs` | 1 | agent_tools.rs | no |
| `workflows_get_run` | 1 | agent_tools.rs | no |
| `workflow_propose_create` | 1 | agent_tools.rs → proposer.rs | **no — returns payload, doesn't mutate** |
| `workflow_propose_update` | 1 | agent_tools.rs → proposer.rs | no |
| `workflow_propose_delete` | 1 | agent_tools.rs | no |
| `workflow_propose_enable` / `workflow_propose_disable` | 1 | agent_tools.rs | no |
| `workflow_propose_run_now` | 1 | agent_tools.rs | no |

**There are no mutating agent tools.** A unit test in `agent_tools_tests.rs` asserts that every tool registered by `workflows/agent_tools.rs` is read-only or propose-only.

All methods return `RpcOutcome<T>` (RPCs) / `serde_json::Value` (agent tools).

---

## 8. Event Bus Additions

```rust
pub enum DomainEvent {
    // Connections (Phase 0)
    ConnectionAdded   { r#ref: ConnectionRef },
    ConnectionRemoved { r#ref: ConnectionRef },
    ConnectionUpdated { r#ref: ConnectionRef },

    // Workflows (Phase 1)
    WorkflowDefined          { workflow_id: WorkflowId, origin: WorkflowOrigin },
    WorkflowUpdated          { workflow_id: WorkflowId },
    WorkflowDeleted          { workflow_id: WorkflowId },
    WorkflowEnabled          { workflow_id: WorkflowId },
    WorkflowDisabled         { workflow_id: WorkflowId },
    WorkflowHealthChanged    { workflow_id: WorkflowId, health: WorkflowHealth },
    WorkflowRunStarted       { workflow_id: WorkflowId, run_id: RunId },
    WorkflowRunStepStarted   { run_id: RunId, node_id: NodeId },
    WorkflowRunStepCompleted { run_id: RunId, node_id: NodeId, status: RunStatus },
    WorkflowRunCompleted     { workflow_id: WorkflowId, run_id: RunId, status: RunStatus },
    WorkflowRunSkipped       { workflow_id: WorkflowId, reason: SkippedReason, attempted_trigger_source: TriggerSource },
}

pub enum SkippedReason {
    AlreadyRunning,
    HealthBlocked { health: WorkflowHealth },
}
```

Domain strings: `"connection"`, `"workflow"`.

### 8.1 Health recomputation subscriber

`src/openhuman/workflows/bus.rs` subscribes to `ConnectionAdded` and `ConnectionRemoved`. On each event:

1. Query `workflows` where any node references the changed connection.
2. Recompute `health` (`Ready` ↔ `NeedsConnections`).
3. Persist + publish `WorkflowHealthChanged`.

Bounded work per event (a single `UPDATE` with a `WHERE` clause). The subscriber runs in a background tokio task off the event-bus broadcast channel.

---

## 9. Frontend Surface

### 9.1 Routes
*(unchanged from prior draft)*

### 9.2 Navigation (Locked OQ-1 = A)
*(unchanged from prior draft — new bottom-tab between Connections and Intelligence)*

### 9.3 Pages & components

```
app/src/pages/Connections/                      // Phase 0
  ConnectionsHub.tsx
  sections/...

app/src/pages/Workflows/                        // Phase 1
  WorkflowsList.tsx                             // "Your workflows" + "Starter workflows" sections
  WorkflowsDetail.tsx                           // read-only Phase 1
  WorkflowsRunDetail.tsx

app/src/components/workflows/                   // Phase 1
  WorkflowCard.tsx                              // list row with enable/disable toggle
  WorkflowEnableToggle.tsx                      // primary action
  WorkflowHealthBadge.tsx                       // ✓ / ⚠️ / ❌
  WorkflowEmptyState.tsx
  WorkflowCreateModal.tsx                       // form fallback
  StarterWorkflowCard.tsx                       // catalog card (read-only)
  StarterWorkflowsSection.tsx                   // "Starter workflows" wrapper
  RunStatusBadge.tsx
  RunHistoryTable.tsx
  preview/                                      // Phase 1 rich-message components
    WorkflowProposalPreview.tsx                 // [Save (paused)] / [Save & Enable] / [Discard]
    WorkflowEditPreview.tsx                     // diff display + [Apply] / [Discard]
    WorkflowDeletePreview.tsx                   // destruction summary + [Delete] / [Cancel]
    WorkflowStatePreview.tsx                    // toggle/run-now preview + [Confirm] / [Cancel]
```

### 9.4 Chat-runtime integration

The preview components are registered with the existing `ChatRuntimeProvider`'s rich-message renderer (`app/src/providers/ChatRuntimeProvider.tsx`). The drafting sub-agent's `emit_proposal` tool returns a payload that the agent harness wraps in a `chat:message:rich` event of type `workflow_proposal_preview`; the chat UI's message-renderer maps that type to `<WorkflowProposalPreview>`.

After button click and RPC success, the UI emits a synthetic user message (`"Saved as 'X'."`) into the chat thread via `coreRpcClient.call('chat.append_user_message', ...)`. This keeps the agent's next turn in conversational continuity.

### 9.5 State + services
*(unchanged from prior draft — `workflowsSlice`, `connectionsSlice`, `services/api/workflows.ts`, `services/api/connections.ts`)*

### 9.6 Visual style
*(unchanged from prior draft)*

---

## 10. External-Platform Interop — Architecture
*(unchanged from prior draft — webhook in / http_request out, no platform-specific code)*

---

## 11. Phase 0 — Connections Hub Architecture
*(unchanged from prior draft)*

---

## 12. Testing Strategy

| Layer | What | First phase |
|---|---|---|
| Rust unit | `ops::create`, `store::persist`, `health::recompute` | 0/1 |
| Rust unit | `validator::validate` — every `ProposalValidationError` variant | 1 |
| Rust unit | `executor::build_node_agent_definition` returns allowlist matching NFR-2.3.7 (no mutating tools, no propose tools) | 1 |
| Rust unit | `agent_tools_tests.rs` asserts no mutating tool is registered for workflows | 1 |
| Rust unit | Concurrent-run drop: simulated parallel triggers publish `WorkflowRunSkipped` for all but the first | 1 |
| Rust unit | Orphan sweep marks crashed runs `Failed { reason: CoreCrashed }` | 1 |
| Rust integration | RPC round-trip (`workflows.create` → `workflows.list` → `workflows.enable`) | 1 |
| Rust integration | `proposer::draft_with_retries` with a mock LLM that fails attempt 1 + 2, succeeds attempt 3 | 1 |
| Rust integration | Health recomputation triggered by simulated `ConnectionAdded` event | 1 |
| Vitest unit | `WorkflowProposalPreview`, `WorkflowEditPreview`, `WorkflowDeletePreview` button-click handlers correctly invoke the RPC clients | 1 |
| Vitest unit | `WorkflowsList` renders both sections; deduplication of starter catalog vs. user-owned rows | 1 |
| E2E (WDIO) | **Hero flow** (NFR-2.6.3) | 1 |
| E2E (WDIO) | **Catalog flow** (NFR-2.6.4) | 1 |
| E2E (WDIO) | Webhook trigger + http_request outbound against mock backend | 2 |
| E2E (WDIO) | Connections Hub renders 6 sections | 0 |

---

## 13. Migration & Rollout

### 13.1 Schema migrations
- `connections.db` (Phase 0): `001_init_generic_http.sql`.
- `workflows.db` (Phase 1): `001_init_workflows.sql`, `002_runs.sql`, `003_run_steps.sql`.
- **No `workspace_state` table.** Removed during the grilling — the catalog model doesn't need a watermark.
- Each domain advances its own `schema_migrations` row independently.

### 13.2 Catalog versioning
Templates ship in-repo; new templates in future versions just add new files in `templates/`. No migration needed. Already-added templates (by `template_id` in `Workflow.origin`) are deduplicated from the catalog response.

### 13.3 Feature flags
No feature flag. Workflows + Starter catalog ship enabled by default.

### 13.4 Backward-incompatible changes
None. `/skills → /connections` and `/channels → /connections#channels` both redirect.

---

## 14. Capability Catalog Update

Per `CLAUDE.md §Capability catalog`: `src/openhuman/about_app/` updated per phase.

- Phase 0: "Connections Hub — manage every connected service in one place."
- Phase 1: "Workflows — describe an automation in chat and OpenHuman builds it. Click Save to commit. Activate from the Workflows tab. Starter templates included."
- Phase 2: "External-platform interop — trigger workflows via inbound webhook or call out via the http_request node."
- Phase 3 (if pursued): "Visual workflow editor."

---

## 15. Open Questions

> Mirrors `requirements.md §8`. All Phase 1 blockers + grilling decisions resolved.
