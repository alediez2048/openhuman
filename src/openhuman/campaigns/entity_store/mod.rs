//! F4-4 — the pluggable backbone for Campaign entity bindings.
//!
//! Every campaign has an `entity_binding: EntityRef` (F4-1 types).
//! This module defines the **`EntityStore` trait** that every
//! adapter (Sheets / Attio / future Airtable / Notion / native)
//! satisfies, plus the registry that dispatches a `EntityRef` to
//! the right adapter.
//!
//! ## Why the trait shape matters
//!
//! Per the 2026-05-26 grill (Option 3 — pluggable adapter), getting
//! this trait right means future adapters cost a single ticket
//! each. Getting it wrong means migrating every existing campaign
//! when the leaky abstraction shows. The contract is deliberately
//! narrow:
//!
//! - **Schema discovery**: every adapter can describe its field
//!   shape so the UI renders columns predictably + the agent can
//!   reason about what fields it can write to.
//! - **Read (`list` + `get`)**: filter-by-field-equality + limit /
//!   offset. Sort + range filters deferred to a follow-up when a
//!   campaign actually needs them.
//! - **Write (`update`)**: field-level merge patches with a strict
//!   atomicity contract — adapters that can't guarantee "success
//!   means readers see it" fail fast.
//! - **Subscribe**: optional inbound stream so workflows can fire
//!   on entity changes (Attio webhook, Sheets polling).
//!
//! See `Automations/Tickets/phase-4-campaigns/F4-4.md`.

pub mod mock;
pub mod registry;
pub mod types;

#[cfg(test)]
mod tests;

pub use mock::MockEntityStore;
pub use registry::open_entity_store;
pub use types::{
    EntityChange, EntityChangeKind, EntityFieldKind, EntityFieldSchema, EntityFilter,
    EntityFilterOp, EntityId, EntityPatch, EntityQuery, EntityRecord, EntitySchema,
};

use async_trait::async_trait;
use futures::Stream;
use std::pin::Pin;

/// Type alias for the change-subscription stream. Adapters wrap
/// their native push surface (Attio webhook → channel) or polling
/// loop (Sheets diff) into a `Stream<Item = EntityChange>`.
pub type EntityChangeStream = Pin<Box<dyn Stream<Item = EntityChange> + Send>>;

/// The contract every campaign-entity adapter implements. Adapters
/// live alongside their respective domains (Sheets via Composio's
/// `googlesheets`, Attio via Composio's `attio`, native via a
/// future `entities` domain) and only the `EntityRef` variant +
/// `open_entity_store` dispatch knows about specific adapter
/// implementations.
#[async_trait]
pub trait EntityStore: Send + Sync {
    /// Stable adapter slug for diagnostics + the `EntityId.adapter`
    /// field. Must round-trip identically across adapter rebuilds
    /// — campaigns that referenced `"google_sheets"` last week
    /// must still find their adapter today.
    fn adapter_id(&self) -> &'static str;

    /// Discover the field shape of the recordset. Used by the UI
    /// to render columns, by the agent to reason about writable
    /// fields, and by the F4-7 `for_each` node to know what to
    /// pass into per-record sub-workflows. May involve a network
    /// round-trip on the first call; adapters cache as they see fit.
    async fn schema(&self) -> anyhow::Result<EntitySchema>;

    /// Query records matching `query`. Sort order is store-defined
    /// for Phase 4. Adapters MAY clamp `limit` to their natural
    /// page size; callers paginate via `offset`.
    async fn list(&self, query: EntityQuery) -> anyhow::Result<Vec<EntityRecord>>;

    /// Fetch a single record by id. Returns `Ok(None)` when the id
    /// is unknown (deleted, never existed). Network errors bubble
    /// up via `Err`.
    async fn get(&self, id: &EntityId) -> anyhow::Result<Option<EntityRecord>>;

    /// Apply `patch` to the record at `id`. Fields not in the
    /// patch are left untouched (field-level merge, not whole-row
    /// replace).
    ///
    /// **Atomicity contract**: a successful return means the write
    /// committed and a subsequent `get` MUST observe the new
    /// values. Adapters that can't guarantee this (e.g. an
    /// eventually-consistent store) fail fast at this call rather
    /// than silently accepting a write that may roll back. Field-
    /// level atomicity is per-field; whole-record atomicity across
    /// multiple fields IS guaranteed when the adapter's underlying
    /// API supports batch writes (Sheets `batchUpdate`,
    /// Attio `PATCH /records/{id}`).
    async fn update(&self, id: &EntityId, patch: EntityPatch) -> anyhow::Result<()>;

    /// Inbound subscription for triggering workflows on entity
    /// changes. Adapters with native push (Attio webhook) return
    /// an event-driven stream; adapters without (Sheets) return a
    /// polling stream backed by `list` diffs.
    ///
    /// `Ok(None)` means the adapter doesn't support subscriptions
    /// at all (read-only adapter, or a write-only sink). Callers
    /// fall back to polling via `list` themselves.
    async fn subscribe(&self) -> anyhow::Result<Option<EntityChangeStream>>;
}
