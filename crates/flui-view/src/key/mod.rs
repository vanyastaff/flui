//! Key system for element identity and reparenting.
//!
//! This module provides key types for view identity tracking.
//! Base key types (`Key`, `ValueKey`, `UniqueKey`, `ViewKey`) are in
//! `flui-foundation`. Widget-layer keys (`ObjectKey`, `GlobalKey`) are defined
//! here.

mod global_key;
mod object_key;
pub(crate) mod registry;

// The registry helpers and handles stay crate-private: production activation is
// scoped by the owning `WidgetsBinding`, a test scopes one with
// `testing::activate_global_key_registry`. Exposing the raw helpers would let
// downstream code bypass ui_runtime entry and break the `GlobalKey::current_*`
// invariants.

// `ValueKey` is flui-foundation's; it is named here beside the widget-layer
// keys so a view author finds every key it writes in one place.
pub use flui_foundation::ValueKey;
pub use global_key::{GlobalKey, KeyLookup};
pub use object_key::ObjectKey;
