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
//!
//! ## Reactive State (Hooks)
//! - [`use_signal`] - Create reactive state
//! - [`use_memo`] - Create derived/computed state
//! - [`use_effect`] - Run side effects on state changes
//! - [`Signal`] - Reactive value handle (cheap to clone)
//!
//! ## Render System
//! - [`Render`] - Trait for custom render objects
//! - Protocol-based contexts (see `render::protocol` module):
//!   - `BoxLayoutContext<A>`, `BoxPaintContext<A>` for box renders
//!   - `SliverLayoutContext<A>`, `SliverPaintContext<A>` for sliver renders
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

/// Run side effects with explicit dependencies
pub use crate::hooks::use_effect_with_deps;

/// Create memoized callback
pub use crate::hooks::use_callback;

/// Create mutable reference (no rebuild on change)
pub use crate::hooks::use_ref;

/// Redux-style state with reducer
pub use crate::hooks::use_reducer;

/// Reactive value handle (Copy, 8 bytes)
pub use crate::hooks::Signal;

/// Computed value with automatic dependency tracking
pub use crate::hooks::Computed;

/// Batch multiple updates
pub use crate::hooks::batch;

/// Dependency identifier for hooks
pub use crate::hooks::DependencyId;

/// Hook context for managing hook state
pub use crate::hooks::HookContext;

// ============================================================
// RENDER SYSTEM (for custom render objects)
// ============================================================

/// Trait for custom render objects (layout + paint)
pub use crate::render::RenderBox;

// Legacy contexts removed - use protocol-based contexts instead:
// - BoxLayoutContext<A>, BoxPaintContext<A> from crate::render::protocol
// - SliverLayoutContext<A>, SliverPaintContext<A> from crate::render::protocol

/// Type-safe arity trait for compile-time child count validation
pub use crate::render::Arity;

/// Runtime arity enum for dynamic child count validation
pub use crate::render::RuntimeArity;

/// Arity types for zero-cost abstraction
pub use crate::render::{AtLeast, Exact, Leaf, Optional, Single, Variable};

/// Children accessor types
pub use crate::render::{ChildrenAccess, FixedChildren, NoChildren, OptionalChild, SliceChildren};

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

// BoxedLayer removed - use Box<flui_engine::PictureLayer> directly
