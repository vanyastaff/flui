//! Hook implementations for reactive state management.
//!
//! This module provides a comprehensive set of React-inspired hooks for managing
//! component state and side effects in FLUI applications.

// Re-export all hook implementations
pub mod callback;
pub mod effect;
pub mod memo;
pub mod reducer;
pub mod r#ref;
#[cfg(feature = "async")]
pub mod resource;

// Re-export types and functions
pub use callback::{use_callback, Callback};
pub use effect::{use_effect, use_effect_always, CleanupFn, EffectFn};
pub use memo::{use_memo, use_memo_once};
pub use r#ref::{use_ref, Ref};
pub use reducer::{use_reducer, Dispatch, Reducer};

#[cfg(feature = "async")]
pub use resource::{use_resource, Resource, ResourceState};
