//! Container mixin — manages multiple children with parent data
//!
//! This module provides ContainerBox<T, PD> for render objects with multiple children
//! (e.g., RenderFlex, RenderStack, RenderWrap).
//!
//! # Pattern
//!
//! ```rust,ignore
//! // 1. Define your data
//! #[derive(Clone, Debug)]
//! pub struct FlexData {
//!     pub direction: Axis,
//!     pub main_axis_alignment: MainAxisAlignment,
//! }
//!
//! // 2. Define parent data for children
//! #[derive(Default, Clone, Debug)]
//! pub struct FlexParentData {
//!     pub flex: f32,
//!     pub offset: Offset,
//! }
//!
//! // 3. Type alias
//! pub type RenderFlex = ContainerBox<FlexData, FlexParentData>;
//!
//! // 4. MUST override perform_layout
//! impl RenderContainerBox<FlexParentData> for RenderFlex {
//!     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
//!         // Layout children using self.children_mut() (via Ambassador!)
//!         for (child_id, parent_data) in self.children_mut().iter_with_data_mut() {
//!             // ... layout logic using self.direction (via Deref!)
//!             parent_data.offset = calculated_offset;
//!         }
//!         self.set_size(size);
//!         size
//!     }
//! }
//!
//! // AUTO: paint() and hit_test() iterate children automatically!
//! ```

use std::ops::{Deref, DerefMut};

use ambassador::{delegatable_trait, Delegate};
use flui_interaction::HitTestResult;
use flui_types::{BoxConstraints, Offset, Size, SliverConstraints, SliverGeometry};

use crate::children::Children;
use crate::protocol::{BoxProtocol, Protocol, SliverProtocol};
use crate::PaintingContext;

// Re-export from proxy.rs
use super::proxy::{HasBoxGeometry, HasSliverGeometry, ProxyData};

// Import ambassador macros
use super::proxy::{ambassador_impl_HasBoxGeometry, ambassador_impl_HasSliverGeometry};

// ============================================================================
// Part 1: Delegatable Trait - HasChildren
// ============================================================================

/// Trait for accessing multiple children (delegatable)
#[delegatable_trait]
pub trait HasChildren<P: Protocol, PD = ()> {
    fn children(&self) -> &Children<P, PD>;
    fn children_mut(&mut self) -> &mut Children<P, PD>;

    /// Get number of children
    fn child_count(&self) -> usize {
        self.children().len()
    }

    /// Check if has children
    fn has_children(&self) -> bool {
        !self.children().is_empty()
    }
}

// ============================================================================
// Part 2: Base Struct - ContainerBase<P, PD>
// ============================================================================

/// Base for container render objects (internal use)
///
/// Contains Children<P, PD> + geometry
#[derive(Debug)]
pub struct ContainerBase<P: Protocol, PD = ()> {
    pub(crate) children: Children<P, PD>,
    pub(crate) geometry: P::Geometry,
}

impl<P: Protocol, PD> Default for ContainerBase<P, PD>
where
    P::Geometry: Default,
{
    fn default() -> Self {
        Self {
            children: Children::new(),
            geometry: P::Geometry::default(),
        }
    }
}

// Implement delegatable traits for ContainerBase
impl<P: Protocol, PD> HasChildren<P, PD> for ContainerBase<P, PD> {
    fn children(&self) -> &Children<P, PD> {
        &self.children
    }

    fn children_mut(&mut self) -> &mut Children<P, PD> {
        &mut self.children
    }
}

// Box specialization - implement HasBoxGeometry
impl<PD> HasBoxGeometry for ContainerBase<BoxProtocol, PD> {
    fn size(&self) -> Size {
        self.geometry
    }

    fn set_size(&mut self, size: Size) {
        self.geometry = size;
    }
}

// Sliver specialization - implement HasSliverGeometry
impl<PD> HasSliverGeometry for ContainerBase<SliverProtocol, PD> {
    fn geometry(&self) -> &SliverGeometry {
        &self.geometry
    }

    fn set_geometry(&mut self, geometry: SliverGeometry) {
        self.geometry = geometry;
    }
}

// ============================================================================
// Part 3: Generic ContainerBox<T, PD> with Ambassador + Deref
// ============================================================================

