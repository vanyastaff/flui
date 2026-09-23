//! BuildContext - interface for View building.
//!
//! This module provides:
//! - [`BuildContext`] - The trait Views use during build
//! - [`BuildContextExt`] - Extension methods for typed lookups
//! - [`ElementBuildContext`] - Concrete implementation for Elements

mod build_context;
mod element_build_context;

pub use build_context::{BuildContext, BuildContextExt, LifecycleContext};
pub use element_build_context::{ElementBuildContext, ElementBuildContextBuilder};
// Build-time borrowed context + its deferred dependent-record (PR-K). Crate
// -internal: constructed by the behaviors during `build_scope`, applied by
// `BuildOwner`.
pub(crate) use element_build_context::{BuildCapabilities, BuildCtx, DependentRecord};
// The unconstructible-outside-this-crate token gating dependency mutation.
pub(crate) use build_context::sealed::CrateToken;
