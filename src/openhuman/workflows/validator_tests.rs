//! F-11 — validator unit tests.
//!
//! One test per `ProposalValidationError` variant per NFR-2.6.5,
//! plus the positive path and the < 50 ms timing assertion.

use super::types::*;
use super::validator::{allowed_node_kinds, fuzzy_candidates, validate};
use crate::openhuman::connections::types::{ConnectionRef, ConnectionStatus, ConnectionView};
use crate::openhuman::connections::verification::{Verification, VerificationResult};
use crate::openhuman::workflows::health::ConnectionsSnapshot;
use chrono::{TimeZone, Utc};

// ── Test fixtures ──────────────────────────────────────────────────────

fn live_view(r#ref: ConnectionRef, requires_verification: bool) -> ConnectionView {
    ConnectionView {
        r#ref,
        display_name: "test".into(),
        status: ConnectionStatus::Connected,
        last_used_at: None,
        mechanism_label: "test".into(),
        verification: if requires_verification {
            Some(Verification {
                last_probed_at: Utc.with_ymd_and_hms(2026, 5, 20, 0, 0, 0).unwrap(),
                result: VerificationResult::Live,
            })
        } else {
            None
        },
    }
}

fn composio_view(toolkit: &str) -> ConnectionView {
    live_view(
        ConnectionRef::Composio {
            toolkit_id: toolkit.into(),
            account_id: None,
        },
        /* requires_verification = */ false,
    )
}

fn agent_node(id: &str, allowed: Vec<ConnectionRef>) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::AgentPrompt,
        config: NodeConfig::AgentPrompt(AgentPromptConfig {
            prompt: "do the thing".into(),
            allowed_connections: allowed,
            iteration_cap: 12,
            model_tier: None,
        }),
        position: None,
        retry_policy: None,
    }
}

/// Build a baseline-valid proposal that touches the cron + edge +
/// connection paths without tripping any of them. Tests then mutate
/// one field to assert a specific failure mode.
fn valid_proposal() -> WorkflowProposal {
    WorkflowProposal {
        name: "Morning digest".into(),
        description: "Send me a 7am summary".into(),
        trigger: Trigger::Cron {
            expr: "0 7 * * *".into(),
            tz: Some("UTC".into()),
            active_hours: None,
        },
        nodes: vec![agent_node("n1", vec![])],
        edges: vec![],
        settings: WorkflowSettings::default(),
        required_connections: vec![],
        rationale: vec![],
        confidence: Confidence::High,
    }
}

// ── allowed_node_kinds ─────────────────────────────────────────────────

#[test]
fn allowed_node_kinds_phase_1_is_only_agent_prompt() {
    let kinds = allowed_node_kinds(1);
    assert_eq!(kinds, &[NodeKind::AgentPrompt]);
}

#[test]
fn allowed_node_kinds_phase_2_adds_phase_2_kinds() {
    let kinds = allowed_node_kinds(2);
    assert!(kinds.contains(&NodeKind::AgentPrompt));
    assert!(kinds.contains(&NodeKind::HttpRequest));
    assert!(kinds.contains(&NodeKind::ChannelMessage));
    assert!(!kinds.contains(&NodeKind::FanOut));
}

// ── Positive path ──────────────────────────────────────────────────────

#[test]
fn validate_accepts_baseline_valid_proposal() {
    let proposal = valid_proposal();
    let snapshot = ConnectionsSnapshot::empty();
    let result = validate(&proposal, &snapshot, /* phase = */ 1);
    assert!(result.is_ok(), "valid_proposal should pass: {result:?}");
}

#[test]
fn validate_passes_when_all_required_connections_are_live_in_snapshot() {
    let mut proposal = valid_proposal();
    proposal.required_connections = vec![ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    }];
    let snapshot = ConnectionsSnapshot::new(vec![composio_view("gmail")]);
    assert!(validate(&proposal, &snapshot, 1).is_ok());
}

// ── MissingRequiredField ───────────────────────────────────────────────

#[test]
fn validate_rejects_empty_name_with_missing_required_field() {
    let mut proposal = valid_proposal();
    proposal.name = "   ".into();
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    assert_eq!(err.kind_label(), "missing_required_field");
    match err {
        ProposalValidationError::MissingRequiredField { field } => assert_eq!(field, "name"),
        other => panic!("expected MissingRequiredField {{ name }}, got {other:?}"),
    }
}

