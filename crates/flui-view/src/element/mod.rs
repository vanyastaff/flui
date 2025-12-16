//! Element system - mutable tree nodes that manage View lifecycle.
//!
//! Elements are the retained, mutable counterparts to immutable Views.
//! They manage:
//! - View lifecycle (mount, build, update, unmount)
//! - State persistence (for StatefulViews)
//! - Child element relationships
//! - RenderObject connections

mod lifecycle;

pub use lifecycle::Lifecycle;

// Element trait and implementations will be added in later phases