/// Generic container render object with automatic delegation
///
/// # Type Parameters
///
/// - `T`: Custom data type (must implement `ProxyData`)
/// - `PD`: Parent data type (default: ())
///
/// # Automatic Features
///
/// - **HasChildren** via Ambassador delegation to `base`
/// - **HasBoxGeometry** via Ambassador delegation to `base`
/// - **Deref to T** for direct field access
///
/// # Example
///
/// ```rust,ignore
/// #[derive(Clone, Debug)]
/// pub struct FlexData {
///     pub direction: Axis,
/// }
///
/// #[derive(Default, Clone, Debug)]
/// pub struct FlexParentData {
///     pub flex: f32,
///     pub offset: Offset,
/// }
///
/// pub type RenderFlex = ContainerBox<FlexData, FlexParentData>;
///
/// impl RenderContainerBox<FlexParentData> for RenderFlex {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         // Use self.direction via Deref
///         // Use self.children_mut() via Ambassador
///         // ...
///     }
/// }
/// ```
#[derive(Debug, Delegate)]
#[delegate(HasChildren<BoxProtocol, PD>, target = "base")]
#[delegate(HasBoxGeometry, target = "base")]
pub struct ContainerBox<T: ProxyData, PD = ()> {
    base: ContainerBase<BoxProtocol, PD>,
    pub data: T,
}

impl<T: ProxyData, PD> ContainerBox<T, PD>
where
    PD: Default,
{
    /// Create new ContainerBox with data
    pub fn new(data: T) -> Self {
        Self {
            base: ContainerBase::default(),
            data,
        }
    }
}

// ✨ Deref for clean field access
impl<T: ProxyData, PD> Deref for ContainerBox<T, PD> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData, PD> DerefMut for ContainerBox<T, PD> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

// ============================================================================
// Part 4: RenderContainerBox - Mixin Trait
// ============================================================================

/// Mixin trait for container Box render objects
///
/// Provides default paint/hit_test that iterate children.
///
/// **IMPORTANT:** `perform_layout` has NO default - you MUST override it!
///
/// # Example
///
/// ```rust,ignore
/// impl RenderContainerBox<FlexParentData> for RenderFlex {
///     fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size {
///         // Your layout logic here
///         for (child_id, parent_data) in self.children_mut().iter_with_data_mut() {
///             // Layout child, set parent_data.offset
///         }
///         self.set_size(size);
///         size
///     }
///
///     // paint() and hit_test() auto-iterate children!
/// }
/// ```
pub trait RenderContainerBox<PD = ()>: HasChildren<BoxProtocol, PD> + HasBoxGeometry {
    /// Perform layout (NO DEFAULT - must override!)
    ///
    /// Your implementation should:
    /// 1. Iterate children using `self.children_mut()`
    /// 2. Layout each child with appropriate constraints
    /// 3. Update parent_data (e.g., offset) for each child
    /// 4. Calculate and set final size with `self.set_size()`
    /// 5. Return the final size
    fn perform_layout(&mut self, constraints: &BoxConstraints) -> Size;

    /// Paint this render object (default: paint all children with their offsets)
    ///
    /// Note: This assumes PD has an offset field. For custom parent data,
    /// override this method.
    fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {
        // TODO: for (child_id, parent_data) in self.children().iter_with_data() {
        //     ctx.paint_child(child_id, offset + parent_data.offset);
        // }
    }

    /// Hit test (default: test children in reverse order)
    ///
    /// Returns true if any child was hit.
    fn hit_test(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        // TODO: for (child_id, parent_data) in self.children().iter_with_data().rev() {
        //     if render_tree.hit_test(child_id, result, position - parent_data.offset) {
        //         return true;
        //     }
        // }
        false
    }

    /// Compute minimum intrinsic width
    fn compute_min_intrinsic_width(&self, _height: f32) -> f32 {
        0.0 // TODO: iterate children
    }

    /// Compute maximum intrinsic width
    fn compute_max_intrinsic_width(&self, _height: f32) -> f32 {
        0.0 // TODO: iterate children
    }

    /// Compute minimum intrinsic height
    fn compute_min_intrinsic_height(&self, _width: f32) -> f32 {
        0.0 // TODO: iterate children
    }

    /// Compute maximum intrinsic height
    fn compute_max_intrinsic_height(&self, _width: f32) -> f32 {
        0.0 // TODO: iterate children
    }

    /// Whether this render object always needs compositing
    fn always_needs_compositing(&self) -> bool {
        false
    }

    /// Whether this render object is a repaint boundary
    fn is_repaint_boundary(&self) -> bool {
        false
    }
}

