//! Core render object trait - behavior interface for all render objects.
//!
//! This module provides the behavior trait for all render objects in FLUI:
//! - [`RenderObject`] - Behavior trait (configuration + callbacks)
//!
//! # Architecture
//!
//! FLUI separates render objects into two parts:
//!
//! | Component | Role | Flutter Equivalent |
//! |-----------|------|-------------------|
//! | `RenderNode` | State container | `RenderObject` fields |
//! | `RenderObject` trait | Behavior interface | `RenderObject` abstract methods |
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                      RenderNode                              │
//! │  (state: parent, depth, needs_layout, constraints, etc.)    │
//! │                                                              │
//! │  ┌─────────────────────────────────────────────────────┐    │
//! │  │              Box<dyn RenderObject>                   │    │
//! │  │  (behavior: sized_by_parent, is_repaint_boundary)   │    │
//! │  └─────────────────────────────────────────────────────┘    │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # What Goes Where
//!
//! ## RenderObject trait (this file) - BEHAVIOR
//! - Configuration flags: `sized_by_parent()`, `is_repaint_boundary()`, `always_needs_compositing()`
//! - Lifecycle hooks: `attach()`, `detach()`, `adopt_child()`, `drop_child()`
//! - Parent data: `create_parent_data()`, `setup_parent_data()`
//! - Transforms: `apply_paint_transform()`
//! - Debug: `debug_name()`, `debug_fill_properties()`
//! - Semantics: `describe_semantics()`, `is_semantics_boundary()`
//!
//! ## RenderNode (node.rs) - STATE
//! - Tree position: `parent`, `depth`, `children`
//! - Dirty flags: `needs_layout`, `needs_paint`, `needs_compositing_bits_update`
//! - Layout cache: `constraints`, `cached_size`, `relayout_boundary`
//! - Compositing: `needs_compositing`, `was_repaint_boundary`, `layer_handle`
//! - Parent data: `parent_data`
//! - Lifecycle: `disposed`, `lifecycle`
//!
//! # Flutter Protocol Compliance
//!
//! | Flutter | FLUI | Location |
//! |---------|------|----------|
//! | `sizedByParent` getter | `sized_by_parent()` | RenderObject trait |
//! | `isRepaintBoundary` getter | `is_repaint_boundary()` | RenderObject trait |
//! | `alwaysNeedsCompositing` getter | `always_needs_compositing()` | RenderObject trait |
//! | `_needsLayout` field | `needs_layout` | RenderNode |
//! | `_needsPaint` field | `needs_paint` | RenderNode |
//! | `_isRelayoutBoundary` field | `relayout_boundary` | RenderNode |
//! | `_needsCompositing` field | `needs_compositing` | RenderNode |
//! | `_constraints` field | `constraints` | RenderNode |
//! | `_layerHandle` field | `layer_handle` | RenderNode |
//! | `parentData` field | `parent_data` | RenderNode |

use std::any::Any;
use std::fmt;

use downcast_rs::{impl_downcast, DowncastSync};

use flui_foundation::{Diagnosticable, RenderId};
use flui_interaction::HitTestTarget;
use flui_painting::Canvas;
use flui_types::events::MouseCursor;
use flui_types::geometry::Matrix4;
use flui_types::semantics::{SemanticsAction, SemanticsProperties};

use crate::HitTestTree;

// ============================================================================
// LAYER HANDLE TYPE ALIAS
// ============================================================================

/// Handle to a compositor layer with reference counting and lifecycle management.
///
/// This uses `flui_layer::AnyLayerHandle` which provides:
/// - Type-safe layer management with polymorphic Layer enum
/// - Reference counting for proper GPU resource lifecycle
/// - Thread-safe access from multiple threads
///
/// # Flutter Equivalence
/// ```dart
/// final LayerHandle<ContainerLayer> _layerHandle = LayerHandle<ContainerLayer>();
/// ```
pub type LayerHandle = flui_layer::AnyLayerHandle;

// ============================================================================
// RENDER OBJECT TRAIT
// ============================================================================

