//! InheritedView - Views that provide data to descendants.
//!
//! InheritedViews propagate data down the tree with O(1) lookup: each
//! [`ElementNode`](crate::tree::ElementNode) carries an `inherited` map
//! (`provider view TypeId → provider ElementId`) built at mount, so
//! `ctx.depend_on::<T>()` is one hash lookup rather than an O(depth) parent
//! walk. Mirrors Flutter's per-element `_inheritedElements` map.

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
pub trait InheritedView: Clone + 'static + Sized {
    /// The data type this InheritedView provides.
    type Data: Clone + 'static;

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
            fn create_element(&self) -> $crate::element::ElementKind {
                $crate::element::ElementKind::inherited(self)
            }
        }
    };
}

// NOTE: InheritedElement implementation has been moved to unified Element
// architecture. See crates/flui-view/src/element/unified.rs and
// element/behavior.rs The type alias is exported from element/mod.rs:
//   pub type InheritedElement<V> = Element<V, Single, InheritedBehavior<V>>;

#[cfg(test)]
mod tests {
    use flui_objects::RenderSizedBox;
    use flui_rendering::protocol::BoxProtocol;

    use flui_foundation::ElementId;

    use super::*;
    use crate::{
        InheritedElement,
        element::{InheritedBehavior, Lifecycle},
        view::View,
    };

    #[derive(Clone, Debug, PartialEq)]
    struct TestTheme {
        color: u32,
    }

    // A dummy view for the child
    #[derive(Clone)]
    struct DummyView;

    impl crate::RenderView for DummyView {
        type Protocol = BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::shrink()
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) {
        }
    }

    impl View for DummyView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
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
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::inherited(self)
        }
    }

    #[test]
    fn test_inherited_element_creation() {
        let provider = TestThemeProvider {
            theme: TestTheme { color: 0x00FF_0000 },
            child: DummyView,
        };

        let element = InheritedElement::new(&provider, InheritedBehavior::new(&provider));
        assert_eq!(element.behavior().data().color, 0x00FF_0000);
        assert_eq!(element.core().lifecycle(), Lifecycle::Initial);
    }

    #[test]
    fn test_inherited_element_dependents() {
        let provider = TestThemeProvider {
            theme: TestTheme { color: 0x00FF_0000 },
            child: DummyView,
        };

        let mut element = InheritedElement::new(&provider, InheritedBehavior::new(&provider));

        let dep1 = ElementId::new(1);
        let dep2 = ElementId::new(2);

        element.behavior_mut().add_dependent(dep1, 3);
        element.behavior_mut().add_dependent(dep2, 4);
        assert_eq!(element.behavior().dependents().len(), 2);
        assert_eq!(element.behavior().dependents().get(&dep1), Some(&3));
        assert_eq!(element.behavior().dependents().get(&dep2), Some(&4));

        // Adding same dependent again should overwrite depth (idempotent
        // dedup via HashMap key) — not duplicate.
        element.behavior_mut().add_dependent(dep1, 5);
        assert_eq!(element.behavior().dependents().len(), 2);
        assert_eq!(element.behavior().dependents().get(&dep1), Some(&5));

        element.behavior_mut().remove_dependent(dep1);
        assert_eq!(element.behavior().dependents().len(), 1);
        assert!(element.behavior().dependents().contains_key(&dep2));
    }

    #[test]
    fn test_inherited_element_update_should_notify() {
        let provider1 = TestThemeProvider {
            theme: TestTheme { color: 0x00FF_0000 },
            child: DummyView,
        };

        let provider2 = TestThemeProvider {
            theme: TestTheme { color: 0x0000_FF00 },
            child: DummyView,
        };

        let provider_same = TestThemeProvider {
            theme: TestTheme { color: 0x00FF_0000 },
            child: DummyView,
        };

        // Different theme should notify
        assert!(provider2.update_should_notify(&provider1));

        // Same theme should not notify
        assert!(!provider_same.update_should_notify(&provider1));
    }
}
