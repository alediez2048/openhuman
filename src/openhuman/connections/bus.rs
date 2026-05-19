//! Event-bus subscribers for the Connections domain.
//!
//! Phase 0 / P0-2 ships a **scaffold** subscriber that will (in follow-up
//! tickets P0-2a..P0-2e) translate per-mechanism lifecycle events into the
//! unified [`DomainEvent::ConnectionAdded`] / [`DomainEvent::ConnectionRemoved`]
//! / [`DomainEvent::ConnectionUpdated`] stream. Phase 1's workflow-health
//! recomputation subscriber subscribes to the unified stream once, rather than
//! five times against per-mechanism events.
//!
//! ## Current state (P0-2)
//!
//! The struct + name + domains list are defined. The `handle()` method is
//! intentionally a no-op — per-mechanism event-name plumbing is deferred to
//! P0-2a (composio), P0-2b (channels), P0-2c (webview), P0-2d (built-in),
//! P0-2e (mcp).
//!
//! See `Automations/systemsdesign.md §8.1`, `Automations/ADRs/ADR-006`.

use crate::core::event_bus::{DomainEvent, EventHandler};

/// Subscriber that republishes per-mechanism connection lifecycle events as
/// unified `ConnectionAdded` / `ConnectionRemoved` / `ConnectionUpdated`.
pub struct ConnectionsLifecycleSubscriber;

#[async_trait::async_trait]
impl EventHandler for ConnectionsLifecycleSubscriber {
    fn name(&self) -> &'static str {
        "connection::lifecycle"
    }

    fn domains(&self) -> Option<&'static [&'static str]> {
        // Observe the 5 per-mechanism domains; emit into "connection".
        Some(&[
            "composio",
            "channel",
            "webview_account",
            "integration",
            "mcp",
        ])
    }

    async fn handle(&self, _event: &DomainEvent) {
        // TODO(P0-2a..P0-2e): match per-mechanism lifecycle events and
        // publish unified DomainEvent::ConnectionAdded/Removed/Updated.
        // For Phase 0 / P0-2 this is intentionally inert — the unified
        // event types exist (ConnectionAdded/Removed/Updated) and downstream
        // subscribers (Phase 1 workflow-health) can already subscribe; this
        // subscriber starts publishing into that stream as each per-mechanism
        // wiring lands.
    }
}
