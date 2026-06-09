//! F4-9 approval queue store + ops tests. Cover the lifecycle:
//! enqueue → list_pending → approve / reject → mark_sent / mark_failed.
//! Requires the migrations applied for the campaign FK to bind.

use serde_json::json;
use tempfile::TempDir;

use super::store;
use super::types::{ApprovalDecision, ApprovalStatus, EnqueueApprovalRequest};
use crate::openhuman::campaigns::store as campaign_store;
use crate::openhuman::campaigns::types::{ApprovalPolicy, Campaign, CampaignStatus, EntityRef};
use crate::openhuman::config::Config;
use crate::openhuman::workflows::store as wf_store;
use crate::openhuman::workflows::types::{
    Trigger, Workflow, WorkflowHealth, WorkflowOrigin, WorkflowSettings,
};

fn fresh() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

/// Insert a stub campaign + workflow so the approval rows can bind
/// their FKs without violating `ON DELETE CASCADE`.
fn seed_campaign_and_workflow(config: &Config) -> (String, String) {
    let now = chrono::Utc::now();
    let campaign = Campaign {
        id: "camp-1".into(),
        schema_version: 1,
        name: "F4-9 test".into(),
        description: None,
        status: CampaignStatus::Active,
        entity_binding: EntityRef::GoogleSheet {
            spreadsheet_id: "sid".into(),
            range: "A1:B".into(),
        },
        throttle: None,
        approval_policy: ApprovalPolicy::DraftAndApprove,
        target_outcome: None,
        created_at: now,
        updated_at: now,
        last_run_at: None,
    };
    campaign_store::insert_campaign(config, &campaign).unwrap();
    let workflow = Workflow {
        id: "wf-1".into(),
        schema_version: 1,
        name: "F4-9 wf".into(),
        description: None,
        enabled: false,
        origin: WorkflowOrigin::UserChat,
        health: WorkflowHealth::Ready,
        trigger: Trigger::Manual,
        nodes: vec![],
        edges: vec![],
        settings: WorkflowSettings::default(),
        created_at: now,
        updated_at: now,
        last_run_at: None,
        campaign_id: Some("camp-1".into()),
    };
    wf_store::insert_workflow(config, &workflow).unwrap();
    ("camp-1".into(), "wf-1".into())
}

fn enqueue_one(config: &Config, campaign_id: &str, workflow_id: &str) -> String {
    store::enqueue(
        config,
        EnqueueApprovalRequest {
            campaign_id: campaign_id.into(),
            workflow_id: workflow_id.into(),
            run_id: "run-1".into(),
            node_id: "node-1".into(),
            action_kind: "channel_message".into(),
            target: "alice@x.io".into(),
            payload: json!({ "body": "draft body" }),
            context: Some(json!({ "record": { "email": "alice@x.io" } })),
        },
    )
    .unwrap()
}

// ── enqueue + get ─────────────────────────────────────────────────

#[test]
fn enqueue_persists_a_pending_row_with_all_fields() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id = enqueue_one(&config, &cid, &wid);
    let entry = store::get(&config, &id).unwrap().expect("row present");
    assert_eq!(entry.id, id);
    assert_eq!(entry.campaign_id, cid);
    assert_eq!(entry.workflow_id, wid);
    assert_eq!(entry.action_kind, "channel_message");
    assert_eq!(entry.target, "alice@x.io");
    assert_eq!(entry.payload, json!({ "body": "draft body" }));
    assert_eq!(
        entry.context,
        Some(json!({ "record": { "email": "alice@x.io" } }))
    );
    assert!(matches!(entry.status, ApprovalStatus::Pending));
    assert!(entry.decided_at.is_none());
    assert!(entry.decided_by.is_none());
    assert!(entry.error.is_none());
}

// ── list ──────────────────────────────────────────────────────────

#[test]
fn list_pending_filters_by_campaign_and_status() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id1 = enqueue_one(&config, &cid, &wid);
    let _id2 = enqueue_one(&config, &cid, &wid);
    // Approve the first one — it should drop from pending list.
    store::record_decision(&config, &id1, ApprovalStatus::Approved, "user", None).unwrap();
    let pending = store::list(&config, Some(&cid), Some(ApprovalStatus::Pending)).unwrap();
    assert_eq!(
        pending.len(),
        1,
        "only the un-approved row should be pending"
    );
    let approved = store::list(&config, Some(&cid), Some(ApprovalStatus::Approved)).unwrap();
    assert_eq!(approved.len(), 1);
    let all = store::list(&config, Some(&cid), None).unwrap();
    assert_eq!(all.len(), 2);
}