#[test]
fn validate_rejects_empty_description() {
    let mut proposal = valid_proposal();
    proposal.description = String::new();
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    match err {
        ProposalValidationError::MissingRequiredField { field } => assert_eq!(field, "description"),
        other => panic!("expected MissingRequiredField {{ description }}, got {other:?}"),
    }
}

#[test]
fn validate_rejects_empty_nodes() {
    let mut proposal = valid_proposal();
    proposal.nodes.clear();
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    match err {
        ProposalValidationError::MissingRequiredField { field } => assert_eq!(field, "nodes"),
        other => panic!("expected MissingRequiredField {{ nodes }}, got {other:?}"),
    }
}

// ── UnsupportedNodeKind ────────────────────────────────────────────────

#[test]
fn validate_rejects_phase_2_kind_in_phase_1() {
    let mut proposal = valid_proposal();
    // Force a Phase-2 node kind (a config-less kind doesn't matter for
    // the validator here — the kind check fires before the config
    // walk).
    proposal.nodes[0].kind = NodeKind::HttpRequest;
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    assert_eq!(err.kind_label(), "unsupported_node_kind");
    match err {
        ProposalValidationError::UnsupportedNodeKind { node_kind, phase } => {
            assert_eq!(node_kind, NodeKind::HttpRequest);
            assert_eq!(phase, 1);
        }
        other => panic!("expected UnsupportedNodeKind, got {other:?}"),
    }
}

// ── InvalidCron ────────────────────────────────────────────────────────

#[test]
fn validate_rejects_bad_cron_expression() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::Cron {
        expr: "@every 2h".into(),
        tz: None,
        active_hours: None,
    };
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    assert_eq!(err.kind_label(), "invalid_cron");
    match err {
        ProposalValidationError::InvalidCron { expr, parse_error } => {
            assert_eq!(expr, "@every 2h");
            assert!(!parse_error.is_empty(), "parse_error must be set");
        }
        other => panic!("expected InvalidCron, got {other:?}"),
    }
}

#[test]
fn validate_accepts_5_field_cron_via_normalize_expression() {
    // The cron crate is 6-field native; our normalize_expression
    // prepends a `0` seconds field. The validator must accept the
    // common 5-field form.
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::Cron {
        expr: "*/15 * * * *".into(),
        tz: None,
        active_hours: None,
    };
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 1).is_ok());
}

// ── F2-15: active_hours validation ─────────────────────────────────────

fn cron_with_active_hours(start: &str, end: &str) -> Trigger {
    Trigger::Cron {
        expr: "0 9 * * *".into(),
        tz: None,
        active_hours: Some(ActiveHours {
            start: start.into(),
            end: end.into(),
        }),
    }
}

#[test]
fn validate_accepts_active_hours_with_valid_window() {
    let mut proposal = valid_proposal();
    proposal.trigger = cron_with_active_hours("09:00", "17:00");
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 1).is_ok());
}

#[test]
fn validate_rejects_active_hours_with_start_greater_than_or_equal_to_end() {
    let mut proposal = valid_proposal();
    proposal.trigger = cron_with_active_hours("17:00", "09:00");
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig {
            node_id, reason, ..
        } => {
            assert_eq!(node_id, "trigger");
            assert!(
                reason.contains("wraparound") || reason.contains("start"),
                "reason must explain ordering; got: {reason}"
            );
        }
        other => panic!("expected InvalidNodeConfig for active_hours order; got {other:?}"),
    }
}

#[test]
fn validate_rejects_active_hours_with_equal_start_and_end() {
    let mut proposal = valid_proposal();
    proposal.trigger = cron_with_active_hours("09:00", "09:00");
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 1).is_err());
}

#[test]
fn validate_rejects_active_hours_with_malformed_time_strings() {
    let mut proposal = valid_proposal();
    proposal.trigger = cron_with_active_hours("9:00", "17:00"); // missing zero-pad
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    assert!(
        matches!(err, ProposalValidationError::InvalidNodeConfig { ref reason, .. }
            if reason.contains("HH:MM")),
        "expected HH:MM format reject; got {err:?}"
    );

    let mut proposal = valid_proposal();
    proposal.trigger = cron_with_active_hours("25:00", "26:00"); // hour > 23
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 1).is_err());

    let mut proposal = valid_proposal();
    proposal.trigger = cron_with_active_hours("09:60", "17:00"); // minute > 59
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 1).is_err());
}

