//! Base trait for all render objects

use std::any::Any;
use std::fmt::Debug;

/// Base trait for all render objects in the rendering tree
///
/// RenderObject is the foundation of the rendering system. All render objects
/// (whether Box or Sliver protocol) implement this trait, which provides:
///
/// - Tree structure (parent, depth, owner)
/// - Lifecycle management (attach, detach)
/// - Dirty tracking (mark_needs_layout, mark_needs_paint)
/// - Parent data access
/// - Type introspection
///
/// # Protocol Specialization
///
/// While RenderObject is the base trait, render objects typically implement
/// protocol-specific traits:
///
/// - **RenderBox**: For 2D box layout
/// - **RenderSliver**: For scrollable content
///
/// # Lifecycle
///
/// ```text
/// Created → Attached → Layout → Paint → Detached → Dropped
///     ↑         ↓                           ↑
///     └─────────┴───────────────────────────┘
///           (can reattach after detach)
/// ```
///
/// # Example
///
/// ```ignore
/// use flui_rendering::traits::RenderObject;
///
/// fn inspect_render_object(obj: &dyn RenderObject) {
///     println!("Depth: {}", obj.depth());
///     println!("Attached: {}", obj.attached());
/// }
/// ```
pub trait RenderObject: Debug + Send + Sync + 'static {
    // ===== Tree Structure =====

    /// Returns the depth of this render object in the tree
    ///
    /// The root has depth 0, its children have depth 1, etc.
    fn depth(&self) -> usize {
        0
    }

    /// Returns whether this render object is currently attached to a tree
    fn attached(&self) -> bool {
        false
    }

    // ===== Lifecycle =====

    /// Attaches this render object to the rendering tree
    ///
    /// Called when the render object becomes part of the tree. This is when
    /// the object should acquire resources, register listeners, etc.
    fn attach(&mut self) {
        // Default implementation does nothing
        // Subclasses override to handle attachment
    }

    /// Detaches this render object from the rendering tree
    ///
    /// Called when the render object is being removed from the tree. This is
    /// when the object should release resources, unregister listeners, etc.
    fn detach(&mut self) {
        // Default implementation does nothing
        // Subclasses override to handle detachment
    }

    // ===== Dirty Tracking =====

    /// Marks this render object as needing layout
    ///
    /// This schedules the object to have its layout recalculated in the next
    /// frame. Typically called when a property changes that affects layout.
    fn mark_needs_layout(&mut self) {
        // Default implementation does nothing
        // Actual implementation provided by container or pipeline
    }

    /// Marks this render object as needing paint
    ///
    /// This schedules the object to be repainted in the next frame.
    /// Typically called when a property changes that affects appearance
    /// but not layout (e.g., color, opacity).
    fn mark_needs_paint(&mut self) {
        // Default implementation does nothing
        // Actual implementation provided by container or pipeline
    }

    /// Marks this render object as needing compositing bits update
    ///
    /// This schedules an update of compositing information (e.g., whether
    /// a layer is needed for this object).
    fn mark_needs_compositing_bits_update(&mut self) {
        // Default implementation does nothing
        // Actual implementation provided by container or pipeline
    }

    // ===== Configuration =====

    /// Returns whether this object's size is determined entirely by constraints
    ///
    /// If true, the object's size depends only on the constraints passed to
    /// perform_layout, not on the sizes of its children. This enables
    /// optimizations in the layout algorithm.
    fn sized_by_parent(&self) -> bool {
        false
    }

    /// Returns whether this object always forms a repaint boundary
    ///
    /// Repaint boundaries prevent unnecessary repaints from propagating.
    /// If true, when this object is marked as needing paint, ancestors
    /// won't be marked as needing paint.
    fn is_repaint_boundary(&self) -> bool {
        false
    }

    /// Returns whether this object always needs compositing
    ///
    /// If true, this object will always create a separate compositing layer.
    /// This is useful for objects that apply effects requiring compositing
    /// (e.g., opacity, clip, transform).
    fn always_needs_compositing(&self) -> bool {
        false
    }

    // ===== Type Introspection =====

    /// Returns this render object as `&dyn Any` for downcasting
    fn as_any(&self) -> &dyn Any;

    /// Returns this render object as `&mut dyn Any` for downcasting
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// Extension methods for render objects
pub trait RenderObjectExt: RenderObject {
    /// Attempts to downcast to a specific render object type
    fn downcast_ref<T: 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }

    /// Attempts to mutably downcast to a specific render object type
    fn downcast_mut<T: 'static>(&mut self) -> Option<&mut T> {
        self.as_any_mut().downcast_mut::<T>()
    }
}

// Blanket implementation for all RenderObject types
impl<T: RenderObject + ?Sized> RenderObjectExt for T {}
