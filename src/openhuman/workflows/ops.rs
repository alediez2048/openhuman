//! Workflow CRUD operations.
//!
//! F-2 fills this module with `list`, `get`, `create`, `update`, `delete`,
//! `enable`, `disable` against `store::with_connection`. Each mutating
//! op publishes the matching `DomainEvent::Workflow*` event. F-8 adds
//! run-row CRUD (`insert_run`, `mark_run_terminal`, `list_runs`,
//! `get_run`, `count_runs`).