// ── F2-10: Composio trigger validation ─────────────────────────────────

#[test]
fn validate_rejects_composio_event_with_empty_toolkit() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ComposioEvent {
        toolkit: "   ".into(),
        trigger_id: "GMAIL_NEW_GMAIL_MESSAGE".into(),
    };
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 2).unwrap_err();
    match err {
        ProposalValidationError::MissingRequiredField { field } => {
            assert_eq!(field, "trigger.toolkit");
        }
        other => panic!("expected MissingRequiredField {{ trigger.toolkit }}, got {other:?}"),
    }
}

#[test]
fn validate_rejects_composio_event_with_empty_trigger_id() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ComposioEvent {
        toolkit: "gmail".into(),
        trigger_id: String::new(),
    };
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 2).unwrap_err();
    match err {
        ProposalValidationError::MissingRequiredField { field } => {
            assert_eq!(field, "trigger.trigger_id");
        }
        other => panic!("expected MissingRequiredField {{ trigger.trigger_id }}, got {other:?}"),
    }
}

#[test]
fn validate_accepts_composio_event_with_populated_fields() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ComposioEvent {
        toolkit: "gmail".into(),
        trigger_id: "GMAIL_NEW_GMAIL_MESSAGE".into(),
    };
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 2).is_ok());
}

// ── F2-11: ChannelMessage trigger validation ───────────────────────────

#[test]
fn validate_rejects_channel_message_with_empty_provider() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ChannelMessage {
        provider: "   ".into(),
        filter: None,
    };
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 2).unwrap_err();
    match err {
        ProposalValidationError::MissingRequiredField { field } => {
            assert_eq!(field, "trigger.provider");
        }
        other => panic!("expected MissingRequiredField {{ trigger.provider }}, got {other:?}"),
    }
}

#[test]
fn validate_rejects_channel_message_filter_with_empty_from_user() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ChannelMessage {
        provider: "slack".into(),
        filter: Some(MessageFilter {
            from_user: Some("   ".into()),
            ..Default::default()
        }),
    };
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 2).unwrap_err();
    match err {
        ProposalValidationError::MissingRequiredField { field } => {
            assert_eq!(field, "trigger.filter.from_user");
        }
        other => {
            panic!("expected MissingRequiredField {{ trigger.filter.from_user }}, got {other:?}")
        }
    }
}

#[test]
fn validate_rejects_channel_message_filter_with_invalid_regex() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ChannelMessage {
        provider: "slack".into(),
        filter: Some(MessageFilter {
            // Unclosed group — guaranteed to fail to compile.
            regex: Some("(unbalanced".into()),
            ..Default::default()
        }),
    };
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 2).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig {
            node_id,
            node_kind,
            reason,
        } => {
            assert_eq!(node_id, "trigger");
            assert_eq!(node_kind, NodeKind::ChannelMessage);
            assert!(
                reason.contains("regex"),
                "reason must mention regex; got: {reason}"
            );
        }
        other => panic!("expected InvalidNodeConfig for trigger regex; got {other:?}"),
    }
}

#[test]
fn validate_accepts_channel_message_with_valid_filter() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ChannelMessage {
        provider: "slack".into(),
        filter: Some(MessageFilter {
            contains: Some("urgent".into()),
            direct_only: true,
            from_user: Some("U42".into()),
            regex: Some(r"^@bot help".into()),
        }),
    };
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 2).is_ok());
}

#[test]
fn validate_accepts_channel_message_with_no_filter() {
    let mut proposal = valid_proposal();
    proposal.trigger = Trigger::ChannelMessage {
        provider: "telegram".into(),
        filter: None,
    };
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 2).is_ok());
}

// ── EdgeIntegrity ──────────────────────────────────────────────────────

#[test]
fn validate_zero_edges_passes_vacuously() {
    let proposal = valid_proposal();
    assert!(proposal.edges.is_empty());
    assert!(validate(&proposal, &ConnectionsSnapshot::empty(), 1).is_ok());
}

#[test]
fn validate_rejects_edge_from_referencing_unknown_node_id() {
    let mut proposal = valid_proposal();
    // Add a second node so the edge can land on a known `to`.
    proposal.nodes.push(agent_node("n2", vec![]));
    proposal.edges = vec![Edge {
        from: "ghost".into(),
        to: "n2".into(),
    }];
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    assert_eq!(err.kind_label(), "edge_integrity");
    match err {
        ProposalValidationError::EdgeIntegrity { from, to, reason } => {
            assert_eq!(from, "ghost");
            assert_eq!(to, "n2");
            assert!(reason.contains("from"));
        }
        other => panic!("expected EdgeIntegrity, got {other:?}"),
    }
}

