//! InheritedView - Views that provide data to descendants.
//!
//! InheritedViews propagate data down the tree efficiently using O(1) lookup
//! via a hash table in BuildOwner, rather than O(depth) parent walks.

use super::view::{ElementBase, View};
use crate::element::Lifecycle;
use flui_foundation::ElementId;
use std::any::TypeId;

/// A View that provides data to its descendants.
///
/// InheritedViews allow efficient data propagation down the tree.
/// Descendants can access the data via `ctx.depend_on::<T>()`.
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `InheritedWidget`:
///
/// ```dart
/// class ThemeData extends InheritedWidget {
///   final Color primaryColor;
///
///   ThemeData({required this.primaryColor, required Widget child})
///       : super(child: child);
///
///   @override
///   bool updateShouldNotify(ThemeData old) {
///     return primaryColor != old.primaryColor;
///   }
///
///   static ThemeData of(BuildContext context) {
///     return context.dependOnInheritedWidgetOfExactType<ThemeData>()!;
///   }
/// }
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{InheritedView, BuildContext, IntoView};
///
/// #[derive(Clone)]
/// struct Theme {
///     primary_color: Color,
/// }
///
/// struct ThemeProvider {
///     theme: Theme,
///     child: Box<dyn View>,
/// }
///
/// impl InheritedView for ThemeProvider {
///     type Data = Theme;
///
///     fn data(&self) -> &Self::Data {
///         &self.theme
///     }
///
///     fn child(&self) -> &dyn View {
///         &*self.child
///     }
///
///     fn update_should_notify(&self, old: &Self) -> bool {
///         self.theme.primary_color != old.theme.primary_color
///     }
/// }
///
/// // Usage in a descendant:
/// fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
///     let theme = ctx.depend_on::<ThemeProvider>().unwrap();
///     Container::new().color(theme.primary_color)
/// }
/// ```
pub trait InheritedView: Send + Sync + 'static + Sized {
    /// The data type this InheritedView provides.
    type Data: Clone + Send + Sync + 'static;

    /// Get the data to provide to descendants.
    fn data(&self) -> &Self::Data;

    /// Get the child View.
    fn child(&self) -> &dyn View;

    /// Should dependents be notified when this View updates?
    ///
    /// Called when a new InheritedView replaces an old one.
    /// If this returns `true`, all dependents will be rebuilt.
    fn update_should_notify(&self, old: &Self) -> bool;
}

/// Implement View for an InheritedView type.
///
/// This macro creates the View implementation for an InheritedView type.
///
/// ```rust,ignore
/// impl InheritedView for MyThemeProvider {
///     type Data = Theme;
///     // ...
/// }
/// impl_inherited_view!(MyThemeProvider);
/// ```
#[macro_export]
macro_rules! impl_inherited_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                Box::new($crate::InheritedElement::new(self))
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
    };
}

// ============================================================================
// InheritedElement
// ============================================================================

/// Element for InheritedViews.
///
/// Manages the lifecycle of an InheritedView and tracks dependents.
/// Registers itself in BuildOwner for O(1) lookup.
pub struct InheritedElement<V: InheritedView> {
    /// The current View configuration.
    view: V,
    /// Cached data for dependents.
    data: V::Data,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
    /// Child element.
    child: Option<Box<dyn ElementBase>>,
    /// Whether we need to rebuild.
    dirty: bool,
    /// Elements that depend on this InheritedElement.
    dependents: Vec<ElementId>,
}

impl<V: InheritedView> InheritedElement<V>
where
    V: Clone,
{
    /// Create a new InheritedElement for the given View.
    pub fn new(view: &V) -> Self {
        let data = view.data().clone();
        Self {
            view: view.clone(),
            data,
            lifecycle: Lifecycle::Initial,
            depth: 0,
            child: None,
            dirty: true,
            dependents: Vec::new(),
        }
    }

    /// Get the provided data.
    pub fn data(&self) -> &V::Data {
        &self.data
    }

    /// Register a dependent element.
    pub fn add_dependent(&mut self, element: ElementId) {
        if !self.dependents.contains(&element) {
            self.dependents.push(element);
        }
    }

    /// Remove a dependent element.
    pub fn remove_dependent(&mut self, element: ElementId) {
        self.dependents.retain(|&id| id != element);
    }

    /// Get all dependent elements.
    pub fn dependents(&self) -> &[ElementId] {
        &self.dependents
    }

    /// Get the type ID for looking up this inherited element.
    pub fn inherited_type_id() -> TypeId {
        TypeId::of::<V>()
    }
}

impl<V: InheritedView + Clone> std::fmt::Debug for InheritedElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InheritedElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("dirty", &self.dirty)
            .field("dependents_count", &self.dependents.len())
            .finish_non_exhaustive()
    }
}

