//! AnyRenderObject - Object-safe base trait for RenderObject
//!
//! This module defines the `AnyRenderObject` trait, which is object-safe and allows
//! render objects to be stored in heterogeneous collections like `Vec<Box<dyn AnyRenderObject>>`.
//!
//! # Design Pattern: Two-Trait Approach
//!
//! Flui uses a two-trait pattern for render objects (similar to Widget/AnyWidget and Element/AnyElement):
//! - **AnyRenderObject** (this trait) - Object-safe base trait for `Box<dyn AnyRenderObject>` collections
//! - **RenderObject** - Extended trait with associated types for zero-cost concrete usage
//!
//! This allows:
//! - Zero-cost parent data access for concrete render object types
//! - Type-safe child relationships via associated types
//! - Heterogeneous render object storage in the render tree
//!
//! # Why AnyRenderObject?
//!
//! The `RenderObject` trait has associated types (`ParentData`, `Child`), which makes it not object-safe.
//! This means you cannot create `Box<dyn crate::AnyRenderObject>` or `Vec<Box<dyn crate::AnyRenderObject>>`.
//!
//! `AnyRenderObject` solves this by being object-safe - it doesn't have associated types.
//! All types that implement `RenderObject` automatically implement `AnyRenderObject` via a blanket impl.
//!
//! # Usage
//!
//! ```rust,ignore
//! // For heterogeneous collections
//! let render_objects: Vec<Box<dyn AnyRenderObject>> = vec![
//!     Box::new(RenderText::new("Hello")),
//!     Box::new(RenderImage::new(image)),
//!     Box::new(RenderFlex::new(children)),
//! ];
//!
//! // For concrete types with zero-cost
//! let padding = RenderPadding::new(child);
//! let size = padding.layout(constraints);  // Uses RenderObject trait, no boxing!
//! ```

use std::any::Any;
use std::fmt;
use std::sync::Arc;

use downcast_rs::{impl_downcast, DowncastSync};
use flui_types::{events::HitTestResult, Offset, Size};
use parking_lot::RwLock;

use crate::{BoxConstraints, ParentData, PipelineOwner};

/// Object-safe base trait for all render objects
///
/// This trait is automatically implemented for all types that implement `RenderObject`.
/// It's used when you need trait objects (`Box<dyn AnyRenderObject>`) for heterogeneous
/// render object collections.
///
/// # Design Pattern
///
/// Flui uses a two-trait pattern:
/// - **AnyRenderObject** (this trait) - Object-safe, for `Box<dyn AnyRenderObject>` collections
/// - **RenderObject** - Has associated types, for zero-cost concrete usage
///
/// # When to Use
///
/// - Use `Box<dyn AnyRenderObject>` when you need to store render objects of different types
/// - Use `RenderObject` trait bound when working with concrete render object types
///
/// # Example
///
/// ```rust,ignore
/// struct RenderFlex {
///     children: Vec<Box<dyn AnyRenderObject>>,  // Heterogeneous children
/// }
///
/// impl RenderFlex {
///     fn new(children: Vec<Box<dyn AnyRenderObject>>) -> Self {
///         Self { children }
///     }
/// }
/// ```
pub trait AnyRenderObject: DowncastSync + fmt::Debug {
    // ========== Core Layout and Painting ==========

    /// Perform layout with given constraints
    ///
    /// Returns the size this render object chose within the constraints.
    /// Must satisfy: `constraints.is_satisfied_by(returned_size)`
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint this render object
    ///
    /// The painter is positioned at the render object's offset.
    /// Offset is relative to parent's coordinate space.
    fn paint(&self, painter: &egui::Painter, offset: Offset);

    /// Get the current size (after layout)
    fn size(&self) -> Size;

    /// Get the constraints used in last layout
    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    // ========== Dirty State Management ==========

    /// Check if this render object needs layout
    fn needs_layout(&self) -> bool {
        false
    }

    /// Mark this render object as needing layout
    fn mark_needs_layout(&mut self);

    /// Check if this render object needs paint
    fn needs_paint(&self) -> bool {
        false
    }

    /// Mark this render object as needing paint
    fn mark_needs_paint(&mut self);

    // ========== Intrinsic Sizing ==========

