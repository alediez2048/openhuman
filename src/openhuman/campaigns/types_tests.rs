//! Round-trip + lifecycle tests for F4-1 types.

use super::types::*;
use chrono::{TimeZone, Utc};

fn ts() -> chrono::DateTime<chrono::Utc> {
    Utc.timestamp_millis_opt(1_780_000_000_000).unwrap()
}

fn sample_campaign() -> Campaign {
    Campaign {
        id: "cmp_acme_outreach".into(),
        schema_version: 1,
        name: "Acme vendor outreach".into(),
        description: Some("30-day daily 20/vendor cadence".into()),
        status: CampaignStatus::Active,
        entity_binding: EntityRef::GoogleSheet {
            spreadsheet_id: "1abcDEFsheetId".into(),
            range: "Vendors!A1:H1000".into(),
        },
        throttle: Some(Throttle {
            max_per_window: 20,
            window: ThrottleWindow::PerDay,
        }),
        approval_policy: ApprovalPolicy::DraftAndApprove,
        target_outcome: Some(OutcomeSpec {
            metric: "meetings_booked".into(),
            target: 20.0,
            deadline: Some(ts()),
        }),
        created_at: ts(),
        updated_at: ts(),
        last_run_at: None,
    }
}

#[test]
fn campaign_round_trips_through_json() {
    let c = sample_campaign();
    let json = serde_json::to_string(&c).expect("serialise");
    let back: Campaign = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(c, back, "Campaign round-trip must be byte-identical");
}

#[test]
fn campaign_status_every_variant_round_trips() {
    for status in [
        CampaignStatus::Draft,
        CampaignStatus::Active,
        CampaignStatus::Paused,
        CampaignStatus::WoundDown,
        CampaignStatus::Archived,
    ] {
        let json = serde_json::to_string(&status).unwrap();
        let back: CampaignStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, back);
    }
}

