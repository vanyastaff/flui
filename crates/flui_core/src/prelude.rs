//! Prelude module for convenient imports
//!
//! The prelude provides the most commonly used types and traits from flui-core.
//! Import everything you need with a single line:
//!
//! ```rust
//! use flui_core::prelude::*;
//! ```
//!
//! # What's included
//!
//! ## Core View System
//! - [`View`] - The main trait for creating UI components
//! - [`BuildContext`] - Context provided during view building
//! - [`IntoElement`] - Trait for converting tuples to elements
//! - [`AnyView`] - Type-erased view for heterogeneous collections
//! - [`AnyElement`] - Type-erased element storage
//!
//! ## Reactive State (Hooks)
//! - [`use_signal`] - Create reactive state
//! - [`use_memo`] - Create derived/computed state
//! - [`use_effect`] - Run side effects on state changes
//! - [`Signal`] - Reactive value handle (cheap to clone)
//!
//! ## Render System
//! - [`Render`] - Trait for custom render objects
//! - [`LayoutContext`] - Context for layout operations
//! - [`PaintContext`] - Context for paint operations
//! - [`Arity`] - Child count specification
//! - [`Children`] - Unified child representation
//!
//! ## Foundation Types
//! - [`Key`] - Compile-time view identifier
//! - [`KeyRef`] - Reference to a key
//! - [`ElementId`] - Runtime element identifier
//!
//! ## External Re-exports (convenience)
//! - [`Size`], [`Offset`] from flui_types
//! - [`BoxedLayer`] from flui_engine
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_core::prelude::*;
//!
//! #[derive(Debug)]
//! struct Counter;
//!
//! impl View for Counter {
//!     fn build(self, ctx: &BuildContext) -> impl IntoElement {
//!         let count = use_signal(ctx, 0);
//!
//!         // Use count, Size, Offset, etc. without additional imports
//!         (MyRenderer::new(), None)
//!     }
//! }
//! ```

// ============================================================
// CORE VIEW SYSTEM (always needed)
// ============================================================

/// The main trait for creating UI components
pub use crate::view::View;

/// Context provided during view building
pub use crate::view::BuildContext;

/// Trait for converting tuples to elements
pub use crate::view::IntoElement;

/// Type-erased view for heterogeneous collections
pub use crate::view::AnyView;

/// Type-erased element storage
pub use crate::view::AnyElement;

/// Element enum (Component/Render/Provider)
pub use crate::element::Element;

// ============================================================
// REACTIVE STATE (HOOKS)
// ============================================================

/// Create reactive state that triggers rebuilds on change
pub use crate::hooks::use_signal;

/// Create derived/computed state from other state
pub use crate::hooks::use_memo;

/// Run side effects when dependencies change
pub use crate::hooks::use_effect;

/// Reactive value handle (cheap to clone, use in closures)
pub use crate::hooks::Signal;

// ============================================================
// RENDER SYSTEM (for custom render objects)
// ============================================================

/// Trait for custom render objects (layout + paint)
pub use crate::render::Render;

/// Context for layout operations (constraints, children)
pub use crate::render::LayoutContext;

/// Context for paint operations (offset, children)
pub use crate::render::PaintContext;

/// Child count specification (Exact(n) or Variable)
pub use crate::render::Arity;

/// Unified child representation (None/Single/Multi)
pub use crate::render::Children;

// ============================================================
// FOUNDATION (keys and IDs)
// ============================================================

/// Compile-time view identifier
pub use crate::foundation::Key;

/// Reference to a key
pub use crate::foundation::KeyRef;

/// Runtime element identifier
pub use crate::foundation::ElementId;

// ============================================================
// EXTERNAL RE-EXPORTS (convenience)
// ============================================================

/// 2D size (width, height)
pub use flui_types::Size;

/// 2D offset (dx, dy)
pub use flui_types::Offset;

/// Boxed layer for rendering
pub use flui_engine::BoxedLayer;
