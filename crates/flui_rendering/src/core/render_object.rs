//! Core render object trait with enhanced safety and Flutter compliance.
//!
//! This module provides the foundation for all render objects in FLUI:
//! - [`RenderObject`] - Base trait for all render objects (protocol-agnostic)
//! - [`RenderObjectExt`] - Extension trait for safe downcasting
//!
//! # Architecture
//!
//! RenderObject provides two complementary APIs that work together:
//!
//! ## Level 1: Dyn-Compatible Methods (Type-Erased, Flutter-Style)
//!
//! These methods enable type-erased rendering operations without compile-time arity knowledge.
//! They work directly with the tree and are called by `RenderElement`:
//!
//! ```rust,ignore
//! // RenderElement calls dyn-compatible methods:
//! let size = render_object.perform_layout(element_id, constraints, tree)?;
//! render_object.paint(element_id, offset, size, canvas, tree);
//! let hit = render_object.hit_test(element_id, position, result, tree);
//! ```
//!
//! **When to use:**
//! - Storing `Box<dyn RenderObject>` in collections
//! - Runtime polymorphism without knowing specific types
//! - Framework-level operations on heterogeneous render objects
//!
//! ## Level 2: Typed Methods (Protocol-Specific, High-Performance)
//!
//! These are the high-performance typed APIs provided by `RenderBox<A>` and
//! `RenderSliver<A>` traits that use typed contexts with compile-time arity validation:
//!
//! ```rust,ignore
//! // Typed API with compile-time validation:
//! impl RenderBox<Single> for RenderPadding {
//!     fn layout(&mut self, ctx: BoxLayoutContext<'_, Single>) -> RenderResult<Size> {
//!         let child_size = ctx.layout_single_child()?;  // Type-safe!
//!         Ok(child_size + self.padding.size())
//!     }
//! }
//! ```
//!
//! **When to use:**
//! - Implementing new render objects
//! - Hot-path operations where performance matters
//! - Leveraging compile-time arity validation
//!
//! # Design Philosophy
//!
//! - **Two-level API**: Type-erased for flexibility, typed for performance
//! - **State externalization**: State lives in `RenderState<P>`, not in render object
//! - **Protocol agnostic**: Base trait works with any protocol (Box, Sliver, custom)
//! - **Flutter compatibility**: Exact Flutter RenderObject semantics
//! - **Zero-cost abstractions**: No overhead for type erasure when not needed
//!
//! # Relationship with Flutter
//!
//! | Flutter | FLUI | Notes |
//! |---------|------|-------|
//! | `RenderObject` | `RenderObject` | Base trait, protocol-agnostic |
//! | `RenderBox` | `RenderBox<A>` | Box protocol with arity `A` |
//! | `RenderSliver` | `RenderSliver<A>` | Sliver protocol with arity `A` |
//! | `performLayout()` | `perform_layout()` | Dyn-compatible layout |
//! | `layout()` (typed) | `layout(ctx)` | Typed layout with contexts |
//! | `paint()` | `paint()` | Dyn-compatible paint |
//! | `hitTest()` | `hit_test()` | Dyn-compatible hit test |
//!
//! # State Management
//!
//! State is managed externally in `RenderState<P>`, **not** in the render object:
//!
//! ```text
//! RenderElement
//!  ├── render_object: Box<dyn RenderObject>  (immutable configuration)
//!  └── state: RenderState<P>                 (mutable state)
//!       ├── flags: AtomicRenderFlags         (lock-free dirty tracking)
//!       ├── geometry: OnceCell<P::Geometry>  (cached layout result)
//!       ├── constraints: OnceCell<P::Constraints>  (cache validation)
//!       └── offset: AtomicOffset             (paint position)
//! ```
//!
//! **Why external state?**
//! - ✅ Enables immutable render objects (functional style)
//! - ✅ Simplifies concurrent access (no &mut self needed for reads)
//! - ✅ Allows state to be protocol-specific while render object is generic
//! - ✅ Makes cloning/comparing render objects trivial
//!
//! # Examples
//!
//! ## Implementing a Leaf Render Object
//!
//! ```rust,ignore
//! use flui_rendering::core::{RenderObject, RenderBox, Leaf};
//!
//! /// Render object for displaying an image
//! #[derive(Debug, Clone)]
//! pub struct RenderImage {
//!     image_data: Arc<ImageData>,
//!     fit: BoxFit,
//! }
//!
//! impl RenderBox<Leaf> for RenderImage {
//!     fn layout(&mut self, ctx: BoxLayoutContext<'_, Leaf>) -> RenderResult<Size> {
//!         // Get intrinsic image size
//!         let intrinsic = self.image_data.size();
//!
//!         // Apply fit and constrain to parent constraints
//!         let size = self.fit.apply(intrinsic, ctx.constraints);
//!         Ok(ctx.constraints.constrain(size))
//!     }
//!
//!     fn paint(&self, ctx: &mut BoxPaintContext<'_, Leaf>) {
//!         let rect = Rect::from_min_size(ctx.offset, ctx.geometry);
//!         ctx.canvas_mut().draw_image(&self.image_data, rect);
//!     }
//! }
//!
//! impl RenderObject for RenderImage {
//!     fn as_any(&self) -> &dyn Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn Any { self }
//!
//!     fn intrinsic_size(&self) -> Option<Size> {
//!         Some(self.image_data.size())
//!     }
//!
//!     fn handles_pointer_events(&self) -> bool {
//!         false  // Images don't handle events by default
//!     }
//! }
//! ```
//!
//! ## Implementing a Container with Dyn-Compatible Methods
//!
//! ```rust,ignore
//! /// Custom column layout (dyn-compatible implementation)
//! #[derive(Debug)]
//! pub struct CustomColumn {
//!     spacing: f32,
//! }
//!
//! impl RenderObject for CustomColumn {
//!     fn as_any(&self) -> &dyn Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn Any { self }
//!
//!     fn perform_layout(
//!         &mut self,
//!         element_id: ElementId,
//!         constraints: BoxConstraints,
//!         tree: &mut dyn LayoutTree,
//!     ) -> RenderResult<Size> {
//!         let children: Vec<_> = tree.children(element_id).collect();
//!
//!         let mut y = 0.0;
//!         let mut max_width = 0.0;
//!
//!         // Layout each child
//!         for child_id in children {
//!             let child_size = tree.perform_layout(child_id, constraints)?;
//!             tree.set_offset(child_id, Offset::new(0.0, y));
//!
//!             y += child_size.height + self.spacing;
//!             max_width = max_width.max(child_size.width);
//!         }
//!
//!         // Remove trailing spacing
//!         if y > 0.0 {
//!             y -= self.spacing;
//!         }
//!
//!         Ok(constraints.constrain(Size::new(max_width, y)))
//!     }
//!
//!     fn paint(
//!         &self,
//!         element_id: ElementId,
//!         offset: Offset,
//!         _size: Size,
//!         _canvas: &mut Canvas,
//!         tree: &dyn PaintTree,
//!     ) {
//!         // Paint all children using their stored offsets
//!         for child_id in tree.children(element_id) {
//!             if let Some(child_offset) = tree.get_offset(child_id) {
//!                 let _ = tree.perform_paint(child_id, offset + child_offset);
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ## Using Both APIs Together
//!
//! ```rust,ignore
//! // Typed API for new implementations (preferred)
//! impl RenderBox<Variable> for MyWidget {
//!     fn layout(&mut self, ctx: BoxLayoutContext<'_, Variable>) -> RenderResult<Size> {
//!         // Use typed context for compile-time safety
//!         for child_id in ctx.children() {
//!             let size = ctx.layout_child(child_id, constraints)?;
//!             // ...
//!         }
//!         Ok(total_size)
//!     }
//! }
//!
//! impl RenderObject for MyWidget {
//!     // Dyn-compatible methods are auto-implemented via blanket impl
//!     fn as_any(&self) -> &dyn Any { self }
//!     fn as_any_mut(&mut self) -> &mut dyn Any { self }
//! }
//!
//! // Framework can use either API as needed:
//! let typed: &dyn RenderBox<Variable> = &my_widget;
//! let erased: &dyn RenderObject = &my_widget;
//! ```
//!
//! # Thread Safety
//!
//! All render objects must be `Send + Sync`:
//! - Enables parallel layout in different subtrees
//! - Allows rendering from multiple threads
//! - Required for web workers and isolates
//!
//! If your render object contains non-Send data, use `Arc` or similar:
//!
//! ```rust,ignore
//! #[derive(Debug)]
//! pub struct RenderCustomPaint {
//!     painter: Arc<dyn CustomPainter>,  // Arc makes it Send + Sync
//! }
//! ```
//!
//! # Performance Considerations
//!
//! - **Use typed API** (`RenderBox<A>`) for new implementations - it's faster
//! - **Dyn-compatible methods** have tiny vtable overhead (~1-2ns per call)
//! - **State is external** so render objects can be immutable (better cache usage)
//! - **Atomic flags** for dirty tracking are lock-free and very fast
//!
//! # Common Patterns
//!
//! ## Relayout Boundary
//!
//! ```rust,ignore
//! impl RenderObject for MyWidget {
//!     fn is_relayout_boundary(&self) -> bool {
//!         true  // Don't propagate layout changes upward
//!     }
//! }
//! ```
//!
//! ## Repaint Boundary
//!
//! ```rust,ignore
//! impl RenderObject for MyWidget {
//!     fn is_repaint_boundary(&self) -> bool {
//!         true  // Enable layer caching
//!     }
//! }
//! ```
//!
//! ## Custom Intrinsics
//!
//! ```rust,ignore
//! impl RenderObject for MyWidget {
//!     fn intrinsic_size(&self) -> Option<Size> {
//!         Some(Size::new(100.0, 50.0))  // Natural size
//!     }
//! }
//! ```

