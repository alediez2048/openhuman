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
