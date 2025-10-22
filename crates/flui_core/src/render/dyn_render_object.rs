//! DynRenderObject - Object-safe base trait for RenderObject
//!
//! This module defines the `DynRenderObject` trait, which is object-safe and allows
//! render objects to be stored in heterogeneous collections like `Vec<Box<dyn DynRenderObject>>`.
//!
//! # Design Pattern: Two-Trait Approach
//!
//! Flui uses a two-trait pattern for render objects (similar to Widget/DynWidget and Element/DynElement):
//! - **DynRenderObject** (this trait) - Object-safe base trait for `Box<dyn DynRenderObject>` collections
//! - **RenderObject** - Extended trait with associated types for zero-cost concrete usage
//!
//! This allows:
//! - Zero-cost parent data access for concrete render object types
//! - Type-safe child relationships via associated types
//! - Heterogeneous render object storage in the render tree
//!
//! # Why DynRenderObject?
//!
//! The `RenderObject` trait has associated types (`ParentData`, `Child`), which makes it not object-safe.
//! This means you cannot create `Box<dyn RenderObject>` or `Vec<Box<dyn RenderObject>>`.
//!
//! `DynRenderObject` solves this by being object-safe - it doesn't have associated types.
//! All types that implement `RenderObject` automatically implement `DynRenderObject` via a blanket impl.
//!
//! # Naming Convention
//!
//! The `Dyn*` prefix is the idiomatic Rust convention for object-safe versions of traits.
//! The `Any*` prefix is reserved for `std::any::Any` and related types.
//!
//! # Usage
//!
//! ```rust,ignore
//! // For heterogeneous collections
//! let render_objects: Vec<Box<dyn DynRenderObject>> = vec![
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
/// It's used when you need trait objects (`Box<dyn DynRenderObject>`) for heterogeneous
/// render object collections.
///
/// # Design Pattern
///
/// Flui uses a two-trait pattern:
/// - **DynRenderObject** (this trait) - Object-safe, for `Box<dyn DynRenderObject>` collections
/// - **RenderObject** - Has associated types, for zero-cost concrete usage
///
/// # When to Use
///
/// - Use `Box<dyn DynRenderObject>` when you need to store render objects of different types
/// - Use `RenderObject` trait bound when working with concrete render object types
///
/// # Example
///
/// ```rust,ignore
/// struct RenderFlex {
///     children: Vec<Box<dyn DynRenderObject>>,  // Heterogeneous children
/// }
///
/// impl RenderFlex {
///     fn new(children: Vec<Box<dyn DynRenderObject>>) -> Self {
///         Self { children }
///     }
/// }
/// ```
pub trait DynRenderObject: DowncastSync + fmt::Debug {
    // ========== Core Layout and Painting ==========

    /// Perform layout with given constraints
    ///
    /// Returns the size this render object chose within the constraints.
    /// Must satisfy: `constraints.is_satisfied_by(returned_size)`
    ///
    /// # Arguments
    ///
    /// - `constraints`: The constraints within which to layout
    ///
    /// # Returns
    ///
    /// The size chosen by this render object
    #[must_use]
    fn layout(&mut self, constraints: BoxConstraints) -> Size;

    /// Paint this render object
    ///
    /// The painter is positioned at the render object's offset.
    /// Offset is relative to the parent's coordinate space.
    ///
    /// # Arguments
    ///
    /// - `painter`: The egui Painter to draw with
    /// - `offset`: Position relative to parent
    fn paint(&self, painter: &egui::Painter, offset: Offset);

    /// Get the current size (after layout)
    ///
    /// Returns the size determined by the last layout pass.
    /// This is only valid after `layout()` has been called.
    #[must_use]
    fn size(&self) -> Size;

    /// Get the constraints used in the last layout
    ///
    /// Returns the constraints from the last layout pass, or None if
    ///  the layout hasn't been performed yet.
    #[must_use]
    fn constraints(&self) -> Option<BoxConstraints> {
        None
    }

    // ========== Dirty State Management ==========

    /// Check if this render object needs layout
    ///
    /// Returns `true` if `mark_needs_layout()` has been called and
    /// layout hasn't been performed yet.
    #[must_use]
    #[inline]
    fn needs_layout(&self) -> bool {
        false
    }

