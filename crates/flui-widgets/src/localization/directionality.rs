//! [`Directionality`] — the ambient text/layout direction for a subtree.
//!
//! Flutter parity: `widgets/directionality.dart` `Directionality`.

use flui_types::typography::TextDirection;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Provides a [`TextDirection`] to its subtree via FLUI's inherited-data
/// mechanism.
///
/// Descendants that need to mirror layout or convert a physical gesture
/// delta into a logical one (e.g. `Navigator`'s edge-swipe back gesture) read
/// the ambient direction with [`Directionality::of`]/[`Directionality::maybe_of`]
/// instead of hard-coding [`TextDirection::Ltr`].
///
/// Flutter parity: `Directionality` (`widgets/directionality.dart`).
#[derive(Clone)]
pub struct Directionality {
    /// The direction this node provides to descendants.
    direction: TextDirection,
    /// The single child subtree this node wraps.
    child: BoxedView,
}

impl Directionality {
    /// Wrap `child` in a `Directionality` that provides `direction` to all
    /// descendants.
    #[must_use]
    pub fn new(direction: TextDirection, child: impl IntoView) -> Self {
        Self {
            direction,
            child: child.into_view().boxed(),
        }
    }

    /// Access the [`TextDirection`] from the nearest ancestor
    /// [`Directionality`], registering a dependency so this element rebuilds
    /// when the direction changes.
    ///
    /// # Panics
    ///
    /// Panics if there is no `Directionality` ancestor. Use
    /// [`maybe_of`](Self::maybe_of) for a non-panicking variant.
    ///
    /// Flutter parity: `Directionality.of(context)`.
    #[must_use]
    pub fn of(ctx: &dyn BuildContext) -> TextDirection {
        Self::maybe_of(ctx).expect(
            "BUG: Directionality::of called with no Directionality ancestor in the tree — \
             wrap the subtree in a Directionality (Localizations does this automatically), \
             or use Directionality::maybe_of with a caller-chosen default",
        )
    }

    /// Look up the nearest ancestor [`Directionality`]'s direction,
    /// registering a dependency. Returns `None` if there is no
    /// `Directionality` ancestor.
    ///
    /// Flutter parity: `Directionality.maybeOf(context)`.
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<TextDirection> {
        ctx.depend_on::<Self, _>(|d| d.direction)
    }
}

impl std::fmt::Debug for Directionality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Directionality")
            .field("direction", &self.direction)
            .finish_non_exhaustive()
    }
}

impl InheritedView for Directionality {
    type Data = TextDirection;

    fn data(&self) -> &Self::Data {
        &self.direction
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        self.direction != old.direction
    }
}

impl_inherited_view!(Directionality);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SizedBox;

    #[test]
    fn directionality_new_wires_direction_and_child() {
        let d = Directionality::new(TextDirection::Rtl, SizedBox::shrink());
        assert_eq!(*d.data(), TextDirection::Rtl);
    }

    #[test]
    fn directionality_create_element_is_inherited_kind() {
        let d = Directionality::new(TextDirection::Ltr, SizedBox::shrink());
        let kind = d.create_element();
        assert!(matches!(
            kind,
            flui_view::element::ElementKind::Inherited(_)
        ));
    }

    #[test]
    fn directionality_update_should_notify_same_direction_is_false() {
        let a = Directionality::new(TextDirection::Ltr, SizedBox::shrink());
        let b = Directionality::new(TextDirection::Ltr, SizedBox::shrink());
        assert!(!a.update_should_notify(&b));
    }

    #[test]
    fn directionality_update_should_notify_different_direction_is_true() {
        let a = Directionality::new(TextDirection::Rtl, SizedBox::shrink());
        let b = Directionality::new(TextDirection::Ltr, SizedBox::shrink());
        assert!(a.update_should_notify(&b));
    }
}