#[test]
fn entity_ref_google_sheet_round_trips() {
    let r = EntityRef::GoogleSheet {
        spreadsheet_id: "abc".into(),
        range: "Sheet1!A:Z".into(),
    };
    let json = serde_json::to_string(&r).unwrap();
    // Tag shape: `{"type":"google_sheet",...}`
    assert!(json.contains(r#""type":"google_sheet""#));
    let back: EntityRef = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn entity_ref_attio_round_trips() {
    let r = EntityRef::Attio {
        workspace_id: "ws_acme".into(),
        object_type: "people".into(),
    };
    let json = serde_json::to_string(&r).unwrap();
    assert!(json.contains(r#""type":"attio""#));
    let back: EntityRef = serde_json::from_str(&json).unwrap();
    assert_eq!(r, back);
}

#[test]
fn throttle_window_every_variant_round_trips() {
    for w in [
        ThrottleWindow::PerDay,
        ThrottleWindow::PerHour,
        ThrottleWindow::PerMinute,
    ] {
        let t = Throttle {
            max_per_window: 20,
            window: w,
        };
        let json = serde_json::to_string(&t).unwrap();
        let back: Throttle = serde_json::from_str(&json).unwrap();
        assert_eq!(t, back);
    }
}

#[test]
fn approval_policy_every_variant_round_trips() {
    for p in [
        ApprovalPolicy::AutoReply,
        ApprovalPolicy::DraftAndApprove,
        ApprovalPolicy::Triage,
        ApprovalPolicy::Tiered { rules: vec![] },
        ApprovalPolicy::Tiered {
            rules: vec![TierRule {
                r#match: "from:trusted@acme.com".into(),
                then: NonTieredApprovalMode::AutoReply,
            }],
        },
    ] {
        let json = serde_json::to_string(&p).unwrap();
        let back: ApprovalPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back, "ApprovalPolicy round-trip drift on variant: {p:?}");
    }
}

#[test]
fn tiered_policy_with_empty_rules_is_valid() {
    // OQ-T-4 of ticket: `Tiered { rules: vec![] }` is structurally
    // valid (falls back to last default at evaluation time).
    let p = ApprovalPolicy::Tiered { rules: vec![] };
    let json = serde_json::to_string(&p).unwrap();
    let back: ApprovalPolicy = serde_json::from_str(&json).unwrap();
    assert_eq!(p, back);
}

#[test]
fn outcome_spec_without_deadline_round_trips() {
    let o = OutcomeSpec {
        metric: "replies_received".into(),
        target: 100.0,
        deadline: None,
    };
    let json = serde_json::to_string(&o).unwrap();
    let back: OutcomeSpec = serde_json::from_str(&json).unwrap();
    assert_eq!(o, back);
}

// ── lifecycle machine ──────────────────────────────────────────────

#[test]
fn campaign_status_can_transition_to_legal_paths() {
    use CampaignStatus::*;
    // Draft → Active
    assert!(Draft.can_transition_to(Active));
    // Active ⇄ Paused
    assert!(Active.can_transition_to(Paused));
    assert!(Paused.can_transition_to(Active));
    // Any non-Archived → WoundDown
    assert!(Draft.can_transition_to(WoundDown));
    assert!(Active.can_transition_to(WoundDown));
    assert!(Paused.can_transition_to(WoundDown));
    // WoundDown → Archived
    assert!(WoundDown.can_transition_to(Archived));
    // Self-transitions are idempotent
    assert!(Active.can_transition_to(Active));
    assert!(Archived.can_transition_to(Archived));
}

#[test]
fn campaign_status_rejects_illegal_transitions() {
    use CampaignStatus::*;
    // Cannot revive Archived → anything else
    assert!(!Archived.can_transition_to(Active));
    assert!(!Archived.can_transition_to(Draft));
    assert!(!Archived.can_transition_to(Paused));
    // Cannot jump Draft → Paused (must go through Active)
    assert!(!Draft.can_transition_to(Paused));
    // Cannot resurrect WoundDown to Active/Paused
    assert!(!WoundDown.can_transition_to(Active));
    assert!(!WoundDown.can_transition_to(Paused));
    // Cannot jump Active/Paused directly to Archived (must WoundDown first)
    assert!(!Active.can_transition_to(Archived));
    assert!(!Paused.can_transition_to(Archived));
}

// ── Workflow.campaign_id additive field ─────────────────────────────

#[test]
fn workflow_without_campaign_id_still_deserialises() {
    // Backwards-compat: pre-F4 workflows have no campaign_id field.
    // `#[serde(default)]` on the new field must mean missing = None.
    use crate::openhuman::workflows::types::{
        Edge, Node, NodeConfig, OnErrorPolicy, Trigger, Workflow, WorkflowHealth, WorkflowOrigin,
        WorkflowSettings,
    };
    let pre_f4 = serde_json::json!({
        "id": "wf_legacy",
        "schema_version": 1,
        "name": "legacy",
        "enabled": false,
        "origin": { "type": "user_chat" },
        "health": { "type": "ready" },
        "trigger": { "type": "manual" },
        "nodes": [],
        "edges": [],
        "settings": { "timeout_secs": 300, "on_error": "halt" },
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
        // NOTE: no campaign_id field
    });
    let wf: Workflow =
        serde_json::from_value(pre_f4).expect("pre-F4 workflow JSON must still deserialise");
    assert_eq!(wf.id, "wf_legacy");
    assert!(
        wf.campaign_id.is_none(),
        "missing campaign_id must default to None"
    );
    // Touch the typed deps so the test fails fast on a type rename.
    let _ = (
        Edge {
            from: "a".into(),
            to: "b".into(),
        },
        Node {
            id: "n".into(),
            kind: crate::openhuman::workflows::types::NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(
                crate::openhuman::workflows::types::AgentPromptConfig {
                    prompt: "".into(),
                    allowed_connections: vec![],
                    iteration_cap: 0,
                    model_tier: None,
                },
            ),
            position: None,
            retry_policy: None,
        },
        Trigger::Manual,
        WorkflowHealth::Ready,
        WorkflowOrigin::UserChat,
        WorkflowSettings {
            timeout_secs: 300,
            on_error: OnErrorPolicy::Halt,
        },
    );
}

#[test]
fn workflow_with_campaign_id_round_trips() {
    use crate::openhuman::workflows::types::Workflow;
    let json = serde_json::json!({
        "id": "wf_with_campaign",
        "schema_version": 1,
        "name": "outbound batch",
        "enabled": true,
        "origin": { "type": "user_chat" },
        "health": { "type": "ready" },
        "trigger": { "type": "manual" },
        "nodes": [],
        "edges": [],
        "settings": { "timeout_secs": 300, "on_error": "halt" },
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "campaign_id": "cmp_acme_outreach"
    });
    let wf: Workflow = serde_json::from_value(json).unwrap();
    assert_eq!(wf.campaign_id.as_deref(), Some("cmp_acme_outreach"));
}