    /// Mark this render object as needing layout
    ///
    /// This schedules the render object for layout during the next frame.
    fn mark_needs_layout(&mut self);

    /// Check if this render object needs paint
    ///
    /// Returns `true` if `mark_needs_paint()` has been called and
    /// painting hasn't been performed yet.
    #[must_use]
    #[inline]
    fn needs_paint(&self) -> bool {
        false
    }

    /// Mark this render object as needing paint
    ///
    /// This schedules the render object for painting during the next frame.
    fn mark_needs_paint(&mut self);

    /// Check if compositing bits need update
    ///
    /// Returns `true` if `mark_needs_compositing_bits_update()` has been called.
    #[must_use]
    fn needs_compositing_bits_update(&self) -> bool {
        false
    }

    /// Mark compositing bits as needing update
    ///
    /// This schedules the render object's compositing bits for recalculation.
    fn mark_needs_compositing_bits_update(&mut self) {
        // Default: no-op
    }

    // ========== Boundaries ==========

    /// Is this a relayout boundary?
    ///
    /// A relayout boundary prevents layout changes from propagating to ancestors.
    /// This is an optimization that allows layout to be performed more efficiently.
    ///
    /// # Returns
    ///
    /// `true` if this render object is a relayout boundary
    #[must_use]
    #[inline]
    fn is_relayout_boundary(&self) -> bool {
        false
    }

    /// Is this a repaint boundary?
    ///
    /// A repaint boundary prevents paint invalidation from propagating to ancestors.
    /// This is an optimization that allows painting to be performed more efficiently.
    ///
    /// # Returns
    ///
    /// `true` if this render object is a repaint boundary
    #[must_use]
    #[inline]
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Is size determined only by parent constraints?
    ///
    /// Returns `true` if this render object's size is entirely determined by
    /// its incoming constraints, without needing to query its children.
    ///
    /// This is an optimization: when `true`, if the constraints don't change,
    /// the render object doesn't need to re-lay out.
    ///
    /// # Example
    ///
    /// `RenderConstrainedBox` with tight constraints returns `true`.
    #[must_use]
    #[inline]
    fn sized_by_parent(&self) -> bool {
        false
    }

    // ========== Intrinsic Sizing ==========

