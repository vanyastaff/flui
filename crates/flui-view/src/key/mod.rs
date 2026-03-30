//! Key system for element identity and reparenting.
//!
//! This module provides key types for view identity tracking.
//! Base key types (`Key`, `ValueKey`, `UniqueKey`, `ViewKey`) are in
//! `flui-foundation`. Widget-layer keys (`ObjectKey`, `GlobalKey`) are defined
//! here, matching Flutter's architecture where they live in
//! `widgets/framework.dart`.

mod global_key;
mod object_key;

// Re-export widget-layer keys (source of truth is here)
// Re-export foundation keys for convenience
pub use flui_foundation::{Key, KeyRef, Keyed, UniqueKey, ValueKey, ViewKey, WithKey};
pub use global_key::GlobalKey;
pub use object_key::ObjectKey;

/// Unique identifier for a GlobalKey.
pub type GlobalKeyId = u64;
