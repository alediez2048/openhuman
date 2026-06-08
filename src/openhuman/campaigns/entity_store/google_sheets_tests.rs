//! Tests for the F4-5 Google Sheets adapter. Uses a `FakeComposioExecutor`
//! that records every execute call + returns canned responses, so the
//! full schema/list/get/update/subscribe flow runs without a live
//! backend or network.

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::google_sheets::{
    column_letter, infer_field_kind, ComposioExecutor, GoogleSheetsAdapter, ADAPTER_ID,
};
use super::types::{
    EntityFieldKind, EntityFilter, EntityFilterOp, EntityId, EntityPatch, EntityQuery,
};
use super::EntityStore;
use crate::openhuman::composio::types::ComposioExecuteResponse;

// ── FakeComposioExecutor ───────────────────────────────────────────

#[derive(Default)]
struct FakeComposioExecutor {
    /// Tool slug → list of canned responses (popped FIFO so multiple
    /// reads in a single test can return different snapshots).
    responses: Mutex<HashMap<String, Vec<ComposioExecuteResponse>>>,
    /// Every (tool, args) tuple the adapter sent through us.
    calls: Mutex<Vec<(String, Value)>>,
    /// Default response when a tool has no canned queue (typically
    /// success+empty body, used so writes don't blow up unless the
    /// test explicitly queues a failure).
    default: Mutex<Option<ComposioExecuteResponse>>,
}

impl FakeComposioExecutor {
    fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    fn queue(&self, tool: &str, resp: ComposioExecuteResponse) {
        self.responses
            .lock()
            .entry(tool.to_string())
            .or_default()
            .push(resp);
    }

    fn set_default(&self, resp: ComposioExecuteResponse) {
        *self.default.lock() = Some(resp);
    }

    fn calls(&self) -> Vec<(String, Value)> {
        self.calls.lock().clone()
    }
}

#[async_trait]
impl ComposioExecutor for FakeComposioExecutor {
    async fn execute(&self, tool: &str, args: Value) -> Result<ComposioExecuteResponse> {
        self.calls.lock().push((tool.to_string(), args));
        if let Some(resp) = self
            .responses
            .lock()
            .get_mut(tool)
            .and_then(|q| if q.is_empty() { None } else { Some(q.remove(0)) })
        {
            return Ok(resp);
        }
        if let Some(default) = self.default.lock().clone() {
            return Ok(default);
        }
        Err(anyhow::anyhow!("no response queued for {tool}"))
    }
}

fn values_resp(values: Value) -> ComposioExecuteResponse {
    ComposioExecuteResponse {
        data: json!({ "values": values }),
        successful: true,
        error: None,
        cost_usd: 0.0,
        markdown_formatted: None,
    }
}

fn ok_empty() -> ComposioExecuteResponse {
    ComposioExecuteResponse {
        data: json!({}),
        successful: true,
        error: None,
        cost_usd: 0.0,
        markdown_formatted: None,
    }
}

// ── infer_field_kind ───────────────────────────────────────────────

#[test]
fn infer_field_kind_recognizes_email() {
    assert!(matches!(
        infer_field_kind("alice@acme.io"),
        EntityFieldKind::EmailAddress
    ));
    assert!(matches!(
        infer_field_kind("  bob+tag@sub.domain.co.uk  "),
        EntityFieldKind::EmailAddress
    ));
}

#[test]
fn infer_field_kind_recognizes_url_before_string() {
    assert!(matches!(
        infer_field_kind("https://example.com/foo"),
        EntityFieldKind::Url
    ));
    assert!(matches!(
        infer_field_kind("http://localhost:8080"),
        EntityFieldKind::Url
    ));
}

#[test]
fn infer_field_kind_recognizes_phone_e164_and_loose() {
    assert!(matches!(
        infer_field_kind("+14155550101"),
        EntityFieldKind::PhoneNumber
    ));
    assert!(matches!(
        infer_field_kind("(415) 555-0101"),
        EntityFieldKind::PhoneNumber
    ));
    assert!(matches!(
        infer_field_kind("415-555-0101"),
        EntityFieldKind::PhoneNumber
    ));
}

