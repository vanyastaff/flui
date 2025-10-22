//! Rendering layer for Flui framework
//!
//! This crate provides the rendering infrastructure for layout and painting:
//! - RenderObject: Base trait for rendering
//! - RenderBox: Box layout protocol
//!
//! # Three-Tree Architecture
//!
//! This is the third tree in Flutter's architecture:
//!
//! ```text
//! Widget (immutable) → Element (mutable) → RenderObject (layout & paint)
//! ```
//!
//! # Layout Protocol
//!
//! 1. Parent sets constraints on child
//! 2. Child chooses size within constraints
//! 3. Parent positions child (sets offset)
//! 4. Parent returns its own size
//!
//! # Painting Protocol
//!
//! 1. Paint yourself
//! 2. Paint children in order
//! 3. Children are painted at their offsets
//!
//! # Module Organization
//!
//! - `core` - Core rendering infrastructure (RenderObject, RenderBox)
//! - `parent_data` - Parent data types for child communication
//! - `objects` - All render object implementations
//!   - `layout` - Layout render objects (Flex, Stack, Padding, etc.)
//!   - `effects` - Visual effects (Opacity, Transform, Clip, etc.)
//!   - `interaction` - Pointer/mouse interaction
//!   - `text` - Text rendering (future)
//!   - `media` - Image/video rendering (future)
//!   - `sliver` - Scrollable content (future)
//! - `painting` - Painting infrastructure
//! - `hit_testing` - Hit testing infrastructure
//! - `egui` - egui integration

#![warn(missing_docs)]

// Core modules
pub mod core;
pub mod parent_data;
pub mod objects;
pub mod painting;
pub mod hit_testing;
pub mod mouse;
pub mod delegates;
pub mod platform;
pub mod utils;
pub mod egui;
pub mod testing;

// Re-exports for macros (hidden from docs)
#[doc(hidden)]
pub use utils::layout_macros::__layout_cache_deps;

// Re-exports from core
pub use core::{RenderBox, RenderFlags, RenderObject, RenderProxyBox};

// Re-exports from parent_data
pub use parent_data::{FlexFit, FlexParentData, StackParentData};

// Re-exports from objects
pub use objects::layout::*;
pub use objects::effects::*;
pub use objects::interaction::*;

// Re-exports from painting
pub use painting::BoxDecorationPainter;

// Re-exports from hit_testing
pub use hit_testing::{HitTestEntry, HitTestResult};

// Re-export types from dependencies
pub use flui_core::BoxConstraints;
pub use flui_types::{Matrix4, Offset, Point, Rect, Size};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::core::{RenderBox, RenderObject, RenderProxyBox};
    pub use crate::parent_data::{FlexFit, FlexParentData, StackParentData};

    // Layout objects
    pub use crate::objects::layout::*;

    // Effects objects
    pub use crate::objects::effects::*;

    // Interaction objects
    pub use crate::objects::interaction::*;

    // Painting
    pub use crate::painting::BoxDecorationPainter;

    // Hit testing
    pub use crate::hit_testing::{HitTestEntry, HitTestResult};

    // Re-exports
    pub use flui_core::BoxConstraints;
    pub use flui_types::{Matrix4, Offset, Point, Rect, Size};
}
