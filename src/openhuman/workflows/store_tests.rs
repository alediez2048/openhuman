//! Migration runner tests against an ephemeral workspace.
//!
//! Guards: file creation, idempotent re-open, schema_migrations ledger,
//! PRAGMA foreign_keys = ON, FK cascade behavior.

use super::store::with_connection;
use crate::openhuman::config::Config;
use rusqlite::params;
use tempfile::TempDir;

fn config_with_temp_workspace() -> (TempDir, Config) {
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    (dir, config)
}

#[test]
fn open_creates_workflows_db_and_applies_all_three_migrations() {
    let (_dir, config) = config_with_temp_workspace();
    with_connection(&config, |_conn| Ok(())).unwrap();

    let db_path = config.workspace_dir.join("workflows.db");
    assert!(
        db_path.exists(),
        "workflows.db must be created on first open"
    );

    // Tables must exist + ledger must have rows 1, 2, 3.
    with_connection(&config, |conn| {
        let names: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for required in [
            "schema_migrations",
            "workflow_run_steps",
            "workflow_runs",
            "workflows",
        ] {
            assert!(
                names.iter().any(|n| n == required),
                "missing table `{required}` — saw {names:?}"
            );
        }

        let versions: Vec<i64> = conn
            .prepare("SELECT version FROM schema_migrations ORDER BY version")
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(
            versions,
            vec![1, 2, 3],
            "ledger must record every migration"
        );
        Ok(())
    })
    .unwrap();
}

#[test]
fn re_open_is_idempotent_and_does_not_duplicate_ledger_rows() {
    let (_dir, config) = config_with_temp_workspace();
    // Open three times; the migration runner must skip already-applied rows.
    for _ in 0..3 {
        with_connection(&config, |_| Ok(())).unwrap();
    }
    with_connection(&config, |conn| {
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 3, "three migrations must record exactly three rows");
        Ok(())
    })
    .unwrap();
}

#[test]
fn foreign_key_pragma_is_enabled_on_every_open() {
    let (_dir, config) = config_with_temp_workspace();
    with_connection(&config, |conn| {
        let on: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(on, 1, "PRAGMA foreign_keys must be ON for cascade deletes");
        Ok(())
    })
    .unwrap();
}

