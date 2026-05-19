//! Controller schemas + registry for the Connections domain.
//!
//! Phase 0 / P0-1 ships an empty controller list. P0-2 and P0-3 populate this
//! file with the `connections_list`, `connections_generic_http_*`, and
//! `connections_test` controllers, wired into `src/core/all.rs`.

use crate::core::all::RegisteredController;
use crate::core::ControllerSchema;

/// All controller schemas declared by the connections domain.
pub fn all_controller_schemas() -> Vec<ControllerSchema> {
    Vec::new()
}

/// All controllers (schema + handler) registered by the connections domain.
pub fn all_registered_controllers() -> Vec<RegisteredController> {
    Vec::new()
}

/// Convenience: same as [`all_controller_schemas`] but uniquely named for
/// the `src/core/all.rs` aggregation point.
pub fn all_connections_controller_schemas() -> Vec<ControllerSchema> {
    all_controller_schemas()
}

/// Convenience: same as [`all_registered_controllers`] but uniquely named for
/// the `src/core/all.rs` aggregation point.
pub fn all_connections_registered_controllers() -> Vec<RegisteredController> {
    all_registered_controllers()
}
