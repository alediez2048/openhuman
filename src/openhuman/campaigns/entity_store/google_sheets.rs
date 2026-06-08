//! Google Sheets [`EntityStore`] adapter (F4-5).
//!
//! Reuses the existing Composio `GOOGLESHEETS` integration for auth +
//! transport — no separate OAuth dance. Sheet headers (row 1 of the
//! configured range) become field keys; subsequent rows are records;
//! `update` writes back via the Composio update-values action.
//!
//! ## Identity caveat
//!
//! Row-number identity is inherently fragile: if a user inserts a row
//! at the top of the sheet, every downstream `EntityId` shifts. The
//! adapter therefore ALSO writes the value of the `primary_field` (by
//! default the first column) onto each [`EntityRecord`] so callers
//! that need cross-run continuity (the F4-9 approval queue, the F4-7
//! `for_each` executor) can re-match by primary value rather than by
//! row number alone.
//!
//! ## Injectable Composio executor
//!
//! The adapter does NOT call `execute_composio_action_kind` directly.
//! Instead it depends on a small [`ComposioExecutor`] trait so tests
//! can drive the full read/list/update/subscribe flow against a
//! `FakeComposioExecutor` without standing up a backend session. The
//! [`LiveComposioExecutor`] production impl wraps the real
//! mode-aware client.

use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::interval;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::openhuman::composio::client::ComposioClientKind;
use crate::openhuman::composio::execute_dispatch::execute_composio_action_kind;
use crate::openhuman::composio::types::ComposioExecuteResponse;

use super::types::{
    EntityChangeKind, EntityFieldKind, EntityFieldSchema, EntityFilter, EntityFilterOp, EntityId,
    EntityPatch, EntityQuery, EntityRecord, EntitySchema,
};
use super::{EntityChange, EntityChangeStream, EntityStore};

pub const ADAPTER_ID: &str = "google_sheets";

/// Default polling cadence for [`GoogleSheetsAdapter::subscribe`]. The
/// Google Sheets REST surface gives us ~100 reads/100s/user — 30s
/// keeps us comfortably under that with headroom for the rest of the
/// app.
pub const DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(30);

const ACTION_GET_VALUES: &str = "GOOGLESHEETS_SPREADSHEETS_VALUES_GET";
const ACTION_UPDATE_VALUES: &str = "GOOGLESHEETS_SPREADSHEETS_VALUES_UPDATE";

// ── Injectable Composio executor ──────────────────────────────────

/// Narrow shim around Composio execute. The adapter never imports the
/// full client surface — it only needs `execute(tool, args)`. Tests
/// drop in a [`FakeComposioExecutor`] that returns canned responses
/// for the two tool slugs we care about.
#[async_trait]
pub trait ComposioExecutor: Send + Sync {
    async fn execute(&self, tool: &str, args: Value) -> Result<ComposioExecuteResponse>;
}

/// Production [`ComposioExecutor`] that routes through the real
/// `execute_composio_action_kind` dispatcher. Holds a
/// [`ComposioClientKind`] (already mode-resolved) + the entity id the
/// direct-mode variant should attribute calls to.
pub struct LiveComposioExecutor {
    kind: parking_lot::Mutex<ComposioClientKindClone>,
    entity_id: String,
}

/// `ComposioClientKind` doesn't impl `Clone` — `Direct(Arc<...>)`
/// would be cheap but `Backend(ComposioClient)` carries reqwest
/// machinery we don't want to deep-clone. We wrap it in a small
/// helper that exposes a `take_for_call` so each execute call
/// borrows the live kind without cloning. Since the dispatcher needs
/// to MOVE the kind (it consumes it by value), we swap it out, run
/// the call, and put it back.
struct ComposioClientKindClone(Option<ComposioClientKind>);

impl LiveComposioExecutor {
    pub fn new(kind: ComposioClientKind, entity_id: impl Into<String>) -> Self {
        Self {
            kind: parking_lot::Mutex::new(ComposioClientKindClone(Some(kind))),
            entity_id: entity_id.into(),
        }
    }
}

