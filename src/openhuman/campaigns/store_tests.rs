//! F4-2 store tests: CRUD round-trip + soft-delete + workflow FK.

use super::store::{
    delete_campaign, get_campaign, get_campaign_including_deleted, insert_campaign, list_campaigns,
    list_workflow_ids_for_campaign, restore_campaign, update_campaign, ListCampaignsFilter,
};
use super::types::{
    ApprovalPolicy, Campaign, CampaignStatus, EntityRef, OutcomeSpec, Throttle, ThrottleWindow,
};
use crate::openhuman::config::Config;
use crate::openhuman::workflows::store::insert_workflow;
use crate::openhuman::workflows::types::{
    AgentPromptConfig, Node, NodeConfig, NodeKind, Trigger, Workflow, WorkflowHealth,
    WorkflowOrigin, WorkflowSettings,
};
use chrono::Utc;
use tempfile::TempDir;

fn config_with_temp_workspace() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

fn sample_campaign(id: &str, status: CampaignStatus) -> Campaign {
    let now = Utc::now();
    Campaign {
        id: id.into(),
        schema_version: 1,
        name: format!("Campaign {id}"),
        description: Some("test campaign".into()),
        status,
        entity_binding: EntityRef::GoogleSheet {
            spreadsheet_id: "abc".into(),
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
        created_at: now,
        updated_at: now,
        last_run_at: None,
    }
}

fn sample_workflow_in_campaign(id: &str, campaign_id: Option<&str>) -> Workflow {
    let now = Utc::now();
    Workflow {
        id: id.into(),
        schema_version: 1,
        name: format!("Workflow {id}"),
        description: None,
        enabled: false,
        origin: WorkflowOrigin::UserChat,
        health: WorkflowHealth::Ready,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "do thing".into(),
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
    }
}

// ── insert + get round-trip ────────────────────────────────────────────

#[test]
fn round_trip_through_insert_and_get_for_every_status_variant() {
    for status in [
        CampaignStatus::Draft,
        CampaignStatus::Active,
        CampaignStatus::Paused,
        CampaignStatus::WoundDown,
        CampaignStatus::Archived,
    ] {
        let (_dir, config) = config_with_temp_workspace();
        let id = format!("cmp_{}", status_label(status));
        let c = sample_campaign(&id, status);
        insert_campaign(&config, &c).unwrap();
        let back = get_campaign(&config, &c.id).unwrap().expect("must exist");
        assert_eq!(c, back, "round-trip mismatch for status {status:?}");
    }
}

fn status_label(s: CampaignStatus) -> &'static str {
    match s {
        CampaignStatus::Draft => "draft",
        CampaignStatus::Active => "active",
        CampaignStatus::Paused => "paused",
        CampaignStatus::WoundDown => "wound_down",
        CampaignStatus::Archived => "archived",
    }
}

#[test]
fn get_campaign_returns_none_for_unknown_id() {
    let (_dir, config) = config_with_temp_workspace();
    assert!(get_campaign(&config, &"ghost".into()).unwrap().is_none());
}

#[test]
fn campaign_without_throttle_or_outcome_round_trips() {
    let (_dir, config) = config_with_temp_workspace();
    let mut c = sample_campaign("cmp_minimal", CampaignStatus::Draft);
    c.throttle = None;
    c.target_outcome = None;
    insert_campaign(&config, &c).unwrap();
    let back = get_campaign(&config, &c.id).unwrap().unwrap();
    assert_eq!(back.throttle, None);
    assert_eq!(back.target_outcome, None);
}

// ── update ─────────────────────────────────────────────────────────────

#[test]
fn update_overwrites_mutable_fields_and_returns_true() {
    let (_dir, config) = config_with_temp_workspace();
    let mut c = sample_campaign("cmp_upd", CampaignStatus::Draft);
    insert_campaign(&config, &c).unwrap();

    c.name = "renamed".into();
    c.status = CampaignStatus::Active;
    c.updated_at = Utc::now();
    let touched = update_campaign(&config, &c).unwrap();
    assert!(touched, "update_campaign must report touched=true");

    let back = get_campaign(&config, &c.id).unwrap().unwrap();
    assert_eq!(back.name, "renamed");
    assert_eq!(back.status, CampaignStatus::Active);
}

