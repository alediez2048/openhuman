//! Serde round-trip tests for every Phase 1 type variant.
//!
//! The wire format is the persisted format (every JSON-blob column
//! stores the same shape), so a Serde regression here is a data
//! corruption risk. We exercise every enum variant + every proposal-
//! validation error so a renamed field or tag is caught immediately.

use super::types::*;
use crate::openhuman::connections::types::ConnectionRef;
use chrono::{TimeZone, Utc};

fn sample_workflow(origin: WorkflowOrigin) -> Workflow {
    Workflow {
        id: "01HXY-test-workflow-id".into(),
        schema_version: 1,
        name: "test wf".into(),
        description: Some("for round-trip".into()),
        enabled: false,
        origin,
        health: WorkflowHealth::Ready,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "do the thing".into(),
                allowed_connections: vec![ConnectionRef::Composio {
                    toolkit_id: "gmail".into(),
                    account_id: None,
                }],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
        }],
        edges: vec![],
        settings: WorkflowSettings::default(),
        created_at: Utc.with_ymd_and_hms(2026, 5, 20, 0, 0, 0).unwrap(),
        updated_at: Utc.with_ymd_and_hms(2026, 5, 20, 0, 0, 0).unwrap(),
        last_run_at: None,
    }
}

fn assert_round_trip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let json = serde_json::to_string(value).expect("serialize");
    let back: T = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(value, &back, "round-trip mismatch via {json}");
}

#[test]
fn trigger_cron_round_trips_with_timezone_and_active_hours() {
    let value = Trigger::Cron {
        expr: "0 8 * * 1-5".into(),
        tz: Some("America/Chicago".into()),
        active_hours: Some(ActiveHours {
            start: "08:00".into(),
            end: "18:00".into(),
        }),
    };
    assert_round_trip(&value);
}

#[test]
fn trigger_manual_round_trips() {
    assert_round_trip(&Trigger::Manual);
}

#[test]
fn trigger_phase_two_stubs_round_trip() {
    // Forward-compat: variants declared from day one (Phase 2). The
    // validator will reject these for Phase 1, but the wire format must
    // survive a round-trip so the JSON columns can store legacy rows
    // when Phase 2 ships.
    let webhook = Trigger::Webhook {
        tunnel_uuid: uuid::Uuid::nil(),
        target_path: "/incoming".into(),
    };
    assert_round_trip(&webhook);
    let composio = Trigger::ComposioEvent {
        trigger_id: "GMAIL_NEW_GMAIL_MESSAGE".into(),
        toolkit: "gmail".into(),
    };
    assert_round_trip(&composio);
    let channel = Trigger::ChannelMessage {
        provider: "slack".into(),
        filter: Some(MessageFilter {
            contains: Some("urgent".into()),
            direct_only: true,
        }),
    };
    assert_round_trip(&channel);
}

#[test]
fn workflow_round_trips_with_seed_origin_preserving_template_id() {
    let wf = sample_workflow(WorkflowOrigin::Seed {
        template_id: "ru-1-founder-morning-digest".into(),
    });
    assert_round_trip(&wf);

    let json = serde_json::to_value(&wf).unwrap();
    // The catalog dedup query reads template_id directly out of the
    // persisted JSON — assert the shape that query expects.
    assert_eq!(json["origin"]["type"], "seed");
    assert_eq!(json["origin"]["template_id"], "ru-1-founder-morning-digest");
}

#[test]
fn workflow_round_trips_every_origin_variant() {
    for origin in [
        WorkflowOrigin::UserChat,
        WorkflowOrigin::UserForm,
        WorkflowOrigin::Seed {
            template_id: "ru-2".into(),
        },
        WorkflowOrigin::Imported,
    ] {
        assert_round_trip(&sample_workflow(origin));
    }
}

#[test]
fn workflow_health_needs_connections_carries_full_payload() {
    let missing = vec![
        ConnectionRef::Channel {
            provider: "telegram".into(),
            channel_id: "@me".into(),
        },
        ConnectionRef::GenericHttp {
            connection_id: "ghc-1".into(),
        },
    ];
    let health = WorkflowHealth::NeedsConnections {
        missing: missing.clone(),
    };
    assert_round_trip(&health);

    let json = serde_json::to_value(&health).unwrap();
    assert_eq!(json["type"], "needs_connections");
    assert_eq!(json["missing"].as_array().unwrap().len(), 2);
}

#[test]
fn workflow_health_all_variants_round_trip() {
    let variants = vec![
        WorkflowHealth::Ready,
        WorkflowHealth::NeedsConnections {
            missing: vec![ConnectionRef::Builtin {
                integration: "twilio".into(),
            }],
        },
        WorkflowHealth::LastRunFailed {
            run_id: "01HX-run".into(),
            reason: "timeout after 300s".into(),
        },
        WorkflowHealth::SessionExpired {
            connection: ConnectionRef::Webview {
                provider: "linkedin".into(),
                account_id: "acct-1".into(),
            },
        },
    ];
    for h in variants {
        assert_round_trip(&h);
    }
}

#[test]
fn run_status_exhaustively_round_trips() {
    for status in [
        RunStatus::Pending,
        RunStatus::Running,
        RunStatus::Succeeded,
        RunStatus::Failed,
        RunStatus::Cancelled,
        RunStatus::TimedOut,
    ] {
        assert_round_trip(&status);
    }
}

#[test]
fn trigger_source_all_variants_round_trip() {
    for src in [
        TriggerSource::Cron,
        TriggerSource::Manual {
            initiator: "user".into(),
        },
        TriggerSource::Webhook,
        TriggerSource::ComposioEvent,
        TriggerSource::ChannelMessage,
    ] {
        assert_round_trip(&src);
    }
}