#[test]
fn infer_field_kind_recognizes_iso_date_and_rfc3339() {
    assert!(matches!(
        infer_field_kind("2026-05-26"),
        EntityFieldKind::DateTime
    ));
    assert!(matches!(
        infer_field_kind("2026-05-26T14:32:00Z"),
        EntityFieldKind::DateTime
    ));
}

#[test]
fn infer_field_kind_recognizes_number_and_bool_falls_through_to_string() {
    assert!(matches!(
        infer_field_kind("42"),
        EntityFieldKind::Number
    ));
    assert!(matches!(
        infer_field_kind("3.14"),
        EntityFieldKind::Number
    ));
    assert!(matches!(
        infer_field_kind("yes"),
        EntityFieldKind::Bool
    ));
    assert!(matches!(
        infer_field_kind("Acme Corp"),
        EntityFieldKind::String
    ));
    assert!(matches!(
        infer_field_kind(""),
        EntityFieldKind::Unknown
    ));
}

// ── column_letter ──────────────────────────────────────────────────

#[test]
fn column_letter_handles_single_and_double_letter() {
    assert_eq!(column_letter(1), "A");
    assert_eq!(column_letter(26), "Z");
    assert_eq!(column_letter(27), "AA");
    assert_eq!(column_letter(52), "AZ");
    assert_eq!(column_letter(53), "BA");
}

// ── schema ─────────────────────────────────────────────────────────

#[tokio::test]
async fn schema_infers_field_kinds_from_first_data_row() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "GOOGLESHEETS_SPREADSHEETS_VALUES_GET",
        values_resp(json!([
            ["email", "name", "status", "last_contacted", "interest_score"],
            ["alice@acme.io", "Alice", "active", "2026-05-26T10:00:00Z", "8.4"]
        ])),
    );
    let adapter = GoogleSheetsAdapter::new(exec, "sheet_abc", "Vendors!A1:E100");
    let schema = adapter.schema().await.unwrap();

    let keys: Vec<&str> = schema.fields.iter().map(|f| f.key.as_str()).collect();
    assert_eq!(keys, vec!["email", "name", "status", "last_contacted", "interest_score"]);

    assert!(matches!(schema.fields[0].kind, EntityFieldKind::EmailAddress));
    assert!(matches!(schema.fields[1].kind, EntityFieldKind::String));
    assert!(matches!(schema.fields[2].kind, EntityFieldKind::String));
    assert!(matches!(schema.fields[3].kind, EntityFieldKind::DateTime));
    assert!(matches!(schema.fields[4].kind, EntityFieldKind::Number));

    // Email column auto-promotes to primary even when it's not first
    // (we'd test "not first" below) — here it IS first, but the rule
    // still applies.
    assert_eq!(schema.primary_field, "email");
}

#[tokio::test]
async fn schema_picks_email_field_as_primary_even_when_not_first() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "GOOGLESHEETS_SPREADSHEETS_VALUES_GET",
        values_resp(json!([
            ["name", "email", "score"],
            ["Alice", "alice@acme.io", "8.4"]
        ])),
    );
    let adapter = GoogleSheetsAdapter::new(exec, "sheet_abc", "Sheet1!A1:C100");
    let schema = adapter.schema().await.unwrap();
    assert_eq!(schema.primary_field, "email");
}

// ── list ───────────────────────────────────────────────────────────

#[tokio::test]
async fn list_returns_one_record_per_data_row() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "GOOGLESHEETS_SPREADSHEETS_VALUES_GET",
        values_resp(json!([
            ["email", "status"],
            ["a@x.io", "active"],
            ["b@x.io", "paused"]
        ])),
    );
    let adapter = GoogleSheetsAdapter::new(exec, "sid_1", "Sheet1!A1:B10");
    let out = adapter.list(EntityQuery::default()).await.unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].fields["email"], json!("a@x.io"));
    assert_eq!(out[1].fields["status"], json!("paused"));
    // Row identity reflects sheet position (2-indexed because row 1 is the header).
    assert_eq!(out[0].id.native, "sid_1:row:2");
    assert_eq!(out[1].id.native, "sid_1:row:3");
}

