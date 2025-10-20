//! Foundation types for the framework
//!
//! This module contains core primitive types used throughout flui-core.

pub mod id;
pub mod lifecycle;
pub mod slot;
pub mod string_cache;


// Re-exports
pub use id::ElementId;
pub use lifecycle::Lifecycle;
pub use slot::Slot;