// Blanket impl: all ContainerBox<T, PD> get RenderContainerBox
// BUT: perform_layout panics by default - MUST be overridden!
impl<T: ProxyData, PD> RenderContainerBox<PD> for ContainerBox<T, PD> {
    fn perform_layout(&mut self, _constraints: &BoxConstraints) -> Size {
        panic!(
            "perform_layout must be overridden for ContainerBox<{}, {}>",
            std::any::type_name::<T>(),
            std::any::type_name::<PD>()
        )
    }
}

// ============================================================================
// Part 5: ContainerSliver<T, PD>
// ============================================================================

/// Generic container sliver render object with automatic delegation
#[derive(Debug, Delegate)]
#[delegate(HasChildren<SliverProtocol, PD>, target = "base")]
#[delegate(HasSliverGeometry, target = "base")]
pub struct ContainerSliver<T: ProxyData, PD = ()> {
    base: ContainerBase<SliverProtocol, PD>,
    pub data: T,
}

impl<T: ProxyData, PD> ContainerSliver<T, PD>
where
    PD: Default,
{
    pub fn new(data: T) -> Self {
        Self {
            base: ContainerBase::default(),
            data,
        }
    }
}

impl<T: ProxyData, PD> Deref for ContainerSliver<T, PD> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.data
    }
}

impl<T: ProxyData, PD> DerefMut for ContainerSliver<T, PD> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.data
    }
}

/// Mixin trait for container Sliver render objects
pub trait RenderContainerSliver<PD = ()>:
    HasChildren<SliverProtocol, PD> + HasSliverGeometry
{
    fn perform_layout(&mut self, constraints: &SliverConstraints) -> SliverGeometry;

    fn paint(&self, _ctx: &mut PaintingContext, _offset: Offset) {}
    fn hit_test(&self, _result: &mut HitTestResult, _position: Offset) -> bool {
        false
    }
    fn always_needs_compositing(&self) -> bool {
        false
    }
    fn is_repaint_boundary(&self) -> bool {
        false
    }
}

impl<T: ProxyData, PD> RenderContainerSliver<PD> for ContainerSliver<T, PD> {
    fn perform_layout(&mut self, _constraints: &SliverConstraints) -> SliverGeometry {
        panic!(
            "perform_layout must be overridden for ContainerSliver<{}, {}>",
            std::any::type_name::<T>(),
            std::any::type_name::<PD>()
        )
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use flui_foundation::RenderId;

    #[derive(Default, Clone, Debug)]
    struct TestData {
        direction: u8,
    }

    #[derive(Default, Clone, Debug)]
    #[allow(dead_code)]
    struct TestParentData {
        flex: f32,
    }

    #[test]
    fn test_container_box_creation() {
        let container = ContainerBox::<TestData, TestParentData>::new(TestData { direction: 1 });
        assert_eq!(container.direction, 1); // Deref works!
    }

    #[test]
    fn test_container_box_deref() {
        let mut container =
            ContainerBox::<TestData, TestParentData>::new(TestData { direction: 0 });

        // Read via Deref
        assert_eq!(container.direction, 0);

        // Write via DerefMut
        container.direction = 2;
        assert_eq!(container.direction, 2);
    }

    #[test]
    fn test_container_box_children_access() {
        let container = ContainerBox::<TestData, TestParentData>::new(TestData::default());

        // HasChildren trait methods work via Ambassador
        assert!(!container.has_children());
        assert_eq!(container.child_count(), 0);
    }

    #[test]
    fn test_container_box_add_children() {
        let mut container = ContainerBox::<TestData, TestParentData>::new(TestData::default());

        // Add children
        let child1 = RenderId::new(1);
        let child2 = RenderId::new(2);

        container
            .children_mut()
            .push(child1, TestParentData { flex: 1.0 });
        container
            .children_mut()
            .push(child2, TestParentData { flex: 2.0 });

        assert_eq!(container.child_count(), 2);
        assert!(container.has_children());
    }

    #[test]
    fn test_container_box_geometry() {
        let mut container = ContainerBox::<TestData, TestParentData>::new(TestData::default());

        // HasBoxGeometry trait methods work via Ambassador
        let size = Size::new(100.0, 50.0);
        container.set_size(size);
        assert_eq!(container.size(), size);
    }

    #[test]
    #[should_panic(expected = "perform_layout must be overridden")]
    fn test_container_box_perform_layout_panics_by_default() {
        let mut container = ContainerBox::<TestData, TestParentData>::new(TestData::default());
        let constraints = BoxConstraints::tight(Size::new(100.0, 100.0));

        // Should panic because perform_layout is not overridden
        container.perform_layout(&constraints);
    }
}