/// Behavior trait for render objects.
///
/// This trait defines the **behavior** of a render object - configuration
/// and callbacks that are specific to each render object type.
///
/// **State is stored in `RenderNode`**, not in the trait implementor.
///
/// # Required vs Optional
///
/// All methods have default implementations. Override only what you need:
///
/// - **Usually override**: `debug_name()`, `create_parent_data()`
/// - **Override for optimization**: `sized_by_parent()`
/// - **Override for layers**: `is_repaint_boundary()`, `always_needs_compositing()`
/// - **Override for transforms**: `apply_paint_transform()`
///
/// # Examples
///
/// ```rust,ignore
/// #[derive(Debug)]
/// struct RenderPadding {
///     padding: EdgeInsets,
/// }
///
/// impl RenderObject for RenderPadding {
///     fn debug_name(&self) -> &'static str {
///         "RenderPadding"
///     }
///
///     // This uses BoxParentData (default), so no override needed
/// }
///
/// // Layout/paint implemented via RenderBox<Single> trait
/// impl RenderBox<Single> for RenderPadding {
///     fn perform_layout(&mut self, constraints: BoxConstraints) -> Size { ... }
///     fn paint(&self, context: &mut PaintingContext, offset: Offset) { ... }
/// }
/// ```
pub trait RenderObject: DowncastSync + fmt::Debug + Diagnosticable + HitTestTarget {
    // ========================================================================
    // DEBUG INFORMATION
    // ========================================================================