// ── record_decision ───────────────────────────────────────────────

#[test]
fn approve_sets_status_decided_at_decided_by() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id = enqueue_one(&config, &cid, &wid);
    let entry =
        store::record_decision(&config, &id, ApprovalStatus::Approved, "user", None).unwrap();
    assert!(matches!(entry.status, ApprovalStatus::Approved));
    assert_eq!(entry.decided_by.as_deref(), Some("user"));
    assert!(entry.decided_at.is_some());
}

#[test]
fn approve_with_edited_payload_swaps_the_action_body() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id = enqueue_one(&config, &cid, &wid);
    let new_body = json!({ "body": "edited body", "subject": "edited" });
    let entry = store::record_decision(
        &config,
        &id,
        ApprovalStatus::Approved,
        "user",
        Some(new_body.clone()),
    )
    .unwrap();
    assert_eq!(entry.payload, new_body);
}

#[test]
fn reject_marks_terminal_without_re_issue_path() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id = enqueue_one(&config, &cid, &wid);
    let entry =
        store::record_decision(&config, &id, ApprovalStatus::Rejected, "user", None).unwrap();
    assert!(matches!(entry.status, ApprovalStatus::Rejected));
    // Subsequent approve attempt must fail since row isn't Pending.
    let err =
        store::record_decision(&config, &id, ApprovalStatus::Approved, "user", None).unwrap_err();
    assert!(err.to_string().contains("not pending"), "got: {err}");
}

#[test]
fn record_decision_rejects_non_terminal_status() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id = enqueue_one(&config, &cid, &wid);
    let err = store::record_decision(&config, &id, ApprovalStatus::Sent, "user", None).unwrap_err();
    assert!(err.to_string().contains("must be approved or rejected"));
}

// ── mark_sent / mark_failed ───────────────────────────────────────

#[test]
fn mark_sent_only_transitions_from_approved() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id = enqueue_one(&config, &cid, &wid);
    // Pending → mark_sent must be a no-op (only matches WHERE status='approved').
    store::mark_sent(&config, &id).unwrap();
    let entry = store::get(&config, &id).unwrap().unwrap();
    assert!(matches!(entry.status, ApprovalStatus::Pending));
    // After approval, mark_sent flips.
    store::record_decision(&config, &id, ApprovalStatus::Approved, "user", None).unwrap();
    store::mark_sent(&config, &id).unwrap();
    let entry = store::get(&config, &id).unwrap().unwrap();
    assert!(matches!(entry.status, ApprovalStatus::Sent));
}

#[test]
fn mark_failed_records_error_and_flips_status() {
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id = enqueue_one(&config, &cid, &wid);
    store::record_decision(&config, &id, ApprovalStatus::Approved, "user", None).unwrap();
    store::mark_failed(&config, &id, "smtp 503 from gmail").unwrap();
    let entry = store::get(&config, &id).unwrap().unwrap();
    assert!(matches!(entry.status, ApprovalStatus::Failed));
    assert_eq!(entry.error.as_deref(), Some("smtp 503 from gmail"));
}

// ── batch_approve (via ops) ───────────────────────────────────────

#[tokio::test]
async fn batch_approve_processes_every_id_skipping_bad_ones() {
    use super::ops::batch_approve;
    let (_dir, config) = fresh();
    let (cid, wid) = seed_campaign_and_workflow(&config);
    let id1 = enqueue_one(&config, &cid, &wid);
    let id2 = enqueue_one(&config, &cid, &wid);
    let id3 = enqueue_one(&config, &cid, &wid);
    let bad_id = "definitely-does-not-exist".to_string();
    let ids = vec![id1.clone(), bad_id, id2.clone(), id3.clone()];
    let out = batch_approve(&config, ids, "user".into()).await.unwrap();
    assert_eq!(out.len(), 3, "3 valid + 1 bad → 3 approved");
    assert!(out
        .iter()
        .all(|e| matches!(e.status, ApprovalStatus::Approved)));
}

#[test]
fn approval_decision_round_trips_through_json() {
    let approve = ApprovalDecision::Approve;
    let reject = ApprovalDecision::Reject {
        reason: Some("not now".into()),
    };
    let edit = ApprovalDecision::Edit {
        new_payload: json!({"body": "x"}),
    };
    for d in [approve, reject, edit] {
        let json = serde_json::to_string(&d).unwrap();
        let back: ApprovalDecision = serde_json::from_str(&json).unwrap();
        assert_eq!(d, back);
    }
}
