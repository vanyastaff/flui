//! Public trait definitions for generic render objects
//!
//! This module provides the unified `Render<A>` and `SliverRender<A>` traits that
//! implement layout and painting for both Box and Sliver protocols with compile-time
//! child count validation.
//!
//! # Architecture
//!
//! - `Render<A>`: Box protocol (BoxConstraints → BoxGeometry, with context `BoxLayoutContext<A>`)
//! - `SliverRender<A>`: Sliver protocol (SliverConstraints → SliverGeometry, with context `SliverLayoutContext<A>`)
//! - `A`: Arity type that specifies child count at compile time
//!
//! Both traits are generic over a layout protocol (Box vs Sliver) through associated types.
//!
//! # Type Parameters
//!
//! ## `A: Arity` - Child Count Type
//!
//! The arity type `A` specifies the expected number of children:
//! - `Leaf`: 0 children (no children access needed)
//! - `Optional`: 0-1 children
//! - `Single`: exactly 1 child
//! - `Pair`: exactly 2 children
//! - `Triple`: exactly 3 children
//! - `Exact<N>`: exactly N children (N: 4+)
//! - `AtLeast<N>`: at least N children
//! - `Variable`: any number of children
//!
//! # Protocol Binding
//!
//! Each trait implements one protocol:
//! - `Render<A>` always uses `BoxProtocol` (layout = BoxLayoutContext, paint = BoxPaintContext)
//! - `SliverRender<A>` always uses `SliverProtocol` (layout = SliverLayoutContext, paint = SliverPaintContext)
//!
//! This binding is implicit - implementors don't need to specify which protocol they use.
//!
//! # Examples
//!
//! ## Leaf Render (0 children)
//!
//! ```rust,ignore
//! use flui_core::render::{Render, Leaf, BoxLayoutContext, BoxPaintContext};
//! use flui_types::Size;
//!
//! #[derive(Debug)]
//! struct RenderText {
//!     text: String,
//! }
//!
//! impl Render<Leaf> for RenderText {
//!     fn layout(&mut self, ctx: &BoxLayoutContext<Leaf>) -> Size {
//!         // No children to layout
//!         let size = measure_text(&self.text);
//!         ctx.constraints.constrain(size)
//!     }
//!
//!     fn paint(&self, ctx: &BoxPaintContext<Leaf>) {
//!         // No children to paint
//!         let mut layer = acquire_picture();
//!         layer.draw_text(&self.text, ctx.offset);
//!     }
//! }
//! ```
//!
//! ## Single Child Render
//!
//! ```rust,ignore
//! use flui_core::render::{Render, Single, BoxLayoutContext, BoxPaintContext};
//! use flui_types::{Size, EdgeInsets};
//!
//! #[derive(Debug)]
//! struct RenderPadding {
//!     padding: EdgeInsets,
//! }
//!
//! impl Render<Single> for RenderPadding {
//!     fn layout(&mut self, ctx: &BoxLayoutContext<Single>) -> Size {
//!         let child = ctx.children().single();
//!         let deflated = ctx.constraints.deflate(&self.padding);
//!         let child_size = ctx.layout_child(child, deflated);
//!         Size::new(
//!             child_size.width + self.padding.horizontal_total(),
//!             child_size.height + self.padding.vertical_total(),
//!         )
//!     }
//!
//!     fn paint(&self, ctx: &BoxPaintContext<Single>) {
//!         let child = ctx.children().single();
//!         let offset = ctx.offset + self.padding.top_left_offset();
//!         ctx.paint_child(child, offset);
//!     }
//! }
//! ```
//!
//! ## Multi-Child Render
//!
//! ```rust,ignore
//! use flui_core::render::{Render, Variable, BoxLayoutContext, BoxPaintContext};
//! use flui_types::Size;
//!
//! #[derive(Debug)]
//! struct RenderColumn {
//!     spacing: f32,
//! }
//!
//! impl Render<Variable> for RenderColumn {
//!     fn layout(&mut self, ctx: &BoxLayoutContext<Variable>) -> Size {
//!         let mut y = 0.0;
//!         let mut max_width = 0.0;
//!
//!         for child in ctx.children().iter() {
//!             let child_size = ctx.layout_child(child, ctx.constraints);
//!             y += child_size.height + self.spacing;
//!             max_width = max_width.max(child_size.width);
//!         }
//!
//!         Size::new(max_width, y)
//!     }
//!
//!     fn paint(&self, ctx: &BoxPaintContext<Variable>) {
//!         let mut y = 0.0;
//!         for child in ctx.children().iter() {
//!             let offset = ctx.offset.with_dy(ctx.offset.dy + y);
//!             ctx.paint_child(child, offset);
//!             // y += child_height  (computed during layout)
//!             y += self.spacing;
//!         }
//!     }
//! }
//! ```