    /// Returns human-readable debug name.
    ///
    /// Used in debug output and diagnostics.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// String get debugName => runtimeType.toString();
    /// ```
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns full type name with module path.
    fn type_name(&self) -> &'static str {
        std::any::type_name::<Self>()
    }

    /// Returns short type name without module path.
    fn short_type_name(&self) -> &'static str {
        let full_name = std::any::type_name::<Self>();
        full_name.rsplit("::").next().unwrap_or(full_name)
    }

    // Note: debug_fill_properties() is inherited from Diagnosticable supertrait

    /// Paints debug visualization overlay.
    ///
    /// Called when debug painting is enabled.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void debugPaint(PaintingContext context, Offset offset) { }
    /// ```
    #[cfg(debug_assertions)]
    fn debug_paint(&self, _canvas: &mut Canvas, _geometry: &dyn Any) {
        // Override for custom debug visualization
    }

    // ========================================================================
    // LAYOUT CONFIGURATION
    // ========================================================================

    /// Whether size is determined solely by constraints.
    ///
    /// If `true`, the framework can optimize layout by:
    /// 1. Computing size in `perform_resize()` using only constraints
    /// 2. Skipping child layout if constraints unchanged
    ///
    /// Return `true` when:
    /// - Size doesn't depend on children (e.g., `SizedBox`)
    /// - Size is always `constraints.biggest()` or `constraints.smallest()`
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool get sizedByParent => false;
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // SizedBox always returns true
    /// fn sized_by_parent(&self) -> bool { true }
    ///
    /// // Container returns false (depends on child)
    /// fn sized_by_parent(&self) -> bool { false }
    /// ```
    fn sized_by_parent(&self) -> bool {
        false
    }

    // ========================================================================
    // COMPOSITING CONFIGURATION
    // ========================================================================

    /// Whether this is a repaint boundary.
    ///
    /// Repaint boundaries create their own compositing layer, enabling:
    /// - Paint caching (unchanged subtrees skip repainting)
    /// - Isolated repainting (changes don't propagate to parent)
    ///
    /// Return `true` for:
    /// - Root render objects
    /// - Frequently animating content
    /// - Expensive-to-paint subtrees
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool get isRepaintBoundary => false;
    /// ```
    ///
    /// # Performance Note
    ///
    /// Repaint boundaries have memory overhead (layer allocation).
    /// Use sparingly - only where paint isolation provides real benefit.
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Whether this render object always needs compositing.
    ///
    /// Return `true` if this render object uses GPU features that require
    /// a compositing layer regardless of children:
    /// - Video playback
    /// - Platform views
    /// - Hardware-accelerated effects
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// bool get alwaysNeedsCompositing => false;
    /// ```
    fn always_needs_compositing(&self) -> bool {
        false
    }

    // ========================================================================
    // INTERACTION
    // ========================================================================

    // Note: handle_event() is inherited from HitTestTarget supertrait

    /// Whether this render object handles pointer events.
    ///
    /// If `true`, hit testing will consider this object for pointer events.
    fn handles_pointer_events(&self) -> bool {
        false
    }

    /// Returns the mouse cursor for this render object.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// MouseCursor get cursor => MouseCursor.defer;
    /// ```
    fn cursor(&self) -> MouseCursor {
        MouseCursor::Defer
    }

    // ========================================================================
    // TRANSFORMS
    // ========================================================================

    /// Applies the transform for painting a child.
    ///
    /// Override to apply custom transformations (rotation, scale, etc.).
    /// The default implementation applies translation based on child's offset.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void applyPaintTransform(RenderObject child, Matrix4 transform) {
    ///   final BoxParentData childParentData = child.parentData as BoxParentData;
    ///   transform.translate(childParentData.offset.dx, childParentData.offset.dy);
    /// }
    /// ```
    ///
    /// # Arguments
    ///
    /// * `child_id` - ID of the child render object
    /// * `transform` - Transform matrix to modify in-place
    /// * `tree` - Tree for accessing child's offset
    fn apply_paint_transform(
        &self,
        child_id: RenderId,
        transform: &mut Matrix4,
        tree: &dyn HitTestTree,
    ) {
        if let Some(offset) = tree.get_offset(child_id) {
            *transform = Matrix4::translation(offset.dx, offset.dy, 0.0) * *transform;
        }
    }

    // ========================================================================
    // PARENT DATA
    // ========================================================================

    /// Creates default parent data for children.
    ///
    /// Override to return custom parent data type for your children.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void setupParentData(RenderObject child) {
    ///   if (child.parentData is! BoxParentData)
    ///     child.parentData = BoxParentData();
    /// }
    /// ```
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // For Stack (uses StackParentData)
    /// fn create_parent_data(&self) -> Box<dyn ParentData> {
    ///     Box::new(StackParentData::default())
    /// }
    ///
    /// // For Flex (uses FlexParentData)
    /// fn create_parent_data(&self) -> Box<dyn ParentData> {
    ///     Box::new(FlexParentData::default())
    /// }
    /// ```
    fn create_parent_data(&self) -> Box<dyn crate::ParentData> {
        Box::new(crate::BoxParentData::default())
    }

    /// Sets up parent data for a child.
    ///
    /// Returns `Some(new_data)` if parent data needs to be replaced,
    /// `None` if existing data is acceptable.
    ///
    /// # Arguments
    ///
    /// * `child_data` - Current parent data on the child (if any)
    ///
    /// # Returns
    ///
    /// * `Some(Box<dyn ParentData>)` - Replace with this new data
    /// * `None` - Keep existing data
    fn setup_parent_data(
        &self,
        child_data: Option<&dyn crate::ParentData>,
    ) -> Option<Box<dyn crate::ParentData>> {
        match child_data {
            Some(_) => None,                         // Keep existing
            None => Some(self.create_parent_data()), // Create new
        }
    }

    // ========================================================================
    // LIFECYCLE HOOKS
    // ========================================================================

    /// Called when this render object is attached to a tree.
    ///
    /// Override to perform initialization that requires tree membership.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void attach(PipelineOwner owner) {
    ///   super.attach(owner);
    ///   // custom initialization
    /// }
    /// ```
    fn attach(&mut self) {
        // Override for custom initialization
    }

    /// Called when this render object is detached from a tree.
    ///
    /// Override to perform cleanup when removed from tree.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void detach() {
    ///   // custom cleanup
    ///   super.detach();
    /// }
    /// ```
    fn detach(&mut self) {
        // Override for custom cleanup
    }

    /// Called when a child is adopted.
    ///
    /// Override to track children or perform custom setup.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void adoptChild(RenderObject child) {
    ///   // custom tracking
    ///   super.adoptChild(child);
    /// }
    /// ```
    fn adopt_child(&mut self, _child_id: RenderId) {
        // Override for custom child tracking
    }

    /// Called when a child is dropped.
    ///
    /// Override to untrack children or perform custom cleanup.
    ///
    /// # Flutter Equivalence
    /// ```dart
    /// void dropChild(RenderObject child) {
    ///   // custom cleanup
    ///   super.dropChild(child);
    /// }
    /// ```
    fn drop_child(&mut self, _child_id: RenderId) {
        // Override for custom child cleanup
    }

    // ========================================================================
    // SEMANTICS / ACCESSIBILITY
    // ========================================================================

    /// Describes semantic properties for accessibility.
    ///
    /// Return semantic information (label, hint, actions) for screen readers.
    fn describe_semantics(&self) -> Option<SemanticsProperties> {
        None
    }

    /// Returns semantic actions this render object supports.
    fn semantics_actions(&self) -> &[SemanticsAction] {
        &[]
    }

    /// Performs a semantic action.
    ///
    /// Returns `true` if action was handled.
    fn perform_semantics_action(&mut self, _action: SemanticsAction) -> bool {
        false
    }

    /// Whether this is a semantics boundary.
    ///
    /// Semantics boundaries create new semantics nodes in the accessibility tree.
    fn is_semantics_boundary(&self) -> bool {
        false
    }

    /// Whether this blocks semantics from children.
    fn blocks_child_semantics(&self) -> bool {
        false
    }

    // ========================================================================
    // BOX PROTOCOL (dyn-compatible)
    // ========================================================================

    /// Performs layout using box protocol constraints.
    ///
    /// Returns `Some(size)` if this render object supports box protocol,
    /// `None` otherwise. This allows dyn-safe dispatch without generics.
    ///
    /// # Flutter Protocol
    /// This corresponds to `performLayout()` in `RenderBox`.
    fn perform_box_layout(
        &mut self,
        _constraints: crate::BoxConstraints,
    ) -> Option<flui_types::Size> {
        None // Default: not a box render object
    }

    /// Performs paint using box protocol.
    ///
    /// Returns `true` if paint was performed, `false` if not a box render object.
    fn perform_box_paint(
        &self,
        _ctx: &mut crate::PaintingContext,
        _offset: flui_types::Offset,
    ) -> bool {
        false // Default: not a box render object
    }

    /// Performs hit testing using box protocol.
    ///
    /// Returns `Some(hit)` if this render object supports box protocol,
    /// `None` otherwise.
    fn perform_box_hit_test(
        &self,
        _result: &mut crate::BoxHitTestResult,
        _position: flui_types::Offset,
    ) -> Option<bool> {
        None // Default: not a box render object
    }

    /// Returns the size of a box render object.
    ///
    /// Returns `Some(size)` if this is a box render object that has been laid out,
    /// `None` otherwise.
    fn box_size(&self) -> Option<flui_types::Size> {
        None
    }

    /// Returns whether this render object supports box protocol.
    fn supports_box_protocol(&self) -> bool {
        false
    }

    // ========================================================================
    // SLIVER PROTOCOL (dyn-compatible)
    // ========================================================================

    /// Performs layout using sliver protocol constraints.
    ///
    /// Returns `Some(geometry)` if this render object supports sliver protocol,
    /// `None` otherwise.
    fn perform_sliver_layout(
        &mut self,
        _constraints: flui_types::SliverConstraints,
    ) -> Option<flui_types::SliverGeometry> {
        None // Default: not a sliver render object
    }

    /// Returns whether this render object supports sliver protocol.
    fn supports_sliver_protocol(&self) -> bool {
        false
    }
}