#[async_trait]
impl ComposioExecutor for LiveComposioExecutor {
    async fn execute(&self, tool: &str, args: Value) -> Result<ComposioExecuteResponse> {
        // The dispatcher takes `ComposioClientKind` by value. We
        // briefly take it out of the mutex, run the async call (no
        // lock held across .await), then put it back. Single-threaded
        // per-adapter so contention is none.
        let kind = {
            let mut guard = self.kind.lock();
            guard
                .0
                .take()
                .ok_or_else(|| anyhow!("composio kind already in use"))?
        };
        let res = execute_composio_action_kind(
            clone_kind_for_call(&kind),
            tool,
            Some(args),
            &self.entity_id,
        )
        .await;
        {
            let mut guard = self.kind.lock();
            guard.0 = Some(kind);
        }
        res.map_err(|msg| anyhow!("composio[{tool}]: {msg}"))
    }
}

/// `ComposioClientKind` has an internal `Direct(Arc<...>)` that IS
/// cheap to clone, and `Backend(ComposioClient)` where the inner
/// `ComposioClient` does impl Clone (its inner is Arc-wrapped). So
/// this is a fast, shallow clone in practice — only needed because
/// the trait we wrap takes the kind by value.
fn clone_kind_for_call(kind: &ComposioClientKind) -> ComposioClientKind {
    match kind {
        ComposioClientKind::Backend(c) => ComposioClientKind::Backend(c.clone()),
        ComposioClientKind::Direct(arc) => ComposioClientKind::Direct(Arc::clone(arc)),
    }
}

// ── Adapter ────────────────────────────────────────────────────────

/// In-memory snapshot of what we've seen on the sheet — used by
/// `subscribe` to detect new/changed rows between polls.
#[derive(Debug, Default, Clone)]
struct PollSnapshot {
    row_count: usize,
    /// Hash of the entire values matrix (cheap) — detects mutations
    /// to existing cells, not just appended rows.
    body_hash: u64,
}

pub struct GoogleSheetsAdapter {
    executor: Arc<dyn ComposioExecutor>,
    spreadsheet_id: String,
    range: String,
    poll_interval: Duration,
    schema_cache: Arc<RwLock<Option<EntitySchema>>>,
}

impl GoogleSheetsAdapter {
    pub fn new(
        executor: Arc<dyn ComposioExecutor>,
        spreadsheet_id: impl Into<String>,
        range: impl Into<String>,
    ) -> Self {
        Self {
            executor,
            spreadsheet_id: spreadsheet_id.into(),
            range: range.into(),
            poll_interval: DEFAULT_POLL_INTERVAL,
            schema_cache: Arc::new(RwLock::new(None)),
        }
    }

    pub fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    fn entity_id(&self, row_number: usize) -> EntityId {
        // Row numbers are 1-indexed in A1 notation and match what the
        // user sees in the Sheets UI. Row 1 is the header so data
        // rows start at 2.
        EntityId::new(
            ADAPTER_ID,
            format!("{}:row:{}", self.spreadsheet_id, row_number),
        )
    }

    async fn fetch_values(&self) -> Result<Vec<Vec<Value>>> {
        let resp = self
            .executor
            .execute(
                ACTION_GET_VALUES,
                json!({
                    "spreadsheetId": self.spreadsheet_id,
                    "range": self.range,
                }),
            )
            .await?;
        if !resp.successful {
            return Err(anyhow!(
                "google_sheets: {} failed: {}",
                ACTION_GET_VALUES,
                resp.error.unwrap_or_else(|| "(no error message)".into())
            ));
        }
        // Composio wraps the upstream Google response under `data`.
        // Real shape: `{ "values": [[...], [...]], "range": "...", "majorDimension": "ROWS" }`
        // (often wrapped under `data.response_data` by the backend).
        // We tolerate either the un-nested or `response_data`-nested form.
        let values = resp
            .data
            .get("values")
            .or_else(|| resp.data.get("response_data").and_then(|d| d.get("values")))
            .cloned()
            .unwrap_or_else(|| Value::Array(Vec::new()));
        let rows = values
            .as_array()
            .ok_or_else(|| anyhow!("google_sheets: values is not an array"))?
            .iter()
            .map(|row| {
                row.as_array()
                    .cloned()
                    .unwrap_or_else(|| vec![Value::Null])
            })
            .collect();
        Ok(rows)
    }

