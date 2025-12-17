//! Key system for element identity and reparenting.
//!
//! Re-exports key types from `flui-foundation` which is the source of truth.

// Re-export from flui-foundation (source of truth)
pub use flui_foundation::{GlobalKey, Key, KeyRef, ObjectKey, UniqueKey, ValueKey, ViewKey};

/// Unique identifier for a GlobalKey (re-exported for backwards compatibility).
pub type GlobalKeyId = u64;
