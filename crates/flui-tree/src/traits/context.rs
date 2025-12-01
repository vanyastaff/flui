//! Tree context abstraction for element tree access.
//!
//! This module provides **read-only** context traits for accessing
//! element tree information. These are abstract interfaces that can
//! be implemented by concrete context types.
//!
//! # Note on Naming
//!
//! This module uses `TreeContext` (not `BuildContext`) to avoid
//! conflict with `flui_element::BuildContext` which is the concrete
//! context used during widget building.
//!
//! - **flui-tree::TreeContext** - Read-only tree access (abstract)
//! - **flui-element::BuildContext** - Full build context with mutations
//!
//! # Example
//!
//! ```rust,ignore
//! use flui_tree::{TreeContext, AncestorLookup};
//!
//! fn inspect<C: TreeContext>(ctx: &C) {
//!     // Read element position
//!     let id = ctx.element_id();
//!     let depth = ctx.depth();
//!
//!     // Get size if available
//!     if let Some(size) = ctx.size() {
//!         println!("Element {} size: {:?}", id, size);
//!     }
//! }
//! ```

use flui_foundation::ElementId;
use flui_types::{Offset, Size};
use std::any::TypeId;

// ============================================================================
// TREE CONTEXT TRAIT
// ============================================================================

/// Read-only interface for tree context.
///
/// This trait provides the minimal interface for reading element
/// tree information. It intentionally provides read-only access
/// to enable safe concurrent reads.
///
/// # Design Notes
///
/// TreeContext is intentionally read-only to enable:
/// - Parallel/concurrent reads
/// - Safe inspection without mutation
/// - Clear separation from mutation operations
///
/// For build-time context with mutation methods, see
/// `flui_element::BuildContext`.
pub trait TreeContext: Send + Sync {
    /// Get the element ID for this context.
    fn element_id(&self) -> ElementId;

    /// Get the widget's runtime type ID.
    ///
    /// Useful for debugging and type-based lookups.
    fn widget_type_id(&self) -> TypeId;

    /// Check if this element is currently mounted.
    fn is_mounted(&self) -> bool;

    /// Get the current size of this element's render object.
    ///
    /// Returns None if:
    /// - Element doesn't have a render object
    /// - Layout hasn't happened yet
    fn size(&self) -> Option<Size>;

    /// Get the global position of this element.
    ///
    /// Returns None if position is not yet determined.
    fn global_position(&self) -> Option<Offset>;

    /// Get the depth of this element in the tree.
    fn depth(&self) -> usize;
}

// ============================================================================
// ANCESTOR LOOKUP
// ============================================================================

/// Trait for looking up ancestors in the tree.
///
/// This enables widgets to find parent elements of specific types,
/// which is essential for InheritedWidget functionality.
pub trait AncestorLookup: TreeContext {
    /// Find the nearest ancestor element of a specific type.
    ///
    /// # Type Safety
    ///
    /// The type parameter allows finding ancestors that implement
    /// specific traits or have specific associated data.
    fn find_ancestor_element<F>(&self, predicate: F) -> Option<ElementId>
    where
        F: Fn(ElementId) -> bool;

    /// Find the nearest ancestor render object of a specific type.
    fn find_ancestor_render_object<F>(&self, predicate: F) -> Option<ElementId>
    where
        F: Fn(ElementId) -> bool;

    /// Find the nearest ancestor state of a specific type.
    ///
    /// This is how `State.of(context)` works in Flutter.
    fn find_ancestor_state_of_type(&self, type_id: TypeId) -> Option<ElementId>;

    /// Visit all ancestors up to the root.
    fn visit_ancestors<F>(&self, visitor: F)
    where
        F: FnMut(ElementId) -> bool; // Return false to stop
}

// ============================================================================
// INHERITED WIDGET LOOKUP
// ============================================================================

/// Trait for inherited widget dependency tracking.
///
/// InheritedWidgets are a key Flutter pattern for efficient
/// data propagation down the tree.
pub trait InheritedLookup: TreeContext {
    /// Find an inherited element by type ID.
    ///
    /// This establishes a dependency - when the inherited widget
    /// changes, this element will be rebuilt.
    fn depend_on_inherited(&mut self, type_id: TypeId) -> Option<ElementId>;

    /// Find inherited element without establishing dependency.
    ///
    /// Use when you want to read but not react to changes.
    fn get_inherited(&self, type_id: TypeId) -> Option<ElementId>;

    /// Get all inherited elements this context depends on.
    fn inherited_dependencies(&self) -> &[ElementId];