    fn derive_schema(&self, rows: &[Vec<Value>]) -> EntitySchema {
        let headers = rows.first().cloned().unwrap_or_default();
        let header_keys: Vec<String> = headers
            .iter()
            .map(|v| v.as_str().unwrap_or("").trim().to_string())
            .collect();

        // For type inference, walk the first non-empty data row per
        // column. We don't scan every row — heuristic only.
        let first_data_row = rows.get(1);
        let fields = header_keys
            .iter()
            .enumerate()
            .map(|(i, key)| {
                let sample = first_data_row
                    .and_then(|row| row.get(i))
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                EntityFieldSchema {
                    key: if key.is_empty() {
                        format!("column_{}", i + 1)
                    } else {
                        key.clone()
                    },
                    label: humanize_header(key),
                    kind: infer_field_kind(sample),
                    required: i == 0, // first column is conventionally the primary
                }
            })
            .collect::<Vec<_>>();

        let primary_field = fields
            .iter()
            .find(|f| matches!(f.kind, EntityFieldKind::EmailAddress))
            .or_else(|| fields.first())
            .map(|f| f.key.clone())
            .unwrap_or_else(|| "column_1".to_string());

        EntitySchema {
            fields,
            primary_field,
        }
    }

    fn rows_to_records(&self, rows: &[Vec<Value>], schema: &EntitySchema) -> Vec<EntityRecord> {
        let header_keys: Vec<String> = schema.fields.iter().map(|f| f.key.clone()).collect();
        rows.iter()
            .enumerate()
            .skip(1) // row 0 is the header
            .map(|(idx, row)| {
                let mut fields = Map::new();
                for (i, key) in header_keys.iter().enumerate() {
                    let cell = row.get(i).cloned().unwrap_or(Value::Null);
                    fields.insert(key.clone(), cell);
                }
                EntityRecord {
                    id: self.entity_id(idx + 1), // +1 because sheet rows are 1-indexed
                    fields,
                    updated_at: None,
                }
            })
            .collect()
    }
}

