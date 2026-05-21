//! F-3 health-recompute tests — TDD-first per the locked Phase 1
//! execution contract. Covers every branch of the Phase 0 honest-
//! connection truth table.

use super::health::{missing_against, recompute, referenced_connections, ConnectionsSnapshot};
use super::types::*;
use crate::openhuman::connections::types::{ConnectionRef, ConnectionStatus, ConnectionView};
use crate::openhuman::connections::verification::{Verification, VerificationResult};
use chrono::{TimeZone, Utc};

fn live_view(r#ref: ConnectionRef, kind_requires_verification: bool) -> ConnectionView {
    ConnectionView {
        r#ref,
        display_name: "test".into(),
        status: ConnectionStatus::Connected,
        last_used_at: None,
        mechanism_label: "test".into(),
        verification: if kind_requires_verification {
            Some(Verification {
                last_probed_at: Utc.with_ymd_and_hms(2026, 5, 20, 0, 0, 0).unwrap(),
                result: VerificationResult::Live,
            })
        } else {
            None
        },
    }
}

fn workflow_with(allowed: Vec<ConnectionRef>) -> Workflow {
    Workflow {
        id: "wf-test".into(),
        schema_version: 1,
        name: "test".into(),
        description: None,
        enabled: false,
        origin: WorkflowOrigin::UserChat,
        health: WorkflowHealth::Ready,
        trigger: Trigger::Manual,
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "x".into(),
                allowed_connections: allowed,
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

#[test]
fn recompute_returns_ready_when_all_refs_are_present_and_connected() {
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let wf = workflow_with(vec![gmail.clone()]);
    let snapshot = ConnectionsSnapshot::new(vec![live_view(gmail, false)]);

    assert_eq!(recompute(&wf, &snapshot), WorkflowHealth::Ready);
}

#[test]
fn recompute_returns_needs_connections_when_ref_absent() {
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let wf = workflow_with(vec![gmail.clone()]);
    let snapshot = ConnectionsSnapshot::empty();

    match recompute(&wf, &snapshot) {
        WorkflowHealth::NeedsConnections { missing } => {
            assert_eq!(missing, vec![gmail]);
        }
        other => panic!("expected NeedsConnections, got {other:?}"),
    }
}

#[test]
fn recompute_treats_not_connected_status_as_missing() {
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let wf = workflow_with(vec![gmail.clone()]);

    let mut view = live_view(gmail.clone(), false);
    view.status = ConnectionStatus::NotConnected;
    let snapshot = ConnectionsSnapshot::new(vec![view]);

    match recompute(&wf, &snapshot) {
        WorkflowHealth::NeedsConnections { missing } => assert_eq!(missing, vec![gmail]),
        other => panic!("expected NeedsConnections, got {other:?}"),
    }
}

#[test]
fn recompute_treats_error_status_as_missing() {
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let wf = workflow_with(vec![gmail.clone()]);

    let mut view = live_view(gmail.clone(), false);
    view.status = ConnectionStatus::Error {
        reason: "deauth'd".into(),
    };
    let snapshot = ConnectionsSnapshot::new(vec![view]);

    assert!(matches!(
        recompute(&wf, &snapshot),
        WorkflowHealth::NeedsConnections { .. }
    ));
}

#[test]
fn http_requires_live_verification_status_alone_is_not_enough() {
    // Honest-status: a GenericHttp row that exists with status=Connected
    // but has never been probed counts as "Configured", not "Live".
    let http = ConnectionRef::GenericHttp {
        connection_id: "ghc-1".into(),
    };
    let wf = workflow_with(vec![http.clone()]);

    // Connected status + no verification → treated as missing.
    let mut view = live_view(http.clone(), false);
    view.status = ConnectionStatus::Connected;
    view.verification = None;
    let snapshot = ConnectionsSnapshot::new(vec![view]);
    assert!(matches!(
        recompute(&wf, &snapshot),
        WorkflowHealth::NeedsConnections { .. }
    ));

    // Connected + Live verification → Ready.
    let snapshot_live = ConnectionsSnapshot::new(vec![live_view(http, true)]);
    assert_eq!(recompute(&wf, &snapshot_live), WorkflowHealth::Ready);
}

#[test]
fn mcp_and_channel_also_require_live_verification() {
    for r#ref in [
        ConnectionRef::Mcp {
            server_id: "higgsfield".into(),
            tool_name: None,
        },
        ConnectionRef::Channel {
            provider: "telegram".into(),
            channel_id: "@me".into(),
        },
    ] {
        let wf = workflow_with(vec![r#ref.clone()]);

        let mut configured = live_view(r#ref.clone(), false);
        configured.verification = None;
        let snapshot = ConnectionsSnapshot::new(vec![configured]);
        assert!(
            matches!(
                recompute(&wf, &snapshot),
                WorkflowHealth::NeedsConnections { .. }
            ),
            "verification-required mechanism without Live probe must be missing: {ref:?}"
        );

        let live_snapshot = ConnectionsSnapshot::new(vec![live_view(r#ref, true)]);
        assert_eq!(recompute(&wf, &live_snapshot), WorkflowHealth::Ready);
    }
}

#[test]
fn http_with_failed_verification_counts_as_missing() {
    let http = ConnectionRef::GenericHttp {
        connection_id: "ghc-1".into(),
    };
    let wf = workflow_with(vec![http.clone()]);

    let mut view = live_view(http.clone(), false);
    view.status = ConnectionStatus::Connected;
    view.verification = Some(Verification {
        last_probed_at: Utc.with_ymd_and_hms(2026, 5, 20, 0, 0, 0).unwrap(),
        result: VerificationResult::Failed {
            reason: "DNS failure".into(),
        },
    });
    let snapshot = ConnectionsSnapshot::new(vec![view]);

    match recompute(&wf, &snapshot) {
        WorkflowHealth::NeedsConnections { missing } => assert_eq!(missing, vec![http]),
        other => panic!("expected NeedsConnections, got {other:?}"),
    }
}

#[test]
fn composio_webview_builtin_do_not_need_verification() {
    // For these three mechanisms, verification is intentionally None —
    // status alone is authoritative (Composio API tells us, cookie probe
    // tells us, session token tells us).
    for r#ref in [
        ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        },
        ConnectionRef::Webview {
            provider: "linkedin".into(),
            account_id: "acct-1".into(),
        },
        ConnectionRef::Builtin {
            integration: "twilio".into(),
        },
    ] {
        let wf = workflow_with(vec![r#ref.clone()]);
        let snapshot = ConnectionsSnapshot::new(vec![live_view(r#ref, false)]);
        assert_eq!(
            recompute(&wf, &snapshot),
            WorkflowHealth::Ready,
            "non-verification mechanism with Connected status must be Ready"
        );
    }
}

#[test]
fn referenced_connections_walks_all_agent_prompt_nodes() {
    // Multi-node workflow (Phase 2 will allow this — F-3 builds the
    // helper now so it doesn't need a refactor when the executor lifts
    // its single-node restriction).
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let slack = ConnectionRef::Composio {
        toolkit_id: "slack".into(),
        account_id: None,
    };
    let mut wf = workflow_with(vec![gmail.clone()]);
    wf.nodes.push(Node {
        id: "n2".into(),
        kind: NodeKind::AgentPrompt,
        config: NodeConfig::AgentPrompt(AgentPromptConfig {
            prompt: "y".into(),
            allowed_connections: vec![slack.clone(), gmail.clone()],
            iteration_cap: 12,
            model_tier: None,
        }),
        position: None,
    });

    let refs = referenced_connections(&wf);
    assert_eq!(refs.len(), 2, "should dedup across nodes");
    assert!(refs.contains(&gmail));
    assert!(refs.contains(&slack));
}

#[test]
fn empty_allowed_connections_yields_ready_with_empty_missing() {
    let wf = workflow_with(vec![]);
    let snapshot = ConnectionsSnapshot::empty();
    assert_eq!(recompute(&wf, &snapshot), WorkflowHealth::Ready);

    let refs = referenced_connections(&wf);
    assert!(refs.is_empty());

    let missing = missing_against(&[], &snapshot);
    assert!(missing.is_empty());
}

#[test]
fn missing_against_returns_only_absent_refs() {
    let gmail = ConnectionRef::Composio {
        toolkit_id: "gmail".into(),
        account_id: None,
    };
    let slack = ConnectionRef::Composio {
        toolkit_id: "slack".into(),
        account_id: None,
    };
    let snapshot = ConnectionsSnapshot::new(vec![live_view(gmail.clone(), false)]);
    let missing = missing_against(&[gmail.clone(), slack.clone()], &snapshot);
    assert_eq!(missing, vec![slack]);
}

#[test]
fn recompute_is_fast_enough_for_phase_one_workflows() {
    // NFR-2.1.5 — validation < 50 ms. Pure Rust, no I/O.
    use std::time::Instant;
    let refs: Vec<_> = (0..20)
        .map(|i| ConnectionRef::Composio {
            toolkit_id: format!("toolkit_{i}"),
            account_id: None,
        })
        .collect();
    let wf = workflow_with(refs.clone());
    let snapshot =
        ConnectionsSnapshot::new(refs.iter().cloned().map(|r| live_view(r, false)).collect());

    let start = Instant::now();
    let _ = recompute(&wf, &snapshot);
    let elapsed = start.elapsed();
    assert!(
        elapsed.as_millis() < 50,
        "recompute took {elapsed:?}, must be < 50 ms"
    );
}