#[test]
fn node_kind_exhaustively_round_trips_all_nine_variants() {
    // All 9 variants — declaring them on day one is the whole point of
    // ADR-002's "forward compat" rule. The validator (F-11) rejects
    // Phase 2 kinds via UnsupportedNodeKind; this test guards the
    // wire-format names the validator matches on.
    let kinds = [
        NodeKind::AgentPrompt,
        NodeKind::ToolCall,
        NodeKind::HttpRequest,
        NodeKind::ChannelMessage,
        NodeKind::Condition,
        NodeKind::Delay,
        NodeKind::Transform,
        NodeKind::AwaitHumanApproval,
        NodeKind::FanOut,
    ];
    for kind in kinds {
        assert_round_trip(&kind);
    }
    // Wire-format spot check — snake_case must be stable; ProposalValidation-
    // Error::UnsupportedNodeKind metric labels rely on it.
    assert_eq!(
        serde_json::to_string(&NodeKind::AwaitHumanApproval).unwrap(),
        "\"await_human_approval\""
    );
}

#[test]
fn proposal_validation_error_every_variant_round_trips() {
    let variants = vec![
        ProposalValidationError::JsonParse {
            reason: "trailing comma at 1:42".into(),
        },
        ProposalValidationError::UnknownConnection {
            r#ref: ConnectionRef::Composio {
                toolkit_id: "gmaill".into(),
                account_id: None,
            },
            candidates: vec![ConnectionRef::Composio {
                toolkit_id: "gmail".into(),
                account_id: None,
            }],
        },
        ProposalValidationError::UnsupportedNodeKind {
            node_kind: NodeKind::HttpRequest,
            phase: 1,
        },
        ProposalValidationError::InvalidCron {
            expr: "@every 2h".into(),
            parse_error: "expected 5 fields".into(),
        },
        ProposalValidationError::EdgeIntegrity {
            from: "n0".into(),
            to: "n-nope".into(),
            reason: "to id not in nodes".into(),
        },
        ProposalValidationError::MissingRequiredField {
            field: "nodes".into(),
        },
    ];
    for v in &variants {
        assert_round_trip(v);
    }
}

#[test]
fn proposal_validation_error_kind_label_is_stable_snake_case() {
    // Metrics labels — must be stable across releases, so this test
    // pins every variant. F-11 increments
    // `metrics::counter!("workflow_proposal_validation_error", "kind" => …)`
    // off these strings.
    let cases = [
        (
            ProposalValidationError::JsonParse { reason: "x".into() },
            "json_parse",
        ),
        (
            ProposalValidationError::UnknownConnection {
                r#ref: ConnectionRef::Composio {
                    toolkit_id: "x".into(),
                    account_id: None,
                },
                candidates: vec![],
            },
            "unknown_connection",
        ),
        (
            ProposalValidationError::UnsupportedNodeKind {
                node_kind: NodeKind::FanOut,
                phase: 1,
            },
            "unsupported_node_kind",
        ),
        (
            ProposalValidationError::InvalidCron {
                expr: "x".into(),
                parse_error: "y".into(),
            },
            "invalid_cron",
        ),
        (
            ProposalValidationError::EdgeIntegrity {
                from: "a".into(),
                to: "b".into(),
                reason: "x".into(),
            },
            "edge_integrity",
        ),
        (
            ProposalValidationError::MissingRequiredField { field: "x".into() },
            "missing_required_field",
        ),
    ];
    for (err, expected) in &cases {
        assert_eq!(err.kind_label(), *expected);
        // Every label is non-empty + lowercase + snake_case (no spaces
        // or uppercase letters slipped in).
        assert!(
            !expected.is_empty()
                && expected
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b == b'_'),
            "label `{expected}` is not stable snake_case"
        );
    }
}

#[test]
fn skipped_reason_round_trips() {
    assert_round_trip(&SkippedReason::AlreadyRunning);
    assert_round_trip(&SkippedReason::HealthBlocked {
        health: WorkflowHealth::NeedsConnections {
            missing: vec![ConnectionRef::Mcp {
                server_id: "higgsfield".into(),
                tool_name: None,
            }],
        },
    });
}

#[test]
fn proposal_types_round_trip() {
    let proposal = WorkflowProposal {
        name: "Test".into(),
        description: "test description".into(),
        trigger: Trigger::Manual,
        nodes: vec![],
        edges: vec![],
        settings: WorkflowSettings::default(),
        required_connections: vec![],
        rationale: vec!["because".into()],
        confidence: Confidence::High,
    };
    assert_round_trip(&proposal);

    let delete = WorkflowDeletePreview {
        workflow_id: "wf-1".into(),
        name: "Founder digest".into(),
        run_count: 5,
        retention_days: 30,
    };
    assert_round_trip(&delete);

    let state = WorkflowStateProposal {
        workflow_id: "wf-1".into(),
        action: StateAction::RunNow,
        rationale: vec!["Median run time: 12s".into()],
        enabled: true,
    };
    assert_round_trip(&state);
}

#[test]
fn workflow_settings_default_is_300s_halt() {
    let s = WorkflowSettings::default();
    assert_eq!(s.timeout_secs, 300);
    assert_eq!(s.on_error, OnErrorPolicy::Halt);
}

#[test]
fn agent_prompt_config_iteration_cap_defaults_to_twelve() {
    let json = r#"{"prompt":"x"}"#;
    let cfg: AgentPromptConfig = serde_json::from_str(json).unwrap();
    assert_eq!(cfg.iteration_cap, 12);
    assert!(cfg.allowed_connections.is_empty());
    assert!(cfg.model_tier.is_none());
}
