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
//! - View traits (StatelessView, StatefulView, etc.)
//! - BuildContext for view building
//! - IntoElement for element conversion
//!
//! ## Reactive State (from flui-reactivity)
//! - Signal, Computed for reactive values
//! - batch for batching updates
//!
//! ## Render System
//! - RenderBox for custom render objects
//! - Arity types for child count validation
//!
//! ## Foundation Types
//! - Key, ElementId for identifiers

// ============================================================
// CORE VIEW SYSTEM
// ============================================================

/// View traits
pub use flui_view::{AnimatedView, ProviderView, ProxyView, StatefulView, StatelessView};

/// Render view trait and update result
pub use flui_view::{RenderView, UpdateResult};

/// Context for view building (abstract trait)
pub use flui_view::BuildContext;

/// Concrete BuildContext implementation
pub use crate::pipeline::PipelineBuildContext;

/// Trait for converting to elements
pub use flui_element::IntoElement;

/// Element from flui-element
pub use flui_element::Element;

// ============================================================
// REACTIVE STATE (from flui-reactivity)
// ============================================================

/// Reactive value handle (Copy, 8 bytes)
pub use flui_reactivity::Signal;

/// Computed value with automatic dependency tracking
pub use flui_reactivity::Computed;

/// Batch multiple updates
pub use flui_reactivity::batch;

/// Dependency identifier
pub use flui_reactivity::DependencyId;

/// Hook context for managing hook state
pub use flui_reactivity::HookContext;

/// Component identifier for hooks
pub use flui_reactivity::ComponentId;

// ============================================================
// RENDER SYSTEM
// ============================================================

/// Trait for custom render objects
pub use flui_rendering::RenderBox;

/// Type-safe arity trait
pub use flui_rendering::Arity;

/// Runtime arity enum
pub use flui_rendering::RuntimeArity;

/// Arity types
pub use flui_rendering::{AtLeast, Exact, Leaf, Optional, Single, Variable};

/// Children accessor types
pub use flui_rendering::{
    ChildrenAccess, FixedChildren, NoChildren, OptionalChild, SliceChildren,
};

// ============================================================
// FOUNDATION (keys and IDs)
// ============================================================

/// Compile-time view identifier
pub use flui_foundation::Key;

/// Reference to a key
pub use flui_foundation::KeyRef;

/// Runtime element identifier
pub use flui_foundation::ElementId;

// ============================================================
// EXTERNAL RE-EXPORTS
// ============================================================

/// 2D size (width, height)
pub use flui_types::Size;

/// 2D offset (dx, dy)
pub use flui_types::Offset;