use std::any::Any;
use std::fmt;

use flui_foundation::ElementId;
use flui_interaction::{HitTestEntry, HitTestResult};
use flui_painting::Canvas;
use flui_types::{Offset, Rect, Size};

use crate::core::{BoxConstraints, HitTestTree, LayoutTree, PaintTree};
use crate::RenderResult;

// ============================================================================
// RENDER OBJECT TRAIT
// ============================================================================

/// Base trait for all render objects (protocol-agnostic).
///
/// This trait provides the foundation for FLUI's rendering system with two
/// complementary APIs:
///
/// 1. **Dyn-compatible methods** for type-erased operations (Flutter-style)
/// 2. **Typed protocol traits** (`RenderBox<A>`, `RenderSliver<A>`) for performance
///
/// # Required Trait Bounds
///
/// - `Send + Sync`: Required for thread-safe tree operations and parallel layout
/// - `Debug`: Required for debugging and error messages
/// - `'static`: Ensures render objects live for the program duration
///
/// # State Management
///
/// State is **NOT** stored in the render object itself. Instead, `RenderElement`
/// maintains `RenderState<P>` separately. This enables:
/// - Immutable render objects (functional style)
/// - Better concurrent access patterns
/// - Protocol-specific state while keeping base trait generic
///
/// # Implementing RenderObject
///
/// Most render objects should implement the typed protocol traits (`RenderBox<A>`
/// or `RenderSliver<A>`) instead of implementing this trait directly. The
/// protocol traits provide:
/// - Compile-time arity validation
/// - Typed contexts with helper methods
/// - Automatic dyn-compatible implementations
///
/// Only implement `RenderObject` directly if:
/// - You need a custom protocol (not Box or Sliver)
/// - You're implementing framework-level infrastructure
/// - You need explicit control over dyn-compatible methods
///
/// # Safety Guarantees
///
/// - ✅ Thread-safe: All methods can be called from any thread
/// - ✅ Memory-safe: Rust ownership prevents use-after-free
/// - ✅ Type-safe: Downcasting uses runtime type checks
/// - ✅ No panics: All fallible operations return `Result`
pub trait RenderObject: Send + Sync + fmt::Debug + 'static {
    // ============================================================================
    // TYPE ERASURE METHODS (Required)
    // ============================================================================

    /// Returns a reference to this object as `&dyn Any` for downcasting.
    ///
    /// This is the safe way to downcast from `&dyn RenderObject` to a concrete type.
    /// Always returns `self` - implementers should not override this.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let render_object: &dyn RenderObject = &my_render_padding;
    /// if let Some(padding) = render_object.as_any().downcast_ref::<RenderPadding>() {
    ///     println!("Padding: {:?}", padding.padding);
    /// }
    /// ```
    fn as_any(&self) -> &dyn Any;

    /// Returns a mutable reference to this object as `&mut dyn Any` for downcasting.
    ///
    /// Like `as_any()` but for mutable access.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let render_object: &mut dyn RenderObject = &mut my_render_padding;
    /// if let Some(padding) = render_object.as_any_mut().downcast_mut::<RenderPadding>() {
    ///     padding.padding = EdgeInsets::all(20.0);
    /// }
    /// ```
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // ============================================================================
    // DEBUG METHODS (Optional)
    // ============================================================================

    /// Returns a human-readable debug name for this render object.
    ///
    /// Default implementation returns the full type name. Override to provide
    /// a custom name for debugging.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// impl RenderObject for MyCustomWidget {
    ///     fn debug_name(&self) -> &'static str {
    ///         "MyCustomWidget"
    ///     }
    /// }
    /// ```
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns the full type name including module path.
    ///
    /// Useful for debugging and error messages where you need the complete type path.
    ///
    /// # Returns
    ///
    /// Full type name like `flui_rendering::widgets::RenderFlex`
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns just the type name without module path.
    ///
    /// Useful for compact debug output and logging.
    ///
    /// # Returns
    ///
    /// Short type name like `RenderFlex` instead of full path
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// println!("Rendering {}", render_object.short_type_name());  // "RenderFlex"
    /// ```
    fn short_type_name(&self) -> &'static str {
        let full_name = std::any::type_name::<Self>();
        full_name.rsplit("::").next().unwrap_or(full_name)
    }

    // ============================================================================
    // INTRINSIC PROPERTIES (Optional)
    // ============================================================================

    /// Returns the natural size independent of constraints.
    ///
    /// This is the size the render object "wants" to be if unconstrained.
    /// Useful for:
    /// - Images (natural image dimensions)
    /// - Text (measured text size)
    /// - Icons (icon design size)
    ///
    /// Returns `None` if the render object has no intrinsic size preference.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// impl RenderObject for RenderImage {
    ///     fn intrinsic_size(&self) -> Option<Size> {
    ///         Some(self.image_data.dimensions())
    ///     }
    /// }
    /// ```
    fn intrinsic_size(&self) -> Option<Size> {
        None
    }

    /// Returns the bounding box in local coordinates.
    ///
    /// Default returns an empty rectangle. Override for accurate bounds,
    /// especially important for hit testing and overflow detection.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// impl RenderObject for MyWidget {
    ///     fn local_bounds(&self) -> Rect {
    ///         Rect::from_min_size(Offset::ZERO, self.cached_size)
    ///     }
    /// }
    /// ```
    fn local_bounds(&self) -> Rect {
        Rect::ZERO
    }

    /// Returns whether this render object handles pointer events.
    ///
    /// If `false`, hit testing will skip this object (performance optimization).
    /// If `true`, the object participates in hit testing.
    ///
    /// Default is `false` (no pointer event handling).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// impl RenderObject for RenderButton {
    ///     fn handles_pointer_events(&self) -> bool {
    ///         true  // Buttons handle clicks
    ///     }
    /// }
    /// ```
    fn handles_pointer_events(&self) -> bool {
        false
    }

    // ============================================================================
    // BOUNDARY FLAGS (Optional)
    // ============================================================================

    /// Returns whether this render object is a relayout boundary.
    ///
    /// Relayout boundaries prevent layout changes from propagating upward,
    /// improving performance by limiting relayout scope.
    ///
    /// Set to `true` when:
    /// - Layout only depends on own constraints (not parent size)
    /// - Children changing doesn't affect parent
    /// - You want to isolate layout computation
    ///
    /// # Flutter Contract
    ///
    /// When a relayout boundary is marked dirty, layout stops propagating
    /// upward. The boundary itself will relayout, but its parent won't.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// impl RenderObject for RenderSizedBox {
    ///     fn is_relayout_boundary(&self) -> bool {
    ///         self.width.is_some() && self.height.is_some()
    ///         // Fixed size = natural boundary
    ///     }
    /// }
    /// ```
    fn is_relayout_boundary(&self) -> bool {
        false
    }

    /// Returns whether this render object is a repaint boundary.
    ///
    /// Repaint boundaries enable layer caching and more efficient repainting.
    /// The subtree below a repaint boundary can be cached and reused without
    /// re-executing paint.
    ///
    /// Set to `true` when:
    /// - Paint is expensive (complex paths, images, filters)
    /// - This subtree rarely changes
    /// - You want to enable GPU layer caching
    ///
    /// # Performance Impact
    ///
    /// - **Pro**: Expensive paint operations happen once, then cached
    /// - **Pro**: Ancestor repaint doesn't trigger child repaint
    /// - **Con**: Extra memory for layer storage
    /// - **Con**: Cache invalidation overhead
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// impl RenderObject for RenderOpacity {
    ///     fn is_repaint_boundary(&self) -> bool {
    ///         self.opacity < 1.0  // Compositing layer needed
    ///     }
    /// }
    /// ```
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    // ============================================================================
    // DYN-COMPATIBLE METHODS (Flutter-style)
    // ============================================================================

    /// Computes the size of this render object (Flutter: `performLayout`).
    ///
    /// This is the dyn-compatible layout method that works without knowing the
    /// arity at compile time. It's called by `RenderElement` and works directly
    /// with the tree.
    ///
    /// # Arguments
    ///
    /// * `element_id` - This element's ID for accessing children via tree
    /// * `constraints` - Box constraints from parent that must be satisfied
    /// * `tree` - Tree interface for child layout operations
    ///
    /// # Returns
    ///
    /// The computed size that satisfies the input constraints, or error if layout fails.
    ///
    /// # Flutter Contract
    ///
    /// - Returned size MUST satisfy input constraints
    /// - Layout must be idempotent (same constraints → same size)
    /// - Must position children by calling `tree.set_offset()`
    /// - Can layout children by calling `tree.perform_layout()`
    ///
    /// # Default Implementation
    ///
    /// Returns the smallest size satisfying the constraints. Override this
    /// method to implement custom layout logic.
    ///
    /// # Performance Note
    ///
    /// For new implementations, prefer using `RenderBox<A>` with typed contexts
    /// instead of implementing this method directly. The typed API provides:
    /// - Compile-time arity validation
    /// - Helper methods for common patterns
    /// - Better type inference
    /// - Identical performance (inlined)
    ///
    /// # Examples
    ///
    /// ## Simple leaf element
    ///
    /// ```rust,ignore
    /// fn perform_layout(
    ///     &mut self,
    ///     _element_id: ElementId,
    ///     constraints: BoxConstraints,
    ///     _tree: &mut dyn LayoutTree,
    /// ) -> RenderResult<Size> {
    ///     let intrinsic = self.compute_intrinsic_size();
    ///     Ok(constraints.constrain(intrinsic))
    /// }
    /// ```
    ///
    /// ## Container with children
    ///
    /// ```rust,ignore
    /// fn perform_layout(
    ///     &mut self,
    ///     element_id: ElementId,
    ///     constraints: BoxConstraints,
    ///     tree: &mut dyn LayoutTree,
    /// ) -> RenderResult<Size> {
    ///     let children: Vec<_> = tree.children(element_id).collect();
    ///
    ///     let mut y = 0.0;
    ///     let mut max_width = 0.0;
    ///
    ///     for child_id in children {
    ///         let child_size = tree.perform_layout(child_id, constraints)?;
    ///         tree.set_offset(child_id, Offset::new(0.0, y));
    ///         y += child_size.height;
    ///         max_width = max_width.max(child_size.width);
    ///     }
    ///
    ///     Ok(constraints.constrain(Size::new(max_width, y)))
    /// }
    /// ```
    fn perform_layout(
        &mut self,
        _element_id: ElementId,
        constraints: BoxConstraints,
        _tree: &mut dyn LayoutTree,
    ) -> RenderResult<Size> {
        // Safe default: smallest size satisfying constraints
        Ok(constraints.smallest())
    }

    /// Paints this render object to canvas (Flutter: `paint`).
    ///
    /// This is the dyn-compatible paint method for type-erased rendering.
    /// It receives the size computed during layout and has direct tree access
    /// for painting children.
    ///
    /// # Arguments
    ///
    /// * `element_id` - This element's ID for accessing children via tree
    /// * `offset` - Position in parent coordinates where to paint
    /// * `size` - Size computed during layout (geometry from `RenderState`)
    /// * `canvas` - Canvas to paint on
    /// * `tree` - Tree interface for child paint operations
    ///
    /// # Flutter Contract
    ///
    /// - MUST NOT call layout during paint (will cause assertion failure)
    /// - MUST use size from layout phase (don't recompute)
    /// - SHOULD save/restore canvas state if modifying it
    /// - MUST paint children using `tree.perform_paint()`
    ///
    /// # Default Implementation
    ///
    /// Does nothing. Override to implement custom painting.
    ///
    /// # Canvas State
    ///
    /// If you modify canvas state (transform, clip, etc.), save and restore it:
    ///
    /// ```rust,ignore
    /// fn paint(..., canvas: &mut Canvas, ...) {
    ///     canvas.save();
    ///     canvas.clip_rect(my_clip);
    ///     // ... paint content ...
    ///     canvas.restore();
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ## Paint background and children
    ///
    /// ```rust,ignore
    /// fn paint(
    ///     &self,
    ///     element_id: ElementId,
    ///     offset: Offset,
    ///     size: Size,
    ///     canvas: &mut Canvas,
    ///     tree: &dyn PaintTree,
    /// ) {
    ///     // Paint background
    ///     let rect = Rect::from_min_size(offset, size);
    ///     canvas.draw_rect(rect, &self.background_paint);
    ///
    ///     // Paint all children at their stored offsets
    ///     for child_id in tree.children(element_id) {
    ///         if let Some(child_offset) = tree.get_offset(child_id) {
    ///             let _ = tree.perform_paint(child_id, offset + child_offset);
    ///         }
    ///     }
    /// }
    /// ```
    fn paint(
        &self,
        _element_id: ElementId,
        _offset: Offset,
        _size: Size,
        _canvas: &mut Canvas,
        _tree: &dyn PaintTree,
    ) {
        // Default: do nothing
    }

    /// Performs hit testing for pointer events (Flutter: `hitTest`).
    ///
    /// This is the dyn-compatible hit test method that determines if a pointer
    /// event intersects this render object or its children.
    ///
    /// # Arguments
    ///
    /// * `element_id` - This element's ID for accessing children via tree
    /// * `position` - Hit position in local coordinates
    /// * `result` - Collection to add hit test entries to
    /// * `tree` - Tree interface for child hit test operations
    ///
    /// # Returns
    ///
    /// `true` if this object or any child was hit, `false` otherwise.
    ///
    /// # Flutter Contract
    ///
    /// - Position is in LOCAL coordinates (relative to this object's offset)
    /// - Test children in REVERSE order (front to back, z-order)
    /// - Return immediately if a child is hit (early exit)
    /// - Add self to result only if actually hit
    /// - Transform position when testing children
    ///
    /// # Default Implementation
    ///
    /// Tests rectangular bounds and children in reverse order. Override for
    /// custom shapes or hit testing logic.
    ///
    /// # Examples
    ///
    /// ## Default rectangular hit test
    ///
    /// ```rust,ignore
    /// // Default is fine for rectangular shapes - no override needed
    /// ```
    ///
    /// ## Custom circular hit test
    ///
    /// ```rust,ignore
    /// fn hit_test(
    ///     &self,
    ///     element_id: ElementId,
    ///     position: Offset,
    ///     result: &mut HitTestResult,
    ///     tree: &dyn HitTestTree,
    /// ) -> bool {
    ///     let center = self.size / 2.0;
    ///     let radius = center.width.min(center.height);
    ///     let distance = (position - center).length();
    ///
    ///     if distance <= radius {
    ///         result.add(HitTestEntry::new(element_id));
    ///         true
    ///     } else {
    ///         false
    ///     }
    /// }
    /// ```
    ///
    /// ## With child transformation
    ///
    /// ```rust,ignore
    /// fn hit_test(
    ///     &self,
    ///     element_id: ElementId,
    ///     position: Offset,
    ///     result: &mut HitTestResult,
    ///     tree: &dyn HitTestTree,
    /// ) -> bool {
    ///     // Test children with transformed position
    ///     for child_id in tree.children_reverse(element_id) {
    ///         if let Some(child_offset) = tree.get_offset(child_id) {
    ///             let child_pos = position - child_offset;
    ///             if tree.perform_hit_test(child_id, child_pos, result) {
    ///                 return true;
    ///             }
    ///         }
    ///     }
    ///
    ///     // Check self
    ///     if self.contains_position(position) {
    ///         result.add(HitTestEntry::new(element_id));
    ///         true
    ///     } else {
    ///         false
    ///     }
    /// }
    /// ```
    fn hit_test(
        &self,
        element_id: ElementId,
        position: Offset,
        result: &mut HitTestResult,
        tree: &dyn HitTestTree,
    ) -> bool {
        // Default: rectangular hit testing with children

        // Get size from tree if available
        let size = tree.get_geometry(element_id).unwrap_or(Size::ZERO);

        // Check bounds
        if position.dx < 0.0
            || position.dx > size.width
            || position.dy < 0.0
            || position.dy > size.height
        {
            return false;
        }

        // Test children in reverse order (front to back)
        for child_id in tree.children_reverse(element_id) {
            if let Some(child_offset) = tree.get_offset(child_id) {
                let child_position = position - child_offset;
                if tree.perform_hit_test(child_id, child_position, result) {
                    return true; // Early exit on first hit
                }
            }
        }

        // Add self to result
        result.add(HitTestEntry::new(element_id));
        true
    }
}

