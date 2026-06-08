//! F4-6 Attio adapter tests. Reuses the FakeComposioExecutor shape
//! from `google_sheets_tests` (private to that module so we redefine
//! a minimal copy here scoped to the slugs we care about).

use anyhow::Result;
use async_trait::async_trait;
use futures::StreamExt;
use parking_lot::Mutex;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use super::attio::{
    translate_filters, verify_attio_signature, AttioAdapter, AttioWebhookHelper, ADAPTER_ID,
};
use super::google_sheets::ComposioExecutor;
use super::types::{
    EntityFieldKind, EntityFilter, EntityFilterOp, EntityId, EntityPatch, EntityQuery,
};
use super::EntityStore;
use crate::openhuman::composio::types::ComposioExecuteResponse;

// ── FakeComposioExecutor (duplicate of google_sheets_tests'; kept
// per-file so test crates stay leaf-isolated) ─────────────────────

#[derive(Default)]
struct FakeComposioExecutor {
    responses: Mutex<HashMap<String, Vec<ComposioExecuteResponse>>>,
    calls: Mutex<Vec<(String, Value)>>,
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
        if let Some(resp) = self.responses.lock().get_mut(tool).and_then(|q| {
            if q.is_empty() {
                None
            } else {
                Some(q.remove(0))
            }
        }) {
            return Ok(resp);
        }
        if let Some(default) = self.default.lock().clone() {
            return Ok(default);
        }
        Err(anyhow::anyhow!("no response queued for {tool}"))
    }
}

fn ok_data(body: Value) -> ComposioExecuteResponse {
    ComposioExecuteResponse {
        data: body,
        successful: true,
        error: None,
        cost_usd: 0.0,
        markdown_formatted: None,
    }
}

fn err_resp(message: &str) -> ComposioExecuteResponse {
    ComposioExecuteResponse {
        data: json!({}),
        successful: false,
        error: Some(message.to_string()),
        cost_usd: 0.0,
        markdown_formatted: None,
    }
}

// ── Filter translation ────────────────────────────────────────────

#[test]
fn translate_filters_empty_returns_empty_object() {
    assert_eq!(translate_filters(&[]), json!({}));
}

#[test]
fn translate_filters_single_eq_unwraps_to_field_clause() {
    let out = translate_filters(&[EntityFilter {
        field: "status".into(),
        op: EntityFilterOp::Eq,
        value: json!("active"),
    }]);
    assert_eq!(out, json!({ "status": { "$eq": "active" } }));
}

#[test]
fn translate_filters_not_eq_wraps_in_not_eq() {
    let out = translate_filters(&[EntityFilter {
        field: "stage".into(),
        op: EntityFilterOp::NotEq,
        value: json!("won"),
    }]);
    assert_eq!(out, json!({ "stage": { "$not": { "$eq": "won" } } }));
}

#[test]
fn translate_filters_contains_uses_attio_contains_op() {
    let out = translate_filters(&[EntityFilter {
        field: "name".into(),
        op: EntityFilterOp::Contains,
        value: json!("Acme"),
    }]);
    assert_eq!(out, json!({ "name": { "$contains": "Acme" } }));
}

#[test]
fn translate_filters_is_null_uses_is_empty() {
    let out = translate_filters(&[EntityFilter {
        field: "phone".into(),
        op: EntityFilterOp::IsNull,
        value: json!(null),
    }]);
    assert_eq!(out, json!({ "phone": { "$is_empty": true } }));
}

#[test]
fn translate_filters_is_not_null_uses_is_not_empty() {
    let out = translate_filters(&[EntityFilter {
        field: "phone".into(),
        op: EntityFilterOp::IsNotNull,
        value: json!(null),
    }]);
    assert_eq!(out, json!({ "phone": { "$is_not_empty": true } }));
}

#[test]
fn translate_filters_multiple_clauses_ands_at_top_level() {
    let out = translate_filters(&[
        EntityFilter {
            field: "status".into(),
            op: EntityFilterOp::Eq,
            value: json!("active"),
        },
        EntityFilter {
            field: "stage".into(),
            op: EntityFilterOp::NotEq,
            value: json!("lost"),
        },
    ]);
    assert_eq!(
        out,
        json!({
            "$and": [
                { "status": { "$eq": "active" } },
                { "stage": { "$not": { "$eq": "lost" } } }
            ]
        })
    );
}

// ── Schema ────────────────────────────────────────────────────────

