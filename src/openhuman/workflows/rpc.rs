//! JSON-RPC handlers for the workflows domain.
//!
//! F-2 fills the mutating + read surface:
//! `workflows_list`, `workflows_get`, `workflows_create`,
//! `workflows_update`, `workflows_delete`, `workflows_enable`,
//! `workflows_disable`. F-7 adds `workflows_run_now` /
//! `workflows_cancel_run`. F-8 adds `workflows_list_runs` /
//! `workflows_get_run`. F-5 adds `workflows_list_starter_templates`.
//!
//! All handlers return `RpcOutcome<T>` per `AGENTS.md`.