#[test]
fn workflow_delete_cascades_runs_and_run_steps() {
    let (_dir, config) = config_with_temp_workspace();
    // Insert a minimal workflow + run + run_step, then delete the workflow
    // and assert the cascade dropped the descendants.
    with_connection(&config, |conn| {
        conn.execute(
            "INSERT INTO workflows (id, schema_version, name, enabled, origin, health, \
             trigger_json, nodes_json, edges_json, settings_json, created_at, updated_at) \
             VALUES (?1, 1, 'wf', 0, '{\"type\":\"user_chat\"}', '{\"type\":\"ready\"}', \
             '{\"type\":\"manual\"}', '[]', '[]', '{\"timeout_secs\":300,\"on_error\":\"halt\"}', \
             ?2, ?3)",
            params!["wf-1", "2026-05-20T00:00:00Z", "2026-05-20T00:00:00Z"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workflow_runs (id, workflow_id, trigger_source, status, started_at) \
             VALUES (?1, ?2, '{\"type\":\"manual\",\"initiator\":\"user\"}', 'running', ?3)",
            params!["run-1", "wf-1", "2026-05-20T00:00:00Z"],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO workflow_run_steps (id, run_id, node_id, status, started_at) \
             VALUES (?1, ?2, 'n1', 'running', ?3)",
            params!["step-1", "run-1", "2026-05-20T00:00:00Z"],
        )
        .unwrap();

        conn.execute("DELETE FROM workflows WHERE id = ?1", params!["wf-1"])
            .unwrap();

        let run_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workflow_runs WHERE workflow_id = ?1",
                params!["wf-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(run_count, 0, "FK cascade must drop the run row");

        let step_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM workflow_run_steps WHERE run_id = ?1",
                params!["run-1"],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(step_count, 0, "FK cascade must drop the step row via run");
        Ok(())
    })
    .unwrap();
}

#[test]
fn migrations_run_inside_nested_workspace_dir() {
    // Workspace dir doesn't exist yet — apply_migrations must create it
    // via the `parent.create_dir_all` step in `with_connection`.
    let dir = TempDir::new().unwrap();
    let mut config = Config::default();
    config.workspace_dir = dir.path().join("deeply").join("nested");

    with_connection(&config, |_| Ok(())).unwrap();
    assert!(config.workspace_dir.exists());
    assert!(config.workspace_dir.join("workflows.db").exists());
}

// ── F-3 helpers: list_workflows_referencing + set_health ────────────────

use super::store::{list_workflows_referencing, set_health};
use crate::openhuman::connections::types::ConnectionRef;
use crate::openhuman::workflows::types::*;
use chrono::Utc;

fn insert_minimal_workflow_with_ref(config: &Config, id: &str, r#ref: ConnectionRef) {
    let wf = Workflow {
        id: id.into(),
        schema_version: 1,
        name: format!("wf-{id}"),
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
                allowed_connections: vec![r#ref],
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
    };
    super::store::insert_workflow(config, &wf).unwrap();
}

#[test]
fn list_workflows_referencing_returns_only_matching_rows() {
    let (_dir, config) = config_with_temp_workspace();
    insert_minimal_workflow_with_ref(
        &config,
        "wf-a",
        ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        },
    );
    insert_minimal_workflow_with_ref(
        &config,
        "wf-b",
        ConnectionRef::Composio {
            toolkit_id: "slack".into(),
            account_id: None,
        },
    );
    insert_minimal_workflow_with_ref(
        &config,
        "wf-c",
        ConnectionRef::GenericHttp {
            connection_id: "ghc-1".into(),
        },
    );

    let gmail_hits = list_workflows_referencing(
        &config,
        &ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        },
    )
    .unwrap();
    assert_eq!(gmail_hits.len(), 1);
    assert_eq!(gmail_hits[0].id, "wf-a");

    let http_hits = list_workflows_referencing(
        &config,
        &ConnectionRef::GenericHttp {
            connection_id: "ghc-1".into(),
        },
    )
    .unwrap();
    assert_eq!(http_hits.len(), 1);
    assert_eq!(http_hits[0].id, "wf-c");
}

#[test]
fn list_workflows_referencing_returns_empty_when_no_matches() {
    let (_dir, config) = config_with_temp_workspace();
    insert_minimal_workflow_with_ref(
        &config,
        "wf-a",
        ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        },
    );

    let hits = list_workflows_referencing(
        &config,
        &ConnectionRef::Composio {
            toolkit_id: "linear".into(),
            account_id: None,
        },
    )
    .unwrap();
    assert!(hits.is_empty());
}

#[test]
fn set_health_updates_only_health_column_and_bumps_updated_at() {
    let (_dir, config) = config_with_temp_workspace();
    insert_minimal_workflow_with_ref(
        &config,
        "wf-a",
        ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        },
    );
    let before = super::store::get_workflow(&config, &"wf-a".to_string())
        .unwrap()
        .unwrap();
    assert_eq!(before.health, WorkflowHealth::Ready);

    let later = Utc::now() + chrono::Duration::seconds(1);
    let new_health = WorkflowHealth::NeedsConnections {
        missing: vec![ConnectionRef::Composio {
            toolkit_id: "gmail".into(),
            account_id: None,
        }],
    };
    let updated = set_health(&config, &"wf-a".to_string(), &new_health, later).unwrap();
    assert!(updated);

    let after = super::store::get_workflow(&config, &"wf-a".to_string())
        .unwrap()
        .unwrap();
    assert!(matches!(
        after.health,
        WorkflowHealth::NeedsConnections { .. }
    ));
    assert!(after.updated_at >= later);
    // Other fields untouched.
    assert_eq!(after.name, before.name);
    assert_eq!(after.enabled, before.enabled);
}

#[test]
fn set_health_returns_false_when_id_unknown() {
    let (_dir, config) = config_with_temp_workspace();
    let updated = set_health(
        &config,
        &"no-such-id".into(),
        &WorkflowHealth::Ready,
        Utc::now(),
    )
    .unwrap();
    assert!(!updated);
}

// ── F-9: soft-cancel + orphan sweep ────────────────────────────────────