#[test]
fn update_unknown_id_returns_false() {
    let (_dir, config) = config_with_temp_workspace();
    let c = sample_campaign("cmp_ghost", CampaignStatus::Active);
    let touched = update_campaign(&config, &c).unwrap();
    assert!(
        !touched,
        "updating a non-existent campaign must return touched=false"
    );
}

// ── list with filters ──────────────────────────────────────────────────

#[test]
fn list_campaigns_filters_by_status_and_excludes_deleted_by_default() {
    let (_dir, config) = config_with_temp_workspace();
    insert_campaign(&config, &sample_campaign("a", CampaignStatus::Active)).unwrap();
    insert_campaign(&config, &sample_campaign("b", CampaignStatus::Paused)).unwrap();
    insert_campaign(&config, &sample_campaign("c", CampaignStatus::Active)).unwrap();

    // Default: include all live, no filter.
    let all = list_campaigns(&config, ListCampaignsFilter::default()).unwrap();
    assert_eq!(all.len(), 3, "default list must include all 3 live rows");

    // Filter by status.
    let active = list_campaigns(
        &config,
        ListCampaignsFilter {
            status: Some(CampaignStatus::Active),
            include_deleted: false,
        },
    )
    .unwrap();
    assert_eq!(active.len(), 2);
    assert!(active.iter().all(|c| c.status == CampaignStatus::Active));

    // Soft-delete one, confirm default list shrinks.
    delete_campaign(&config, &"a".into()).unwrap();
    let after_delete = list_campaigns(&config, ListCampaignsFilter::default()).unwrap();
    assert_eq!(after_delete.len(), 2);
    assert!(after_delete.iter().all(|c| c.id != "a"));
}

#[test]
fn list_campaigns_include_deleted_surfaces_soft_deleted_rows() {
    let (_dir, config) = config_with_temp_workspace();
    insert_campaign(&config, &sample_campaign("x", CampaignStatus::Draft)).unwrap();
    delete_campaign(&config, &"x".into()).unwrap();

    let with_deleted = list_campaigns(
        &config,
        ListCampaignsFilter {
            status: None,
            include_deleted: true,
        },
    )
    .unwrap();
    assert_eq!(with_deleted.len(), 1, "include_deleted must surface 'x'");
}

// ── soft-delete + restore ──────────────────────────────────────────────

#[test]
fn soft_delete_excludes_from_default_get_but_visible_via_including_deleted() {
    let (_dir, config) = config_with_temp_workspace();
    let c = sample_campaign("cmp_sd", CampaignStatus::Active);
    insert_campaign(&config, &c).unwrap();

    let touched = delete_campaign(&config, &c.id).unwrap();
    assert!(touched, "delete must touch one row");

    // Default get hides it.
    assert!(get_campaign(&config, &c.id).unwrap().is_none());
    // including-deleted view surfaces it.
    let raw = get_campaign_including_deleted(&config, &c.id).unwrap();
    assert!(
        raw.is_some(),
        "soft-deleted row must be visible via including_deleted"
    );
}

#[test]
fn restore_clears_deleted_at_and_returns_row_to_default_views() {
    let (_dir, config) = config_with_temp_workspace();
    let c = sample_campaign("cmp_restore", CampaignStatus::Active);
    insert_campaign(&config, &c).unwrap();
    delete_campaign(&config, &c.id).unwrap();
    assert!(get_campaign(&config, &c.id).unwrap().is_none());

    let touched = restore_campaign(&config, &c.id).unwrap();
    assert!(touched);
    let back = get_campaign(&config, &c.id).unwrap();
    assert!(back.is_some(), "restored row must be visible again");
}

#[test]
fn restore_on_non_deleted_row_returns_false() {
    let (_dir, config) = config_with_temp_workspace();
    let c = sample_campaign("cmp_live", CampaignStatus::Active);
    insert_campaign(&config, &c).unwrap();
    let touched = restore_campaign(&config, &c.id).unwrap();
    assert!(
        !touched,
        "restore on a non-deleted row is a no-op (touched=false)"
    );
}

// ── workflows ↔ campaigns FK ──────────────────────────────────────────

