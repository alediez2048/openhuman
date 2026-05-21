//! Controller schemas + registry for the Workflows domain.
//!
//! F-1 lands the scaffold with an empty controller list — Phase 1 has no
//! RPC handlers wired yet. F-2 fills in `workflows_list`,
//! `workflows_get`, `workflows_create`, `workflows_update`,
//! `workflows_delete`, `workflows_enable`, `workflows_disable`. F-5,
//! F-7, F-8 each add their own controllers.

use crate::core::all::RegisteredController;
use crate::core::ControllerSchema;

/// All controller schemas declared by the workflows domain. Empty in
/// F-1; populated by F-2 onwards.
pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    Vec::new()
}

/// All controllers (schema + handler) registered by the workflows
/// domain. Empty in F-1; populated by F-2 onwards.
pub fn all_registered_controllers() -> Vec<RegisteredController> {
    Vec::new()
}

/// Alias used by `core/all.rs` to compose every domain's schemas.
pub fn all_workflows_controller_schemas() -> Vec<ControllerSchema> {
    all_controller_schemas()
}

/// Alias used by `core/all.rs` to compose every domain's controllers.
pub fn all_workflows_registered_controllers() -> Vec<RegisteredController> {
    all_registered_controllers()
}