#[tokio::test]
async fn schema_maps_attio_attribute_types_to_field_kinds() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "ATTIO_LIST_ATTRIBUTES",
        ok_data(json!({
            "data": [
                { "api_slug": "name", "title": "Name", "type": "text", "is_required": true },
                { "api_slug": "email_addresses", "title": "Email", "type": "email-address" },
                { "api_slug": "phone_numbers", "title": "Phone", "type": "phone-number" },
                { "api_slug": "score", "title": "Score", "type": "number" },
                { "api_slug": "is_active", "title": "Active", "type": "checkbox" },
                { "api_slug": "created", "title": "Created", "type": "timestamp" },
                { "api_slug": "homepage", "title": "Homepage", "type": "url" },
                { "api_slug": "stage", "title": "Stage", "type": "status" }
            ]
        })),
    );
    let adapter = AttioAdapter::new(exec, "ws_1", "people");
    let schema = adapter.schema().await.unwrap();

    let kinds: Vec<_> = schema.fields.iter().map(|f| f.kind.clone()).collect();
    assert!(matches!(kinds[0], EntityFieldKind::String));
    assert!(matches!(kinds[1], EntityFieldKind::EmailAddress));
    assert!(matches!(kinds[2], EntityFieldKind::PhoneNumber));
    assert!(matches!(kinds[3], EntityFieldKind::Number));
    assert!(matches!(kinds[4], EntityFieldKind::Bool));
    assert!(matches!(kinds[5], EntityFieldKind::DateTime));
    assert!(matches!(kinds[6], EntityFieldKind::Url));
    assert!(matches!(kinds[7], EntityFieldKind::Enum { .. }));

    // First required field becomes primary when no `record_id` field
    // is reported.
    assert_eq!(schema.primary_field, "name");
}

#[tokio::test]
async fn schema_is_cached_after_first_fetch() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "ATTIO_LIST_ATTRIBUTES",
        ok_data(json!({
            "data": [{ "api_slug": "name", "title": "Name", "type": "text", "is_required": true }]
        })),
    );
    let exec_clone = Arc::clone(&exec);
    let adapter = AttioAdapter::new(exec, "ws_1", "people");
    let _ = adapter.schema().await.unwrap();
    let _ = adapter.schema().await.unwrap(); // second call MUST hit cache, not the executor
    let calls = exec_clone.calls();
    assert_eq!(calls.len(), 1, "schema must cache; got {} calls", calls.len());
}

// ── list / get ────────────────────────────────────────────────────

#[tokio::test]
async fn list_parses_records_and_flattens_attio_value_arrays() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "ATTIO_QUERY_RECORDS",
        ok_data(json!({
            "data": [
                {
                    "id": { "record_id": "rec_abc" },
                    "values": {
                        "name": [{ "value": "Alice", "attribute_type": "text" }],
                        "email_addresses": [{ "value": "alice@acme.io", "attribute_type": "email-address" }]
                    }
                },
                {
                    "id": { "record_id": "rec_xyz" },
                    "values": {
                        "name": [{ "value": "Bob", "attribute_type": "text" }]
                    }
                }
            ]
        })),
    );
    let adapter = AttioAdapter::new(exec, "ws_1", "people");
    let out = adapter.list(EntityQuery::default()).await.unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[0].id.native, "rec_abc");
    assert_eq!(out[0].fields["name"], json!("Alice"));
    assert_eq!(out[0].fields["email_addresses"], json!("alice@acme.io"));
    assert_eq!(out[1].id.native, "rec_xyz");
}