#[async_trait]
impl EntityStore for GoogleSheetsAdapter {
    fn adapter_id(&self) -> &'static str {
        ADAPTER_ID
    }

    async fn schema(&self) -> Result<EntitySchema> {
        if let Some(cached) = self.schema_cache.read().clone() {
            return Ok(cached);
        }
        let rows = self.fetch_values().await?;
        let schema = self.derive_schema(&rows);
        *self.schema_cache.write() = Some(schema.clone());
        Ok(schema)
    }

    async fn list(&self, query: EntityQuery) -> Result<Vec<EntityRecord>> {
        let rows = self.fetch_values().await?;
        let schema = self.derive_schema(&rows);
        // Refresh cache while we're here.
        *self.schema_cache.write() = Some(schema.clone());
        let mut records = self.rows_to_records(&rows, &schema);
        records.retain(|r| query.filters.iter().all(|f| matches_filter(r, f)));
        let offset = query.offset.unwrap_or(0) as usize;
        let limit = query.limit.unwrap_or(u32::MAX) as usize;
        Ok(records.into_iter().skip(offset).take(limit).collect())
    }

    async fn get(&self, id: &EntityId) -> Result<Option<EntityRecord>> {
        let row_number = parse_row_number(&id.native)
            .with_context(|| format!("google_sheets: invalid entity id {}", id.native))?;
        let rows = self.fetch_values().await?;
        let schema = self.derive_schema(&rows);
        let records = self.rows_to_records(&rows, &schema);
        Ok(records.into_iter().find(|r| {
            parse_row_number(&r.id.native).is_ok_and(|rn| rn == row_number)
        }))
    }

    async fn update(&self, id: &EntityId, patch: EntityPatch) -> Result<()> {
        let row_number = parse_row_number(&id.native)
            .with_context(|| format!("google_sheets: invalid entity id {}", id.native))?;
        let schema = self.schema().await?;
        let header_keys: Vec<String> = schema.fields.iter().map(|f| f.key.clone()).collect();

        // We need to read the existing row to preserve untouched
        // cells — Sheets `UPDATE_VALUES` is range-replace, not
        // cell-merge. Read once, patch in-memory, write back.
        let rows = self.fetch_values().await?;
        let target_row_idx = row_number.saturating_sub(1);
        let mut existing = rows
            .get(target_row_idx)
            .cloned()
            .ok_or_else(|| anyhow!("google_sheets: row {row_number} not found"))?;
        // Pad to header width.
        while existing.len() < header_keys.len() {
            existing.push(Value::Null);
        }
        for (i, key) in header_keys.iter().enumerate() {
            if let Some(new_value) = patch.fields.get(key) {
                existing[i] = new_value.clone();
            }
        }

        // Build the A1 range for just this row. e.g. `Sheet1!A5:H5`
        // for a 8-column sheet, row 5. We parse the sheet name out of
        // the configured range; if there's no `!`, default to the
        // first sheet (Sheets treats a bare range as the active one).
        let sheet_name = self.range.split('!').next().unwrap_or("Sheet1");
        let last_col = column_letter(header_keys.len());
        let single_row_range = format!("{}!A{row_number}:{last_col}{row_number}", sheet_name);

        let resp = self
            .executor
            .execute(
                ACTION_UPDATE_VALUES,
                json!({
                    "spreadsheetId": self.spreadsheet_id,
                    "range": single_row_range,
                    "valueInputOption": "USER_ENTERED",
                    "values": [existing],
                }),
            )
            .await?;
        if !resp.successful {
            return Err(anyhow!(
                "google_sheets: update failed: {}",
                resp.error.unwrap_or_else(|| "(no error message)".into())
            ));
        }
        Ok(())
    }

    async fn subscribe(&self) -> Result<Option<EntityChangeStream>> {
        let executor = Arc::clone(&self.executor);
        let spreadsheet_id = self.spreadsheet_id.clone();
        let range = self.range.clone();
        let poll = self.poll_interval;

        let (tx, rx) = mpsc::unbounded_channel::<EntityChange>();
        tokio::spawn(async move {
            let mut snap = PollSnapshot::default();
            let mut ticker = interval(poll);
            // First tick fires immediately — establish a baseline
            // without emitting anything.
            ticker.tick().await;
            if let Ok(rows) = fetch_for_poll(&*executor, &spreadsheet_id, &range).await {
                snap = snapshot_for(&rows);
            }
            loop {
                ticker.tick().await;
                let rows = match fetch_for_poll(&*executor, &spreadsheet_id, &range).await {
                    Ok(r) => r,
                    Err(_) => continue, // best-effort polling
                };
                let next = snapshot_for(&rows);
                if next.row_count > snap.row_count {
                    // PollSnapshot::row_count tracks *data* rows
                    // (excludes the header). Sheet row N = data row
                    // N + 1 since row 1 is the header.
                    for data_row_idx in (snap.row_count + 1)..=next.row_count {
                        let sheet_row = data_row_idx + 1;
                        let ev = EntityChange {
                            id: EntityId::new(
                                ADAPTER_ID,
                                format!("{spreadsheet_id}:row:{sheet_row}"),
                            ),
                            kind: EntityChangeKind::Created,
                            at: Utc::now(),
                        };
                        if tx.send(ev).is_err() {
                            return;
                        }
                    }
                } else if next.body_hash != snap.body_hash {
                    // Cell value changed on an existing row — emit a
                    // batch Updated without per-field diff (Sheets
                    // doesn't tell us which cell changed cheaply).
                    let ev = EntityChange {
                        id: EntityId::new(ADAPTER_ID, format!("{spreadsheet_id}:*")),
                        kind: EntityChangeKind::Updated { fields: Vec::new() },
                        at: Utc::now(),
                    };
                    if tx.send(ev).is_err() {
                        return;
                    }
                }
                snap = next;
            }
        });

        Ok(Some(Box::pin(UnboundedReceiverStream::new(rx))))
    }
}

async fn fetch_for_poll(
    executor: &dyn ComposioExecutor,
    spreadsheet_id: &str,
    range: &str,
) -> Result<Vec<Vec<Value>>> {
    let resp = executor
        .execute(
            ACTION_GET_VALUES,
            json!({ "spreadsheetId": spreadsheet_id, "range": range }),
        )
        .await?;
    if !resp.successful {
        return Err(anyhow!("poll: get_values failed"));
    }
    let values = resp
        .data
        .get("values")
        .or_else(|| resp.data.get("response_data").and_then(|d| d.get("values")))
        .cloned()
        .unwrap_or_else(|| Value::Array(Vec::new()));
    Ok(values
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|row| row.as_array().cloned().unwrap_or_default())
                .collect()
        })
        .unwrap_or_default())
}

fn snapshot_for(rows: &[Vec<Value>]) -> PollSnapshot {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    // Hash the entire serialized form — stable across runs because
    // serde_json::to_string yields deterministic output for arrays
    // of plain JSON values.
    let s = serde_json::to_string(rows).unwrap_or_default();
    s.hash(&mut h);
    PollSnapshot {
        row_count: rows.len().saturating_sub(1), // exclude header
        body_hash: h.finish(),
    }
}

// ── Helpers ────────────────────────────────────────────────────────

