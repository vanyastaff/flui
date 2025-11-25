//! # FLUI View
//!
//! View traits and abstractions for the FLUI UI framework.
//!
//! This crate provides the core View traits that define how declarative UI
//! components are structured. It is designed to be independent of the concrete
//! element tree implementation.
//!
//! ## Architecture
//!
//! ```text
//! View (immutable) → Element (mutable) → RenderObject (layout/paint)
//! ```
//!
//! ## View Types
//!
//! - [`StatelessView`] - Simple views without state
//! - [`StatefulView`] - Views with persistent state
//! - [`ProxyView`] - Views that wrap single child
//! - [`RenderView`] - Views that create render objects
//!
//! ## Design Philosophy
//!
//! 1. **Trait-based**: Views are defined by traits, not concrete types
//! 2. **Protocol system**: Compile-time view categorization
//! 3. **Abstract context**: BuildContext is trait-based for flexibility
//! 4. **Thread-safe**: All views must be `Send + 'static`
//!
//! ## Example
//!
//! ```rust,ignore
//! use flui_view::{StatelessView, BuildContext, IntoElement};
//!
//! struct Greeting {
//!     name: String,
//! }
//!
//! impl StatelessView for Greeting {
//!     type Context = MyBuildContext;
//!
//!     fn build(self, ctx: &Self::Context) -> impl IntoElement<Self::Context> {
//!         Text::new(format!("Hello, {}!", self.name))
//!     }
//! }
//! ```

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    clippy::all,
    clippy::pedantic
)]
#![allow(
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::return_self_not_must_use
)]

// ============================================================================
// MODULES
// ============================================================================

pub mod context;
pub mod into_element;
pub mod protocol;
pub mod state;
pub mod traits;
pub mod update;

// ============================================================================
// RE-EXPORTS
// ============================================================================

// Context traits
pub use context::{BuildContext, ViewContext};

// Protocol types
pub use protocol::{ViewMode, ViewProtocol};

// State trait
pub use state::ViewState;

// Update result
pub use update::UpdateResult;

// View traits
pub use traits::{ProxyView, RenderView, StatefulView, StatelessView};

// IntoElement trait
pub use into_element::IntoElement;

// Re-export ElementId for convenience
pub use flui_foundation::ElementId;

// ============================================================================
// PRELUDE
// ============================================================================

/// Commonly used types for convenient importing.
pub mod prelude {
    pub use crate::{
        BuildContext, ElementId, IntoElement, ProxyView, RenderView, StatefulView, StatelessView,
        UpdateResult, ViewContext, ViewMode, ViewState,
    };
}

// ============================================================================
// VERSION
// ============================================================================

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
