//! Adapter registry — dispatch an [`EntityRef`] to the right
//! [`EntityStore`] implementation (F4-4).
//!
//! Phase 4 F4-4 ships the dispatch shape only — concrete adapters
//! land in F4-5 (Google Sheets) and F4-6 (Attio). Until then both
//! variants return a `not_yet_implemented` error so any caller
//! that wires through this surface gets a clear, structured
//! failure rather than a panic.

use crate::openhuman::campaigns::types::EntityRef;
use crate::openhuman::config::Config;

use super::EntityStore;

/// Dispatch table from [`EntityRef`] variant to the matching
/// [`EntityStore`] adapter. F4-4 returns
/// `Err("not_yet_implemented")` for both real variants — F4-5 +
/// F4-6 fill in the real adapter constructors.
///
/// Tests + early callers that need a working store use
/// [`super::MockEntityStore`] directly via [`open_mock`].
pub fn open_entity_store(
    _config: &Config,
    binding: &EntityRef,
) -> anyhow::Result<Box<dyn EntityStore>> {
    match binding {
        EntityRef::GoogleSheet { spreadsheet_id, .. } => Err(anyhow::anyhow!(
            "[entity_store] google_sheets adapter not yet implemented (F4-5) — \
             campaign references spreadsheet_id={spreadsheet_id}"
        )),
        EntityRef::Attio {
            workspace_id,
            object_type,
        } => Err(anyhow::anyhow!(
            "[entity_store] attio adapter not yet implemented (F4-6) — \
             campaign references workspace_id={workspace_id} object_type={object_type}"
        )),
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