/// Parse the trailing `:row:N` from an entity id native string.
fn parse_row_number(native: &str) -> Result<usize> {
    let suffix = native
        .rsplit(":row:")
        .next()
        .filter(|s| !s.is_empty() && *s != native)
        .ok_or_else(|| anyhow!("missing :row:N suffix"))?;
    suffix
        .parse::<usize>()
        .map_err(|e| anyhow!("row number not numeric: {e}"))
}

/// Convert a 1-indexed column count to a Sheets A1-notation letter
/// (1 → "A", 26 → "Z", 27 → "AA", 52 → "AZ"). We never exceed 256
/// columns in practice but handle two-letter columns correctly.
pub(crate) fn column_letter(n: usize) -> String {
    if n == 0 {
        return "A".to_string();
    }
    let mut s = String::new();
    let mut n = n;
    while n > 0 {
        let rem = (n - 1) % 26;
        s.insert(0, (b'A' + rem as u8) as char);
        n = (n - 1) / 26;
    }
    s
}

fn humanize_header(raw: &str) -> String {
    let raw = raw.trim();
    if raw.is_empty() {
        return "Untitled".to_string();
    }
    // Spreadsheet headers are already human-readable most of the
    // time — just snake-case underscores to spaces + title-case the
    // first letter of each word.
    raw.replace('_', " ")
        .split_whitespace()
        .map(|w| {
            let mut chars = w.chars();
            match chars.next() {
                Some(c) => c.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Heuristic field-type classification from a sample cell value.
///
/// Strict in order: email > url > ISO date > phone > number > bool >
/// String. Date comes before phone because dates like `"2026-05-26"`
/// match phone's "digits + dashes" rule but are obviously dates.
/// Empty samples default to `Unknown` so the UI can mark them as
/// such and the F4-7 executor can decline to write into them without
/// more info.
pub(crate) fn infer_field_kind(sample: &str) -> EntityFieldKind {
    let s = sample.trim();
    if s.is_empty() {
        return EntityFieldKind::Unknown;
    }
    if looks_like_email(s) {
        return EntityFieldKind::EmailAddress;
    }
    if looks_like_url(s) {
        return EntityFieldKind::Url;
    }
    if looks_like_iso_date(s) {
        return EntityFieldKind::DateTime;
    }
    if looks_like_phone(s) {
        return EntityFieldKind::PhoneNumber;
    }
    if s.parse::<f64>().is_ok() {
        return EntityFieldKind::Number;
    }
    if looks_like_bool(s) {
        return EntityFieldKind::Bool;
    }
    EntityFieldKind::String
}

fn looks_like_email(s: &str) -> bool {
    // Crude but stable: exactly one `@`, at least one char before
    // and after, and a `.` somewhere in the domain.
    let mut iter = s.split('@');
    let local = iter.next().unwrap_or("");
    let domain = iter.next().unwrap_or("");
    if iter.next().is_some() {
        return false;
    }
    !local.is_empty() && domain.contains('.') && !domain.starts_with('.')
}

fn looks_like_url(s: &str) -> bool {
    s.starts_with("http://") || s.starts_with("https://")
}

fn looks_like_phone(s: &str) -> bool {
    // E.164-ish: starts with `+` followed by 7-15 digits, or 7+ raw
    // digits with optional separators.
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 7 || digits.len() > 15 {
        return false;
    }
    let allowed = |c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | ' ' | '(' | ')' | '.');
    s.chars().all(allowed)
}

fn looks_like_iso_date(s: &str) -> bool {
    DateTime::parse_from_rfc3339(s).is_ok()
        || chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
        || chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").is_ok()
}

fn looks_like_bool(s: &str) -> bool {
    matches!(
        s.to_ascii_lowercase().as_str(),
        "true" | "false" | "yes" | "no" | "y" | "n"
    )
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
        (Some(v), EntityFilterOp::Eq) => values_equal(v, &filter.value),
        (Some(v), EntityFilterOp::NotEq) => !values_equal(v, &filter.value),
        (Some(v), EntityFilterOp::Contains) => match (v.as_str(), filter.value.as_str()) {
            (Some(haystack), Some(needle)) => haystack
                .to_lowercase()
                .contains(&needle.to_lowercase()),
            _ => false,
        },
    }
}

/// String-permissive equality: Sheets returns everything as strings,
/// but callers may pass json numbers/bools. Compare by string form
/// when both sides are scalars.
fn values_equal(a: &Value, b: &Value) -> bool {
    if a == b {
        return true;
    }
    let to_s = |v: &Value| match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    };
    matches!((to_s(a), to_s(b)), (Some(x), Some(y)) if x == y)
}
