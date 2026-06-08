//! Adapter registry — dispatch an [`EntityRef`] to the right
//! [`EntityStore`] implementation (F4-4 trait, F4-5 Sheets, F4-6 Attio).
//!
//! Both production adapters are wired by F4-6. The factory resolves
//! the live mode-aware Composio client once per call and hands it to
//! the adapter.

use std::sync::Arc;

use crate::openhuman::campaigns::types::EntityRef;
use crate::openhuman::composio::client::create_composio_client;
use crate::openhuman::config::Config;

use super::attio::AttioAdapter;
use super::google_sheets::{GoogleSheetsAdapter, LiveComposioExecutor};
use super::EntityStore;

/// Dispatch table from [`EntityRef`] variant to the matching
/// [`EntityStore`] adapter.
///
/// - `GoogleSheet` → [`GoogleSheetsAdapter`] (F4-5).
/// - `Attio`       → [`AttioAdapter`] (F4-6).
///
/// Both adapters share the same `LiveComposioExecutor` shape — they
/// route reads/writes/subscribe through the same mode-aware client.
///
/// Returns an error if the user has neither a backend session nor a
/// direct-mode API key.
///
/// Tests + early callers that need a working store use
/// [`super::MockEntityStore`] directly via [`open_mock`].
pub fn open_entity_store(
    config: &Config,
    binding: &EntityRef,
) -> anyhow::Result<Box<dyn EntityStore>> {
    let kind = create_composio_client(config)
        .map_err(|e| anyhow::anyhow!("[entity_store] adapter requires a Composio client: {e}"))?;
    let executor = Arc::new(LiveComposioExecutor::new(
        kind,
        config.composio.entity_id.clone(),
    ));
    match binding {
        EntityRef::GoogleSheet {
            spreadsheet_id,
            range,
        } => Ok(Box::new(GoogleSheetsAdapter::new(
            executor,
            spreadsheet_id.clone(),
            range.clone(),
        ))),
        EntityRef::Attio {
            workspace_id,
            object_type,
        } => Ok(Box::new(AttioAdapter::new(
            executor,
            workspace_id.clone(),
            object_type.clone(),
        ))),
    }
}

/// Open a [`super::MockEntityStore`] for tests + early prototyping.
/// Independent of the real registry so a missing adapter doesn't
/// block the F4-7 executor / F4-9 approval queue from being tested
/// end-to-end against the trait contract.
#[cfg(test)]
pub fn open_mock(seed: Vec<super::EntityRecord>) -> Box<dyn EntityStore> {
    Box::new(super::MockEntityStore::with_records(seed))
}