// Enable downcasting for RenderObject
impl_downcast!(sync RenderObject);

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestRenderObject {
        sized_by_parent: bool,
        is_repaint_boundary: bool,
    }

    impl Default for TestRenderObject {
        fn default() -> Self {
            Self {
                sized_by_parent: false,
                is_repaint_boundary: false,
            }
        }
    }

    impl Diagnosticable for TestRenderObject {}

    impl HitTestTarget for TestRenderObject {
        fn handle_event(
            &self,
            _event: &flui_types::events::PointerEvent,
            _entry: &flui_interaction::HitTestEntry,
        ) {
        }
    }

    impl RenderObject for TestRenderObject {
        fn debug_name(&self) -> &'static str {
            "TestRenderObject"
        }

        fn sized_by_parent(&self) -> bool {
            self.sized_by_parent
        }

        fn is_repaint_boundary(&self) -> bool {
            self.is_repaint_boundary
        }
    }

    #[test]
    fn test_debug_name() {
        let obj = TestRenderObject::default();
        assert_eq!(obj.debug_name(), "TestRenderObject");
    }

    #[test]
    fn test_default_configuration() {
        let obj = TestRenderObject::default();
        assert!(!obj.sized_by_parent());
        assert!(!obj.is_repaint_boundary());
        assert!(!obj.always_needs_compositing());
        assert!(!obj.handles_pointer_events());
        assert!(!obj.is_semantics_boundary());
    }

    #[test]
    fn test_custom_configuration() {
        let obj = TestRenderObject {
            sized_by_parent: true,
            is_repaint_boundary: true,
        };
        assert!(obj.sized_by_parent());
        assert!(obj.is_repaint_boundary());
    }

    #[test]
    fn test_downcast() {
        let obj: Box<dyn RenderObject> = Box::new(TestRenderObject::default());
        assert!(obj.as_any().downcast_ref::<TestRenderObject>().is_some());
    }

    #[test]
    fn test_create_parent_data() {
        let obj = TestRenderObject::default();
        let parent_data = obj.create_parent_data();
        // Default creates BoxParentData
        assert!(parent_data
            .as_any()
            .downcast_ref::<crate::BoxParentData>()
            .is_some());
    }
}