#[tokio::test]
async fn list_applies_eq_filter() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "GOOGLESHEETS_SPREADSHEETS_VALUES_GET",
        values_resp(json!([
            ["email", "status"],
            ["a@x.io", "active"],
            ["b@x.io", "paused"],
            ["c@x.io", "active"]
        ])),
    );
    let adapter = GoogleSheetsAdapter::new(exec, "sid_1", "Sheet1!A1:B10");
    let out = adapter
        .list(EntityQuery {
            filters: vec![EntityFilter {
                field: "status".into(),
                op: EntityFilterOp::Eq,
                value: json!("active"),
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(out.len(), 2);
    assert!(out.iter().all(|r| r.fields["status"] == json!("active")));
}

#[tokio::test]
async fn list_respects_limit_and_offset() {
    let exec = FakeComposioExecutor::new();
    let mut rows = vec![json!(["i"])];
    for i in 0..10 {
        rows.push(json!([i.to_string()]));
    }
    exec.queue(
        "GOOGLESHEETS_SPREADSHEETS_VALUES_GET",
        values_resp(Value::Array(rows)),
    );
    let adapter = GoogleSheetsAdapter::new(exec, "sid", "Sheet1!A1:A100");
    let out = adapter
        .list(EntityQuery {
            limit: Some(3),
            offset: Some(5),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].fields["i"], json!("5"));
    assert_eq!(out[2].fields["i"], json!("7"));
}

// ── get ────────────────────────────────────────────────────────────

#[tokio::test]
async fn get_returns_record_for_known_row_and_none_for_unknown() {
    let exec = FakeComposioExecutor::new();
    // Two fetches: one for the known row, one for the unknown.
    let body = json!([["email"], ["a@x.io"]]);
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body.clone()));
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body));
    let adapter = GoogleSheetsAdapter::new(exec, "sid", "Sheet1!A1:A10");

    let hit = adapter
        .get(&EntityId::new(ADAPTER_ID, "sid:row:2"))
        .await
        .unwrap();
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().fields["email"], json!("a@x.io"));

    let miss = adapter
        .get(&EntityId::new(ADAPTER_ID, "sid:row:99"))
        .await
        .unwrap();
    assert!(miss.is_none());
}

#[tokio::test]
async fn get_errors_when_native_id_is_malformed() {
    let exec = FakeComposioExecutor::new();
    let adapter = GoogleSheetsAdapter::new(exec, "sid", "Sheet1!A1:A10");
    let err = adapter
        .get(&EntityId::new(ADAPTER_ID, "not-a-row-id"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("invalid entity id"));
}

// ── update ─────────────────────────────────────────────────────────

#[tokio::test]
async fn update_writes_full_row_through_update_values_action() {
    let exec = FakeComposioExecutor::new();
    let exec_clone = Arc::clone(&exec);
    // Two reads happen: one for schema cache (schema() inside update),
    // and one for the existing-row fetch (also a GET).
    let body = json!([
        ["email", "status", "note"],
        ["a@x.io", "active", "hello"]
    ]);
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body.clone()));
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body));
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_UPDATE", ok_empty());

    let adapter = GoogleSheetsAdapter::new(exec, "sid", "Sheet1!A1:C100");
    let mut patch = serde_json::Map::new();
    patch.insert("status".into(), json!("paused"));
    adapter
        .update(
            &EntityId::new(ADAPTER_ID, "sid:row:2"),
            EntityPatch { fields: patch },
        )
        .await
        .unwrap();

    let calls = exec_clone.calls();
    let update_call = calls
        .iter()
        .find(|(tool, _)| tool == "GOOGLESHEETS_SPREADSHEETS_VALUES_UPDATE")
        .expect("update was called");
    assert_eq!(update_call.1["spreadsheetId"], json!("sid"));
    assert_eq!(update_call.1["range"], json!("Sheet1!A2:C2"));
    assert_eq!(update_call.1["valueInputOption"], json!("USER_ENTERED"));
    // The written row preserved untouched cells + applied the patch.
    assert_eq!(
        update_call.1["values"],
        json!([["a@x.io", "paused", "hello"]])
    );
}

