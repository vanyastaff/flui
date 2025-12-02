//! Type-erasure wrappers and utility types for render objects.
//!
//! This module provides wrapper types for working with render objects in
//! type-erased contexts, such as storing them in collections or passing
//! them across API boundaries.
//!
//! # Design Philosophy
//!
//! - **Type erasure**: Store concrete render objects as trait objects
//! - **Arity preservation**: Wrappers maintain arity information
//! - **Protocol preservation**: Box/Sliver protocol is maintained
//! - **Zero overhead**: Minimal wrapper cost
//!
//! # Wrapper Types
//!
//! ## BoxRenderWrapper
//!
//! Type-erased wrapper for box protocol render objects:
//! - Stores any `RenderBox<A>` as `Box<dyn RenderBox<A>>`
//! - Preserves arity at compile time
//! - Use for: collections, dynamic dispatch, API boundaries
//!
//! ## SliverRenderWrapper
//!
//! Type-erased wrapper for sliver protocol render objects:
//! - Stores any `RenderSliver<A>` as `Box<dyn RenderSliver<A>>`
//! - Preserves arity at compile time
//! - Use for: collections, dynamic dispatch, API boundaries
//!
//! # Use Cases
//!
//! - **Collections**: Store heterogeneous render objects
//! - **Dynamic dispatch**: Switch between different implementations
//! - **API boundaries**: Pass render objects without exposing concrete types
//! - **Plugin systems**: Accept external render object implementations
//!
//! # Examples
//!
//! ## Storing in Collections
//!
//! ```rust,ignore
//! use flui_rendering::core::{BoxRenderWrapper, Variable};
//!
//! let mut children: Vec<BoxRenderWrapper<Variable>> = vec![
//!     BoxRenderWrapper::new(RenderText::new("Hello")),
//!     BoxRenderWrapper::new(RenderImage::new("icon.png")),
//!     BoxRenderWrapper::new(RenderContainer::new()),
//! ];
//!
//! // All stored as trait objects, but arity is preserved!
//! ```
//!
//! ## Dynamic Dispatch
//!
//! ```rust,ignore
//! fn create_render(kind: &str) -> BoxRenderWrapper<Single> {
//!     match kind {
//!         "padding" => BoxRenderWrapper::new(RenderPadding::new()),
//!         "opacity" => BoxRenderWrapper::new(RenderOpacity::new(0.5)),
//!         _ => BoxRenderWrapper::new(RenderContainer::new()),
//!     }
//! }
//! ```

use std::fmt;

use super::arity::Arity;
use super::contexts::{
    BoxHitTestContext, BoxLayoutContext, BoxPaintContext, SliverHitTestContext,
    SliverLayoutContext, SliverPaintContext,
};
use super::render_box::RenderBox;
use super::render_object::RenderObject;
use super::render_sliver::RenderSliver;
use crate::RenderResult;
use flui_interaction::HitTestResult;
use flui_types::{Rect, Size, SliverGeometry};

// ============================================================================
// BOX RENDER WRAPPER
// ============================================================================

/// Type-erased wrapper for box protocol render objects.
///
/// This wrapper allows storing any concrete `RenderBox<A>` implementation as a
/// trait object while preserving arity information at compile time.
///
/// # Type Parameters
///
/// - `A`: Arity type (preserved at compile time)
///
/// # Use Cases
///
/// - Store heterogeneous render objects in collections
/// - Pass render objects across API boundaries
/// - Dynamic dispatch based on runtime conditions
/// - Plugin systems with external implementations
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use flui_rendering::core::{BoxRenderWrapper, Single};
///
/// let padding = RenderPadding::new(EdgeInsets::all(10.0));
/// let wrapper: BoxRenderWrapper<Single> = BoxRenderWrapper::new(padding);
///
/// // Use as RenderBox<Single>
/// let size = wrapper.layout(ctx);
/// ```
///
/// ## Collections
///
/// ```rust,ignore
/// use flui_rendering::core::{BoxRenderWrapper, Variable};
///
/// let children: Vec<BoxRenderWrapper<Variable>> = vec![
///     BoxRenderWrapper::new(RenderText::new("Title")),
///     BoxRenderWrapper::new(RenderImage::new("icon.png")),
///     BoxRenderWrapper::new(RenderButton::new()),
/// ];
///
/// // All stored with same type, different implementations
/// for child in &children {
///     child.layout(ctx);
/// }
/// ```
///
/// ## Dynamic Creation
///
/// ```rust,ignore
/// fn create_decorator(kind: &str) -> BoxRenderWrapper<Single> {
///     match kind {
///         "padding" => BoxRenderWrapper::new(RenderPadding::default()),
///         "opacity" => BoxRenderWrapper::new(RenderOpacity::new(0.8)),
///         "transform" => BoxRenderWrapper::new(RenderTransform::identity()),
///         _ => BoxRenderWrapper::new(RenderContainer::default()),
///     }
/// }
/// ```
pub struct BoxRenderWrapper<A: Arity> {
    inner: Box<dyn RenderBox<A>>,
}

