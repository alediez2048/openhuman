//! Supporting types for the [`super::EntityStore`] trait (F4-4).
//!
//! The catalog is intentionally compact: every adapter (Sheets,
//! Attio, future Airtable / Notion / native) must round-trip these
//! types verbatim, so adding a new variant means migrating every
//! adapter. Wire format is `#[serde(tag = ...)]` for forward-compat
//! deserialization.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Identity ────────────────────────────────────────────────────────

/// Composite identifier for an entity record. The `adapter` half
/// scopes the id so two adapters can mint colliding `native` strings
/// without ambiguity (a Sheets row id `"42"` and an Attio record id
/// `"42"` are distinct entities).
///
/// `EntityId` round-trips through JSON as `{"adapter": "...",
/// "native": "..."}` — flat shape so it's safe to embed in JSON-blob
/// columns and pass through the agent surface.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct EntityId {
    /// Stable adapter slug (`"google_sheets"`, `"attio"`,
    /// `"native"`, `"mock"`). Matches
    /// [`super::EntityStore::adapter_id`] on the adapter that
    /// minted this id.
    pub adapter: String,
    /// Adapter-native identifier. Stable across schema changes
    /// within the adapter (Sheets uses row-id columns or row index;
    /// Attio uses the record's UUID).
    pub native: String,
}

impl EntityId {
    /// Construct an id with the given adapter + native components.
    pub fn new(adapter: impl Into<String>, native: impl Into<String>) -> Self {
        Self {
            adapter: adapter.into(),
            native: native.into(),
        }
    }
}

// ── Schema discovery ────────────────────────────────────────────────

/// User-visible field shape for the records this adapter exposes.
/// `Sheets` derives this from column headers; `Attio` returns the
/// typed object schema; the future `Native` adapter reads it from
/// `entities.db`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntitySchema {
    /// Fields in display order — adapters preserve the underlying
    /// store's natural ordering (column order for Sheets, attribute
    /// order for Attio) so the UI renders columns predictably.
    pub fields: Vec<EntityFieldSchema>,
    /// Which field is the primary identity for display — `"email"`
    /// for a People sheet, `"record_id"` for Attio. Not required to
    /// be unique by itself; that's
    /// [`super::EntityStore::get`]'s job via [`EntityId`].
    pub primary_field: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityFieldSchema {
    /// Wire key — matches the JSON object key in
    /// [`EntityRecord::fields`].
    pub key: String,
    /// Human-readable label — `"Email Address"` for a `key: "email"`.
    /// Adapters fall back to title-cased `key` when the store has
    /// no display label.
    pub label: String,
    pub kind: EntityFieldKind,
    /// Whether the adapter considers this field required for
    /// `update`. Sheets: typically false (columns are nullable);
    /// Attio: matches the attribute's `is_required` flag.
    pub required: bool,
}

/// Coarse-grained field type. `Unknown` is the safe fallback for
/// Sheets columns whose header doesn't hint at a type — the agent
/// just treats the value as a string. Adapters MUST NOT invent
/// stricter classifications than the underlying store guarantees.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityFieldKind {
    String,
    Number,
    Bool,
    /// Closed set of string values (Attio Select attribute, custom
    /// Status columns). `variants` is the full enumeration in
    /// store-side order.
    Enum {
        variants: Vec<String>,
    },
    DateTime,
    EmailAddress,
    PhoneNumber,
    Url,
    /// Adapter couldn't classify the field — UI renders as plain
    /// string; agent treats opaquely.
    Unknown,
}

// ── Query ───────────────────────────────────────────────────────────

/// Filter + pagination shape every adapter understands. Sort order
/// is store-defined for Phase 4; explicit sort spec deferred to a
/// follow-up when a campaign needs it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityQuery {
    #[serde(default)]
    pub filters: Vec<EntityFilter>,
    /// `None` = adapter default (typically 100). Adapters MAY clamp
    /// to a per-store upper bound (Sheets reads ~1000 rows/req;
    /// Attio defaults to 25/page).
    #[serde(default)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EntityFilter {
    /// Matches a field key from [`EntitySchema`]. Unknown keys are
    /// silently skipped (no match) per adapter discretion.
    pub field: String,
    pub op: EntityFilterOp,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EntityFilterOp {
    Eq,
    NotEq,
    /// Case-insensitive substring against string field values.
    /// Skipped on non-string fields.
    Contains,
    IsNull,
    IsNotNull,
}

// ── Records + writes ────────────────────────────────────────────────

/// One row of the campaign's recordset. `fields` carries the
/// adapter-decoded values — strings, numbers, bools, ISO-8601
/// timestamps — keyed by [`EntityFieldSchema::key`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityRecord {
    pub id: EntityId,
    pub fields: serde_json::Map<String, serde_json::Value>,
    /// When the adapter last observed this record changed. Sheets:
    /// derived from row's `Last Modified` column if present; Attio:
    /// the native `updated_at`. `None` when unknown.
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// Field-level merge patch — adapters apply the supplied fields and
/// leave everything else untouched. Atomicity is per-field; whole-
/// record atomicity isn't guaranteed across all adapters (Sheets
/// updates cell-by-cell).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct EntityPatch {
    pub fields: serde_json::Map<String, serde_json::Value>,
}

// ── Change subscription ─────────────────────────────────────────────

/// One observed change to a record. Subscriptions emit these as
/// records are created / updated / deleted in the underlying store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EntityChange {
    pub id: EntityId,
    pub kind: EntityChangeKind,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EntityChangeKind {
    Created,
    /// Field-level granularity when the adapter can compute it
    /// (Attio's webhook payload names changed attributes); empty
    /// list when the adapter only knows "something changed"
    /// (Sheets polling sees row-level diffs).
    Updated {
        fields: Vec<String>,
    },
    Deleted,
}
