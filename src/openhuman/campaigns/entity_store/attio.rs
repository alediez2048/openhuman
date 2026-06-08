//! Attio [`EntityStore`] adapter (F4-6).
//!
//! Second concrete adapter — typed CRM-shaped records, stable
//! record UUIDs, real server-side filtering, and (eventually) push
//! subscriptions via Attio webhooks. Reuses the
//! [`super::google_sheets::ComposioExecutor`] trait + the live
//! mode-aware Composio client.
//!
//! ## Subscribe path
//!
//! F4-6 ships the polling subscribe (the same shape as F4-5 Sheets)
//! as the live path. The webhook-driven subscribe needs two
//! cross-domain plumbing pieces:
//!
//! 1. An OpenHuman-managed tunnel URL Attio can POST into. F2-9
//!    built this for `Trigger::Webhook` — wiring a campaign-scoped
//!    variant here is mostly mechanical but lives outside this
//!    ticket's blast radius.
//! 2. A `DomainEvent::EntityChanged` bus message published when an
//!    inbound webhook is verified — so the adapter's `subscribe`
//!    stream picks it up regardless of which adapter instance is
//!    live. Same shape as `DomainEvent::ChannelInbound` for
//!    channels.
//!
//! [`AttioWebhookHelper::register`] and [`verify_attio_signature`]
//! are present and tested so adopting the webhook path later is a
//! pure wiring change — the cryptographic + registration surface is
//! already in place.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use hmac::{Hmac, Mac};
use parking_lot::RwLock;
use serde_json::{json, Map, Value};
use sha2::Sha256;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_stream::wrappers::UnboundedReceiverStream;

use super::google_sheets::ComposioExecutor;
use super::types::{
    EntityChangeKind, EntityFieldKind, EntityFieldSchema, EntityFilter, EntityFilterOp, EntityId,
    EntityPatch, EntityQuery, EntityRecord, EntitySchema,
};
use super::{EntityChange, EntityChangeStream, EntityStore};

pub const ADAPTER_ID: &str = "attio";

/// Default polling cadence — Attio's REST surface allows ~600
/// req/min/workspace; 30s gives us headroom across multiple active
/// adapters.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

// Composio action slugs. The Attio integration exposes these
// directly; the adapter never assembles raw HTTP requests itself.
const ACTION_LIST_ATTRIBUTES: &str = "ATTIO_LIST_ATTRIBUTES";
const ACTION_QUERY_RECORDS: &str = "ATTIO_QUERY_RECORDS";
const ACTION_GET_RECORD: &str = "ATTIO_GET_RECORD";
const ACTION_UPDATE_RECORD: &str = "ATTIO_UPDATE_RECORD";
const ACTION_CREATE_WEBHOOK: &str = "ATTIO_CREATE_WEBHOOK";
const ACTION_DELETE_WEBHOOK: &str = "ATTIO_DELETE_WEBHOOK";

// ── Adapter ────────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
struct PollSnapshot {
    /// Per-record-id checksum so we can tell when any record was
    /// mutated (not just new ones).
    record_hashes: std::collections::BTreeMap<String, u64>,
}

pub struct AttioAdapter {
    executor: Arc<dyn ComposioExecutor>,
    workspace_id: String,
    object_type: String,
    poll_interval: Duration,
    schema_cache: Arc<RwLock<Option<EntitySchema>>>,
}