    /// Clear all inherited dependencies.
    ///
    /// Called before rebuild to reset dependencies.
    fn clear_dependencies(&mut self);
}

// ============================================================================
// DESCENDANT LOOKUP
// ============================================================================

/// Trait for looking up descendants in the tree.
///
/// Less common than ancestor lookup, but useful for:
/// - Focus management
/// - Form validation
/// - Scrollable finding
pub trait DescendantLookup: TreeContext {
    /// Find a descendant element matching a predicate.
    fn find_descendant_element<F>(&self, predicate: F) -> Option<ElementId>
    where
        F: Fn(ElementId) -> bool;

    /// Visit all descendants.
    fn visit_descendants<F>(&self, visitor: F)
    where
        F: FnMut(ElementId) -> bool; // Return false to skip subtree
}

// ============================================================================
// RENDER CONTEXT
// ============================================================================

/// Extended context for render object operations.
///
/// Available during layout and paint phases, providing additional
/// information not available during build.
pub trait RenderContext: TreeContext {
    /// Get constraints passed from parent during layout.
    fn constraints(&self) -> Option<&dyn std::any::Any>;

    /// Check if layout is needed.
    fn needs_layout(&self) -> bool;

    /// Check if paint is needed.
    fn needs_paint(&self) -> bool;

    /// Get the parent's size (for relative sizing).
    fn parent_size(&self) -> Option<Size>;

    /// Get the offset from parent (position within parent).
    fn offset_in_parent(&self) -> Option<Offset>;
}

// ============================================================================
// OWNER CONTEXT
// ============================================================================

/// Context operations that require the build owner.
///
/// These operations affect the build scheduling and are typically
/// used during state changes.
pub trait OwnerContext: TreeContext {
    /// Schedule this element for rebuild.
    ///
    /// This is how `setState()` triggers rebuilds.
    fn schedule_rebuild(&self);

    /// Schedule rebuild for a specific element.
    fn schedule_rebuild_for(&self, element: ElementId);

    /// Check if currently in build phase.
    fn is_building(&self) -> bool;

    /// Check if currently in layout phase.
    fn is_laying_out(&self) -> bool;
}

// ============================================================================
// NAVIGATION CONTEXT
// ============================================================================

/// Context for navigation operations.
///
/// Provides access to navigation state and routes.
pub trait NavigationContext: TreeContext {
    /// Get the current route name.
    fn current_route(&self) -> Option<&str>;

    /// Check if can pop (go back).
    fn can_pop(&self) -> bool;

    /// Get navigation depth (number of routes on stack).
    fn navigation_depth(&self) -> usize;
}

// ============================================================================
// FULL BUILD CONTEXT
// ============================================================================

/// Combined trait for full tree context functionality.
///
/// This combines all context traits that are typically available
/// during tree operations.
pub trait FullTreeContext:
    TreeContext + AncestorLookup + InheritedLookup + DescendantLookup
{
}

// Blanket implementation
impl<T> FullTreeContext for T where
    T: TreeContext + AncestorLookup + InheritedLookup + DescendantLookup
{
}

// ============================================================================
// CONTEXT WRAPPER
// ============================================================================

/// A minimal read-only context wrapper.
///
/// Used to provide limited access to element during build,
/// preventing direct tree manipulation.
#[derive(Debug, Clone, Copy)]
pub struct ReadOnlyContext {
    element_id: ElementId,
    depth: usize,
    is_mounted: bool,
}

impl ReadOnlyContext {
    /// Create a new read-only context.
    pub fn new(element_id: ElementId, depth: usize, is_mounted: bool) -> Self {
        Self {
            element_id,
            depth,
            is_mounted,
        }
    }
}

impl TreeContext for ReadOnlyContext {
    fn element_id(&self) -> ElementId {
        self.element_id
    }

    fn widget_type_id(&self) -> TypeId {
        // Default implementation - concrete types override
        TypeId::of::<()>()
    }

    fn is_mounted(&self) -> bool {
        self.is_mounted
    }

    fn size(&self) -> Option<Size> {
        None // Not available in minimal context
    }

    fn global_position(&self) -> Option<Offset> {
        None // Not available in minimal context
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_context() {
        let id = ElementId::new(42);
        let ctx = ReadOnlyContext::new(id, 5, true);

        assert_eq!(ctx.element_id(), id);
        assert_eq!(ctx.depth(), 5);
        assert!(ctx.is_mounted());
        assert!(ctx.size().is_none());
        assert!(ctx.global_position().is_none());
    }
}