impl<V: InheritedView + Clone> ElementBase for InheritedElement<V> {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn update(&mut self, new_view: &dyn View) {
        // Use View::as_any() for safe downcasting
        if let Some(v) = new_view.as_any().downcast_ref::<V>() {
            let old_view = std::mem::replace(&mut self.view, v.clone());

            // Check if dependents should be notified
            if self.view.update_should_notify(&old_view) {
                self.data = self.view.data().clone();
                // Mark all dependents as needing rebuild
                // This is handled by BuildOwner in a full implementation
                self.dirty = true;
            }
        }
    }

    fn mark_needs_build(&mut self) {
        self.dirty = true;
    }

    fn perform_build(&mut self) {
        if !self.dirty || !self.lifecycle.can_build() {
            return;
        }

        // TODO: Build child element
        // In a full implementation, we would:
        // 1. Get BuildContext
        // 2. Create/update child element from view.child()
        self.dirty = false;
    }

    fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {
        self.lifecycle = Lifecycle::Active;
        // TODO: Register with BuildOwner for O(1) lookup
        // owner.register_inherited(TypeId::of::<V>(), self_id);
        self.dirty = true;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        if let Some(child) = &mut self.child {
            child.deactivate();
        }
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        if let Some(child) = &mut self.child {
            child.activate();
        }
    }

    fn unmount(&mut self) {
        self.lifecycle = Lifecycle::Defunct;
        // TODO: Unregister from BuildOwner
        // owner.unregister_inherited(TypeId::of::<V>());
        self.dependents.clear();
        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        // InheritedElement manages its child internally
        let _ = visitor;
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct TestTheme {
        color: u32,
    }

    // A dummy view for the child
    #[derive(Clone)]
    struct DummyView;

    impl View for DummyView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(DummyElement)
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    struct DummyElement;

    impl ElementBase for DummyElement {
        fn view_type_id(&self) -> TypeId {
            TypeId::of::<DummyView>()
        }
        fn lifecycle(&self) -> Lifecycle {
            Lifecycle::Active
        }
        fn update(&mut self, _: &dyn View) {}
        fn mark_needs_build(&mut self) {}
        fn perform_build(&mut self) {}
        fn mount(&mut self, _: Option<ElementId>, _: usize) {}
        fn deactivate(&mut self) {}
        fn activate(&mut self) {}
        fn unmount(&mut self) {}
        fn visit_children(&self, _: &mut dyn FnMut(ElementId)) {}
        fn depth(&self) -> usize {
            0
        }
    }

    /// Test provider that owns its child as a concrete type (Clone-friendly)
    #[derive(Clone)]
    struct TestThemeProvider {
        theme: TestTheme,
        child: DummyView,
    }

    impl InheritedView for TestThemeProvider {
        type Data = TestTheme;

        fn data(&self) -> &Self::Data {
            &self.theme
        }

        fn child(&self) -> &dyn View {
            &self.child
        }

        fn update_should_notify(&self, old: &Self) -> bool {
            self.theme != old.theme
        }
    }

    impl View for TestThemeProvider {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(InheritedElement::new(self))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_inherited_element_creation() {
        let provider = TestThemeProvider {
            theme: TestTheme { color: 0xFF0000 },
            child: DummyView,
        };

        let element = InheritedElement::new(&provider);
        assert_eq!(element.data().color, 0xFF0000);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_inherited_element_dependents() {
        let provider = TestThemeProvider {
            theme: TestTheme { color: 0xFF0000 },
            child: DummyView,
        };

        let mut element = InheritedElement::new(&provider);

        let dep1 = ElementId::new(1);
        let dep2 = ElementId::new(2);

        element.add_dependent(dep1);
        element.add_dependent(dep2);
        assert_eq!(element.dependents().len(), 2);

        // Adding same dependent again should not duplicate
        element.add_dependent(dep1);
        assert_eq!(element.dependents().len(), 2);

        element.remove_dependent(dep1);
        assert_eq!(element.dependents().len(), 1);
        assert_eq!(element.dependents()[0], dep2);
    }

    #[test]
    fn test_inherited_element_update_should_notify() {
        let provider1 = TestThemeProvider {
            theme: TestTheme { color: 0xFF0000 },
            child: DummyView,
        };

        let provider2 = TestThemeProvider {
            theme: TestTheme { color: 0x00FF00 },
            child: DummyView,
        };

        let provider_same = TestThemeProvider {
            theme: TestTheme { color: 0xFF0000 },
            child: DummyView,
        };

        // Different theme should notify
        assert!(provider2.update_should_notify(&provider1));

        // Same theme should not notify
        assert!(!provider_same.update_should_notify(&provider1));
    }
}