#[test]
fn validate_rejects_edge_to_referencing_unknown_node_id() {
    let mut proposal = valid_proposal();
    proposal.nodes.push(agent_node("n2", vec![]));
    proposal.edges = vec![Edge {
        from: "n1".into(),
        to: "ghost".into(),
    }];
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    match err {
        ProposalValidationError::EdgeIntegrity { from, to, reason } => {
            assert_eq!(from, "n1");
            assert_eq!(to, "ghost");
            assert!(reason.contains("to"));
        }
        other => panic!("expected EdgeIntegrity, got {other:?}"),
    }
}

// ── UnknownConnection ──────────────────────────────────────────────────

#[test]
fn validate_rejects_required_connection_missing_from_snapshot() {
    let mut proposal = valid_proposal();
    proposal.required_connections = vec![ConnectionRef::Composio {
        toolkit_id: "linear".into(),
        account_id: None,
    }];
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    assert_eq!(err.kind_label(), "unknown_connection");
    match err {
        ProposalValidationError::UnknownConnection { r#ref, candidates } => {
            assert!(matches!(r#ref, ConnectionRef::Composio { .. }));
            // Empty snapshot has no candidates.
            assert!(candidates.is_empty());
        }
        other => panic!("expected UnknownConnection, got {other:?}"),
    }
}

#[test]
fn validate_unknown_connection_suggests_fuzzy_candidates_for_typos() {
    let mut proposal = valid_proposal();
    proposal.required_connections = vec![ConnectionRef::Composio {
        toolkit_id: "gmaill".into(), // typo
        account_id: None,
    }];
    let snapshot = ConnectionsSnapshot::new(vec![
        composio_view("gmail"),
        composio_view("slack"),
        composio_view("linear"),
    ]);
    let err = validate(&proposal, &snapshot, 1).unwrap_err();
    match err {
        ProposalValidationError::UnknownConnection {
            r#ref: _,
            candidates,
        } => {
            let names: Vec<String> = candidates
                .iter()
                .map(|r| match r {
                    ConnectionRef::Composio { toolkit_id, .. } => toolkit_id.clone(),
                    _ => String::new(),
                })
                .collect();
            assert!(
                names.iter().any(|n| n == "gmail"),
                "fuzzy candidates must include `gmail` for typo `gmaill`, got {names:?}"
            );
            // Lev distance limit ≤ 3; "linear" is 5 from "gmaill" and
            // must not appear.
            assert!(!names.iter().any(|n| n == "linear"));
        }
        other => panic!("expected UnknownConnection, got {other:?}"),
    }
}

#[test]
fn validate_per_node_allowed_connections_must_also_be_live() {
    // The proposal's `required_connections` is empty but a node's
    // `allowed_connections` references something missing. The
    // per-node walk must still catch it.
    let mut proposal = valid_proposal();
    proposal.nodes = vec![agent_node(
        "n1",
        vec![ConnectionRef::Composio {
            toolkit_id: "github".into(),
            account_id: None,
        }],
    )];
    let err = validate(&proposal, &ConnectionsSnapshot::empty(), 1).unwrap_err();
    match err {
        ProposalValidationError::UnknownConnection { .. } => {}
        other => panic!("expected UnknownConnection for per-node walk, got {other:?}"),
    }
}

// ── fuzzy_candidates only matches same mechanism ────────────────────────

#[test]
fn fuzzy_candidates_does_not_cross_mechanism_boundaries() {
    let unknown = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    // A Channel + Webview with the same name string must not appear
    // — the agent's typos almost always stay within a mechanism, and
    // cross-mechanism suggestions are noise.
    let snapshot = ConnectionsSnapshot::new(vec![
        live_view(
            ConnectionRef::Channel {
                provider: "gmail".into(),
                channel_id: "x".into(),
            },
            /* requires_verification = */ true,
        ),
        live_view(
            ConnectionRef::Webview {
                provider: "gmail".into(),
                account_id: "x".into(),
            },
            false,
        ),
    ]);
    let suggestions = fuzzy_candidates(&unknown, &snapshot);
    assert!(suggestions.is_empty());
}

// ── JsonParse — produced by the caller; we assert the variant exists ──

#[test]
fn json_parse_variant_round_trips() {
    let err = ProposalValidationError::JsonParse {
        reason: "expected `,` at line 4".into(),
    };
    assert_eq!(err.kind_label(), "json_parse");
    let json = serde_json::to_value(&err).unwrap();
    let back: ProposalValidationError = serde_json::from_value(json).unwrap();
    assert_eq!(back, err);
}

// ── < 50 ms timing guarantee (NFR-2.1.5) ───────────────────────────────

#[test]
fn validate_runs_under_50ms_on_a_realistic_proposal() {
    let mut proposal = valid_proposal();
    // Realistic proposal: a handful of allowed_connections + a few
    // edges between two nodes (Phase 2 shape, but the validator
    // walks the same checks at Phase 1 rates).
    proposal.required_connections = vec![
        ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        },
        ConnectionRef::Composio {
            toolkit_id: "slack".into(),
            account_id: None,
        },
    ];
    proposal.nodes[0] = agent_node("n1", proposal.required_connections.clone());

    let snapshot = ConnectionsSnapshot::new(vec![
        composio_view("gmail"),
        composio_view("slack"),
        composio_view("linear"),
        composio_view("github"),
        composio_view("notion"),
    ]);

    let start = std::time::Instant::now();
    for _ in 0..10 {
        validate(&proposal, &snapshot, 1).unwrap();
    }
    let elapsed = start.elapsed();
    // 10 calls under 50ms total — each call ≤ 5ms. The < 50 ms NFR
    // is per-call, so this is a generous ceiling that still catches
    // accidental quadratic regressions.
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "validator must stay sub-50ms; 10× = {elapsed:?}"
    );
}