impl<A: Arity> BoxRenderWrapper<A> {
    /// Creates a new wrapper around a render object.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let padding = RenderPadding::new(EdgeInsets::all(10.0));
    /// let wrapper = BoxRenderWrapper::new(padding);
    /// ```
    pub fn new<R: RenderBox<A> + 'static>(render: R) -> Self {
        Self {
            inner: Box::new(render),
        }
    }

    /// Creates a wrapper from a boxed trait object.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let boxed: Box<dyn RenderBox<Single>> = Box::new(RenderPadding::default());
    /// let wrapper = BoxRenderWrapper::from_box(boxed);
    /// ```
    pub fn from_box(inner: Box<dyn RenderBox<A>>) -> Self {
        Self { inner }
    }

    /// Gets a reference to the inner render object.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrapper = BoxRenderWrapper::new(padding);
    /// let inner: &dyn RenderBox<Single> = wrapper.inner();
    /// ```
    pub fn inner(&self) -> &dyn RenderBox<A> {
        &*self.inner
    }

    /// Gets a mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut dyn RenderBox<A> {
        &mut *self.inner
    }

    /// Attempts to downcast to a specific render object type.
    ///
    /// Returns `Some(&R)` if the inner object is of type `R`, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrapper = BoxRenderWrapper::new(RenderPadding::default());
    ///
    /// if let Some(padding) = wrapper.downcast_ref::<RenderPadding>() {
    ///     println!("Padding: {:?}", padding.padding);
    /// }
    /// ```
    pub fn downcast_ref<R: RenderBox<A> + 'static>(&self) -> Option<&R> {
        (self.inner.as_ref() as &dyn RenderObject)
            .as_any()
            .downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    ///
    /// Returns `Some(&mut R)` if the inner object is of type `R`, `None` otherwise.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut wrapper = BoxRenderWrapper::new(RenderOpacity::new(0.5));
    ///
    /// if let Some(opacity) = wrapper.downcast_mut::<RenderOpacity>() {
    ///     opacity.opacity = 1.0;
    /// }
    /// ```
    pub fn downcast_mut<R: RenderBox<A> + 'static>(&mut self) -> Option<&mut R> {
        (self.inner.as_mut() as &mut dyn RenderObject)
            .as_any_mut()
            .downcast_mut::<R>()
    }

    /// Unwraps the wrapper, returning the inner boxed trait object.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrapper = BoxRenderWrapper::new(padding);
    /// let boxed: Box<dyn RenderBox<Single>> = wrapper.into_inner();
    /// ```
    pub fn into_inner(self) -> Box<dyn RenderBox<A>> {
        self.inner
    }
}

impl<A: Arity> fmt::Debug for BoxRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BoxRenderWrapper")
            .field("inner", &self.inner.as_ref().debug_name())
            .finish()
    }
}

// Implement RenderBox by delegating to inner
impl<A: Arity> RenderBox<A> for BoxRenderWrapper<A> {
    fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut BoxPaintContext<'_, A>) {
        self.inner.paint(ctx)
    }

    fn hit_test(&self, ctx: &BoxHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        self.inner.hit_test(ctx, result)
    }

    fn intrinsic_width(&self, height: f32) -> Option<f32> {
        self.inner.intrinsic_width(height)
    }

    fn intrinsic_height(&self, width: f32) -> Option<f32> {
        self.inner.intrinsic_height(width)
    }

    fn baseline_offset(&self) -> Option<f32> {
        self.inner.baseline_offset()
    }

    fn local_bounds(&self) -> Rect {
        self.inner.local_bounds()
    }
}

impl<A: Arity> RenderObject for BoxRenderWrapper<A> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }
}

// ============================================================================
// SLIVER RENDER WRAPPER
// ============================================================================

