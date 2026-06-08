//! F4-4 trait-conformance tests against the MockEntityStore. Every
//! real adapter (F4-5 Sheets, F4-6 Attio) gets a parallel test
//! suite running the same shape against the live API.

use super::registry::open_entity_store;
use super::types::*;
use super::{EntityStore, MockEntityStore};
use crate::openhuman::campaigns::types::EntityRef;
use crate::openhuman::config::Config;
use futures::StreamExt;
use serde_json::{json, Map};

fn record(adapter: &str, native: &str, fields: Map<String, serde_json::Value>) -> EntityRecord {
    EntityRecord {
        id: EntityId::new(adapter, native),
        fields,
        updated_at: None,
    }
}

fn map_of(pairs: &[(&str, serde_json::Value)]) -> Map<String, serde_json::Value> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_string(), v.clone()))
        .collect()
}

// ── EntityId ───────────────────────────────────────────────────────

#[test]
fn entity_id_round_trips_through_json_with_flat_shape() {
    let id = EntityId::new("google_sheets", "row-42");
    let json = serde_json::to_string(&id).unwrap();
    assert_eq!(json, r#"{"adapter":"google_sheets","native":"row-42"}"#);
    let back: EntityId = serde_json::from_str(&json).unwrap();
    assert_eq!(id, back);
}

#[test]
fn entity_id_is_collision_free_across_adapters() {
    let sheets_42 = EntityId::new("google_sheets", "42");
    let attio_42 = EntityId::new("attio", "42");
    assert_ne!(sheets_42, attio_42);
    // Hash distinct too — used as keys in HashMaps in the F4-7 executor.
    use std::collections::HashSet;
    let mut set = HashSet::new();
    set.insert(sheets_42);
    set.insert(attio_42);
    assert_eq!(set.len(), 2);
}

// ── Field-kind round-trip ─────────────────────────────────────────

#[test]
fn entity_field_kind_round_trips_every_variant() {
    let variants = vec![
        EntityFieldKind::String,
        EntityFieldKind::Number,
        EntityFieldKind::Bool,
        EntityFieldKind::Enum {
            variants: vec!["draft".into(), "active".into()],
        },
        EntityFieldKind::DateTime,
        EntityFieldKind::EmailAddress,
        EntityFieldKind::PhoneNumber,
        EntityFieldKind::Url,
        EntityFieldKind::Unknown,
    ];
    for v in variants {
        let json = serde_json::to_string(&v).unwrap();
        let back: EntityFieldKind = serde_json::from_str(&json).unwrap();
        assert_eq!(v, back);
    }
}

// ── MockEntityStore ────────────────────────────────────────────────

#[tokio::test]
async fn mock_schema_derives_field_keys_from_seeded_records() {
    let store = MockEntityStore::with_records(vec![
        record(
            "mock",
            "1",
            map_of(&[("email", json!("a@b.com")), ("status", json!("active"))]),
        ),
        record(
            "mock",
            "2",
            map_of(&[("email", json!("c@d.com")), ("score", json!(42))]),
        ),
    ]);
    let schema = store.schema().await.unwrap();
    let keys: Vec<&str> = schema.fields.iter().map(|f| f.key.as_str()).collect();
    // Alphabetical for determinism.
    assert_eq!(keys, vec!["email", "score", "status"]);
    assert_eq!(schema.primary_field, "email");
    // All inferred to Unknown — the mock can't classify.
    for f in &schema.fields {
        assert!(matches!(f.kind, EntityFieldKind::Unknown));
    }
}

#[tokio::test]
async fn mock_list_returns_all_seeded_records() {
    let store = MockEntityStore::with_records(vec![
        record("mock", "1", map_of(&[("name", json!("a"))])),
        record("mock", "2", map_of(&[("name", json!("b"))])),
    ]);
    let out = store.list(EntityQuery::default()).await.unwrap();
    assert_eq!(out.len(), 2);
}

#[tokio::test]
async fn mock_list_applies_eq_filter() {
    let store = MockEntityStore::with_records(vec![
        record(
            "mock",
            "1",
            map_of(&[("status", json!("active")), ("name", json!("a"))]),
        ),
        record(
            "mock",
            "2",
            map_of(&[("status", json!("paused")), ("name", json!("b"))]),
        ),
    ]);
    let out = store
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
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].id.native, "1");
}

