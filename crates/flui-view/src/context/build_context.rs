//! BuildContext - the interface Elements provide during build.
//!
//! BuildContext is passed to Views during the build phase, providing
//! access to tree information and dependency injection.

use flui_foundation::ElementId;
use std::any::TypeId;

/// Context provided to Views during the build phase.
///
/// `BuildContext` provides Views with:
/// - Element identity and tree position
/// - Dependency injection (InheritedView lookups)
/// - Dirty marking for rebuilds
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `BuildContext` abstract class.
/// In Flutter, `Element` implements `BuildContext` - same pattern here.
///
/// # Example
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///         // Access inherited data
///         let theme = ctx.depend_on::<ThemeData>();
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
    /// Get the ElementId of the Element providing this context.
    fn element_id(&self) -> ElementId;

    /// Get the depth of this Element in the tree.
    ///
    /// Root Element has depth 0.
    fn depth(&self) -> usize;

    /// Look up data from an ancestor InheritedView.
    ///
    /// This registers a dependency - when the InheritedView's data changes,
    /// this Element will be rebuilt.
    ///
    /// Uses O(1) hash table lookup, not O(n) parent walk.
    ///
    /// # Type Parameters
    ///
    /// * `T` - The InheritedView type to look up
    ///
    /// # Returns
    ///
    /// The data if an ancestor InheritedView of type `T` exists, None otherwise.
    fn depend_on_inherited(&self, type_id: TypeId) -> Option<&dyn std::any::Any>;

    /// Mark this Element as needing a rebuild.
    ///
    /// The Element will be rebuilt in the next build phase.
    fn mark_needs_build(&self);

    /// Check if we're currently in a build phase.
    fn is_building(&self) -> bool;

    /// Get the nearest ancestor Element of a specific type.
    ///
    /// Unlike `depend_on_inherited`, this does NOT register a dependency.
    fn find_ancestor_element(&self, type_id: TypeId) -> Option<ElementId>;
}

/// Extension trait for typed InheritedView lookups.
pub trait BuildContextExt: BuildContext {
    /// Look up data from an ancestor InheritedView.
    ///
    /// This is the typed version of `depend_on_inherited`.
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
}

impl<C: BuildContext + ?Sized> BuildContextExt for C {}

#[cfg(test)]
mod tests {
    use super::*;

    // Check that BuildContext is object-safe
    fn _assert_object_safe(_: &dyn BuildContext) {}
}