/// Type-erased wrapper for sliver protocol render objects.
///
/// This wrapper allows storing any concrete `RenderSliver<A>` implementation as a
/// trait object while preserving arity information at compile time.
///
/// # Type Parameters
///
/// - `A`: Arity type (preserved at compile time)
///
/// # Use Cases
///
/// - Store heterogeneous sliver render objects in collections
/// - Pass sliver render objects across API boundaries
/// - Dynamic dispatch based on runtime conditions
/// - Plugin systems with external implementations
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use flui_rendering::core::{SliverRenderWrapper, Single};
///
/// let padding = RenderSliverPadding::new(10.0);
/// let wrapper: SliverRenderWrapper<Single> = SliverRenderWrapper::new(padding);
///
/// // Use as RenderSliver<Single>
/// let geometry = wrapper.layout(ctx);
/// ```
///
/// ## Collections
///
/// ```rust,ignore
/// use flui_rendering::core::{SliverRenderWrapper, Variable};
///
/// let slivers: Vec<SliverRenderWrapper<Variable>> = vec![
///     SliverRenderWrapper::new(RenderSliverList::new()),
///     SliverRenderWrapper::new(RenderSliverGrid::new()),
///     SliverRenderWrapper::new(RenderSliverAppBar::new()),
/// ];
///
/// // All stored with same type, different implementations
/// for sliver in &slivers {
///     sliver.layout(ctx);
/// }
/// ```
pub struct SliverRenderWrapper<A: Arity> {
    inner: Box<dyn RenderSliver<A>>,
}

impl<A: Arity> SliverRenderWrapper<A> {
    /// Creates a new wrapper around a sliver render object.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let padding = RenderSliverPadding::new(10.0);
    /// let wrapper = SliverRenderWrapper::new(padding);
    /// ```
    pub fn new<R: RenderSliver<A> + 'static>(render: R) -> Self {
        Self {
            inner: Box::new(render),
        }
    }

    /// Creates a wrapper from a boxed trait object.
    pub fn from_box(inner: Box<dyn RenderSliver<A>>) -> Self {
        Self { inner }
    }

    /// Gets a reference to the inner render object.
    pub fn inner(&self) -> &dyn RenderSliver<A> {
        &*self.inner
    }

    /// Gets a mutable reference to the inner render object.
    pub fn inner_mut(&mut self) -> &mut dyn RenderSliver<A> {
        &mut *self.inner
    }

    /// Attempts to downcast to a specific render object type.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let wrapper = SliverRenderWrapper::new(RenderSliverList::default());
    ///
    /// if let Some(list) = wrapper.downcast_ref::<RenderSliverList>() {
    ///     println!("List item count: {}", list.item_count);
    /// }
    /// ```
    pub fn downcast_ref<R: RenderSliver<A> + 'static>(&self) -> Option<&R> {
        (self.inner.as_ref() as &dyn RenderObject)
            .as_any()
            .downcast_ref::<R>()
    }

    /// Attempts to mutably downcast to a specific render object type.
    pub fn downcast_mut<R: RenderSliver<A> + 'static>(&mut self) -> Option<&mut R> {
        (self.inner.as_mut() as &mut dyn RenderObject)
            .as_any_mut()
            .downcast_mut::<R>()
    }

    /// Unwraps the wrapper, returning the inner boxed trait object.
    pub fn into_inner(self) -> Box<dyn RenderSliver<A>> {
        self.inner
    }
}

impl<A: Arity> fmt::Debug for SliverRenderWrapper<A> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliverRenderWrapper")
            .field("inner", &self.inner.as_ref().debug_name())
            .finish()
    }
}

// Implement RenderSliver by delegating to inner
impl<A: Arity> RenderSliver<A> for SliverRenderWrapper<A> {
    fn layout(&mut self, ctx: SliverLayoutContext<'_, A>) -> SliverGeometry {
        self.inner.layout(ctx)
    }

    fn paint(&self, ctx: &mut SliverPaintContext<'_, A>) {
        self.inner.paint(ctx)
    }

    fn hit_test(&self, ctx: &SliverHitTestContext<'_, A>, result: &mut HitTestResult) -> bool {
        self.inner.hit_test(ctx, result)
    }

    fn child_keep_alive_count(&self) -> usize {
        self.inner.child_keep_alive_count()
    }

    fn has_visual_overflow(&self) -> bool {
        self.inner.has_visual_overflow()
    }

    fn local_bounds(&self) -> Rect {
        self.inner.local_bounds()
    }
}

