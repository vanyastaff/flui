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
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not implement `InheritedView`",
    label = "missing `impl InheritedView for {Self}`",
    note = "an inherited view publishes `type Data` to its subtree: `impl InheritedView for {Self} {{ type Data = ..; fn data(&self) -> &Self::Data {{ .. }} fn child(&self) -> &dyn View {{ .. }} fn update_should_notify(&self, old: &Self) -> bool {{ .. }} }}`; descendants read it with `ctx.depend_on::<{Self}>()`"
)]
/// Which fields of a provider's data a dependent read, or a provider update
/// changed — a 64-bit set, one bit per field (issue #1090, ADR-0008 §2).
///
/// `ALL` is the whole-type dependency every plain [`BuildContextExt::depend_on`]
/// registers (vanilla `InheritedWidget` parity); a `#[derive(InheritedData)]`
/// data type exposes one `FIELD_*` constant per field and computes the changed
/// set on update. Notification is the intersection: a dependent rebuilds only
/// when a field it read changed.
///
/// [`BuildContextExt::depend_on`]: crate::BuildContextExt::depend_on
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct FieldMask(u64);

impl FieldMask {
    /// No field.
    pub const NONE: Self = Self(0);
    /// Every field, including ones a future derive adds: the whole-type
    /// dependency.
    pub const ALL: Self = Self(u64::MAX);

    /// The mask of field number `index` (0-based, in declaration order).
    ///
    /// # Panics
    ///
    /// At compile time (in a `const`) or at run time if `index >= 64`; the
    /// derive refuses structs with more fields than the mask carries.
    #[must_use]
    pub const fn bit(index: u32) -> Self {
        assert!(
            index < 64,
            "FieldMask carries 64 fields; the derive refuses larger structs"
        );
        Self(1u64 << index)
    }

    /// Both sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether the two sets share at least one field.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Whether no field is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The raw bits (diagnostics).
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.0
    }
}

impl std::ops::BitOr for FieldMask {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for FieldMask {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// Provider data that can say **which fields** changed between two values.
///
/// Opt in with `#[derive(InheritedData)]` (every field must be `PartialEq`),
/// which also emits one `pub const FIELD_<NAME>: FieldMask` per field on the
/// type. An `InheritedView` whose `Data` implements this overrides
/// [`InheritedView::changed_fields`] with `self.data().field_mask_diff(old.data())`
/// so dependents that used [`BuildContextExt::depend_on_field`] are notified
/// per field. Whole-type dependents (`depend_on`) still rebuild on any change.
///
/// [`BuildContextExt::depend_on_field`]: crate::BuildContextExt::depend_on_field
pub trait InheritedData: Clone + 'static {
    /// The fields whose values differ between `self` and `other`.
    fn field_mask_diff(&self, other: &Self) -> FieldMask;
}

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

    /// Which fields changed since `old` — the field-granular form of
    /// [`update_should_notify`](Self::update_should_notify).
    ///
    /// Default: [`FieldMask::ALL`] when `update_should_notify` is true and
    /// [`FieldMask::NONE`] otherwise, so a provider that never opted in keeps
    /// vanilla `InheritedWidget` behaviour. Providers whose `Data:
    /// InheritedData` override this with `self.data().field_mask_diff(old.data())`.
    /// The notify path uses only this method; `update_should_notify` is its
    /// whole-type default, never a second gate.
    fn changed_fields(&self, old: &Self) -> FieldMask {
        if self.update_should_notify(old) {
            FieldMask::ALL
        } else {
            FieldMask::NONE
        }
    }
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
        ) -> flui_rendering::RenderUpdateImpact {
            flui_rendering::RenderUpdateImpact::NONE
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

        element
            .behavior_mut()
            .add_dependent(dep1, 3, FieldMask::ALL);
        element
            .behavior_mut()
            .add_dependent(dep2, 4, FieldMask::ALL);
        assert_eq!(element.behavior().dependents().len(), 2);
        assert_eq!(element.behavior().dependents().get(&dep1), Some(&3));
        assert_eq!(element.behavior().dependents().get(&dep2), Some(&4));

        // Adding same dependent again should overwrite depth (idempotent
        // dedup via HashMap key) — not duplicate.
        element
            .behavior_mut()
            .add_dependent(dep1, 5, FieldMask::ALL);
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
