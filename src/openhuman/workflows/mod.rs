//! Workflows domain — Phase 1 of Workflows & Automations.
//!
//! Builds the workflows engine on top of the Phase 0 Connections Hub:
//! definitions live in `workflows.db`, runs + run-steps live alongside,
//! and `WorkflowHealth` is recomputed from the connections aggregator
//! on every `ConnectionAdded` / `ConnectionRemoved` / `ConnectionUpdated`
//! event.
//!
//! ## Module layout
//!
//! - [`types`] — the full type universe (`Workflow`, `Trigger`, `Node`,
//!   `Edge`, `Run`, `RunStep`, `WorkflowHealth`, `WorkflowOrigin`,
//!   `ProposalValidationError`, …).
//! - [`store`] — SQLite persistence + migration runner against
//!   `${OPENHUMAN_WORKSPACE}/workflows.db`.
//! - [`schemas`] — controller registry scaffold. Empty in F-1; filled
//!   one ticket at a time by F-2..F-8.
//! - [`ops`] — CRUD operations (F-2 + F-8).
//! - [`rpc`] — JSON-RPC handlers (F-2 onwards).
//! - [`scheduler`] — cron + manual dispatch (F-7).
//! - [`executor`] — run lifecycle + sub-agent allowlist (F-8 + F-9).
//! - [`proposer`] — drafting sub-agent + retry loop (F-11).
//! - [`validator`] — deterministic proposal validator (F-11).
//! - [`agent_tools`] — read + propose-only agent tools (F-10 + F-12).
//! - [`health`] — `WorkflowHealth::recompute` (F-3).
//! - [`bus`] — event-bus subscribers (F-3).
//! - `templates/` — RU-1..RU-4 JSON templates (F-5).
//!
//! See:
//! - `Automations/systemsdesign.md §1.2, §2.2, §2.3, §8`
//! - `Automations/ADRs/ADR-003` (separate SQLite databases)
//! - `Automations/ADRs/ADR-014` (single-flight + orphan recovery)
//! - `Automations/ADRs/ADR-017` (`WorkflowHealth` computed-field model)
//! - `Automations/ADRs/ADR-018` (`WorkflowOrigin` discriminator)
//! - `Automations/ADRs/ADR-019` (`ProposalValidationError` variants)

pub mod agent_tools;
pub mod bus;
pub mod diff;
pub mod executor;
pub mod health;
pub mod ops;
pub mod proposer;
pub mod rpc;
pub mod scheduler;
pub mod schemas;
pub mod store;
pub mod templates;
pub mod types;
pub mod validator;

#[cfg(test)]
mod bus_tests;
#[cfg(test)]
mod diff_tests;
#[cfg(test)]
mod executor_tests;
#[cfg(test)]
mod health_tests;
#[cfg(test)]
mod ops_tests;
#[cfg(test)]
mod proposer_tests;
#[cfg(test)]
mod scheduler_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod templates_tests;
#[cfg(test)]
mod types_tests;
#[cfg(test)]
mod validator_tests;

pub use schemas::{all_workflows_controller_schemas, all_workflows_registered_controllers};
pub use types::{
    ActiveHours, AgentPromptConfig, CanvasPosition, Confidence, CreateWorkflowRequest,
    DraftFailure, Edge, HealthFilter, ListFilter, ListStarterTemplatesRequest, ManualInitiator,
    MessageFilter, Node, NodeConfig, NodeKind, OnErrorPolicy, ProposalValidationError, Run, RunId,
    RunNowError, RunStatus, RunStep, RunStepId, SkippedReason, StarterTemplate,
    StarterTemplateView, StateAction, Trigger, TriggerSource, UpdateWorkflowRequest, Workflow,
    WorkflowDeletePreview, WorkflowEditProposal, WorkflowHealth, WorkflowId, WorkflowOrigin,
    WorkflowPatch, WorkflowProposal, WorkflowSettings, WorkflowStateProposal,
};