// ── F2-1: Phase 2 NodeConfig variants + validation arms ────────────────

#[test]
fn allowed_node_kinds_phase_2_contains_full_phase_2_set() {
    // Stronger than the existing phase-2 test: pins the EXACT set so a
    // future bump (e.g. promoting Transform / AwaitHumanApproval into
    // Phase 2) is forced through this test as a deliberate change.
    let kinds = allowed_node_kinds(2);
    assert_eq!(
        kinds,
        &[
            NodeKind::AgentPrompt,
            NodeKind::ToolCall,
            NodeKind::HttpRequest,
            NodeKind::ChannelMessage,
            NodeKind::Condition,
            NodeKind::Delay,
        ]
    );
}

/// Build a Phase-2 node with arbitrary kind + config. Helper for the
/// per-config validation tests below.
fn node_with_config(id: &str, kind: NodeKind, config: NodeConfig) -> Node {
    Node {
        id: id.into(),
        kind,
        config,
        position: None,
        retry_policy: None,
    }
}

#[test]
fn validate_phase_1_rejects_tool_call_node() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::ToolCall,
        NodeConfig::ToolCall(ToolCallConfig {
            tool_name: "current_time".into(),
            arguments_template: serde_json::json!({}),
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 1) {
        Err(ProposalValidationError::UnsupportedNodeKind { node_kind, phase }) => {
            assert_eq!(node_kind, NodeKind::ToolCall);
            assert_eq!(phase, 1);
        }
        other => panic!("expected UnsupportedNodeKind, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_accepts_tool_call_node() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::ToolCall,
        NodeConfig::ToolCall(ToolCallConfig {
            tool_name: "current_time".into(),
            arguments_template: serde_json::json!({}),
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    validate(&proposal, &snapshot, 2).expect("tool_call valid under phase=2");
}

#[test]
fn validate_phase_2_rejects_tool_call_with_empty_name() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::ToolCall,
        NodeConfig::ToolCall(ToolCallConfig {
            tool_name: "  ".into(), // whitespace-only — should reject
            arguments_template: serde_json::json!({}),
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig {
            node_id, reason, ..
        }) => {
            assert_eq!(node_id, NodeId::from("n1"));
            assert!(
                reason.contains("tool_name must be non-empty"),
                "got: {reason}"
            );
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_http_request_missing_connection_id() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::HttpRequest,
        NodeConfig::HttpRequest(HttpRequestConfig {
            connection_id: "".into(),
            method: HttpMethod::Get,
            path_template: "/health".into(),
            headers: Default::default(),
            body_template: None,
            response_capture: Default::default(),
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(reason.contains("connection_id"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_http_request_missing_path_template() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::HttpRequest,
        NodeConfig::HttpRequest(HttpRequestConfig {
            connection_id: "conn-1".into(),
            method: HttpMethod::Post,
            path_template: "".into(),
            headers: Default::default(),
            body_template: None,
            response_capture: Default::default(),
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(reason.contains("path_template"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_channel_message_missing_body() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::ChannelMessage,
        NodeConfig::ChannelMessage(ChannelMessageConfig {
            connection_id: "slack".into(),
            channel_id: Some("C123".into()),
            body_template: "  ".into(),
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(reason.contains("body_template"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_condition_with_empty_left() {
    let mut proposal = valid_proposal();
    // Two AgentPrompt nodes so the condition's then_node_id resolves.
    proposal.nodes = vec![
        agent_node("n2", vec![]),
        node_with_config(
            "n1",
            NodeKind::Condition,
            NodeConfig::Condition(ConditionConfig {
                left: "".into(),
                op: CompareOp::Eq,
                right: "x".into(),
                then_node_id: "n2".into(),
                else_node_id: None,
            }),
        ),
    ];
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(reason.contains("left"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_condition_with_dangling_then_node_id() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::Condition,
        NodeConfig::Condition(ConditionConfig {
            left: "x".into(),
            op: CompareOp::Eq,
            right: "x".into(),
            then_node_id: "nonexistent".into(),
            else_node_id: None,
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(
                reason.contains("then_node_id") && reason.contains("nonexistent"),
                "got: {reason}"
            );
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_condition_with_dangling_else_node_id() {
    let mut proposal = valid_proposal();
    proposal.nodes = vec![
        agent_node("n2", vec![]),
        node_with_config(
            "n1",
            NodeKind::Condition,
            NodeConfig::Condition(ConditionConfig {
                left: "x".into(),
                op: CompareOp::Eq,
                right: "x".into(),
                then_node_id: "n2".into(),
                else_node_id: Some("nonexistent".into()),
            }),
        ),
    ];
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(
                reason.contains("else_node_id") && reason.contains("nonexistent"),
                "got: {reason}"
            );
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_delay_zero_seconds() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::Delay,
        NodeConfig::Delay(DelayConfig { seconds: 0 }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(reason.contains("seconds must be > 0"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_rejects_delay_over_24h_cap() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::Delay,
        NodeConfig::Delay(DelayConfig {
            seconds: 86_401, // 1s over the 24h cap
        }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    match validate(&proposal, &snapshot, 2) {
        Err(ProposalValidationError::InvalidNodeConfig { reason, .. }) => {
            assert!(reason.contains("86400"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn validate_phase_2_accepts_delay_at_24h_boundary() {
    let mut proposal = valid_proposal();
    proposal.nodes[0] = node_with_config(
        "n1",
        NodeKind::Delay,
        NodeConfig::Delay(DelayConfig { seconds: 86_400 }),
    );
    let snapshot = ConnectionsSnapshot::empty();
    validate(&proposal, &snapshot, 2).expect("24h delay accepted at boundary");
}

#[test]
fn invalid_node_config_carries_stable_kind_label() {
    let err = ProposalValidationError::InvalidNodeConfig {
        node_id: NodeId::from("n1"),
        node_kind: NodeKind::Delay,
        reason: "x".into(),
    };
    assert_eq!(err.kind_label(), "invalid_node_config");
}

// ── F2-1: serde round-trip of new NodeConfig variants ─────────────────

#[test]
fn node_config_tool_call_round_trips_through_serde() {
    let original = NodeConfig::ToolCall(ToolCallConfig {
        tool_name: "current_time".into(),
        arguments_template: serde_json::json!({ "tz": "{{trigger.payload.user_tz}}" }),
    });
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"kind\":\"tool_call\""));
    assert!(json.contains("current_time"));
    let parsed: NodeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn node_config_http_request_round_trips_through_serde() {
    let mut headers = std::collections::BTreeMap::new();
    headers.insert("X-Trace".into(), "{{node.start.output.run_id}}".into());
    let original = NodeConfig::HttpRequest(HttpRequestConfig {
        connection_id: "01F9".into(),
        method: HttpMethod::Post,
        path_template: "/users".into(),
        headers,
        body_template: Some("{\"x\": 1}".into()),
        response_capture: Default::default(),
    });
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"kind\":\"http_request\""));
    assert!(json.contains("\"method\":\"POST\""));
    let parsed: NodeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn node_config_channel_message_round_trips_through_serde() {
    let original = NodeConfig::ChannelMessage(ChannelMessageConfig {
        connection_id: "slack".into(),
        channel_id: Some("C0123".into()),
        body_template: "hi".into(),
    });
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"kind\":\"channel_message\""));
    let parsed: NodeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn node_config_condition_round_trips_through_serde() {
    let original = NodeConfig::Condition(ConditionConfig {
        left: "{{node.classify.output.label}}".into(),
        op: CompareOp::Contains,
        right: "URGENT".into(),
        then_node_id: "send_slack".into(),
        else_node_id: Some("send_email".into()),
    });
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"kind\":\"condition\""));
    assert!(json.contains("\"op\":{\"kind\":\"contains\"}"));
    let parsed: NodeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn node_config_delay_round_trips_through_serde() {
    let original = NodeConfig::Delay(DelayConfig { seconds: 60 });
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"kind\":\"delay\""));
    assert!(json.contains("\"seconds\":60"));
    let parsed: NodeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn tool_call_arguments_template_defaults_to_empty_object() {
    // The drafter / UI can omit `arguments_template` for tools with
    // no arguments; serde must materialise it as `{}` so the executor
    // doesn't have to special-case missing.
    let json = r#"{"kind":"tool_call","tool_name":"current_time"}"#;
    let parsed: NodeConfig = serde_json::from_str(json).unwrap();
    match parsed {
        NodeConfig::ToolCall(cfg) => {
            assert_eq!(cfg.tool_name, "current_time");
            assert_eq!(cfg.arguments_template, serde_json::json!({}));
        }
        other => panic!("expected ToolCall, got {other:?}"),
    }
}

// ── F3-4 BrowserAction validator coverage ──────────────────────────────

use crate::openhuman::browser_agent::cdp::types::BrowserProfile;

fn browser_node(id: &str, cfg: BrowserActionConfig) -> Node {
    Node {
        id: id.into(),
        kind: NodeKind::BrowserAction,
        config: NodeConfig::BrowserAction(cfg),
        position: None,
        retry_policy: None,
    }
}

fn baseline_browser_cfg() -> BrowserActionConfig {
    BrowserActionConfig {
        goal: "Log in to example.com, navigate to /portfolio, extract the balance.".into(),
        start_url: Some("https://example.com/login".into()),
        profile: BrowserProfile::EphemeralIsolated,
        iteration_cap: 25,
        allowed_hosts: vec!["example.com".into()],
        output_schema: None,
        allowed_connections: vec![],
        dry_run: false,
        max_session_wall_clock_secs: 600,
    }
}

#[test]
fn browser_action_allowed_in_phase_3_and_4() {
    assert!(allowed_node_kinds(3).contains(&NodeKind::BrowserAction));
    assert!(allowed_node_kinds(4).contains(&NodeKind::BrowserAction));
}

#[test]
fn browser_action_not_allowed_in_phase_2() {
    assert!(!allowed_node_kinds(2).contains(&NodeKind::BrowserAction));
}

#[test]
fn browser_action_baseline_valid_proposal_passes() {
    let mut p = valid_proposal();
    p.nodes = vec![browser_node("b1", baseline_browser_cfg())];
    let snap = ConnectionsSnapshot::new(vec![]);
    assert!(validate(&p, &snap, 3).is_ok());
}

#[test]
fn browser_action_empty_goal_rejected() {
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.goal = "   ".into();
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![]);
    let err = validate(&p, &snap, 3).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig { reason, .. } => {
            assert!(reason.contains("goal must be non-empty"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn browser_action_iteration_cap_out_of_range_rejected() {
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.iteration_cap = 999;
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![]);
    let err = validate(&p, &snap, 3).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig { reason, .. } => {
            assert!(reason.contains("iteration_cap"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn browser_action_malformed_start_url_rejected() {
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.start_url = Some("not a url at all".into());
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![]);
    let err = validate(&p, &snap, 3).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig { reason, .. } => {
            assert!(reason.contains("not a parseable URL"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn browser_action_templated_start_url_passes() {
    // A `{{node.x.output.url}}` reference is pre-substituted at
    // dispatch; the validator must let it through.
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.start_url = Some("{{node.upstream.output.url}}".into());
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![]);
    assert!(validate(&p, &snap, 3).is_ok());
}

#[test]
fn browser_action_host_with_scheme_rejected() {
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.allowed_hosts = vec!["https://example.com/".into()];
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![]);
    let err = validate(&p, &snap, 3).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig { reason, .. } => {
            assert!(reason.contains("bare hostname"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn browser_action_reuse_authenticated_without_matching_connection_rejected() {
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.profile = BrowserProfile::ReuseAuthenticated {
        provider: "linkedin".into(),
    };
    // allowed_connections does NOT contain a Webview{provider="linkedin"}.
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![]);
    let err = validate(&p, &snap, 3).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig { reason, .. } => {
            assert!(reason.contains("ConnectionRef::Webview"), "got: {reason}");
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }
}

#[test]
fn browser_action_reuse_authenticated_with_matching_connection_passes() {
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.profile = BrowserProfile::ReuseAuthenticated {
        provider: "linkedin".into(),
    };
    cfg.allowed_connections = vec![ConnectionRef::Webview {
        provider: "linkedin".into(),
        account_id: "abc".into(),
    }];
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![live_view(
        ConnectionRef::Webview {
            provider: "linkedin".into(),
            account_id: "abc".into(),
        },
        /* requires_verification = */ false,
    )]);
    assert!(validate(&p, &snap, 3).is_ok());
}

#[test]
fn browser_action_roundtrips_through_serde() {
    let cfg = baseline_browser_cfg();
    let original = NodeConfig::BrowserAction(cfg);
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"kind\":\"browser_action\""));
    let parsed: NodeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}

#[test]
fn browser_action_defaults_apply_on_deserialize() {
    // Minimal payload — only `goal` set. The rest must default
    // (profile = EphemeralIsolated, iteration_cap = 25, empty hosts,
    // dry_run = false).
    let json = r#"{"kind":"browser_action","goal":"do thing"}"#;
    let parsed: NodeConfig = serde_json::from_str(json).unwrap();
    match parsed {
        NodeConfig::BrowserAction(cfg) => {
            assert_eq!(cfg.goal, "do thing");
            assert!(cfg.start_url.is_none());
            assert_eq!(cfg.profile, BrowserProfile::EphemeralIsolated);
            assert_eq!(cfg.iteration_cap, 25);
            assert!(cfg.allowed_hosts.is_empty());
            assert!(cfg.output_schema.is_none());
            assert!(cfg.allowed_connections.is_empty());
            // F3-6 chunk 1: dry-run defaults to false (real dispatch).
            assert!(!cfg.dry_run);
        }
        other => panic!("expected BrowserAction, got {other:?}"),
    }
}

#[test]
fn browser_action_wall_clock_cap_clamped_to_30_3600_range() {
    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.max_session_wall_clock_secs = 10; // below floor
    p.nodes = vec![browser_node("b1", cfg)];
    let snap = ConnectionsSnapshot::new(vec![]);
    let err = validate(&p, &snap, 3).unwrap_err();
    match err {
        ProposalValidationError::InvalidNodeConfig { reason, .. } => {
            assert!(reason.contains("max_session_wall_clock_secs"));
            assert!(reason.contains("[30, 3600]"));
        }
        other => panic!("expected InvalidNodeConfig, got {other:?}"),
    }

    let mut p = valid_proposal();
    let mut cfg = baseline_browser_cfg();
    cfg.max_session_wall_clock_secs = 86_400; // above ceiling
    p.nodes = vec![browser_node("b1", cfg)];
    let err = validate(&p, &snap, 3).unwrap_err();
    assert!(matches!(
        err,
        ProposalValidationError::InvalidNodeConfig { .. }
    ));
}

#[test]
fn browser_action_wall_clock_cap_defaults_to_600() {
    // Serde default — minimal payload gets the safety-conscious 10-min cap.
    let json = r#"{"kind":"browser_action","goal":"x"}"#;
    let parsed: NodeConfig = serde_json::from_str(json).unwrap();
    match parsed {
        NodeConfig::BrowserAction(cfg) => {
            assert_eq!(cfg.max_session_wall_clock_secs, 600);
        }
        other => panic!("expected BrowserAction, got {other:?}"),
    }
}

#[test]
fn browser_action_dry_run_roundtrips_through_serde() {
    let mut cfg = baseline_browser_cfg();
    cfg.dry_run = true;
    let original = NodeConfig::BrowserAction(cfg);
    let json = serde_json::to_string(&original).unwrap();
    assert!(json.contains("\"dry_run\":true"));
    let parsed: NodeConfig = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, original);
}
