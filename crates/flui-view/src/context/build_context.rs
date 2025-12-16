//! BuildContext - the interface Elements provide during build.
//!
//! BuildContext is passed to Views during the build phase, providing
//! access to tree information and dependency injection.
//!
//! # Flutter Equivalent
//!
//! This corresponds to Flutter's `BuildContext` abstract class.
//! In Flutter, `Element` implements `BuildContext` - same pattern here.

use flui_foundation::ElementId;
use std::any::TypeId;

/// Context provided to Views during the build phase.
///
/// `BuildContext` provides Views with:
/// - Element identity and tree position
/// - Dependency injection (InheritedView lookups)
/// - Ancestor lookups (find ancestors by type)
/// - Dirty marking for rebuilds
///
/// # Important Notes
///
/// - Most methods should only be called during build
/// - `depend_on_inherited` registers a dependency (causes rebuild on change)
/// - `get_inherited` does NOT register a dependency (one-time lookup)
/// - Ancestor lookups walk the tree - use sparingly
///
/// # Example
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         // Access inherited data (registers dependency)
///         let theme = ctx.depend_on::<ThemeData>();
///
///         // One-time lookup (no dependency)
///         let config = ctx.get::<AppConfig>();
///
///         // Get element info
///         let depth = ctx.depth();
///
///         // Build child
///         Text::new("Hello")
///     }
/// }
/// ```
pub trait BuildContext: Send + Sync {
    // ========================================================================
    // Identity & State
    // ========================================================================

    /// Get the ElementId of the Element providing this context.
    fn element_id(&self) -> ElementId;

    /// Get the depth of this Element in the tree.
    ///
    /// Root Element has depth 0.
    fn depth(&self) -> usize;

    /// Check if this Element is currently mounted in the tree.
    ///
    /// Returns false after `unmount()` has been called.
    fn mounted(&self) -> bool;

    /// Check if we're currently in a build phase.
    ///
    /// Only valid in debug builds.
    fn is_building(&self) -> bool;

    // ========================================================================
    // Owner Access
    // ========================================================================

    /// Get the BuildOwner managing this context.
    ///
    /// The BuildOwner coordinates build phases across the tree.
    fn owner(&self) -> Option<&crate::BuildOwner>;

    // ========================================================================
    // Inherited Data (Dependency Injection)
    // ========================================================================

    /// Look up data from an ancestor InheritedView and register a dependency.
    ///
    /// This registers a dependency - when the InheritedView's data changes,
    /// this Element will be rebuilt and `did_change_dependencies()` called.
    ///
    /// Uses O(1) hash table lookup, not O(n) parent walk.
    ///
    /// # Type Parameters
    ///
    /// * `type_id` - The TypeId of the InheritedView to look up
    ///
    /// # Returns
    ///
    /// The data if an ancestor InheritedView of that type exists, None otherwise.
    fn depend_on_inherited(&self, type_id: TypeId) -> Option<&dyn std::any::Any>;

    /// Look up data from an ancestor InheritedView WITHOUT registering a dependency.
    ///
    /// Unlike `depend_on_inherited`, this does NOT cause rebuilds when the
    /// InheritedView changes. Use this for one-time lookups where you don't
    /// need to track changes.
    ///
    /// # Type Parameters
    ///
    /// * `type_id` - The TypeId of the InheritedView to look up
    fn get_inherited(&self, type_id: TypeId) -> Option<&dyn std::any::Any>;

    // ========================================================================
    // Ancestor Lookups
    // ========================================================================

    /// Get the nearest ancestor Element of a specific type.
    ///
    /// Walks up the tree until an Element with matching view type is found.
    /// This does NOT register a dependency.
    ///
    /// # Performance
    ///
    /// O(n) where n is distance to ancestor. Use sparingly.
    fn find_ancestor_element(&self, type_id: TypeId) -> Option<ElementId>;

    /// Get the nearest ancestor View of a specific type.
    ///
    /// Similar to `find_ancestor_element` but returns the View configuration.
    fn find_ancestor_view(&self, type_id: TypeId) -> Option<&dyn std::any::Any>;

    /// Get the nearest ancestor State of a specific type.
    ///
    /// Useful for StatefulViews to find parent states.
    fn find_ancestor_state(&self, type_id: TypeId) -> Option<&dyn std::any::Any>;

    /// Get the root ancestor State of a specific type.
    ///
    /// Unlike `find_ancestor_state`, this finds the furthest ancestor, not nearest.
    fn find_root_ancestor_state(&self, type_id: TypeId) -> Option<&dyn std::any::Any>;

    // ========================================================================
    // RenderObject Access
    // ========================================================================

    /// Find the nearest RenderObject.
    ///
    /// If this Element is a RenderElement, returns its RenderObject.
    /// Otherwise, walks down to find the first descendant RenderObject.
    ///
    /// # Returns
    ///
    /// The RenderObject ID if found, None otherwise.
    fn find_render_object(&self) -> Option<flui_foundation::RenderId>;

    // ========================================================================
    // Tree Traversal
    // ========================================================================

    /// Visit ancestor Elements from this Element up to root.
    ///
    /// The visitor returns `true` to continue, `false` to stop.
    fn visit_ancestor_elements(&self, visitor: &mut dyn FnMut(ElementId) -> bool);

    /// Visit child Elements of this Element.
    ///
    /// # Note
    ///
    /// Cannot be called during build - will panic in debug mode.
    fn visit_child_elements(&self, visitor: &mut dyn FnMut(ElementId));

    // ========================================================================
    // Rebuild Control
    // ========================================================================

    /// Mark this Element as needing a rebuild.
    ///
    /// The Element will be rebuilt in the next build phase.
    fn mark_needs_build(&self);
}

/// Extension trait for typed InheritedView lookups.
pub trait BuildContextExt: BuildContext {
    /// Look up data from an ancestor InheritedView (with dependency).
    ///
    /// This is the typed version of `depend_on_inherited`.
    /// Registers a dependency - this Element rebuilds when data changes.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let theme: Option<&ThemeData> = ctx.depend_on::<Theme>();
    /// ```
    fn depend_on<T: 'static>(&self) -> Option<&T> {
        self.depend_on_inherited(TypeId::of::<T>())
            .and_then(|any| any.downcast_ref::<T>())
    }

    /// Look up data from an ancestor InheritedView (without dependency).
    ///
    /// This is the typed version of `get_inherited`.
    /// Does NOT register a dependency - use for one-time lookups.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let config: Option<&AppConfig> = ctx.get::<AppConfig>();
    /// ```
    fn get<T: 'static>(&self) -> Option<&T> {
        self.get_inherited(TypeId::of::<T>())
            .and_then(|any| any.downcast_ref::<T>())
    }

    /// Find the nearest ancestor View of type T.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let scaffold: Option<&Scaffold> = ctx.find_ancestor::<Scaffold>();
    /// ```
    fn find_ancestor<T: 'static>(&self) -> Option<&T> {
        self.find_ancestor_view(TypeId::of::<T>())
            .and_then(|any| any.downcast_ref::<T>())
    }

    /// Find the nearest ancestor State of type T.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let state: Option<&ScaffoldState> = ctx.find_state::<ScaffoldState>();
    /// ```
    fn find_state<T: 'static>(&self) -> Option<&T> {
        self.find_ancestor_state(TypeId::of::<T>())
            .and_then(|any| any.downcast_ref::<T>())
    }
}

impl<C: BuildContext + ?Sized> BuildContextExt for C {}

#[cfg(test)]
mod tests {
    use super::*;

    // Check that BuildContext is object-safe
    fn _assert_object_safe(_: &dyn BuildContext) {}
}
