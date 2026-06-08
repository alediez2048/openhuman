//! F4-3 ops tests — lifecycle invariants + workflow cascade behavior.

use super::ops;
use super::store::{insert_campaign, ListCampaignsFilter};
use super::types::{
    ApprovalPolicy, Campaign, CampaignOpError, CampaignPatch, CampaignStatus,
    CreateCampaignRequest, EntityRef, OutcomeSpec, Throttle, ThrottleWindow, UpdateCampaignRequest,
};
use crate::core::event_bus::init_global;
use crate::openhuman::config::Config;
use crate::openhuman::workflows::store::{get_workflow, insert_workflow};
use crate::openhuman::workflows::types::{
    AgentPromptConfig, Node, NodeConfig, NodeKind, Trigger, Workflow, WorkflowHealth,
    WorkflowOrigin, WorkflowSettings,
};
use chrono::Utc;
use tempfile::TempDir;

fn ensure_bus() {
    // ops::pause / resume publish DomainEvents; the bus singleton
    // must be initialised or `publish_global` no-ops silently. Tests
    // that assert events would race; we only assert state today.
    let _ = init_global(128);
}

fn temp_config() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

fn sample_create_request(name: &str) -> CreateCampaignRequest {
    CreateCampaignRequest {
        name: name.into(),
        description: Some("test".into()),
        entity_binding: EntityRef::GoogleSheet {
            spreadsheet_id: "ss".into(),
            range: "Sheet1!A1:H100".into(),
        },
        throttle: Some(Throttle {
            max_per_window: 20,
            window: ThrottleWindow::PerDay,
        }),
        approval_policy: ApprovalPolicy::DraftAndApprove,
        target_outcome: Some(OutcomeSpec {
            metric: "replies".into(),
            target: 10.0,
            deadline: None,
        }),
    }
}

fn seed_campaign(config: &Config, id: &str, status: CampaignStatus) -> Campaign {
    let now = Utc::now();
    let c = Campaign {
        id: id.into(),
        schema_version: 1,
        name: format!("Campaign {id}"),
        description: None,
        status,
        entity_binding: EntityRef::GoogleSheet {
            spreadsheet_id: "ss".into(),
            range: "A1:Z".into(),
        },
        throttle: None,
        approval_policy: ApprovalPolicy::DraftAndApprove,
        target_outcome: None,
        created_at: now,
        updated_at: now,
        last_run_at: None,
    };
    insert_campaign(config, &c).unwrap();
    c
}

fn seed_workflow(config: &Config, id: &str, enabled: bool, campaign_id: Option<&str>) {
    let now = Utc::now();
    let wf = Workflow {
        id: id.into(),
        schema_version: 1,
        name: format!("Workflow {id}"),
        description: None,
        enabled,
        origin: WorkflowOrigin::UserChat,
        health: WorkflowHealth::Ready,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: vec![],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
            retry_policy: None,
        }],
        edges: vec![],
        settings: WorkflowSettings::default(),
        created_at: now,
        updated_at: now,
        last_run_at: None,
        campaign_id: campaign_id.map(str::to_string),
    };
    insert_workflow(config, &wf).unwrap();
}

// ── CRUD via ops ───────────────────────────────────────────────────────

#[tokio::test]
async fn create_stamps_id_and_sets_status_draft() {
    ensure_bus();
    let (_dir, config) = temp_config();
    let req = sample_create_request("Acme");
    let result = ops::create(&config, req).await.expect("create").value;
    assert_eq!(result.status, CampaignStatus::Draft);
    assert!(!result.id.is_empty());
    assert!(result.last_run_at.is_none());
}

#[tokio::test]
async fn get_returns_none_for_unknown_id() {
    ensure_bus();
    let (_dir, config) = temp_config();
    let out = ops::get(&config, "ghost".into()).await.unwrap().value;
    assert!(out.is_none());
}

#[tokio::test]
async fn update_applies_patch_and_returns_updated() {
    ensure_bus();
    let (_dir, config) = temp_config();
    let c = seed_campaign(&config, "u1", CampaignStatus::Active);
    let patched = ops::update(
        &config,
        UpdateCampaignRequest {
            id: c.id.clone(),
            patch: CampaignPatch {
                name: Some("renamed".into()),
                description: Some("new desc".into()),
                ..Default::default()
            },
        },
    )
    .await
    .unwrap()
    .value;
    assert_eq!(patched.name, "renamed");
    assert_eq!(patched.description.as_deref(), Some("new desc"));
    // status untouched — patch never goes through update for status
    assert_eq!(patched.status, CampaignStatus::Active);
}