use super::arity::{Arity as ArityTrait, ChildrenAccess};
use super::protocol::{
    BoxHitTestContext as ProtocolBoxHitTestContext, BoxLayoutContext as ProtocolBoxLayoutContext,
    BoxPaintContext as ProtocolBoxPaintContext, SliverGeometry,
    SliverHitTestContext as ProtocolSliverHitTestContext,
    SliverLayoutContext as ProtocolSliverLayoutContext,
    SliverPaintContext as ProtocolSliverPaintContext,
};
use crate::element::hit_test::{BoxHitTestResult, SliverHitTestResult};
use std::fmt::Debug;

/// Public trait for box-based render objects with typed arity
///
/// This is the primary trait for implementing layout and painting with compile-time
/// child count validation. Implementors specify their child count via the `A` type parameter.
///
/// # Type Parameters
///
/// - `A: ArityTrait` - Compile-time child count specification (Leaf, Single, Variable, etc.)
///
/// # Protocol Binding
///
/// This trait always uses the **Box protocol**:
/// - Constraints: `BoxConstraints` (min/max width/height)
/// - Geometry: `BoxGeometry` (computed size)
/// - Contexts: `BoxLayoutContext<A>`, `BoxPaintContext<A>`, `BoxHitTestContext<A>`
///
/// # Thread Safety
///
/// All render objects must be `Send + Sync + 'static`:
/// - **`Send`**: Can be moved between threads
/// - **`Sync`**: Can be accessed concurrently from multiple threads
/// - **`'static`**: No borrowed data (owns all state)
///
/// This enables parallel layout and concurrent rendering.
///
/// # Required Methods
///
/// 1. **`layout`**: Compute size given constraints
///    - Receives: `BoxLayoutContext<A>` with constraints and typed children
///    - Returns: `Size` constrained by the input constraints
///    - Layout children via `ctx.layout_child()`
///
/// 2. **`paint`**: Generate layer tree for rendering
///    - Receives: `BoxPaintContext<A>` with offset and typed children
///    - Returns: Paints into the context (no explicit return value)
///    - Paint children via `ctx.paint_child()`
///
/// 3. **`hit_test`**: Perform pointer hit testing
///    - Receives: `BoxHitTestContext<A>` with position and typed children
///    - Parameters: `result` for accumulating hit entries
///    - Returns: `true` if this or any child was hit
///
/// # Optional Methods
///
/// - `intrinsic_width`: Compute intrinsic width given optional height
/// - `intrinsic_height`: Compute intrinsic height given optional width
/// - `debug_name`: Return debug identifier for diagnostics
///
/// # Default Implementations
///
/// - `hit_test`: Tests children first (front-to-back), then self
/// - `hit_test_self`: Returns `false` (only override for custom shapes)
/// - `hit_test_children`: Iterates children via `ctx.children()`
/// - `intrinsic_width`: Returns `None`
/// - `intrinsic_height`: Returns `None`
/// - `debug_name`: Returns type name
pub trait Render<A: ArityTrait>: Send + Sync + Debug + 'static {
    /// Compute layout with constraints
    ///
    /// This method is called during the layout phase to compute the size
    /// given the constraints from the parent.
    ///
    /// # Contract
    ///
    /// The returned size must satisfy the constraints:
    /// - `min_width ≤ size.width ≤ max_width`
    /// - `min_height ≤ size.height ≤ max_height`
    ///
    /// Most implementations use `ctx.constraints.constrain()` to enforce this.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Layout context with:
    ///   - `ctx.constraints`: Box constraints from parent
    ///   - `ctx.children()`: Type-safe children accessor
    ///   - `ctx.layout_child()`: Method to recursively layout children
    fn layout(&mut self, ctx: &ProtocolBoxLayoutContext<A>) -> crate::prelude::Size;

    /// Paint this render object
    ///
    /// This method is called during the paint phase to generate visual output
    /// by drawing into the provided mutable context's canvas.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Mutable paint context with:
    ///   - `ctx.offset`: Translation offset for this element's coordinate space
    ///   - `ctx.children()`: Type-safe children accessor
    ///   - `ctx.canvas()`: Mutable canvas for drawing operations
    ///   - `ctx.paint_child()`: Method to recursively paint children
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut BoxPaintContext<Leaf>) {
    ///     ctx.canvas().draw_rect(rect, &paint);
    /// }
    /// ```
    fn paint(&self, ctx: &mut ProtocolBoxPaintContext<A>);

    /// Perform hit testing on this render object
    ///
    /// Default implementation:
    /// 1. Tests children first (front-to-back order)
    /// 2. Tests self (via `hit_test_self`)
    /// 3. Adds entry to result if hit
    ///
    /// # Override Patterns
    ///
    /// Most renderers don't need to override this. Override when you need:
    /// - **AbsorbPointer**: Block events from reaching children
    /// - **IgnorePointer**: Skip hit testing completely
    /// - **Transform**: Apply inverse transform to position
    ///
    /// # Parameters
    ///
    /// - `ctx`: Hit test context with:
    ///   - `ctx.position`: Hit position in local coordinates
    ///   - `ctx.size`: Size from layout
    ///   - `ctx.children()`: Type-safe children accessor
    /// - `result`: Accumulator for hit test entries (adds from child to parent)
    ///
    /// # Returns
    ///
    /// - `true` if this render object or any child was hit
    /// - `false` if nothing was hit
    fn hit_test(&self, ctx: &ProtocolBoxHitTestContext<A>, result: &mut BoxHitTestResult) -> bool {
        // Default: test children first, then self
        let hit_children = self.hit_test_children(ctx, result);

        if hit_children || self.hit_test_self(ctx.position) {
            result.add(
                ctx.element_id,
                crate::element::hit_test_entry::BoxHitTestEntry::new(ctx.position, ctx.size),
            );
            return true;
        }
        false
    }

    /// Test if position hits this render object itself (ignoring children)
    ///
    /// Default: returns `false` (child objects must be explicit).
    ///
    /// Override this method when you have:
    /// - **Leaf renderers** (Text, Image): Check against visual bounds
    /// - **Custom hit shapes**: Circles, paths, or non-rectangular areas
    ///
    /// Don't override this for:
    /// - **Pass-through wrappers**: Default (returns false) is correct
    /// - **Visibility control**: Override `hit_test` instead
    fn hit_test_self(&self, _position: flui_types::Offset) -> bool {
        false
    }

    /// Test all children for hits (called by default `hit_test`)
    ///
    /// Default implementation iterates through children and tests each one.
    /// Rarely needs to be overridden.
    ///
    /// Override only for:
    /// - **Custom child order**: Test children in non-standard order
    /// - **Child filtering**: Skip certain children during hit testing
    /// - **Lazy hit testing**: Only test visible children in large lists
    fn hit_test_children(
        &self,
        ctx: &ProtocolBoxHitTestContext<A>,
        result: &mut BoxHitTestResult,
    ) -> bool {
        let mut hit = false;
        for &child in ctx.children.as_slice().iter() {
            if ctx
                .tree
                .hit_test_box_child(child.into(), ctx.position, result)
            {
                hit = true;
            }
        }
        hit
    }

    /// Compute intrinsic width given optional height
    ///
    /// Used by parent layouts to determine natural sizing when constraints are loose.
    /// Default: returns `None` (no intrinsic width).
    ///
    /// Override for:
    /// - **Fixed-size elements** (buttons, icons): Return actual width
    /// - **Aspect-ratio constraints**: Compute width based on height
    ///
    /// Don't override for:
    /// - **Layout-dependent elements**: Return `None`
    /// - **Text**: Use measured size instead
    fn intrinsic_width(&self, _height: Option<f32>) -> Option<f32> {
        None
    }

    /// Compute intrinsic height given optional width
    ///
    /// Used by parent layouts to determine natural sizing when constraints are loose.
    /// Default: returns `None` (no intrinsic height).
    ///
    /// Override for:
    /// - **Fixed-size elements** (buttons, icons): Return actual height
    /// - **Aspect-ratio constraints**: Compute height based on width
    ///
    /// Don't override for:
    /// - **Layout-dependent elements**: Return `None`
    /// - **Text**: Use measured size instead
    fn intrinsic_height(&self, _width: Option<f32>) -> Option<f32> {
        None
    }

    /// Get debug name for diagnostics
    ///
    /// Default: returns type name via `std::any::type_name`.
    /// Override to provide custom debug output.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

/// Public trait for sliver-based render objects with typed arity
///
/// This trait implements layout and painting for scrollable content with compile-time
/// child count validation. Implementors specify their child count via the `A` type parameter.
///
/// # Type Parameters
///
/// - `A: ArityTrait` - Compile-time child count specification (Leaf, Single, Variable, etc.)
///
/// # Protocol Binding
///
/// This trait always uses the **Sliver protocol**:
/// - Constraints: `SliverConstraints` (scroll state)
/// - Geometry: `SliverGeometry` (scroll/paint/cache extents)
/// - Contexts: `SliverLayoutContext<A>`, `SliverPaintContext<A>`, `SliverHitTestContext<A>`
///
/// # Slivers vs Box Renders
///
/// Slivers are viewport-aware render objects designed for:
/// - **Lazy loading**: Only layout/paint visible content
/// - **Efficient scrolling**: Support infinite lists without memory overhead
/// - **Scroll awareness**: Know their position relative to the viewport
///
/// Compared to box renders which assume fixed, non-scrollable layouts.
///
/// # Thread Safety
///
/// All sliver render objects must be `Send + Sync + 'static`:
/// - **`Send`**: Can be moved between threads
/// - **`Sync`**: Can be accessed concurrently from multiple threads
/// - **`'static`**: No borrowed data (owns all state)
///
/// This enables parallel layout and concurrent rendering.
///
/// # Required Methods
///
/// 1. **`layout`**: Compute sliver geometry given constraints
///    - Receives: `SliverLayoutContext<A>` with scroll constraints and typed children
///    - Returns: `SliverGeometry` with scroll/paint/cache extents
///    - Layout children via `ctx.layout_child()`
///
/// 2. **`paint`**: Generate layer tree for rendering
///    - Receives: `SliverPaintContext<A>` with viewport offset and typed children
///    - Returns: Paints into the context (no explicit return value)
///    - Paint children via `ctx.paint_child()`
///
/// 3. **`hit_test`**: Perform pointer hit testing
///    - Receives: `SliverHitTestContext<A>` with position and typed children
///    - Parameters: `result` for accumulating hit entries
///    - Returns: `true` if this or any child was hit
///
/// # Optional Methods
///
/// - `hit_test_self`: Check custom hit shapes (default: returns false)
/// - `debug_name`: Return debug identifier for diagnostics
///
/// # Default Implementations
///
/// - `hit_test`: Checks visible region, tests children, then self
/// - `hit_test_self`: Returns `false` (only override for custom shapes)
/// - `hit_test_children`: Iterates children via `ctx.children()`
/// - `debug_name`: Returns type name
pub trait SliverRender<A: ArityTrait>: Send + Sync + Debug + 'static {
    /// Compute sliver layout with constraints
    ///
    /// This method is called during the layout phase to compute the sliver geometry
    /// given the sliver constraints from the viewport.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Sliver layout context with:
    ///   - `ctx.constraints`: Sliver constraints (scroll state)
    ///   - `ctx.children()`: Type-safe children accessor
    ///   - `ctx.layout_child()`: Method to recursively layout children
    ///
    /// # Returns
    ///
    /// Sliver geometry describing:
    /// - `scroll_extent`: Total scrollable extent
    /// - `paint_extent`: Currently visible extent (should be ≤ remaining_paint_extent)
    /// - `cache_extent`: Additional space to buffer for smooth scrolling
    fn layout(&mut self, ctx: &ProtocolSliverLayoutContext<A>) -> SliverGeometry;

    /// Paint this sliver render object
    ///
    /// This method is called during the paint phase to generate visual output
    /// for the visible region of this sliver and its children by drawing into
    /// the provided mutable context's canvas.
    ///
    /// # Parameters
    ///
    /// - `ctx`: Mutable sliver paint context with:
    ///   - `ctx.offset`: Translation offset for sliver's coordinate space
    ///   - `ctx.children()`: Type-safe children accessor
    ///   - `ctx.canvas()`: Mutable canvas for drawing operations
    ///   - `ctx.paint_child()`: Method to recursively paint children
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn paint(&self, ctx: &mut SliverPaintContext<Leaf>) {
    ///     ctx.canvas().draw_text(text, position, &style);
    /// }
    /// ```
    fn paint(&self, ctx: &mut ProtocolSliverPaintContext<A>);

    /// Perform hit testing on this sliver render object
    ///
    /// Default implementation:
    /// 1. Checks if hit is in visible region (0 ≤ main_axis_position < paint_extent)
    /// 2. Tests children first (bottom-to-top order, visible first)
    /// 3. Tests self (via `hit_test_self`)
    /// 4. Adds entry to result if hit
    ///
    /// # Override Patterns
    ///
    /// Most slivers don't need to override this. Override when you need:
    /// - **SliverIgnorePointer**: Skip hit testing completely
    /// - **SliverOpacity**: Block if fully transparent
    /// - **Custom visibility**: Implement custom visibility rules
    ///
    /// # Parameters
    ///
    /// - `ctx`: Sliver hit test context with:
    ///   - `ctx.main_axis_position`: Position along scroll direction
    ///   - `ctx.cross_axis_position`: Position perpendicular to scroll
    ///   - `ctx.geometry`: Sliver geometry from layout
    ///   - `ctx.children()`: Type-safe children accessor
    /// - `result`: Accumulator for hit test entries (adds from child to parent)
    ///
    /// # Returns
    ///
    /// - `true` if this sliver or any child was hit
    /// - `false` if nothing was hit (including if scrolled off-screen)
    fn hit_test(
        &self,
        ctx: &ProtocolSliverHitTestContext<A>,
        result: &mut SliverHitTestResult,
    ) -> bool {
        // 1. Check if hit is in visible region
        if !ctx.is_visible() {
            return false; // Scrolled off-screen
        }

        // 2. Test children first
        let hit_children = self.hit_test_children(ctx, result);

        // 3. Test self
        if hit_children || self.hit_test_self(ctx.main_axis_position, ctx.cross_axis_position) {
            result.add(
                ctx.element_id,
                crate::element::hit_test_entry::SliverHitTestEntry::new(
                    ctx.local_position(),
                    ctx.geometry,
                    ctx.scroll_offset,
                    ctx.main_axis_position,
                ),
            );
            return true;
        }
        false
    }

    /// Test if position hits this sliver itself (ignoring children)
    ///
    /// Default: returns `false` (child slivers must be explicit).
    ///
    /// Override this method when you have:
    /// - **Leaf slivers**: Check against visual bounds
    /// - **Custom hit shapes**: Non-rectangular hit regions
    ///
    /// Don't override for:
    /// - **Pass-through wrappers**: Default (returns false) is correct
    /// - **Visibility control**: Override `hit_test` instead
    fn hit_test_self(&self, _main_axis_position: f32, _cross_axis_position: f32) -> bool {
        false
    }

    /// Test all children for hits (called by default `hit_test`)
    ///
    /// Default implementation iterates through children and tests each one.
    /// Rarely needs to be overridden.
    ///
    /// Override only for:
    /// - **Custom child order**: Test children in non-standard order
    /// - **Child filtering**: Skip certain children during hit testing
    /// - **Lazy hit testing**: Only test visible children in large lists
    fn hit_test_children(
        &self,
        ctx: &ProtocolSliverHitTestContext<A>,
        result: &mut SliverHitTestResult,
    ) -> bool {
        let mut hit = false;
        for &child in ctx.children.as_slice().iter() {
            if ctx
                .tree
                .hit_test_sliver_child(child.into(), ctx.local_position(), result)
            {
                hit = true;
            }
        }
        hit
    }

    /// Get debug name for diagnostics
    ///
    /// Default: returns type name via `std::any::type_name`.
    /// Override to provide custom debug output.
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::Leaf;

    // Compile-time test: Render<A> with different arity types
    #[test]
    fn test_render_trait_exists() {
        // This test just verifies the trait can be referenced
        let _phantom: std::marker::PhantomData<dyn Render<Leaf>> = std::marker::PhantomData;
    }

    // Compile-time test: SliverRender<A> with different arity types
    #[test]
    fn test_sliver_render_trait_exists() {
        // This test just verifies the trait can be referenced
        let _phantom: std::marker::PhantomData<dyn SliverRender<Leaf>> = std::marker::PhantomData;
    }
}