/// Insert a minimal `workflows` parent row so `workflow_runs.workflow_id`
/// FK constraints (cascade-delete enabled via migration 002) don't
/// reject the test's run rows.
fn seed_workflow_row(config: &Config, id: &str) {
    with_connection(config, |conn| {
        conn.execute(
            "INSERT INTO workflows (id, schema_version, name, enabled, origin, health, \
             trigger_json, nodes_json, edges_json, settings_json, created_at, updated_at) \
             VALUES (?1, 1, 'wf', 0, '{\"type\":\"user_chat\"}', '{\"type\":\"ready\"}', \
             '{\"type\":\"manual\"}', '[]', '[]', '{\"timeout_secs\":300,\"on_error\":\"halt\"}', \
             ?2, ?3)",
            params![id, "2026-05-20T00:00:00Z", "2026-05-20T00:00:00Z"],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
}

#[test]
fn set_and_read_cancelled_flag_round_trip() {
    use super::store;
    use super::types::*;

    let (_dir, config) = config_with_temp_workspace();
    seed_workflow_row(&config, "wf-x");
    let run = Run {
        id: "r1".into(),
        workflow_id: "wf-x".into(),
        trigger_source: TriggerSource::Cron,
        status: RunStatus::Running,
        started_at: Utc::now(),
        completed_at: None,
        error: None,
        cancelled: false,
    };
    store::insert_run(&config, &run).unwrap();

    assert!(
        !store::is_cancelled(&config, &run.id).unwrap(),
        "fresh row must read cancelled=false"
    );
    let touched = store::set_cancelled_flag(&config, &run.id).unwrap();
    assert!(touched, "set_cancelled_flag must touch one row");
    assert!(store::is_cancelled(&config, &run.id).unwrap());
}

#[test]
fn is_cancelled_returns_false_for_unknown_run_id() {
    use super::store;
    let (_dir, config) = config_with_temp_workspace();
    let flag = store::is_cancelled(&config, &"ghost".into()).unwrap();
    assert!(
        !flag,
        "unknown id reads as not-cancelled (graceful fallback)"
    );
}

#[test]
fn orphan_running_runs_marks_running_rows_failed_with_core_crashed() {
    use super::store;
    use super::types::*;

    let (_dir, config) = config_with_temp_workspace();
    seed_workflow_row(&config, "wf-a");
    seed_workflow_row(&config, "wf-b");
    // Two stale Running rows + one Succeeded row that must NOT be
    // touched.
    let r_run1 = Run {
        id: "r-run1".into(),
        workflow_id: "wf-a".into(),
        trigger_source: TriggerSource::Cron,
        status: RunStatus::Running,
        started_at: Utc::now(),
        completed_at: None,
        error: None,
        cancelled: false,
    };
    let r_run2 = Run {
        id: "r-run2".into(),
        workflow_id: "wf-b".into(),
        trigger_source: TriggerSource::Manual {
            initiator: "user".into(),
        },
        status: RunStatus::Running,
        started_at: Utc::now(),
        completed_at: None,
        error: None,
        cancelled: false,
    };
    let r_succ = Run {
        id: "r-succ".into(),
        workflow_id: "wf-a".into(),
        trigger_source: TriggerSource::Cron,
        status: RunStatus::Succeeded,
        started_at: Utc::now(),
        completed_at: Some(Utc::now()),
        error: None,
        cancelled: false,
    };
    store::insert_run(&config, &r_run1).unwrap();
    store::insert_run(&config, &r_run2).unwrap();
    store::insert_run(&config, &r_succ).unwrap();

    let pairs = store::orphan_running_runs(&config, Utc::now()).unwrap();
    assert_eq!(pairs.len(), 2, "exactly the two Running rows should sweep");
    let ids: std::collections::HashSet<_> = pairs.iter().map(|(_, r)| r.clone()).collect();
    assert!(ids.contains("r-run1"));
    assert!(ids.contains("r-run2"));
    assert!(!ids.contains("r-succ"), "Succeeded row must not be swept");

    // Idempotent: a second sweep on the same DB returns zero pairs.
    let pairs_2 = store::orphan_running_runs(&config, Utc::now()).unwrap();
    assert!(pairs_2.is_empty());

    // Touched rows now read as Failed with error = CoreCrashed.
    let (run1, _) = store::get_run(&config, &"r-run1".into()).unwrap().unwrap();
    assert!(matches!(run1.status, RunStatus::Failed));
    assert_eq!(run1.error.as_deref(), Some("CoreCrashed"));
    assert!(run1.completed_at.is_some());

    // Succeeded row untouched.
    let (succ, _) = store::get_run(&config, &"r-succ".into()).unwrap().unwrap();
    assert!(matches!(succ.status, RunStatus::Succeeded));
    assert_ne!(succ.error.as_deref(), Some("CoreCrashed"));
}

#[test]
fn orphan_running_runs_on_clean_db_returns_empty_vec() {
    use super::store;
    let (_dir, config) = config_with_temp_workspace();
    let pairs = store::orphan_running_runs(&config, Utc::now()).unwrap();
    assert!(pairs.is_empty());
}
