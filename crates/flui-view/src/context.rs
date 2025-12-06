//! BuildContext trait - Gateway to framework services
//!
//! Like Flutter's BuildContext, this provides access to:
//! - Element tree navigation
//! - Inherited data lookup (dependency injection)
//! - Framework services (Theme, MediaQuery, etc.)
//!
//! # Architecture
//!
//! ```text
//! BuildContext (trait) - defined here in flui-view
//!     │
//!     └── Element (impl BuildContext) - in flui-element
//! ```
//!
//! This follows Flutter's pattern where `BuildContext` is an abstract
//! interface and `Element` is the concrete implementation.

use flui_foundation::ElementId;
use std::any::{Any, TypeId};
use std::sync::Arc;

// ============================================================================
// BUILD CONTEXT TRAIT
// ============================================================================

/// BuildContext - Gateway to framework services
///
/// This is the primary interface through which views interact with the
/// framework. It provides access to:
///
/// - **Element identity**: `element_id()`, `depth()`, `parent_id()`
/// - **Dependency injection**: `depend_on<T>()` for inherited data
/// - **Dirty tracking**: `mark_dirty()` to trigger rebuilds
/// - **Tree walking**: `visit_ancestors()` for advanced use cases
///
/// # Design Philosophy
///
/// BuildContext is the ONLY interface views need to interact with the
/// framework. All services are accessed through it using the Flutter-style
/// `.of(context)` pattern:
///
/// ```rust,ignore
/// fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///     let theme = ctx.depend_on::<ThemeData>()
///         .expect("ThemeProvider not found");
///
///     Button::new("Click").color(theme.primary_color)
/// }
/// ```
///
/// # Thread Safety
///
/// BuildContext must be `Send + Sync` to enable concurrent access from
/// different parts of the framework.
///
/// # Flutter Equivalent
///
/// This is equivalent to Flutter's `BuildContext` abstract class, which
/// is implemented by `Element`.
pub trait BuildContext: Send + Sync {
    // ========== ELEMENT IDENTITY ==========

    /// Get the current element's unique identifier.
    ///
    /// **Flutter equivalent:** `context.widget` (indirectly via element)
    fn element_id(&self) -> ElementId;

    /// Get the element's depth in the tree (0 = root).
    ///
    /// Useful for debugging and understanding tree structure.
    fn depth(&self) -> usize;

    /// Get the parent element's ID.
    ///
    /// Returns `None` if this is the root element.
    fn parent_id(&self) -> Option<ElementId>;

    // ========== DEPENDENCY INJECTION ==========

    /// Look up inherited data by TypeId (low-level API).
    ///
    /// This is the low-level method that powers the type-safe `depend_on<T>()`
    /// helper. Most code should use `depend_on<T>()` instead.
    ///
    /// When called, this method:
    /// 1. Walks up the element tree to find a Provider<T>
    /// 2. Registers a dependency so changes trigger rebuilds
    /// 3. Returns `Arc<dyn Any>` that can be downcast to T
    ///
    /// **Flutter equivalent:** `dependOnInheritedWidgetOfExactType<T>()`
    fn depend_on_raw(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>>;

    /// Find ancestor widget without registering dependency.
    ///
    /// Similar to `depend_on_raw`, but does NOT register a dependency.
    /// Use when you need to read data but don't want rebuilds when it changes.
    ///
    /// **Flutter equivalent:** `findAncestorWidgetOfExactType<T>()`
    fn find_ancestor_widget(&self, type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>>;

    // ========== DIRTY TRACKING ==========

    /// Mark the current element as needing rebuild.
    ///
    /// Call this when your view's state changes and you need to rebuild.
    /// The rebuild will happen in the next frame.
    ///
    /// **Flutter equivalent:** `setState()` calls this internally
    fn mark_dirty(&self);

    /// Schedule rebuild for a specific element.
    ///
    /// Less common than `mark_dirty()`, but useful when you need to
    /// trigger rebuilds of other elements.
    fn schedule_rebuild(&self, element_id: ElementId);

    /// Create a rebuild callback for async operations.
    ///
    /// Returns a callback that can be called from async contexts (like
    /// animation listeners) to trigger rebuilds of this element.
    ///
    /// The callback captures whatever state is needed to schedule rebuilds
    /// (e.g., dirty set reference, element ID) and can be called from any
    /// thread at any time.
    ///
    /// **Flutter equivalent:** Similar to `setState()` captured in closures
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// fn init(&mut self, ctx: &dyn BuildContext) {
    ///     let rebuild = ctx.create_rebuild_callback();
    ///
    ///     animation.add_listener(Box::new(move || {
    ///         rebuild();  // Triggers rebuild from animation thread
    ///     }));
    /// }
    /// ```
    fn create_rebuild_callback(&self) -> Box<dyn Fn() + Send + Sync>;

    // ========== TREE WALKING ==========

    /// Visit all ancestors from parent to root.
    ///
    /// The visitor function is called for each ancestor, starting with the
    /// immediate parent and moving up to the root. If the visitor returns
    /// `false`, iteration stops.
    ///
    /// **Flutter equivalent:** `visitAncestorElements()`
    fn visit_ancestors(&self, visitor: &mut dyn FnMut(ElementId) -> bool);

    // ========== DOWNCASTING ==========

    /// Downcast to concrete BuildContext implementation.
    ///
    /// Useful for advanced use cases where you need access to
    /// implementation-specific methods.
    fn as_any(&self) -> &dyn Any;
}

// ============================================================================
// BUILD CONTEXT EXTENSION TRAIT
// ============================================================================

/// Extension methods for BuildContext.
///
/// Provides type-safe, ergonomic wrappers around the low-level
/// `depend_on_raw()` and `find_ancestor_widget()` methods.
pub trait BuildContextExt: BuildContext {
    /// Type-safe dependency lookup (registers dependency).
    ///
    /// This is the primary way to access inherited data:
    /// 1. Looks up the nearest Provider<T> in the ancestor chain
    /// 2. Registers a dependency so changes trigger rebuilds
    /// 3. Returns `Arc<T>` with proper type
    ///
    /// **Flutter equivalent:** `context.dependOnInheritedWidgetOfExactType<T>()`
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let theme = ctx.depend_on::<ThemeData>()
    ///     .expect("ThemeProvider not found");
    /// let color = theme.primary_color;
    /// ```
    fn depend_on<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let type_id = TypeId::of::<T>();
        let any_arc = self.depend_on_raw(type_id)?;
        any_arc.downcast::<T>().ok()
    }

    /// Type-safe ancestor lookup (no dependency).
    ///
    /// Like `depend_on<T>()`, but does NOT register a dependency.
    /// Use when you need to read data but don't want rebuilds.
    ///
    /// **Flutter equivalent:** `context.findAncestorWidgetOfExactType<T>()`
    fn find_ancestor<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        let type_id = TypeId::of::<T>();
        let any_arc = self.find_ancestor_widget(type_id)?;
        any_arc.downcast::<T>().ok()
    }

