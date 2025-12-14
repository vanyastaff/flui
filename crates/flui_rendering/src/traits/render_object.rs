//! Base RenderObject trait.
//!
//! RenderObject is the base class for all objects in the render tree.

use std::any::Any;
use std::fmt::Debug;

use crate::parent_data::ParentData;
use crate::pipeline::PipelineOwner;

// ============================================================================
// RenderObject Trait
// ============================================================================

/// Base trait for all render objects.
///
/// RenderObject is the core abstraction of the rendering system. It provides:
/// - Tree structure (parent, children, depth)
/// - Lifecycle management (attach, detach, dispose)
/// - Dirty marking (layout, paint, compositing, semantics)
/// - Parent data storage
///
/// # Flutter Equivalence
///
/// This corresponds to Flutter's `RenderObject` abstract class in
/// `rendering/object.dart`.
///
/// # Implementation Notes
///
/// Most render objects don't implement this trait directly. Instead:
/// - For 2D box layout: implement [`RenderBox`](super::RenderBox)
/// - For scrollable content: implement [`RenderSliver`](super::RenderSliver)
///
/// # Thread Safety
///
/// All render objects must be `Send + Sync` to support parallel layout
/// and rendering operations.
pub trait RenderObject: Debug + Send + Sync + 'static {
    // ========================================================================
    // Tree Structure
    // ========================================================================

    /// Returns the parent render object, if any.
    fn parent(&self) -> Option<&dyn RenderObject>;

    /// Returns the depth of this node in the render tree.
    ///
    /// The root has depth 0, its children have depth 1, etc.
    fn depth(&self) -> usize;

    /// Returns the pipeline owner that manages this render object.
    fn owner(&self) -> Option<&PipelineOwner>;

    // ========================================================================
    // Lifecycle
    // ========================================================================

    /// Called when this render object is attached to a pipeline owner.
    ///
    /// This is called when the render object is inserted into the tree
    /// or when the tree is attached to a pipeline owner.
    fn attach(&mut self, owner: &PipelineOwner);

    /// Called when this render object is detached from its pipeline owner.
    ///
    /// This is called when the render object is removed from the tree
    /// or when the tree is detached from its pipeline owner.
    fn detach(&mut self);

    /// Releases any resources held by this render object.
    ///
    /// Called when the render object will never be used again.
    /// After calling dispose, the object is no longer usable.
    fn dispose(&mut self) {}

    // ========================================================================
    // Dirty Marking
    // ========================================================================

    /// Marks this render object as needing layout.
    ///
    /// Call this when something changes that affects the layout of this
    /// object or its descendants.
    fn mark_needs_layout(&mut self);

    /// Marks this render object as needing paint.
    ///
    /// Call this when something changes that affects the visual appearance
    /// of this object but not its layout.
    fn mark_needs_paint(&mut self);

    /// Marks this render object as needing compositing bits update.
    ///
    /// Call this when something changes that affects whether this object
    /// or its descendants need compositing.
    fn mark_needs_compositing_bits_update(&mut self);

    /// Marks this render object as needing semantics update.
    ///
    /// Call this when something changes that affects the semantics
    /// (accessibility) of this object.
    fn mark_needs_semantics_update(&mut self);

    // ========================================================================
    // Layout Configuration
    // ========================================================================

    /// Whether this render object's size is determined entirely by its parent.
    ///
    /// If true, the parent can skip calling layout on this object when
    /// only the parent's constraints change but the child's intrinsic
    /// dimensions haven't changed.
    ///
    /// Default is `false`.
    fn sized_by_parent(&self) -> bool {
        false
    }

    /// Whether this render object creates a new paint layer.
    ///
    /// If true, this render object will be painted into its own layer,
    /// which can improve performance when parts of the UI change frequently.
    ///
    /// Default is `false`.
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Whether this render object always needs compositing.
    ///
    /// If true, this render object requires a compositing layer even
    /// if it has no children that require compositing.
    ///
    /// Default is `false`.
    fn always_needs_compositing(&self) -> bool {
        false
    }

    // ========================================================================
    // Parent Data
    // ========================================================================

    /// Sets up the parent data for a child.
    ///
    /// Called when a child is added to this render object. Override this
    /// to set up custom parent data types.
    fn setup_parent_data(&self, child: &mut dyn RenderObject) {
        let _ = child;
    }

    /// Returns the parent data for this render object.
    fn parent_data(&self) -> Option<&dyn ParentData>;

    /// Returns mutable parent data for this render object.
    fn parent_data_mut(&mut self) -> Option<&mut dyn ParentData>;

    /// Sets the parent data for this render object.
    fn set_parent_data(&mut self, data: Box<dyn ParentData>);

    // ========================================================================
    // Children
    // ========================================================================

    /// Visits each child render object.
    fn visit_children(&self, visitor: &mut dyn FnMut(&dyn RenderObject));

    /// Visits each child render object mutably.
    fn visit_children_mut(&mut self, visitor: &mut dyn FnMut(&mut dyn RenderObject));

    // ========================================================================
    // Type Inspection
    // ========================================================================

    /// Returns self as `Any` for downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Returns self as mutable `Any` for downcasting.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// ============================================================================
// Helper Extensions
// ============================================================================

/// Extension trait for downcasting render objects.
pub trait RenderObjectExt: RenderObject {
    /// Attempts to downcast to a concrete type.
    fn downcast_ref<T: RenderObject>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Attempts to downcast to a concrete type mutably.
    fn downcast_mut<T: RenderObject>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

impl<T: RenderObject + ?Sized> RenderObjectExt for T {}
