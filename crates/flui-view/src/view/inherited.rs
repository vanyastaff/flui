//! InheritedView - Views that provide data to descendants.
//!
//! InheritedViews propagate data down the tree with O(1) lookup: each
//! [`ElementNode`](crate::tree::ElementNode) carries an `inherited` map
//! (`provider view TypeId → provider ElementId`) built at mount, so
//! `ctx.depend_on::<T>()` is one hash lookup rather than an O(depth) parent
//! walk. Mirrors Flutter's per-element `_inheritedElements` map.

use super::view::View;

use std::marker::PhantomData;

/// An **untyped** set of provider-data fields — the type-erased form of
/// [`FieldMask<D>`] that element storage and the object-safe context methods
/// carry (issue #1090, ADR-0074 §5.5).
///
/// Application code cannot build one with specific fields (only `NONE` and
/// `ALL`): it passes a typed [`FieldMask<D>`], which the crate lowers once the
/// data type has been checked against the provider. `ALL` is the whole-type dependency every
/// plain [`BuildContextExt::depend_on`] registers.
///
/// [`BuildContextExt::depend_on`]: crate::BuildContextExt::depend_on
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub struct FieldSet(u64);

impl FieldSet {
    /// No field.
    pub const NONE: Self = Self(0);
    /// Every field, including ones a future derive adds.
    pub const ALL: Self = Self(u64::MAX);

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

impl std::ops::BitOr for FieldSet {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl std::ops::BitOrAssign for FieldSet {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// Which fields of the provider data type `D` a dependent read, or a provider
/// update changed — one bit per field (issue #1090, ADR-0074 §5.5).
///
/// The data type is part of the mask's type, so a selector cannot be passed to
/// a provider whose data it does not describe: `#[derive(InheritedData)]` emits
/// `pub const FIELD_<NAME>: FieldMask<Self>`, and
/// [`BuildContextExt::depend_on_field`] takes `FieldMask<T::Data>` for provider
/// `T`. Notification is the intersection: a dependent rebuilds only when a
/// field it read changed.
///
/// A mask of another data type is a compile error (`wants_size(FieldMask::<Size>::bit(0))`
/// compiles; the `Theme` mask below does not):
///
/// ```compile_fail,E0308
/// use flui_view::FieldMask;
///
/// struct Size;
/// struct Theme;
///
/// fn wants_size(_mask: FieldMask<Size>) {}
/// wants_size(FieldMask::<Theme>::bit(0));
/// ```
///
/// ```
/// use flui_view::FieldMask;
///
/// struct Size;
///
/// fn wants_size(_mask: FieldMask<Size>) {}
/// wants_size(FieldMask::<Size>::bit(0));
/// ```
///
/// Nor can a mask be lowered to the untyped [`FieldSet`] outside this crate
/// and passed to another provider through the object-safe context method:
///
/// ```compile_fail,E0624
/// use flui_view::FieldMask;
///
/// struct Theme;
///
/// let _untyped = FieldMask::<Theme>::bit(0).erase();
/// ```
///
/// [`BuildContextExt::depend_on_field`]: crate::BuildContextExt::depend_on_field
pub struct FieldMask<D> {
    set: FieldSet,
    // `fn(D) -> D`: INVARIANT in `D` (a covariant marker would let subtyping
    // coerce e.g. `FieldMask<for<'a> fn(&'a str)>` into
    // `FieldMask<fn(&'static str)>` — two distinct provider data types), and
    // Send/Sync/Copy regardless of `D`.
    data: PhantomData<fn(D) -> D>,
}

impl<D> FieldMask<D> {
    /// No field.
    pub const NONE: Self = Self::from_set(FieldSet::NONE);
    /// Every field, including ones a future derive adds: the whole-type
    /// dependency.
    pub const ALL: Self = Self::from_set(FieldSet::ALL);

    const fn from_set(set: FieldSet) -> Self {
        Self {
            set,
            data: PhantomData,
        }
    }

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
        Self::from_set(FieldSet(1u64 << index))
    }

    /// Both sets.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self::from_set(self.set.union(other.set))
    }

    /// Whether the two sets share at least one field.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.set.intersects(other.set)
    }

    /// Whether no field is set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.set.is_empty()
    }

    /// The raw bits (diagnostics).
    #[must_use]
    pub const fn bits(self) -> u64 {
        self.set.bits()
    }

    /// Lower to the untyped [`FieldSet`] element storage carries, once the
    /// data type has been checked. Crate-private: a public lowering would let
    /// application code hand a selector of one data type to another provider
    /// through the object-safe context method.
    #[must_use]
    pub(crate) const fn erase(self) -> FieldSet {
        self.set
    }
}

// Manual impls: a derive would demand `D: Clone`/`D: PartialEq`/…, but the
// data type is only a marker here.
impl<D> Clone for FieldMask<D> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<D> Copy for FieldMask<D> {}

impl<D> PartialEq for FieldMask<D> {
    fn eq(&self, other: &Self) -> bool {
        self.set == other.set
    }
}

impl<D> Eq for FieldMask<D> {}

impl<D> std::hash::Hash for FieldMask<D> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.set.hash(state);
    }
}

impl<D> std::fmt::Debug for FieldMask<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FieldMask")
            .field(&format_args!("{:#x}", self.set.bits()))
            .finish()
    }
}

impl<D> Default for FieldMask<D> {
    fn default() -> Self {
        Self::NONE
    }
}

impl<D> std::ops::BitOr for FieldMask<D> {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

impl<D> std::ops::BitOrAssign for FieldMask<D> {
    fn bitor_assign(&mut self, rhs: Self) {
        *self = self.union(rhs);
    }
}

/// Provider data that can say **which fields** changed between two values.
///
/// Opt in with `#[derive(InheritedData)]` (every field must be `PartialEq`),
/// which also emits one `pub const FIELD_<NAME>: FieldMask<Self>` per field on the
/// type. An `InheritedView` whose `Data` implements this overrides
/// [`InheritedView::changed_fields`] with `self.data().field_mask_diff(old.data())`
/// so dependents that used [`BuildContextExt::depend_on_field`] are notified
/// per field. Whole-type dependents (`depend_on`) still rebuild on any change.
///
/// [`BuildContextExt::depend_on_field`]: crate::BuildContextExt::depend_on_field
pub trait InheritedData: Clone + 'static {
    /// The fields whose values differ between `self` and `other`.
    fn field_mask_diff(&self, other: &Self) -> FieldMask<Self>;
}

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
pub trait InheritedView: Clone + 'static + Sized {
    /// The data type this InheritedView provides.
    type Data: Clone + 'static;

    /// Get the data to provide to descendants.
    fn data(&self) -> &Self::Data;

    /// Get the child View.
    fn child(&self) -> &dyn View;

    /// Should dependents be notified when this View updates?
    ///
    /// Called when a new InheritedView replaces an old one. It feeds the
    /// **default** [`changed_fields`](Self::changed_fields): `true` means every
    /// field (`FieldMask::ALL`), `false` means none. The notify path reads only
    /// `changed_fields`, so a provider that overrides that method (a
    /// `#[derive(InheritedData)]` data type) never has this one consulted —
    /// keep it consistent (`self.data != old.data`) for readers of the code,
    /// but it is not a second gate.
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
    fn changed_fields(&self, old: &Self) -> FieldMask<Self::Data> {
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
