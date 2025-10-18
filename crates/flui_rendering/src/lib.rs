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

#![warn(missing_docs)]
pub mod decoration_painter;
pub mod egui_ext;
pub mod flex_parent_data;
pub mod render_aspect_ratio;
pub mod render_box;
pub mod render_constrained_box;
pub mod render_decorated_box;
pub mod render_flex;
pub mod render_fractionally_sized_box;
pub mod render_indexed_stack;
pub mod render_limited_box;
pub mod render_object;
pub mod render_opacity;
pub mod render_padding;
pub mod render_positioned_box;
pub mod render_stack;
pub mod stack_parent_data;















// Re-exports
pub use decoration_painter::BoxDecorationPainter;
pub use flex_parent_data::{FlexFit, FlexParentData};
pub use render_aspect_ratio::RenderAspectRatio;
pub use render_box::{RenderBox, RenderProxyBox};
pub use render_constrained_box::RenderConstrainedBox;
pub use render_decorated_box::{DecorationPosition, RenderDecoratedBox};
pub use render_flex::RenderFlex;
pub use render_fractionally_sized_box::RenderFractionallySizedBox;
pub use render_indexed_stack::RenderIndexedStack;
pub use render_limited_box::RenderLimitedBox;
pub use render_object::RenderObject;
pub use render_opacity::RenderOpacity;
pub use render_padding::RenderPadding;
pub use render_positioned_box::RenderPositionedBox;
pub use render_stack::{RenderStack, StackFit};
pub use stack_parent_data::StackParentData;

// Re-export types from dependencies
pub use flui_core::BoxConstraints;
pub use flui_types::{Offset, Point, Rect, Size};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::decoration_painter::BoxDecorationPainter;
    pub use crate::flex_parent_data::{FlexFit, FlexParentData};
    pub use crate::render_aspect_ratio::RenderAspectRatio;
    pub use crate::render_box::{RenderBox, RenderProxyBox};
    pub use crate::render_constrained_box::RenderConstrainedBox;
    pub use crate::render_decorated_box::{DecorationPosition, RenderDecoratedBox};
    pub use crate::render_flex::RenderFlex;
    pub use crate::render_fractionally_sized_box::RenderFractionallySizedBox;
    pub use crate::render_indexed_stack::RenderIndexedStack;
    pub use crate::render_limited_box::RenderLimitedBox;
    pub use crate::render_object::RenderObject;
    pub use crate::render_opacity::RenderOpacity;
    pub use crate::render_padding::RenderPadding;
    pub use crate::render_positioned_box::RenderPositionedBox;
    pub use crate::render_stack::{RenderStack, StackFit};
    pub use crate::stack_parent_data::StackParentData;
    pub use flui_core::BoxConstraints;
    pub use flui_types::{Offset, Point, Rect, Size};
}