#[tokio::test]
async fn update_errors_when_row_does_not_exist() {
    let exec = FakeComposioExecutor::new();
    let body = json!([["email"], ["a@x.io"]]);
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body.clone()));
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body));
    let adapter = GoogleSheetsAdapter::new(exec, "sid", "Sheet1!A1:A100");
    let mut patch = serde_json::Map::new();
    patch.insert("email".into(), json!("z@x.io"));
    let err = adapter
        .update(
            &EntityId::new(ADAPTER_ID, "sid:row:99"),
            EntityPatch { fields: patch },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"), "got: {err}");
}

#[tokio::test]
async fn update_propagates_composio_failure() {
    let exec = FakeComposioExecutor::new();
    let body = json!([["email"], ["a@x.io"]]);
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body.clone()));
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", values_resp(body));
    exec.queue(
        "GOOGLESHEETS_SPREADSHEETS_VALUES_UPDATE",
        ComposioExecuteResponse {
            data: json!({}),
            successful: false,
            error: Some("quota exceeded".to_string()),
            cost_usd: 0.0,
            markdown_formatted: None,
        },
    );
    let adapter = GoogleSheetsAdapter::new(exec, "sid", "Sheet1!A1:A100");
    let mut patch = serde_json::Map::new();
    patch.insert("email".into(), json!("z@x.io"));
    let err = adapter
        .update(
            &EntityId::new(ADAPTER_ID, "sid:row:2"),
            EntityPatch { fields: patch },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("quota exceeded"), "got: {err}");
}

// ── subscribe ──────────────────────────────────────────────────────

#[tokio::test]
async fn subscribe_emits_created_event_when_new_row_appears() {
    let exec = FakeComposioExecutor::new();
    // First poll establishes baseline (1 data row). Second poll
    // (after the new row appears) sees the appended row. We set the
    // default so any further background polls keep returning the
    // post-append snapshot without panicking the test on extra calls.
    let initial = values_resp(json!([
        ["email"],
        ["a@x.io"]
    ]));
    let after_append = values_resp(json!([
        ["email"],
        ["a@x.io"],
        ["b@x.io"]
    ]));
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", initial.clone());
    exec.queue("GOOGLESHEETS_SPREADSHEETS_VALUES_GET", after_append.clone());
    exec.set_default(after_append);

    // 25ms cadence keeps the test well under 1s wall time. The
    // adapter takes one tick to establish baseline and a second to
    // detect the new row.
    let adapter = GoogleSheetsAdapter::new(exec, "sid", "Sheet1!A1:A100")
        .with_poll_interval(Duration::from_millis(25));
    let stream = adapter
        .subscribe()
        .await
        .unwrap()
        .expect("sheets always supports polling subscribe");
    let mut stream = Box::pin(stream);

    let change = tokio::time::timeout(Duration::from_millis(500), stream.next())
        .await
        .expect("subscribe must emit within 500ms")
        .expect("stream not closed");
    assert_eq!(change.id.adapter, ADAPTER_ID);
    assert_eq!(change.id.native, "sid:row:3");
    assert!(matches!(
        change.kind,
        super::types::EntityChangeKind::Created
    ));
}

// ── identity collision ────────────────────────────────────────────

#[test]
fn entity_id_native_includes_spreadsheet_so_same_row_in_different_sheets_collides_safely() {
    // Two adapters bound to different spreadsheets must produce
    // distinct EntityIds for "row 2" so the F4-9 approval queue
    // can't collide them.
    let a = EntityId::new(ADAPTER_ID, "sheet_a:row:2");
    let b = EntityId::new(ADAPTER_ID, "sheet_b:row:2");
    assert_ne!(a, b);
}