#[test]
fn workflows_carry_campaign_id_round_trip() {
    let (_dir, config) = config_with_temp_workspace();
    let c = sample_campaign("cmp_fk", CampaignStatus::Active);
    insert_campaign(&config, &c).unwrap();

    let wf = sample_workflow_in_campaign("wf_in_campaign", Some(&c.id));
    insert_workflow(&config, &wf).unwrap();

    let back = crate::openhuman::workflows::store::get_workflow(&config, &wf.id)
        .unwrap()
        .expect("workflow exists");
    assert_eq!(
        back.campaign_id.as_deref(),
        Some(c.id.as_str()),
        "workflow's campaign_id must survive insert + read"
    );
}

#[test]
fn list_workflow_ids_for_campaign_returns_only_matching_rows() {
    let (_dir, config) = config_with_temp_workspace();
    insert_campaign(&config, &sample_campaign("camp_a", CampaignStatus::Active)).unwrap();
    insert_campaign(&config, &sample_campaign("camp_b", CampaignStatus::Active)).unwrap();

    insert_workflow(
        &config,
        &sample_workflow_in_campaign("wf_a1", Some("camp_a")),
    )
    .unwrap();
    insert_workflow(
        &config,
        &sample_workflow_in_campaign("wf_a2", Some("camp_a")),
    )
    .unwrap();
    insert_workflow(
        &config,
        &sample_workflow_in_campaign("wf_b1", Some("camp_b")),
    )
    .unwrap();
    insert_workflow(&config, &sample_workflow_in_campaign("wf_standalone", None)).unwrap();

    let in_a = list_workflow_ids_for_campaign(&config, &"camp_a".into()).unwrap();
    assert_eq!(in_a.len(), 2);
    assert!(in_a.contains(&"wf_a1".to_string()));
    assert!(in_a.contains(&"wf_a2".to_string()));
    assert!(!in_a.contains(&"wf_b1".to_string()));
    assert!(!in_a.contains(&"wf_standalone".to_string()));
}

#[test]
fn workflow_without_campaign_id_continues_to_work() {
    // Backwards-compat: standalone workflows (the Phase 1+2 shape)
    // round-trip with campaign_id = None even after migration 008
    // adds the column.
    let (_dir, config) = config_with_temp_workspace();
    let wf = sample_workflow_in_campaign("wf_solo", None);
    insert_workflow(&config, &wf).unwrap();
    let back = crate::openhuman::workflows::store::get_workflow(&config, &wf.id)
        .unwrap()
        .unwrap();
    assert!(back.campaign_id.is_none());
}

#[test]
fn deleting_campaign_sets_workflow_campaign_id_to_null_via_fk_cascade() {
    // ON DELETE SET NULL on workflows.campaign_id: when the campaign
    // row is HARD-deleted (retention sweep), child workflows survive
    // with campaign_id = NULL. We exercise this by direct SQL hard-
    // delete since soft-delete leaves the row + FK in place.
    let (_dir, config) = config_with_temp_workspace();
    let c = sample_campaign("cmp_cascade", CampaignStatus::Active);
    insert_campaign(&config, &c).unwrap();
    let wf = sample_workflow_in_campaign("wf_child", Some(&c.id));
    insert_workflow(&config, &wf).unwrap();

    // Hard-delete the campaign row directly.
    crate::openhuman::workflows::store::with_connection(&config, |db| {
        db.execute(
            "DELETE FROM campaigns WHERE id = ?1",
            rusqlite::params![c.id],
        )?;
        Ok(())
    })
    .unwrap();

    let back = crate::openhuman::workflows::store::get_workflow(&config, &wf.id)
        .unwrap()
        .expect("workflow row must survive — orphan, not deleted");
    assert!(
        back.campaign_id.is_none(),
        "FK ON DELETE SET NULL must clear campaign_id on parent hard-delete"
    );
}

// ── migration ──────────────────────────────────────────────────────────

#[test]
fn migration_008_replays_cleanly() {
    let (_dir, config) = config_with_temp_workspace();
    // First open creates DB + runs all migrations.
    insert_campaign(&config, &sample_campaign("a", CampaignStatus::Active)).unwrap();
    // Re-open: migration runner must be idempotent and not touch
    // existing campaign rows.
    insert_campaign(&config, &sample_campaign("b", CampaignStatus::Draft)).unwrap();
    let rows = list_campaigns(&config, ListCampaignsFilter::default()).unwrap();
    assert_eq!(
        rows.len(),
        2,
        "migration replay across opens must preserve previously-inserted rows"
    );
}
