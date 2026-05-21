//! Key system for element identity and reparenting.
//!
//! This module provides key types for view identity tracking.
//! Base key types (`Key`, `ValueKey`, `UniqueKey`, `ViewKey`) are in
//! `flui-foundation`. Widget-layer keys (`ObjectKey`, `GlobalKey`) are defined
//! here, matching Flutter's architecture where they live in
//! `widgets/framework.dart`.

mod global_key;
mod object_key;
pub(crate) mod registry;

// Registry install/take/handle stay crate-private. Production install
// happens in `crate::WidgetsBinding::new`; tests use the explicit
// `crate::test_only_*` shims. Exposing these would let downstream code
// arbitrarily replace the process-wide registry and break the
// `GlobalKey::current_*` invariants the binding installs. Callers
// inside the crate reach the registry helpers via
// `crate::key::registry::{install_registry, take_registry,
// GlobalKeyRegistryHandle}` directly.

// Re-export widget-layer keys (source of truth is here)
// Re-export foundation keys for convenience
pub use flui_foundation::{Key, KeyRef, Keyed, UniqueKey, ValueKey, ViewKey, WithKey};
pub use global_key::GlobalKey;
pub use object_key::ObjectKey;

/// Unique identifier for a GlobalKey.
pub type GlobalKeyId = u64;
