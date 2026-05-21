//! Event-bus subscribers for the workflows domain.
//!
//! F-3 fills this module with `WorkflowHealthRecomputeSubscriber`:
//! subscribes to the connection domain (`ConnectionAdded` /
//! `ConnectionRemoved` / `ConnectionUpdated`), queries workflows whose
//! `nodes_json` references the changed connection, recomputes `health`
//! via `health::recompute`, persists, and publishes
//! `DomainEvent::WorkflowHealthChanged` on every real transition.
