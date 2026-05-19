//! Tests for the connections store opener + migration runner.
//!
//! These tests build a minimal `Config` with a tempdir-backed `workspace_dir`
//! and exercise the open + migration paths.

use super::*;
use crate::openhuman::config::Config;
use tempfile::TempDir;

fn config_with_workspace(dir: &TempDir) -> Config {
    let mut config = Config::default();
    config.workspace_dir = dir.path().to_path_buf();
    config
}

#[test]
fn open_creates_database_and_tables() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let table_count = with_connection(&config, |conn| {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master \
                 WHERE type='table' AND name IN ('generic_http_connections','schema_migrations')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        Ok(count)
    })
    .unwrap();

    assert_eq!(table_count, 2, "expected both tables to exist after open");
    assert!(
        dir.path().join("connections.db").exists(),
        "connections.db should exist after first open"
    );
}

#[test]
fn migration_is_recorded() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    let version: i64 = with_connection(&config, |conn| {
        let v = conn
            .query_row(
                "SELECT version FROM schema_migrations ORDER BY version DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        Ok(v)
    })
    .unwrap();

    assert_eq!(version, 1);
}

#[test]
fn second_open_does_not_re_apply_migration() {
    let dir = TempDir::new().unwrap();
    let config = config_with_workspace(&dir);

    // First open: applies migration.
    with_connection(&config, |_| Ok(())).unwrap();

    // Second open: should be a no-op for the migration insert.
    let row_count: i64 = with_connection(&config, |conn| {
        let n = conn
            .query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        Ok(n)
    })
    .unwrap();

    assert_eq!(
        row_count, 1,
        "schema_migrations should still have exactly one row"
    );
}

#[test]
fn workspace_dir_is_created_if_missing() {
    let parent = TempDir::new().unwrap();
    let nested = parent.path().join("nested").join("workspace");
    let mut config = Config::default();
    config.workspace_dir = nested.clone();

    with_connection(&config, |_| Ok(())).unwrap();

    assert!(
        nested.exists(),
        "store.rs should create nested workspace dir on open"
    );
    assert!(nested.join("connections.db").exists());
}