    /// Try to downcast to specific BuildContext implementation.
    fn downcast_ref<T: BuildContext + 'static>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>()
    }
}

// Blanket implementation: all BuildContext implementations get these helpers
impl<T: BuildContext + ?Sized> BuildContextExt for T {}

// ============================================================================
// MOCK FOR TESTING
// ============================================================================

/// Mock implementation of `BuildContext` for testing.
///
/// Provides a simple implementation that can be used in unit tests.
#[cfg(any(test, feature = "test-utils"))]
#[derive(Debug)]
pub struct MockBuildContext {
    /// The element ID for this context.
    pub element_id: ElementId,
    /// The parent element ID.
    pub parent_id: Option<ElementId>,
    /// The depth in the tree.
    pub depth: usize,
}

#[cfg(any(test, feature = "test-utils"))]
impl MockBuildContext {
    /// Create a new mock context with the given element ID.
    pub fn new(element_id: ElementId) -> Self {
        Self {
            element_id,
            parent_id: None,
            depth: 0,
        }
    }

    /// Create a mock context with parent information.
    pub fn with_parent(element_id: ElementId, parent_id: ElementId, depth: usize) -> Self {
        Self {
            element_id,
            parent_id: Some(parent_id),
            depth,
        }
    }
}

#[cfg(any(test, feature = "test-utils"))]
impl BuildContext for MockBuildContext {
    fn element_id(&self) -> ElementId {
        self.element_id
    }

    fn parent_id(&self) -> Option<ElementId> {
        self.parent_id
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn mark_dirty(&self) {
        // no-op for mock
    }

    fn schedule_rebuild(&self, _element_id: ElementId) {
        // no-op for mock
    }

    fn create_rebuild_callback(&self) -> Box<dyn Fn() + Send + Sync> {
        // no-op for mock
        Box::new(|| {})
    }

    fn depend_on_raw(&self, _type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }

    fn find_ancestor_widget(&self, _type_id: TypeId) -> Option<Arc<dyn Any + Send + Sync>> {
        None
    }

    fn visit_ancestors(&self, _visitor: &mut dyn FnMut(ElementId) -> bool) {
        // Mock: no ancestors
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_context() {
        let id = ElementId::new(1);
        let ctx = MockBuildContext::new(id);

        assert_eq!(ctx.element_id(), id);
        assert_eq!(ctx.parent_id(), None);
        assert_eq!(ctx.depth(), 0);
    }

    #[test]
    fn test_mock_context_with_parent() {
        let id = ElementId::new(2);
        let parent = ElementId::new(1);
        let ctx = MockBuildContext::with_parent(id, parent, 1);

        assert_eq!(ctx.element_id(), id);
        assert_eq!(ctx.parent_id(), Some(parent));
        assert_eq!(ctx.depth(), 1);
    }

    #[test]
    fn test_downcast() {
        let id = ElementId::new(1);
        let ctx: &dyn BuildContext = &MockBuildContext::new(id);

        let downcasted = ctx.downcast_ref::<MockBuildContext>();
        assert!(downcasted.is_some());
        assert_eq!(downcasted.unwrap().element_id, id);
    }

    #[test]
    fn test_depend_on_returns_none() {
        let ctx = MockBuildContext::new(ElementId::new(1));
        let result = ctx.depend_on::<String>();
        assert!(result.is_none());
    }
}