// ============================================================================
// EXTENSION TRAIT FOR DOWNCASTING
// ============================================================================

/// Extension trait providing safe downcasting operations for RenderObject.
///
/// This trait is automatically implemented for all `RenderObject` trait objects
/// and provides convenient methods for downcasting to concrete types.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_rendering::core::RenderObjectExt;
///
/// let render: &dyn RenderObject = get_render_object();
///
/// // Safe downcasting
/// if let Some(padding) = render.downcast_ref::<RenderPadding>() {
///     println!("Padding: {:?}", padding.padding);
/// }
///
/// // Mutable downcasting
/// if let Some(padding) = render_mut.downcast_mut::<RenderPadding>() {
///     padding.padding = EdgeInsets::all(20.0);
/// }
///
/// // Check type without downcasting
/// if render.is::<RenderPadding>() {
///     println!("This is a RenderPadding!");
/// }
/// ```
pub trait RenderObjectExt {
    /// Attempts to downcast to a concrete type.
    ///
    /// Returns `Some(&T)` if the render object is of type `T`, `None` otherwise.
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T>;

    /// Attempts to mutably downcast to a concrete type.
    ///
    /// Returns `Some(&mut T)` if the render object is of type `T`, `None` otherwise.
    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T>;

    /// Checks if this render object is of a specific type.
    ///
    /// Returns `true` if the render object is type `T`, `false` otherwise.
    fn is<T: RenderObject>(&self) -> bool;
}