    /// Get minimum intrinsic width for given height
    fn get_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Get maximum intrinsic width for given height
    fn get_max_intrinsic_width(&self, _height: f32) -> f32 {
        f32::INFINITY
    }

    /// Get minimum intrinsic height for given width
    fn get_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Get maximum intrinsic height for given width
    fn get_max_intrinsic_height(&self, _width: f32) -> f32 {
        f32::INFINITY
    }

    // ========== Hit Testing ==========

    /// Hit test this render object and its children
    fn hit_test(&self, result: &mut HitTestResult, position: Offset) -> bool {
        // Check bounds first
        if position.dx < 0.0
            || position.dx >= self.size().width
            || position.dy < 0.0
            || position.dy >= self.size().height
        {
            return false;
        }

        // Check children first (front to back)
        let hit_child = self.hit_test_children(result, position);

        // Then check self
        let hit_self = self.hit_test_self(position);

        // Add to result if hit
        if hit_child || hit_self {
            result.add(flui_types::events::HitTestEntry::new(
                position,
                self.size(),
            ));
            return true;
        }

        false
    }

    /// Hit test only this render object (not children)
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    /// Hit test children
    fn hit_test_children(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        false
    }

    // ========== Child Traversal ==========

    /// Visit all children (read-only)
    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn AnyRenderObject)) {
        // Default: no children
    }

    /// Visit all children (mutable)
    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn AnyRenderObject)) {
        // Default: no children
    }

    // ========== ParentData (Type-Erased) ==========

    /// Get parent data (type-erased)
    ///
    /// For type-safe access, use `RenderObject::parent_data()` instead.
    fn parent_data_any(&self) -> Option<&dyn Any> {
        None
    }

    /// Get mutable parent data (type-erased)
    ///
    /// For type-safe access, use `RenderObject::parent_data_mut()` instead.
    fn parent_data_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    /// Set parent data (type-erased)
    fn set_parent_data(&mut self, _parent_data: Box<dyn ParentData>) {
        // Default: no parent data storage
    }

    /// Setup parent data for a child
    fn setup_parent_data(&self, _child: &mut dyn AnyRenderObject) {
        // Default: no setup needed
    }

    // ========== Tree Structure ==========

    /// Get the parent render object
    fn parent(&self) -> Option<&dyn AnyRenderObject> {
        None
    }

    /// Get the depth of this render object in the tree
    fn depth(&self) -> i32 {
        0
    }

    // ========== Lifecycle ==========

    /// Attach this render object to a PipelineOwner
    fn attach(&mut self, _owner: Arc<RwLock<PipelineOwner>>) {
        // Default: no attachment needed
    }

    /// Detach this render object from its PipelineOwner
    fn detach(&mut self) {
        // Default: no detachment needed
    }

    /// Dispose of this render object
    fn dispose(&mut self) {
        // Default: no disposal needed
    }

    /// Adopt a child render object
    fn adopt_child(&mut self, _child: &mut dyn AnyRenderObject) {
        // Default: no children
    }

    /// Drop a child render object
    fn drop_child(&mut self, _child: &mut dyn AnyRenderObject) {
        // Default: no children
    }

    // ========== Layout Optimization ==========

    /// Whether the size depends only on the constraints
    fn sizes_are_determined_by_constraints(&self) -> bool {
        false
    }

    /// Perform resize (optimization for constraint-only sizing)
    fn perform_resize(&mut self) {
        // Default: no resize optimization
    }

    /// Perform layout (main layout logic)
    fn perform_layout(&mut self) {
        // Default: no layout logic
    }

    // ========== Clipping ==========

    /// Paint bounds for clipping
    ///
    /// Returns the bounds that should be used for clipping children.
    /// Default is the size of this render object.
    fn paint_bounds(&self) -> flui_types::Rect {
        flui_types::Rect::from_xywh(0.0, 0.0, self.size().width, self.size().height)
    }

    /// Apply the paint transform to descendants
    ///
    /// Returns the transform matrix to apply to children during painting.
    fn apply_paint_transform(&self, _child: &dyn AnyRenderObject) -> glam::Mat4 {
        glam::Mat4::IDENTITY
    }
}

// Enable downcasting for AnyRenderObject trait objects
impl_downcast!(sync AnyRenderObject);