#[tokio::test]
async fn update_unknown_id_returns_not_found() {
    ensure_bus();
    let (_dir, config) = temp_config();
    let err = ops::update(
        &config,
        UpdateCampaignRequest {
            id: "ghost".into(),
            patch: CampaignPatch::default(),
        },
    )
    .await
    .unwrap_err();
    assert!(matches!(err, CampaignOpError::NotFound { .. }));
    assert_eq!(err.code(), "not_found");
}

// ── Lifecycle invariants ──────────────────────────────────────────────

#[tokio::test]
async fn pause_succeeds_from_active_and_rejects_from_draft() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "active", CampaignStatus::Active);
    let ok = ops::pause(&config, "active".into()).await.unwrap().value;
    assert_eq!(ok.status, CampaignStatus::Paused);

    seed_campaign(&config, "draft", CampaignStatus::Draft);
    let err = ops::pause(&config, "draft".into()).await.unwrap_err();
    assert!(matches!(err, CampaignOpError::InvalidTransition { .. }));
    assert_eq!(err.code(), "invalid_transition");
}

#[tokio::test]
async fn resume_from_draft_or_paused_succeeds() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "from_draft", CampaignStatus::Draft);
    let a = ops::resume(&config, "from_draft".into())
        .await
        .unwrap()
        .value;
    assert_eq!(a.status, CampaignStatus::Active);

    seed_campaign(&config, "from_paused", CampaignStatus::Paused);
    let b = ops::resume(&config, "from_paused".into())
        .await
        .unwrap()
        .value;
    assert_eq!(b.status, CampaignStatus::Active);
}

#[tokio::test]
async fn archive_only_from_wound_down() {
    ensure_bus();
    let (_dir, config) = temp_config();
    // Direct Active → Archived must fail (must wind_down first).
    seed_campaign(&config, "active2", CampaignStatus::Active);
    let err = ops::archive(&config, "active2".into()).await.unwrap_err();
    assert!(matches!(err, CampaignOpError::InvalidTransition { .. }));

    // WoundDown → Archived succeeds.
    seed_campaign(&config, "wd", CampaignStatus::WoundDown);
    let ok = ops::archive(&config, "wd".into()).await.unwrap().value;
    assert_eq!(ok.status, CampaignStatus::Archived);
}

#[tokio::test]
async fn archived_is_terminal() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "archived", CampaignStatus::Archived);
    let err = ops::resume(&config, "archived".into()).await.unwrap_err();
    assert!(matches!(err, CampaignOpError::InvalidTransition { .. }));
}

#[tokio::test]
async fn re_pausing_a_paused_campaign_is_idempotent_noop() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "already", CampaignStatus::Paused);
    let out = ops::pause(&config, "already".into()).await.unwrap().value;
    // Self-transition succeeds (no error), status stays Paused.
    assert_eq!(out.status, CampaignStatus::Paused);
}

// ── Cascade behavior ──────────────────────────────────────────────────

#[tokio::test]
async fn pause_disables_every_child_workflow() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "cmp", CampaignStatus::Active);
    seed_workflow(&config, "wf_a", true, Some("cmp"));
    seed_workflow(&config, "wf_b", true, Some("cmp"));
    // Standalone workflow — must NOT be touched.
    seed_workflow(&config, "wf_solo", true, None);

    ops::pause(&config, "cmp".into()).await.unwrap();

    let a = get_workflow(&config, &"wf_a".into()).unwrap().unwrap();
    let b = get_workflow(&config, &"wf_b".into()).unwrap().unwrap();
    let solo = get_workflow(&config, &"wf_solo".into()).unwrap().unwrap();
    assert!(!a.enabled, "child wf_a must be disabled after pause");
    assert!(!b.enabled, "child wf_b must be disabled after pause");
    assert!(solo.enabled, "standalone workflow must NOT be touched");
}