impl<A: Arity> RenderObject for SliverRenderWrapper<A> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn debug_name(&self) -> &'static str {
        self.inner.as_ref().debug_name()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::arity::{Leaf, Single, Variable};
    use std::marker::PhantomData;

    // Test render box
    #[derive(Debug)]
    struct TestRenderBox<A: Arity> {
        value: i32,
        _phantom: PhantomData<A>,
    }

    impl<A: Arity> TestRenderBox<A> {
        fn new(value: i32) -> Self {
            Self {
                value,
                _phantom: PhantomData,
            }
        }
    }

    impl<A: Arity> RenderBox<A> for TestRenderBox<A> {
        fn layout(&mut self, ctx: BoxLayoutContext<'_, A>) -> RenderResult<Size> {
            Ok(Size::new(self.value as f32, self.value as f32))
        }

        fn paint(&self, _ctx: &mut BoxPaintContext<'_, A>) {}
    }

    impl<A: Arity> RenderObject for TestRenderBox<A> {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }

        fn debug_name(&self) -> &'static str {
            "TestRenderBox"
        }
    }

    // Test render sliver
    #[derive(Debug)]
    struct TestRenderSliver<A: Arity> {
        extent: f32,
        _phantom: PhantomData<A>,
    }

    impl<A: Arity> TestRenderSliver<A> {
        fn new(extent: f32) -> Self {
            Self {
                extent,
                _phantom: PhantomData,
            }
        }
    }

    impl<A: Arity> RenderSliver<A> for TestRenderSliver<A> {
        fn layout(&mut self, _ctx: SliverLayoutContext<'_, A>) -> SliverGeometry {
            SliverGeometry {
                scroll_extent: self.extent,
                paint_extent: self.extent,
                ..Default::default()
            }
        }

        fn paint(&self, _ctx: &mut SliverPaintContext<'_, A>) {}
    }

    impl<A: Arity> RenderObject for TestRenderSliver<A> {
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }

        fn debug_name(&self) -> &'static str {
            "TestRenderSliver"
        }
    }

    #[test]
    fn test_box_wrapper_creation() {
        let render = TestRenderBox::<Single>::new(42);
        let wrapper = BoxRenderWrapper::new(render);

        // Should compile - wrapper implement RenderBox?
        let _: &dyn RenderBox<Single> = &wrapper;
    }

    #[test]
    fn test_box_wrapper_arity() {
        let _leaf: BoxRenderWrapper<Leaf> = BoxRenderWrapper::new(TestRenderBox::new(1));
        let _single: BoxRenderWrapper<Single> = BoxRenderWrapper::new(TestRenderBox::new(2));
        let _variable: BoxRenderWrapper<Variable> = BoxRenderWrapper::new(TestRenderBox::new(3));
        // Compiles = arity is preserved
    }

    #[test]
    fn test_box_wrapper_downcast() {
        let mut wrapper = BoxRenderWrapper::new(TestRenderBox::<Single>::new(42));

        // Downcast should work
        assert!(wrapper.downcast_ref::<TestRenderBox<Single>>().is_some());
        assert_eq!(
            wrapper
                .downcast_ref::<TestRenderBox<Single>>()
                .unwrap()
                .value,
            42
        );

        // Mutable downcast
        if let Some(render) = wrapper.downcast_mut::<TestRenderBox<Single>>() {
            render.value = 100;
        }

        assert_eq!(
            wrapper
                .downcast_ref::<TestRenderBox<Single>>()
                .unwrap()
                .value,
            100
        );
    }

    #[test]
    fn test_sliver_wrapper_creation() {
        let render = TestRenderSliver::<Single>::new(100.0);
        let wrapper = SliverRenderWrapper::new(render);

        // Should compile - wrapper implement RenderSliver?
        let _: &dyn RenderSliver<Single> = &wrapper;
    }

    #[test]
    fn test_sliver_wrapper_downcast() {
        let wrapper = SliverRenderWrapper::new(TestRenderSliver::<Variable>::new(100.0));

        // Downcast should work
        assert!(wrapper
            .downcast_ref::<TestRenderSliver<Variable>>()
            .is_some());
        assert_eq!(
            wrapper
                .downcast_ref::<TestRenderSliver<Variable>>()
                .unwrap()
                .extent,
            100.0
        );
    }

    #[test]
    fn test_wrapper_collection() {
        let wrappers: Vec<BoxRenderWrapper<Variable>> = vec![
            BoxRenderWrapper::new(TestRenderBox::new(1)),
            BoxRenderWrapper::new(TestRenderBox::new(2)),
            BoxRenderWrapper::new(TestRenderBox::new(3)),
        ];

        assert_eq!(wrappers.len(), 3);
        // All stored with same type!
    }
}