    /// Computes minimum intrinsic width for a given height
    ///
    /// Returns the smallest width that this render object could be while still
    /// fitting all of its contents at the given height.
    ///
    /// # Naming Convention
    ///
    /// This method follows Rust API Guidelines (C-GETTER) by omitting the `get_`
    /// prefix. The method name directly describes what is being computed.
    ///
    /// # Arguments
    ///
    /// - `height`: The height constraint (may be infinite)
    ///
    /// # Returns
    ///
    /// The minimum intrinsic width
    #[must_use]
    fn min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0
    }

    /// Computes maximum intrinsic width for a given height
    ///
    /// Returns the largest width that this render object would prefer to be
    /// at the given height.
    ///
    /// # Arguments
    ///
    /// - `height`: The height constraint (may be infinite)
    ///
    /// # Returns
    ///
    /// The maximum intrinsic width (may be infinite)
    #[must_use]
    fn max_intrinsic_width(&self, _height: f32) -> f32 {
        f32::INFINITY
    }

    /// Computes minimum intrinsic height for a given width
    ///
    /// Returns the smallest height that this render object could be while still
    /// fitting all of its contents at the given width.
    ///
    /// # Arguments
    ///
    /// - `width`: The width constraint (may be infinite)
    ///
    /// # Returns
    ///
    /// The minimum intrinsic height
    #[must_use]
    fn min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0
    }

    /// Computes maximum intrinsic height for a given width
    ///
    /// Returns the largest height that this render object would prefer to be
    /// at the given width.
    ///
    /// # Arguments
    ///
    /// - `width`: The width constraint (may be infinite)
    ///
    /// # Returns
    ///
    /// The maximum intrinsic height (may be infinite)
    #[must_use]
    fn max_intrinsic_height(&self, _width: f32) -> f32 {
        f32::INFINITY
    }

    // ========== Hit Testing ==========

    /// Hit test this render object and its children
    ///
    /// Determines if the given position intersects this render object or any of its children.
    /// Children are tested first (front to back) before testing self.
    ///
    /// # Arguments
    ///
    /// - `result`: Hit test result to add entries to
    /// - `position`: Position in local coordinates
    ///
    /// # Returns
    ///
    /// `true` if the position hits this render object or its children
    #[must_use]
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

        // Add to the result if hit
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
    ///
    /// Returns `true` if the given position is within this render object's bounds.
    /// Override this to implement custom hit testing logic.
    ///
    /// # Arguments
    ///
    /// - `position`: Position in local coordinates
    ///
    /// # Returns
    ///
    /// `true` if a position hits this render object (default: always true if in bounds)
    #[must_use]
    #[inline]
    fn hit_test_self(&self, _position: Offset) -> bool {
        true
    }

    /// Hit test children
    ///
    /// Tests if the given position hits any children of this render object.
    /// Override this to implement child hit testing.
    ///
    /// # Arguments
    ///
    /// - `result`: Hit test result to add entries to
    /// - `position`: Position in local coordinates
    ///
    /// # Returns
    ///
    /// `true` if the position hits any child (default: false - no children)
    #[must_use]
    fn hit_test_children(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        false
    }

    // ========== Child Traversal ==========

    /// Visit all children (read-only)
    ///
    /// Calls the visitor function for each child render object.
    /// The default implementation does nothing (no children).
    ///
    /// # Arguments
    ///
    /// - `visitor`: Function to call for each child
    fn visit_children(&self, _visitor: &mut dyn FnMut(&dyn DynRenderObject)) {
        // Default: no children
    }

    /// Visit all children (mutable)
    ///
    /// Calls the visitor function for each child render object (mutable access).
    /// The default implementation does nothing (no children).
    ///
    /// # Arguments
    ///
    /// - `visitor`: Function to call for each child
    fn visit_children_mut(&mut self, _visitor: &mut dyn FnMut(&mut dyn DynRenderObject)) {
        // Default: no children
    }

    // ========== ParentData (Type-Erased) ==========

    /// Get parent data (type-erased)
    ///
    /// Returns the parent data as `&dyn Any` for downcasting.
    /// For type-safe access, use `RenderObject::parent_data()` instead.
    ///
    /// # Returns
    ///
    /// Parent data as Any, or None if no parent data
    #[must_use]
    fn parent_data_any(&self) -> Option<&dyn Any> {
        None
    }

    /// Get mutable parent data (type-erased)
    ///
    /// Returns the parent data as `&mut dyn Any` for downcasting.
    /// For type-safe access, use `RenderObject::parent_data_mut()` instead.
    ///
    /// # Returns
    ///
    /// Mutable parent data as Any, or None if no parent data
    #[must_use]
    fn parent_data_any_mut(&mut self) -> Option<&mut dyn Any> {
        None
    }

    /// Set parent data (type-erased)
    ///
    /// Sets the parent data for this render object.
    /// Called by the parent when adopting a child.
    ///
    /// # Arguments
    ///
    /// - `parent_data`: The parent data to set
    fn set_parent_data(&mut self, _parent_data: Box<dyn ParentData>) {
        // Default: no parent data storage
    }

    /// Setup parent data for a child
    ///
    /// Called when adopting a child to initialize its parent data.
    /// Override this to create and set appropriate parent data.
    ///
    /// # Arguments
    ///
    /// - `child`: The child has to set up parent data for
    fn setup_parent_data(&self, _child: &mut dyn DynRenderObject) {
        // Default: no setup needed
    }

    // ========== Tree Structure ==========

    /// Get the parent render object
    ///
    /// Returns a reference to the parent render object, or None if this is root.
    #[must_use]
    fn parent(&self) -> Option<&dyn DynRenderObject> {
        None
    }

    /// Get the depth of this render object in the tree
    ///
    /// Returns the depth (distance from root). Root has depth 0.
    #[must_use]
    #[inline]
    fn depth(&self) -> usize {
        0
    }

    /// Set the depth of this render object in the tree
    ///
    /// Called when the render object is moved to a different depth in the tree.
    ///
    /// # Arguments
    ///
    /// - `depth`: New depth value
    fn set_depth(&mut self, _depth: usize) {
        // Default: no-op
    }

    /// Update depth of a child
    ///
    /// Called to recursively update a child's depth after reparenting.
    ///
    /// # Arguments
    ///
    /// - `child`: The child to update
    fn redepth_child(&mut self, _child: &mut dyn DynRenderObject) {
        // Default: no-op
    }

    // ========== Lifecycle ==========

    /// Attach this render object to a PipelineOwner
    ///
    /// Called when the render object is inserted into the render tree.
    ///
    /// # Arguments
    ///
    /// - `owner`: The PipelineOwner managing this render tree
    fn attach(&mut self, _owner: Arc<RwLock<PipelineOwner>>) {
        // Default: no attachment needed
    }

    /// Detach this render object from its PipelineOwner
    ///
    /// Called when the render object is removed from the render tree.
    fn detach(&mut self) {
        // Default: no detachment needed
    }

    /// Dispose of this render object
    ///
    /// Called when the render object is permanently removed.
    /// Override to clean up resources.
    fn dispose(&mut self) {
        // Default: no disposal needed
    }

    /// Adopt a child render object
    ///
    /// Called when adding a child to this render object.
    /// Override to maintain a child list.
    ///
    /// # Arguments
    ///
    /// - `child`: The child to adopt
    fn adopt_child(&mut self, _child: &mut dyn DynRenderObject) {
        // Default: no children
    }

    /// Drop a child render object
    ///
    /// Called when removing a child from this render object.
    /// Override to maintain a child list.
    ///
    /// # Arguments
    ///
    /// - `child`: The child to drop
    fn drop_child(&mut self, _child: &mut dyn DynRenderObject) {
        // Default: no children
    }

    // ========== Layout Optimization ==========

    /// Whether the size depends only on the constraints
    ///
    /// Returns `true` if this render object's size is entirely determined
    /// by its constraints (identical to `sized_by_parent()`).
    ///
    /// # Note
    ///
    /// This is very similar to `sized_by_parent()`. The difference is subtle:
    /// - `sized_by_parent()`: optimization flag
    /// - `sizes_are_determined_by_constraints()`: General query method
    ///
    /// In practice, they usually return the same value.
    #[must_use]
    #[inline]
    fn sizes_are_determined_by_constraints(&self) -> bool {
        false
    }

    /// Perform resize (optimization for constraint-only sizing)
    ///
    /// Called when `sized_by_parent()` is true and constraints change.
    /// Sets the size without laying out children.
    fn perform_resize(&mut self) {
        // Default: no resize optimization
    }

    /// Perform layout (main layout logic)
    ///
    /// Called to compute the size and position of children.
    /// Override this to implement layout logic.
    fn perform_layout(&mut self) {
        // Default: no layout logic
    }

    // ========== Clipping ==========

    /// Paint bounds for clipping
    ///
    /// Returns the bounds that should be used for clipping children.
    /// Default is the size of this render object.
    ///
    /// # Returns
    ///
    /// Rectangle defining the paint bounds
    #[must_use]
    fn paint_bounds(&self) -> flui_types::Rect {
        flui_types::Rect::from_xywh(0.0, 0.0, self.size().width, self.size().height)
    }

    /// Apply the paint transform to descendants
    ///
    /// Returns the transform matrix to apply to a child during painting.
    /// Override this to implement transforms (e.g., rotation, scaling).
    ///
    /// # Arguments
    ///
    /// - `child`: The child to get transform for
    ///
    /// # Returns
    ///
    /// Transform matrix (default: identity)
    #[must_use]
    fn apply_paint_transform(&self, _child: &dyn DynRenderObject) -> glam::Mat4 {
        glam::Mat4::IDENTITY
    }
}

// Enable downcasting for DynRenderObject trait objects
impl_downcast!(sync DynRenderObject);

/// Boxed render object trait object
///
/// Commonly used for heterogeneous collections of render objects.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::BoxedRenderObject;
///
/// let render_objects: Vec<BoxedRenderObject> = vec![
///     Box::new(RenderText::new("Hello")),
///     Box::new(RenderImage::new(image)),
/// ];
/// ```
pub type BoxedRenderObject = Box<dyn DynRenderObject>;