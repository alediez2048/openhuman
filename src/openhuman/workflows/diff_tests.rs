//! F-12 — `workflow_diff` unit tests covering each diff path.

use super::diff::{workflow_diff, MAX_DIFF_BULLETS};
use super::types::*;
use crate::openhuman::connections::types::ConnectionRef;
use chrono::Utc;

fn sample_workflow(name: &str) -> Workflow {
    Workflow {
        id: "wf-x".into(),
        schema_version: 1,
        name: name.into(),
        description: Some("seed".into()),
        enabled: false,
        origin: WorkflowOrigin::UserChat,
        health: WorkflowHealth::Ready,
        trigger: Trigger::Cron {
            expr: "0 8 * * 1-5".into(),
            tz: Some("UTC".into()),
            active_hours: None,
        },
        nodes: vec![Node {
            id: "n1".into(),
            kind: NodeKind::AgentPrompt,
            config: NodeConfig::AgentPrompt(AgentPromptConfig {
                prompt: "do the thing".into(),
                allowed_connections: vec![],
                iteration_cap: 12,
                model_tier: None,
            }),
            position: None,
        }],
        edges: vec![],
        settings: WorkflowSettings::default(),
        created_at: Utc::now(),
        updated_at: Utc::now(),
        last_run_at: None,
    }
}

#[test]
fn workflow_diff_identical_workflows_returns_empty() {
    let wf = sample_workflow("Morning digest");
    assert!(workflow_diff(&wf, &wf).is_empty());
}

#[test]
fn workflow_diff_rename_produces_renamed_bullet() {
    let current = sample_workflow("Morning digest");
    let mut proposed = current.clone();
    proposed.name = "Daily brief".into();
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 1);
    assert!(bullets[0].contains("Renamed"));
    assert!(bullets[0].contains("Morning digest"));
    assert!(bullets[0].contains("Daily brief"));
}

#[test]
fn workflow_diff_cron_change_produces_schedule_bullet() {
    let current = sample_workflow("x");
    let mut proposed = current.clone();
    proposed.trigger = Trigger::Cron {
        expr: "0 9 * * 1-5".into(),
        tz: Some("UTC".into()),
        active_hours: None,
    };
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 1);
    assert!(bullets[0].contains("cron schedule"));
    assert!(bullets[0].contains("0 8 * * 1-5"));
    assert!(bullets[0].contains("0 9 * * 1-5"));
}

#[test]
fn workflow_diff_trigger_kind_change_produces_label_bullet() {
    let current = sample_workflow("x");
    let mut proposed = current.clone();
    proposed.trigger = Trigger::Manual;
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 1);
    assert!(bullets[0].contains("Cron"));
    assert!(bullets[0].contains("Manual"));
}

#[test]
fn workflow_diff_added_allowed_connection_produces_added_bullet() {
    let current = sample_workflow("x");
    let mut proposed = current.clone();
    let NodeConfig::AgentPrompt(cfg) = &mut proposed.nodes[0].config;
    cfg.allowed_connections.push(ConnectionRef::Composio {
        toolkit_id: "slack".into(),
        account_id: None,
    });
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 1);
    assert!(bullets[0].contains("Added"));
    assert!(bullets[0].contains("slack"));
    assert!(bullets[0].contains("step 1"));
}

#[test]
fn workflow_diff_removed_allowed_connection_produces_removed_bullet() {
    let mut current = sample_workflow("x");
    let NodeConfig::AgentPrompt(cfg) = &mut current.nodes[0].config;
    cfg.allowed_connections.push(ConnectionRef::Composio {
        toolkit_id: "slack".into(),
        account_id: None,
    });
    let mut proposed = current.clone();
    let NodeConfig::AgentPrompt(cfg) = &mut proposed.nodes[0].config;
    cfg.allowed_connections.clear();
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 1);
    assert!(bullets[0].contains("Removed"));
    assert!(bullets[0].contains("slack"));
}

#[test]
fn workflow_diff_settings_change_produces_timeout_bullet() {
    let current = sample_workflow("x");
    let mut proposed = current.clone();
    proposed.settings.timeout_secs = 600;
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 1);
    assert!(bullets[0].contains("timeout"));
    assert!(bullets[0].contains("300"));
    assert!(bullets[0].contains("600"));
}

#[test]
fn workflow_diff_compound_changes_produces_multiple_bullets() {
    let current = sample_workflow("Morning digest");
    let mut proposed = current.clone();
    proposed.name = "Daily brief".into();
    proposed.trigger = Trigger::Cron {
        expr: "0 9 * * 1-5".into(),
        tz: Some("UTC".into()),
        active_hours: None,
    };
    proposed.settings.timeout_secs = 600;
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 3);
    assert!(bullets.iter().any(|b| b.contains("Renamed")));
    assert!(bullets.iter().any(|b| b.contains("cron schedule")));
    assert!(bullets.iter().any(|b| b.contains("timeout")));
}

#[test]
fn workflow_diff_caps_long_change_lists_with_tail_bullet() {
    // Force more than MAX_DIFF_BULLETS deltas by toggling a long
    // string of allowed_connections on / off in proposed vs. current.
    // We add (MAX + 5) distinct connections; current has none.
    let mut current = sample_workflow("x");
    let mut proposed = current.clone();
    let NodeConfig::AgentPrompt(cfg) = &mut proposed.nodes[0].config;
    for i in 0..(MAX_DIFF_BULLETS + 5) {
        cfg.allowed_connections.push(ConnectionRef::Composio {
            toolkit_id: format!("toolkit_{i}"),
            account_id: None,
        });
    }
    // Add a rename + schedule change to make sure the cap accounts
    // for all bullet sources, not just connection adds.
    proposed.name = "renamed".into();
    let NodeConfig::AgentPrompt(_) = &mut current.nodes[0].config;

    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), MAX_DIFF_BULLETS);
    let last = bullets.last().unwrap();
    assert!(
        last.contains("more changes"),
        "tail bullet must summarise overflow, got {last}"
    );
}

#[test]
fn workflow_diff_iteration_cap_change_produces_cap_bullet() {
    let current = sample_workflow("x");
    let mut proposed = current.clone();
    let NodeConfig::AgentPrompt(cfg) = &mut proposed.nodes[0].config;
    cfg.iteration_cap = 24;
    let bullets = workflow_diff(&current, &proposed);
    assert_eq!(bullets.len(), 1);
    assert!(bullets[0].contains("iteration cap"));
    assert!(bullets[0].contains("12"));
    assert!(bullets[0].contains("24"));
}