impl AttioAdapter {
    pub fn new(
        executor: Arc<dyn ComposioExecutor>,
        workspace_id: impl Into<String>,
        object_type: impl Into<String>,
    ) -> Self {
        Self {
            executor,
            workspace_id: workspace_id.into(),
            object_type: object_type.into(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            schema_cache: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    fn entity_id(&self, record_id: &str) -> EntityId {
        EntityId::new(ADAPTER_ID, record_id)
    }

    async fn fetch_records(&self, filters: &[EntityFilter]) -> Result<Vec<Value>> {
        let query = json!({
            "workspaceId": self.workspace_id,
            "objectType": self.object_type,
            "filter": translate_filters(filters),
        });
        let resp = self.executor.execute(ACTION_QUERY_RECORDS, query).await?;
        if !resp.successful {
            return Err(anyhow!(
                "attio: {} failed: {}",
                ACTION_QUERY_RECORDS,
                resp.error.unwrap_or_else(|| "(no error message)".into())
            ));
        }
        // Attio wraps records under `data` (top-level array). Composio
        // typically re-wraps as `{ data: { data: [...] } }` or
        // surfaces directly as `data: { records: [...] }` depending
        // on the action's binding — tolerate both.
        let records = resp
            .data
            .get("data")
            .or_else(|| resp.data.get("records"))
            .or_else(|| resp.data.get("response_data").and_then(|d| d.get("data")))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        Ok(records.as_array().cloned().unwrap_or_default())
    }
}

#[async_trait]
impl EntityStore for AttioAdapter {
    fn adapter_id(&self) -> &'static str {
        ADAPTER_ID
    }

    async fn schema(&self) -> Result<EntitySchema> {
        if let Some(cached) = self.schema_cache.read().clone() {
            return Ok(cached);
        }
        let resp = self
            .executor
            .execute(
                ACTION_LIST_ATTRIBUTES,
                json!({
                    "workspaceId": self.workspace_id,
                    "objectType": self.object_type,
                }),
            )
            .await?;
        if !resp.successful {
            return Err(anyhow!(
                "attio: {} failed: {}",
                ACTION_LIST_ATTRIBUTES,
                resp.error.unwrap_or_else(|| "(no error message)".into())
            ));
        }

        let attrs_value = resp
            .data
            .get("data")
            .or_else(|| resp.data.get("attributes"))
            .or_else(|| resp.data.get("response_data").and_then(|d| d.get("data")))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let attrs = attrs_value.as_array().cloned().unwrap_or_default();
        let fields = attrs
            .iter()
            .filter_map(|attr| {
                let api_slug = attr
                    .get("api_slug")
                    .or_else(|| attr.get("slug"))
                    .or_else(|| attr.get("id"))
                    .and_then(|v| v.as_str())?;
                let title = attr
                    .get("title")
                    .or_else(|| attr.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or(api_slug);
                let kind = attr
                    .get("type")
                    .or_else(|| attr.get("kind"))
                    .and_then(|v| v.as_str())
                    .map(map_attio_type)
                    .unwrap_or(EntityFieldKind::Unknown);
                let required = attr
                    .get("is_required")
                    .or_else(|| attr.get("required"))
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);
                Some(EntityFieldSchema {
                    key: api_slug.to_string(),
                    label: title.to_string(),
                    kind,
                    required,
                })
            })
            .collect::<Vec<_>>();

        // Attio records always carry a stable `record_id` UUID — we
        // use it as the primary field for cross-run continuity.
        let primary_field = fields
            .iter()
            .find(|f| f.key == "record_id")
            .or_else(|| fields.iter().find(|f| f.required))
            .or_else(|| fields.first())
            .map(|f| f.key.clone())
            .unwrap_or_else(|| "record_id".to_string());

        let schema = EntitySchema {
            fields,
            primary_field,
        };
        *self.schema_cache.write() = Some(schema.clone());
        Ok(schema)
    }

    async fn list(&self, query: EntityQuery) -> Result<Vec<EntityRecord>> {
        let records = self.fetch_records(&query.filters).await?;
        let mut out: Vec<EntityRecord> = records
            .into_iter()
            .filter_map(|r| parse_attio_record(self, &r))
            .collect();
        let offset = query.offset.unwrap_or(0) as usize;
        let limit = query.limit.unwrap_or(u32::MAX) as usize;
        out = out.into_iter().skip(offset).take(limit).collect();
        Ok(out)
    }

    async fn get(&self, id: &EntityId) -> Result<Option<EntityRecord>> {
        let resp = self
            .executor
            .execute(
                ACTION_GET_RECORD,
                json!({
                    "workspaceId": self.workspace_id,
                    "objectType": self.object_type,
                    "recordId": id.native,
                }),
            )
            .await?;
        if !resp.successful {
            // Attio surfaces a 404 as `successful: false` — treat that
            // as Ok(None) so callers don't need to inspect error text.
            let err = resp.error.unwrap_or_default();
            if err.contains("not found") || err.contains("404") {
                return Ok(None);
            }
            return Err(anyhow!("attio: {} failed: {}", ACTION_GET_RECORD, err));
        }
        let body = resp
            .data
            .get("data")
            .or_else(|| resp.data.get("record"))
            .cloned()
            .unwrap_or(resp.data);
        Ok(parse_attio_record(self, &body))
    }

    async fn update(&self, id: &EntityId, patch: EntityPatch) -> Result<()> {
        let resp = self
            .executor
            .execute(
                ACTION_UPDATE_RECORD,
                json!({
                    "workspaceId": self.workspace_id,
                    "objectType": self.object_type,
                    "recordId": id.native,
                    "data": { "values": patch.fields },
                }),
            )
            .await?;
        if !resp.successful {
            return Err(anyhow!(
                "attio: {} failed: {}",
                ACTION_UPDATE_RECORD,
                resp.error.unwrap_or_else(|| "(no error message)".into())
            ));
        }
        Ok(())
    }

    async fn subscribe(&self) -> Result<Option<EntityChangeStream>> {
        // Polling-based subscribe. Webhook-driven subscribe is the
        // intended upgrade — see module-level doc note. The trait
        // contract says `Ok(None)` means "no subscription"; we return
        // an active polling stream so callers don't fall back to
        // their own polling unnecessarily.
        let executor = Arc::clone(&self.executor);
        let workspace_id = self.workspace_id.clone();
        let object_type = self.object_type.clone();
        let poll = self.poll_interval;

        let (tx, rx) = mpsc::unbounded_channel::<EntityChange>();
        tokio::spawn(async move {
            let mut snap = PollSnapshot::default();
            let mut ticker = interval(poll);
            // First tick is immediate — used to establish baseline.
            ticker.tick().await;
            if let Ok(records) = fetch_for_poll(&*executor, &workspace_id, &object_type).await {
                snap = snapshot_for(&records);
            }
            loop {
                ticker.tick().await;
                let records = match fetch_for_poll(&*executor, &workspace_id, &object_type).await {
                    Ok(r) => r,
                    Err(_) => continue,
                };
                let next = snapshot_for(&records);
                // Emit Created for newly-seen record ids.
                for (id, _) in next.record_hashes.iter() {
                    if !snap.record_hashes.contains_key(id) {
                        let ev = EntityChange {
                            id: EntityId::new(ADAPTER_ID, id.clone()),
                            kind: EntityChangeKind::Created,
                            at: Utc::now(),
                        };
                        if tx.send(ev).is_err() {
                            return;
                        }
                    }
                }
                // Emit Updated for ids whose hash changed.
                for (id, hash) in next.record_hashes.iter() {
                    if let Some(prev) = snap.record_hashes.get(id) {
                        if prev != hash {
                            let ev = EntityChange {
                                id: EntityId::new(ADAPTER_ID, id.clone()),
                                kind: EntityChangeKind::Updated { fields: Vec::new() },
                                at: Utc::now(),
                            };
                            if tx.send(ev).is_err() {
                                return;
                            }
                        }
                    }
                }
                // Emit Deleted for ids that disappeared.
                for id in snap.record_hashes.keys() {
                    if !next.record_hashes.contains_key(id) {
                        let ev = EntityChange {
                            id: EntityId::new(ADAPTER_ID, id.clone()),
                            kind: EntityChangeKind::Deleted,
                            at: Utc::now(),
                        };
                        if tx.send(ev).is_err() {
                            return;
                        }
                    }
                }
                snap = next;
            }
        });
        Ok(Some(Box::pin(UnboundedReceiverStream::new(rx))))
    }
}

fn parse_attio_record(adapter: &AttioAdapter, raw: &Value) -> Option<EntityRecord> {
    // Attio records carry an `id.record_id` UUID as the identity. Be
    // permissive about shape: tolerate `id: "uuid"` shorthand too.
    let record_id = raw
        .get("id")
        .and_then(|v| {
            if let Some(s) = v.as_str() {
                Some(s.to_string())
            } else {
                v.get("record_id")
                    .and_then(|x| x.as_str())
                    .map(String::from)
            }
        })
        .or_else(|| {
            raw.get("record_id")
                .and_then(|v| v.as_str())
                .map(String::from)
        })?;
    // Attio stores field values under `values` keyed by api_slug.
    // Fall back to top-level fields for tests that mirror a simpler
    // shape.
    let mut fields = Map::new();
    if let Some(values) = raw.get("values").and_then(|v| v.as_object()) {
        for (k, v) in values {
            fields.insert(k.clone(), simplify_attio_value(v));
        }
    } else if let Some(obj) = raw.as_object() {
        for (k, v) in obj {
            if k != "id" && k != "record_id" {
                fields.insert(k.clone(), v.clone());
            }
        }
    }
    Some(EntityRecord {
        id: adapter.entity_id(&record_id),
        fields,
        updated_at: None,
    })
}

/// Attio returns each cell as an array of typed value objects:
/// `[{ value: "alice@acme.io", attribute_type: "email-address" }, ...]`.
/// Flatten to a single value when the array has one element, else
/// keep the array of `value` projections.
fn simplify_attio_value(v: &Value) -> Value {
    if let Some(arr) = v.as_array() {
        let simple: Vec<Value> = arr
            .iter()
            .map(|item| item.get("value").cloned().unwrap_or_else(|| item.clone()))
            .collect();
        if simple.len() == 1 {
            simple.into_iter().next().unwrap_or(Value::Null)
        } else {
            Value::Array(simple)
        }
    } else {
        v.clone()
    }
}

async fn fetch_for_poll(
    executor: &dyn ComposioExecutor,
    workspace_id: &str,
    object_type: &str,
) -> Result<Vec<Value>> {
    let resp = executor
        .execute(
            ACTION_QUERY_RECORDS,
            json!({
                "workspaceId": workspace_id,
                "objectType": object_type,
                "filter": {},
            }),
        )
        .await?;
    if !resp.successful {
        return Err(anyhow!("poll: query failed"));
    }
    let records = resp
        .data
        .get("data")
        .or_else(|| resp.data.get("records"))
        .or_else(|| resp.data.get("response_data").and_then(|d| d.get("data")))
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    Ok(records.as_array().cloned().unwrap_or_default())
}

fn snapshot_for(records: &[Value]) -> PollSnapshot {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut map = std::collections::BTreeMap::new();
    for r in records {
        let id = r
            .get("id")
            .and_then(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else {
                    v.get("record_id")
                        .and_then(|x| x.as_str())
                        .map(String::from)
                }
            })
            .or_else(|| {
                r.get("record_id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
            });
        if let Some(id) = id {
            let mut h = DefaultHasher::new();
            serde_json::to_string(r).unwrap_or_default().hash(&mut h);
            map.insert(id, h.finish());
        }
    }
    PollSnapshot { record_hashes: map }
}

// ── Filter translation ────────────────────────────────────────────

/// Translate a list of [`EntityFilter`]s into Attio's query DSL.
///
/// Returns the JSON shape Attio's `POST /v2/objects/{type}/records/query`
/// expects under its `filter` field. The trait's small ops set maps
/// cleanly onto Attio's `$eq` / `$not` / `$contains` / `$is_null` /
/// `$is_not_null` operators. Multiple filters AND together at the top
/// level.
pub fn translate_filters(filters: &[EntityFilter]) -> Value {
    if filters.is_empty() {
        return json!({});
    }
    let mut clauses = Vec::with_capacity(filters.len());
    for f in filters {
        clauses.push(translate_one(f));
    }
    if clauses.len() == 1 {
        clauses.into_iter().next().unwrap_or_else(|| json!({}))
    } else {
        json!({ "$and": clauses })
    }
}

fn translate_one(filter: &EntityFilter) -> Value {
    match filter.op {
        EntityFilterOp::Eq => json!({ &filter.field: { "$eq": filter.value } }),
        EntityFilterOp::NotEq => json!({ &filter.field: { "$not": { "$eq": filter.value } } }),
        EntityFilterOp::Contains => {
            json!({ &filter.field: { "$contains": filter.value } })
        }
        EntityFilterOp::IsNull => json!({ &filter.field: { "$is_empty": true } }),
        EntityFilterOp::IsNotNull => json!({ &filter.field: { "$is_not_empty": true } }),
    }
}

// ── Type mapping ──────────────────────────────────────────────────

fn map_attio_type(attio_kind: &str) -> EntityFieldKind {
    match attio_kind {
        "text" | "string" | "rich-text" => EntityFieldKind::String,
        "number" | "currency" | "rating" => EntityFieldKind::Number,
        "checkbox" | "boolean" => EntityFieldKind::Bool,
        "date" | "timestamp" => EntityFieldKind::DateTime,
        "email-address" | "email" => EntityFieldKind::EmailAddress,
        "phone-number" | "phone" => EntityFieldKind::PhoneNumber,
        "url" | "domain" => EntityFieldKind::Url,
        "select" | "status" => EntityFieldKind::Enum {
            variants: Vec::new(),
        },
        _ => EntityFieldKind::Unknown,
    }
}

// ── Webhook helpers (HMAC + registration) ──────────────────────────

/// Verify an Attio webhook signature using HMAC-SHA256.
///
/// Attio signs each webhook payload by HMAC-SHA256 over the raw body
/// using a per-webhook signing secret returned at registration. The
/// expected signature header is `X-Attio-Signature` with the value
/// formatted as `sha256=<hex>`. This helper accepts either the raw
/// hex or the `sha256=` prefixed form.
///
/// Returns `true` if the body authenticates against the secret. Use
/// constant-time comparison to avoid leaking the secret via timing.
pub fn verify_attio_signature(secret: &str, body: &[u8], signature_header: &str) -> bool {
    let expected_hex = compute_attio_signature_hex(secret, body);
    let presented = signature_header
        .trim()
        .strip_prefix("sha256=")
        .unwrap_or(signature_header.trim());
    constant_time_eq_hex(&expected_hex, presented)
}

fn compute_attio_signature_hex(secret: &str, body: &[u8]) -> String {
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes())
        .expect("HMAC-SHA256 accepts any key length");
    mac.update(body);
    let bytes = mac.finalize().into_bytes();
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn constant_time_eq_hex(a: &str, b: &str) -> bool {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a_bytes.iter().zip(b_bytes.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Stand-alone webhook lifecycle helper. The live `subscribe` path
/// uses polling today; this helper exists so the F4-6.x follow-up
/// that wires the F2-9 tunnel pipeline can register + tear down
/// webhooks without re-discovering the Composio action shape.
pub struct AttioWebhookHelper {
    executor: Arc<dyn ComposioExecutor>,
    workspace_id: String,
    object_type: String,
}

impl AttioWebhookHelper {
    pub fn new(
        executor: Arc<dyn ComposioExecutor>,
        workspace_id: impl Into<String>,
        object_type: impl Into<String>,
    ) -> Self {
        Self {
            executor,
            workspace_id: workspace_id.into(),
            object_type: object_type.into(),
        }
    }

    /// Register a webhook that POSTs to `target_url` whenever a
    /// record changes. Returns the Attio webhook id (used for
    /// teardown).
    pub async fn register(&self, target_url: &str) -> Result<String> {
        let resp = self
            .executor
            .execute(
                ACTION_CREATE_WEBHOOK,
                json!({
                    "workspaceId": self.workspace_id,
                    "targetUrl": target_url,
                    "events": [
                        format!("{}.record.created", self.object_type),
                        format!("{}.record.updated", self.object_type),
                        format!("{}.record.deleted", self.object_type),
                    ],
                }),
            )
            .await?;
        if !resp.successful {
            return Err(anyhow!(
                "attio: {} failed: {}",
                ACTION_CREATE_WEBHOOK,
                resp.error.unwrap_or_else(|| "(no error message)".into())
            ));
        }
        let webhook_id = resp
            .data
            .get("id")
            .and_then(|v| v.as_str())
            .or_else(|| {
                resp.data
                    .get("data")
                    .and_then(|d| d.get("id").and_then(|v| v.as_str()))
            })
            .ok_or_else(|| anyhow!("attio: webhook registration response missing id"))?
            .to_string();
        Ok(webhook_id)
    }

    /// Tear down a previously-registered webhook by id.
    pub async fn teardown(&self, webhook_id: &str) -> Result<()> {
        let resp = self
            .executor
            .execute(
                ACTION_DELETE_WEBHOOK,
                json!({
                    "workspaceId": self.workspace_id,
                    "webhookId": webhook_id,
                }),
            )
            .await?;
        if !resp.successful {
            return Err(anyhow!(
                "attio: {} failed: {}",
                ACTION_DELETE_WEBHOOK,
                resp.error.unwrap_or_else(|| "(no error message)".into())
            ));
        }
        Ok(())
    }
}

// Re-exported for the registry constructor.
pub use super::google_sheets::LiveComposioExecutor;

/// Make the `Context` import live so error-attribution helpers don't
/// trip dead-code lints when used selectively above.
#[allow(dead_code)]
fn _ctx_anchor<T>(r: Result<T>) -> Result<T> {
    r.with_context(|| "attio adapter")
}
