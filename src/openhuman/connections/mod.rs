//! Connections domain — Phase 0 of Workflows & Automations.
//!
//! Aggregates the existing connection mechanisms (composio, channels, webview
//! accounts, integrations, MCP) into a unified surface and introduces the new
//! **Generic HTTP** connection type. The aggregator does not own the auth
//! flows for existing mechanisms — it composes their state, presents it
//! uniformly via [`types::ConnectionView`], and adds Generic HTTP as its own
//! first-class entity in `connections.db`.
//!
//! ## Module layout
//!
//! - [`types`] — domain types: `ConnectionRef`, `GenericHttpConnection`,
//!   `ConnectionView`, `AuthKind`, `ConnectionStatus`.
//! - [`store`] — SQLite persistence + migrations. Opens
//!   `${OPENHUMAN_WORKSPACE}/connections.db`.
//! - [`ops`] — pure CRUD operations (filled by P0-3).
//! - [`aggregator`] — unified `Vec<ConnectionView>` read-model (filled by P0-2).
//! - [`rpc`] — JSON-RPC handlers (filled by P0-2 and P0-3).
//! - [`schemas`] — controller schemas + registration.
//! - [`bus`] — event-bus subscribers + publishers (filled by P0-2).
//!
//! See `Automations/systemsdesign.md §1.1` and `Automations/ADRs/ADR-003`,
//! `ADR-006` for the design rationale.

pub mod aggregator;
pub mod bus;
pub mod ops;
pub mod rpc;
pub mod schemas;
pub mod store;
pub mod types;
pub mod verification;

pub use schemas::{all_connections_controller_schemas, all_connections_registered_controllers};
pub use types::{
    AuthKind, ConnectionRef, ConnectionStatus, ConnectionView, GenericHttpConnection,
    GenericHttpConnectionId, SecretRef,
};
