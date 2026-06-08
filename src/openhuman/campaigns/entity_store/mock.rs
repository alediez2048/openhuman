//! In-memory [`EntityStore`] for tests + the F4-7/F4-9 executor
//! prototypes that need a working entity surface before the
//! Sheets/Attio adapters land.
//!
//! Stores records in a `parking_lot::RwLock<Vec<EntityRecord>>` so
//! tests can seed + mutate without async machinery. Schema is
//! either explicitly provided or derived from the union of fields
//! across seeded records.
//!
//! Not exposed to the production registry — `open_entity_store`
//! never returns a `MockEntityStore`. Callers that need it import
//! it directly from `super::mock` (cfg-test only).

use anyhow::Result;
use async_trait::async_trait;
use chrono::Utc;
use parking_lot::RwLock;
use serde_json::Value;
use std::sync::Arc;

use super::types::{
    EntityChangeKind, EntityFieldKind, EntityFieldSchema, EntityFilter, EntityFilterOp, EntityId,
    EntityPatch, EntityQuery, EntityRecord, EntitySchema,
};
use super::{EntityChange, EntityChangeStream, EntityStore};

/// In-memory adapter. Owns its records under an `RwLock`; `subscribe`
/// returns a stream backed by a `tokio::sync::broadcast` channel so
/// `update`/`insert` calls during a test surface as `EntityChange`
/// events on every active subscriber.
pub struct MockEntityStore {
    records: Arc<RwLock<Vec<EntityRecord>>>,
    schema_override: Option<EntitySchema>,
    change_tx: tokio::sync::broadcast::Sender<EntityChange>,
}

impl Default for MockEntityStore {
    fn default() -> Self {
        Self::with_records(Vec::new())
    }
}

impl MockEntityStore {
    pub fn with_records(seed: Vec<EntityRecord>) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(64);
        Self {
            records: Arc::new(RwLock::new(seed)),
            schema_override: None,
            change_tx: tx,
        }
    }

    /// Pin an explicit schema instead of letting the mock derive one
    /// from the records.
    pub fn with_schema(mut self, schema: EntitySchema) -> Self {
        self.schema_override = Some(schema);
        self
    }

    /// Seed (or upsert) a record. Emits a `Created` change event so
    /// active subscribers observe the insert.
    pub fn insert(&self, record: EntityRecord) {
        let id = record.id.clone();
        let mut rows = self.records.write();
        if let Some(existing) = rows.iter_mut().find(|r| r.id == id) {
            *existing = record;
        } else {
            rows.push(record);
        }
        let _ = self.change_tx.send(EntityChange {
            id,
            kind: EntityChangeKind::Created,
            at: Utc::now(),
        });
    }
}

#[async_trait]
impl EntityStore for MockEntityStore {
    fn adapter_id(&self) -> &'static str {
        "mock"
    }

    async fn schema(&self) -> Result<EntitySchema> {
        if let Some(s) = self.schema_override.clone() {
            return Ok(s);
        }
        // Derive: union every field key across seeded records,
        // sorted alphabetically for determinism. Every field is
        // typed `Unknown` since the mock can't infer.
        let rows = self.records.read();
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for r in rows.iter() {
            for k in r.fields.keys() {
                keys.insert(k.clone());
            }
        }
        let fields = keys
            .into_iter()
            .map(|key| EntityFieldSchema {
                label: title_case(&key),
                key,
                kind: EntityFieldKind::Unknown,
                required: false,
            })
            .collect::<Vec<_>>();
        let primary_field = fields
            .first()
            .map(|f| f.key.clone())
            .unwrap_or_else(|| "id".to_string());
        Ok(EntitySchema {
            fields,
            primary_field,
        })
    }

    async fn list(&self, query: EntityQuery) -> Result<Vec<EntityRecord>> {
        let rows = self.records.read().clone();
        let filtered: Vec<EntityRecord> = rows
            .into_iter()
            .filter(|r| query.filters.iter().all(|f| matches_filter(r, f)))
            .collect();
        let offset = query.offset.unwrap_or(0) as usize;
        let limit = query.limit.unwrap_or(u32::MAX) as usize;
        Ok(filtered.into_iter().skip(offset).take(limit).collect())
    }

    async fn get(&self, id: &EntityId) -> Result<Option<EntityRecord>> {
        Ok(self.records.read().iter().find(|r| &r.id == id).cloned())
    }

    async fn update(&self, id: &EntityId, patch: EntityPatch) -> Result<()> {
        let mut rows = self.records.write();
        let Some(record) = rows.iter_mut().find(|r| &r.id == id) else {
            anyhow::bail!("mock_entity_store: id not found");
        };
        for (k, v) in patch.fields.iter() {
            record.fields.insert(k.clone(), v.clone());
        }
        record.updated_at = Some(Utc::now());
        let changed_fields: Vec<String> = patch.fields.keys().cloned().collect();
        let _ = self.change_tx.send(EntityChange {
            id: id.clone(),
            kind: EntityChangeKind::Updated {
                fields: changed_fields,
            },
            at: Utc::now(),
        });
        Ok(())
    }

    async fn subscribe(&self) -> Result<Option<EntityChangeStream>> {
        let rx = self.change_tx.subscribe();
        let stream = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(|res| {
            std::future::ready(match res {
                Ok(change) => Some(change),
                Err(_lag) => None, // skip lagged messages — best-effort
            })
        });
        Ok(Some(Box::pin(stream)))
    }
}

fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut next_upper = true;
    for ch in s.chars() {
        if ch == '_' || ch == '-' {
            out.push(' ');
            next_upper = true;
        } else if next_upper {
            for upper in ch.to_uppercase() {
                out.push(upper);
            }
            next_upper = false;
        } else {
            out.push(ch);
        }
    }
    out
}

fn matches_filter(record: &EntityRecord, filter: &EntityFilter) -> bool {
    let cell = record.fields.get(&filter.field);
    match (cell, filter.op) {
        (None, EntityFilterOp::IsNull) => true,
        (None, EntityFilterOp::IsNotNull) => false,
        (Some(Value::Null), EntityFilterOp::IsNull) => true,
        (Some(Value::Null), EntityFilterOp::IsNotNull) => false,
        (None, _) | (Some(Value::Null), _) => false,
        (Some(v), EntityFilterOp::IsNull) => matches!(v, Value::Null),
        (Some(v), EntityFilterOp::IsNotNull) => !matches!(v, Value::Null),
        (Some(v), EntityFilterOp::Eq) => v == &filter.value,
        (Some(v), EntityFilterOp::NotEq) => v != &filter.value,
        (Some(v), EntityFilterOp::Contains) => match (v.as_str(), filter.value.as_str()) {
            (Some(haystack), Some(needle)) => {
                haystack.to_lowercase().contains(&needle.to_lowercase())
            }
            _ => false,
        },
    }
}

use futures::StreamExt;
