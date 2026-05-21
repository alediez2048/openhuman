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