impl RenderObjectExt for dyn RenderObject {
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }

    fn is<T: RenderObject>(&self) -> bool {
        self.as_any().is::<T>()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestRenderObject {
        value: i32,
    }

    impl RenderObject for TestRenderObject {
        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn test_downcast() {
        let obj = TestRenderObject { value: 42 };
        let render: &dyn RenderObject = &obj;

        // Successful downcast
        let downcasted = render.downcast_ref::<TestRenderObject>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().value, 42);

        // Failed downcast
        #[derive(Debug)]
        struct OtherType;
        impl RenderObject for OtherType {
            fn as_any(&self) -> &dyn Any {
                self
            }
            fn as_any_mut(&mut self) -> &mut dyn Any {
                self
            }
        }

        let failed = render.downcast_ref::<OtherType>();
        assert!(failed.is_none());
    }

    #[test]
    fn test_is_type() {
        let obj = TestRenderObject { value: 42 };
        let render: &dyn RenderObject = &obj;

        assert!(render.is::<TestRenderObject>());
    }

    #[test]
    fn test_default_methods() {
        let obj = TestRenderObject { value: 42 };

        assert_eq!(obj.intrinsic_size(), None);
        assert_eq!(obj.local_bounds(), Rect::ZERO);
        assert!(!obj.handles_pointer_events());
        assert!(!obj.is_relayout_boundary());
        assert!(!obj.is_repaint_boundary());
    }

    #[test]
    fn test_type_names() {
        let obj = TestRenderObject { value: 42 };
        let render: &dyn RenderObject = &obj;

        let type_name = render.type_name();
        assert!(type_name.contains("TestRenderObject"));

        let short_name = render.short_type_name();
        assert_eq!(short_name, "TestRenderObject");
    }
}