#[tokio::test]
async fn resume_re_enables_every_child_workflow() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "cmp", CampaignStatus::Paused);
    seed_workflow(&config, "wf_a", false, Some("cmp"));
    seed_workflow(&config, "wf_b", false, Some("cmp"));

    ops::resume(&config, "cmp".into()).await.unwrap();

    let a = get_workflow(&config, &"wf_a".into()).unwrap().unwrap();
    let b = get_workflow(&config, &"wf_b".into()).unwrap().unwrap();
    assert!(a.enabled);
    assert!(b.enabled);
}

#[tokio::test]
async fn archive_disables_every_child_workflow() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "cmp", CampaignStatus::WoundDown);
    seed_workflow(&config, "wf_a", true, Some("cmp"));

    ops::archive(&config, "cmp".into()).await.unwrap();
    let a = get_workflow(&config, &"wf_a".into()).unwrap().unwrap();
    assert!(!a.enabled, "archive must disable every child workflow");
}

// ── Workflow link / unlink ────────────────────────────────────────────

#[tokio::test]
async fn add_workflow_links_existing_standalone_workflow() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "cmp", CampaignStatus::Active);
    seed_workflow(&config, "wf_solo", false, None);

    let touched = ops::add_workflow(&config, "cmp".into(), "wf_solo".into())
        .await
        .unwrap()
        .value;
    assert!(touched);

    let wf = get_workflow(&config, &"wf_solo".into()).unwrap().unwrap();
    assert_eq!(wf.campaign_id.as_deref(), Some("cmp"));
}

#[tokio::test]
async fn add_workflow_returns_false_for_unknown_workflow() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "cmp", CampaignStatus::Active);
    let touched = ops::add_workflow(&config, "cmp".into(), "ghost".into())
        .await
        .unwrap()
        .value;
    assert!(!touched);
}

#[tokio::test]
async fn add_workflow_errors_when_campaign_unknown() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_workflow(&config, "wf", false, None);
    let err = ops::add_workflow(&config, "ghost".into(), "wf".into())
        .await
        .unwrap_err();
    assert!(matches!(err, CampaignOpError::NotFound { .. }));
}

#[tokio::test]
async fn remove_workflow_unlinks_only_when_linked_to_this_campaign() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "cmp_a", CampaignStatus::Active);
    seed_campaign(&config, "cmp_b", CampaignStatus::Active);
    seed_workflow(&config, "wf", false, Some("cmp_a"));

    // Wrong campaign → no-op.
    let touched = ops::remove_workflow(&config, "cmp_b".into(), "wf".into())
        .await
        .unwrap()
        .value;
    assert!(!touched);

    // Correct campaign → unlinks.
    let touched = ops::remove_workflow(&config, "cmp_a".into(), "wf".into())
        .await
        .unwrap()
        .value;
    assert!(touched);
    let wf = get_workflow(&config, &"wf".into()).unwrap().unwrap();
    assert!(wf.campaign_id.is_none());
}

// ── Soft-delete + restore ─────────────────────────────────────────────

#[tokio::test]
async fn delete_then_restore_round_trips() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "x", CampaignStatus::Active);
    assert!(ops::delete(&config, "x".into()).await.unwrap().value);
    assert!(ops::get(&config, "x".into()).await.unwrap().value.is_none());
    let restored = ops::restore(&config, "x".into()).await.unwrap().value;
    assert!(restored.is_some());
    assert!(ops::get(&config, "x".into()).await.unwrap().value.is_some());
}

#[tokio::test]
async fn list_default_excludes_deleted_include_deleted_surfaces() {
    ensure_bus();
    let (_dir, config) = temp_config();
    seed_campaign(&config, "live", CampaignStatus::Active);
    seed_campaign(&config, "gone", CampaignStatus::Active);
    ops::delete(&config, "gone".into()).await.unwrap();

    let default = ops::list(&config, ListCampaignsFilter::default())
        .await
        .unwrap()
        .value;
    assert_eq!(default.len(), 1);
    assert_eq!(default[0].id, "live");

    let all = ops::list(
        &config,
        ListCampaignsFilter {
            status: None,
            include_deleted: true,
        },
    )
    .await
    .unwrap()
    .value;
    assert_eq!(all.len(), 2);
}