#[tokio::test]
async fn mock_list_applies_contains_filter_case_insensitive() {
    let store = MockEntityStore::with_records(vec![
        record("mock", "1", map_of(&[("name", json!("Acme Corp"))])),
        record("mock", "2", map_of(&[("name", json!("Beta LLC"))])),
        record("mock", "3", map_of(&[("name", json!("acme widgets"))])),
    ]);
    let out = store
        .list(EntityQuery {
            filters: vec![EntityFilter {
                field: "name".into(),
                op: EntityFilterOp::Contains,
                value: json!("acme"),
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(out.len(), 2);
}

#[tokio::test]
async fn mock_list_applies_is_null_filter() {
    let store = MockEntityStore::with_records(vec![
        record(
            "mock",
            "1",
            map_of(&[("name", json!("a")), ("notes", json!(null))]),
        ),
        record("mock", "2", map_of(&[("name", json!("b"))])),
        record(
            "mock",
            "3",
            map_of(&[("name", json!("c")), ("notes", json!("present"))]),
        ),
    ]);
    let out = store
        .list(EntityQuery {
            filters: vec![EntityFilter {
                field: "notes".into(),
                op: EntityFilterOp::IsNull,
                value: json!(null),
            }],
            ..Default::default()
        })
        .await
        .unwrap();
    // Records 1 (null) + 2 (missing field) both qualify.
    assert_eq!(out.len(), 2);
    let ids: Vec<&str> = out.iter().map(|r| r.id.native.as_str()).collect();
    assert!(ids.contains(&"1"));
    assert!(ids.contains(&"2"));
}

#[tokio::test]
async fn mock_list_respects_limit_and_offset() {
    let store = MockEntityStore::with_records(
        (0..10)
            .map(|i| record("mock", &i.to_string(), map_of(&[("i", json!(i))])))
            .collect(),
    );
    let out = store
        .list(EntityQuery {
            limit: Some(3),
            offset: Some(5),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(out.len(), 3);
    assert_eq!(out[0].id.native, "5");
    assert_eq!(out[2].id.native, "7");
}

#[tokio::test]
async fn mock_get_returns_record_when_id_known() {
    let store = MockEntityStore::with_records(vec![record(
        "mock",
        "x",
        map_of(&[("name", json!("Acme"))]),
    )]);
    let id = EntityId::new("mock", "x");
    let out = store.get(&id).await.unwrap();
    assert!(out.is_some());
    assert_eq!(out.unwrap().fields["name"], json!("Acme"));
}

#[tokio::test]
async fn mock_get_returns_none_for_unknown_id() {
    let store = MockEntityStore::default();
    let out = store.get(&EntityId::new("mock", "ghost")).await.unwrap();
    assert!(out.is_none());
}

#[tokio::test]
async fn mock_update_applies_patch_at_field_level_leaves_others_alone() {
    let store = MockEntityStore::with_records(vec![record(
        "mock",
        "1",
        map_of(&[
            ("name", json!("Acme")),
            ("status", json!("active")),
            ("note", json!("hello")),
        ]),
    )]);
    let id = EntityId::new("mock", "1");
    store
        .update(
            &id,
            EntityPatch {
                fields: map_of(&[("status", json!("paused"))]),
            },
        )
        .await
        .unwrap();
    let row = store.get(&id).await.unwrap().unwrap();
    assert_eq!(row.fields["status"], json!("paused"));
    assert_eq!(
        row.fields["name"],
        json!("Acme"),
        "untouched field preserved"
    );
    assert_eq!(
        row.fields["note"],
        json!("hello"),
        "untouched field preserved"
    );
    assert!(row.updated_at.is_some());
}

#[tokio::test]
async fn mock_update_errors_when_id_unknown() {
    let store = MockEntityStore::default();
    let err = store
        .update(
            &EntityId::new("mock", "ghost"),
            EntityPatch {
                fields: map_of(&[("name", json!("Acme"))]),
            },
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[tokio::test]
async fn mock_subscribe_emits_change_event_on_update() {
    let store = MockEntityStore::with_records(vec![record(
        "mock",
        "1",
        map_of(&[("status", json!("active"))]),
    )]);
    let stream = store
        .subscribe()
        .await
        .unwrap()
        .expect("mock supports subscribe");
    let mut stream = Box::pin(stream);

    // Race: update fires AFTER subscriber is attached.
    let id = EntityId::new("mock", "1");
    let id_clone = id.clone();
    let store_arc = std::sync::Arc::new(store);
    let store_for_task = std::sync::Arc::clone(&store_arc);
    let task = tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        store_for_task
            .update(
                &id_clone,
                EntityPatch {
                    fields: map_of(&[("status", json!("paused"))]),
                },
            )
            .await
            .unwrap();
    });

    let change = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
        .await
        .expect("subscribe must emit within 1s")
        .expect("stream not closed");
    assert_eq!(change.id, id);
    match change.kind {
        EntityChangeKind::Updated { fields } => {
            assert_eq!(fields, vec!["status".to_string()]);
        }
        other => panic!("expected Updated, got {other:?}"),
    }
    task.await.unwrap();
}

// ── Registry ───────────────────────────────────────────────────────

#[test]
fn registry_returns_clear_error_for_unimplemented_sheets_adapter() {
    let config = Config::default();
    let err = match open_entity_store(
        &config,
        &EntityRef::GoogleSheet {
            spreadsheet_id: "abc".into(),
            range: "A1:Z".into(),
        },
    ) {
        Err(e) => e,
        Ok(_) => panic!("expected unimplemented error"),
    };
    assert!(err
        .to_string()
        .contains("google_sheets adapter not yet implemented"));
    assert!(err.to_string().contains("F4-5"));
    assert!(err.to_string().contains("abc"));
}

#[test]
fn registry_returns_clear_error_for_unimplemented_attio_adapter() {
    let config = Config::default();
    let err = match open_entity_store(
        &config,
        &EntityRef::Attio {
            workspace_id: "ws_acme".into(),
            object_type: "people".into(),
        },
    ) {
        Err(e) => e,
        Ok(_) => panic!("expected unimplemented error"),
    };
    assert!(err
        .to_string()
        .contains("attio adapter not yet implemented"));
    assert!(err.to_string().contains("F4-6"));
    assert!(err.to_string().contains("ws_acme"));
}