#[tokio::test]
async fn list_passes_filter_translation_into_query_body() {
    let exec = FakeComposioExecutor::new();
    let exec_clone = Arc::clone(&exec);
    exec.queue(
        "ATTIO_QUERY_RECORDS",
        ok_data(json!({ "data": [] })),
    );
    let adapter = AttioAdapter::new(exec, "ws_1", "people");
    let _ = adapter
        .list(EntityQuery {
            filters: vec![EntityFilter {
                field: "stage".into(),
                op: EntityFilterOp::Eq,
                value: json!("active"),
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    let calls = exec_clone.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ATTIO_QUERY_RECORDS");
    assert_eq!(calls[0].1["filter"], json!({ "stage": { "$eq": "active" } }));
    assert_eq!(calls[0].1["objectType"], json!("people"));
    assert_eq!(calls[0].1["workspaceId"], json!("ws_1"));
}

#[tokio::test]
async fn get_returns_record_when_found_and_none_on_attio_404() {
    let exec = FakeComposioExecutor::new();
    exec.queue(
        "ATTIO_GET_RECORD",
        ok_data(json!({
            "data": {
                "id": "rec_abc",
                "values": { "name": [{ "value": "Alice" }] }
            }
        })),
    );
    exec.queue("ATTIO_GET_RECORD", err_resp("record not found"));
    let adapter = AttioAdapter::new(exec, "ws_1", "people");

    let hit = adapter
        .get(&EntityId::new(ADAPTER_ID, "rec_abc"))
        .await
        .unwrap();
    assert!(hit.is_some());
    assert_eq!(hit.unwrap().fields["name"], json!("Alice"));

    let miss = adapter
        .get(&EntityId::new(ADAPTER_ID, "rec_missing"))
        .await
        .unwrap();
    assert!(miss.is_none(), "404 from Attio must become Ok(None)");
}

#[tokio::test]
async fn get_propagates_non_404_errors() {
    let exec = FakeComposioExecutor::new();
    exec.queue("ATTIO_GET_RECORD", err_resp("internal server error"));
    let adapter = AttioAdapter::new(exec, "ws_1", "people");
    let err = adapter
        .get(&EntityId::new(ADAPTER_ID, "rec_abc"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("internal server error"));
}

// ── update ────────────────────────────────────────────────────────

#[tokio::test]
async fn update_sends_patch_under_data_values() {
    let exec = FakeComposioExecutor::new();
    let exec_clone = Arc::clone(&exec);
    exec.queue("ATTIO_UPDATE_RECORD", ok_data(json!({})));
    let adapter = AttioAdapter::new(exec, "ws_1", "people");
    let mut patch = serde_json::Map::new();
    patch.insert("stage".into(), json!("won"));
    patch.insert("score".into(), json!(95));
    adapter
        .update(
            &EntityId::new(ADAPTER_ID, "rec_abc"),
            EntityPatch { fields: patch },
        )
        .await
        .unwrap();

    let calls = exec_clone.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ATTIO_UPDATE_RECORD");
    assert_eq!(calls[0].1["recordId"], json!("rec_abc"));
    assert_eq!(calls[0].1["objectType"], json!("people"));
    assert_eq!(calls[0].1["data"]["values"]["stage"], json!("won"));
    assert_eq!(calls[0].1["data"]["values"]["score"], json!(95));
}

#[tokio::test]
async fn update_propagates_composio_failure() {
    let exec = FakeComposioExecutor::new();
    exec.queue("ATTIO_UPDATE_RECORD", err_resp("validation failed"));
    let adapter = AttioAdapter::new(exec, "ws_1", "people");
    let mut patch = serde_json::Map::new();
    patch.insert("stage".into(), json!("won"));
    let err = adapter
        .update(
            &EntityId::new(ADAPTER_ID, "rec_abc"),
            EntityPatch { fields: patch },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("validation failed"));
}

// ── subscribe (polling) ───────────────────────────────────────────

#[tokio::test]
async fn subscribe_emits_created_event_for_newly_appearing_record() {
    let exec = FakeComposioExecutor::new();
    let baseline = ok_data(json!({
        "data": [
            { "id": "rec_1", "values": {} }
        ]
    }));
    let after_create = ok_data(json!({
        "data": [
            { "id": "rec_1", "values": {} },
            { "id": "rec_2", "values": {} }
        ]
    }));
    exec.queue("ATTIO_QUERY_RECORDS", baseline.clone());
    exec.queue("ATTIO_QUERY_RECORDS", after_create.clone());
    exec.set_default(after_create);

    let adapter = AttioAdapter::new(exec, "ws_1", "people")
        .with_poll_interval(Duration::from_millis(25));
    let stream = adapter
        .subscribe()
        .await
        .unwrap()
        .expect("polling subscribe is the live path");
    let mut stream = Box::pin(stream);

    let change = tokio::time::timeout(Duration::from_millis(500), stream.next())
        .await
        .expect("subscribe must emit within 500ms")
        .expect("stream open");
    assert_eq!(change.id.adapter, ADAPTER_ID);
    assert_eq!(change.id.native, "rec_2");
    assert!(matches!(
        change.kind,
        super::types::EntityChangeKind::Created
    ));
}

#[tokio::test]
async fn subscribe_emits_deleted_event_when_record_disappears() {
    let exec = FakeComposioExecutor::new();
    let baseline = ok_data(json!({
        "data": [
            { "id": "rec_1", "values": {} },
            { "id": "rec_2", "values": {} }
        ]
    }));
    let after_delete = ok_data(json!({
        "data": [
            { "id": "rec_1", "values": {} }
        ]
    }));
    exec.queue("ATTIO_QUERY_RECORDS", baseline.clone());
    exec.queue("ATTIO_QUERY_RECORDS", after_delete.clone());
    exec.set_default(after_delete);

    let adapter = AttioAdapter::new(exec, "ws_1", "people")
        .with_poll_interval(Duration::from_millis(25));
    let stream = adapter.subscribe().await.unwrap().expect("subscribe live");
    let mut stream = Box::pin(stream);

    let change = tokio::time::timeout(Duration::from_millis(500), stream.next())
        .await
        .expect("subscribe must emit within 500ms")
        .expect("stream open");
    assert_eq!(change.id.native, "rec_2");
    assert!(matches!(
        change.kind,
        super::types::EntityChangeKind::Deleted
    ));
}

// ── HMAC verification ─────────────────────────────────────────────

#[test]
fn verify_attio_signature_accepts_correctly_signed_body() {
    let secret = "whsec_test_secret_abcdef";
    let body = b"{\"event\":\"record.updated\",\"record_id\":\"rec_abc\"}";
    // Pre-computed via HMAC-SHA256(secret, body) for the assertions.
    let hex = expected_hmac_hex(secret, body);
    assert!(verify_attio_signature(secret, body, &hex));
    assert!(verify_attio_signature(
        secret,
        body,
        &format!("sha256={hex}")
    ));
}

#[test]
fn verify_attio_signature_rejects_tampered_body_or_wrong_secret() {
    let secret = "right_secret";
    let body = b"{\"a\":1}";
    let hex = expected_hmac_hex(secret, body);

    // Tampered body — same secret + signature, but the body changed.
    let tampered = b"{\"a\":2}";
    assert!(!verify_attio_signature(secret, tampered, &hex));

    // Wrong secret with the original body.
    assert!(!verify_attio_signature("wrong_secret", body, &hex));

    // Empty signature.
    assert!(!verify_attio_signature(secret, body, ""));
}

fn expected_hmac_hex(secret: &str, body: &[u8]) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    mac.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect()
}

// ── Webhook register/teardown helper ──────────────────────────────

#[tokio::test]
async fn webhook_helper_register_returns_id_and_posts_correct_events() {
    let exec = FakeComposioExecutor::new();
    let exec_clone = Arc::clone(&exec);
    exec.queue(
        "ATTIO_CREATE_WEBHOOK",
        ok_data(json!({ "id": "wh_123" })),
    );
    let helper = AttioWebhookHelper::new(exec, "ws_1", "people");
    let id = helper.register("https://oh.io/hooks/attio").await.unwrap();
    assert_eq!(id, "wh_123");

    let calls = exec_clone.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "ATTIO_CREATE_WEBHOOK");
    assert_eq!(calls[0].1["targetUrl"], json!("https://oh.io/hooks/attio"));
    let events = calls[0].1["events"]
        .as_array()
        .expect("events array")
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect::<Vec<_>>();
    assert!(events.contains(&"people.record.created".to_string()));
    assert!(events.contains(&"people.record.updated".to_string()));
    assert!(events.contains(&"people.record.deleted".to_string()));
}

#[tokio::test]
async fn webhook_helper_teardown_posts_delete_with_webhook_id() {
    let exec = FakeComposioExecutor::new();
    let exec_clone = Arc::clone(&exec);
    exec.queue("ATTIO_DELETE_WEBHOOK", ok_data(json!({})));
    let helper = AttioWebhookHelper::new(exec, "ws_1", "people");
    helper.teardown("wh_123").await.unwrap();

    let calls = exec_clone.calls();
    assert_eq!(calls[0].0, "ATTIO_DELETE_WEBHOOK");
    assert_eq!(calls[0].1["webhookId"], json!("wh_123"));
}

#[tokio::test]
async fn webhook_helper_register_errors_when_response_missing_id() {
    let exec = FakeComposioExecutor::new();
    exec.queue("ATTIO_CREATE_WEBHOOK", ok_data(json!({})));
    let helper = AttioWebhookHelper::new(exec, "ws_1", "people");
    let err = helper.register("https://oh.io/hooks").await.unwrap_err();
    assert!(err.to_string().contains("missing id"), "got: {err}");
}
