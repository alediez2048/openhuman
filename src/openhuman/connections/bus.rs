//! Event-bus subscribers for the Connections domain.
//!
//! Phase 0 / P0-1 leaves this module intentionally empty. P0-2 wires
//! `ConnectionAdded` / `ConnectionRemoved` / `ConnectionUpdated` publication
//! when Generic HTTP CRUD mutates rows, and surfaces hooks that downstream
//! domains (e.g. Phase 1 workflows health recomputation) subscribe to.
