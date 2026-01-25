//! InheritedView - Views that provide data to descendants.
//!
//! InheritedViews propagate data down the tree efficiently using O(1) lookup
//! via a hash table in BuildOwner, rather than O(depth) parent walks.

use super::view::View;

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
pub trait InheritedView: Clone + Send + Sync + 'static + Sized {
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
                use $crate::element::InheritedBehavior;
                Box::new($crate::InheritedElement::new(
                    self,
                    InheritedBehavior::new(self),
                ))
            }
        }
    };
}

// NOTE: InheritedElement implementation has been moved to unified Element architecture.
// See crates/flui-view/src/element/unified.rs and element/behavior.rs
// The type alias is exported from element/mod.rs:
//   pub type InheritedElement<V> = Element<V, Single, InheritedBehavior<V>>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{InheritedBehavior, Lifecycle};
    use crate::view::{ElementBase, View};
    use crate::InheritedElement;
    use flui_foundation::ElementId;
    use std::any::TypeId;

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
            Box::new(InheritedElement::new(self, InheritedBehavior::new(self)))
        }
    }

    #[test]
    fn test_inherited_element_creation() {
        let provider = TestThemeProvider {
            theme: TestTheme { color: 0xFF0000 },
            child: DummyView,
        };

        let element = InheritedElement::new(&provider, InheritedBehavior::new(&provider));
        assert_eq!(element.behavior().data().color, 0xFF0000);
        assert_eq!(element.core().lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_inherited_element_dependents() {
        let provider = TestThemeProvider {
            theme: TestTheme { color: 0xFF0000 },
            child: DummyView,
        };

        let mut element = InheritedElement::new(&provider, InheritedBehavior::new(&provider));

        let dep1 = ElementId::new(1);
        let dep2 = ElementId::new(2);

        element.behavior_mut().add_dependent(dep1);
        element.behavior_mut().add_dependent(dep2);
        assert_eq!(element.behavior().dependents().len(), 2);

        // Adding same dependent again should not duplicate
        element.behavior_mut().add_dependent(dep1);
        assert_eq!(element.behavior().dependents().len(), 2);

        element.behavior_mut().remove_dependent(dep1);
        assert_eq!(element.behavior().dependents().len(), 1);
        assert_eq!(element.behavior().dependents()[0], dep2);
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
