//! [`Directionality`] — the ambient text/layout direction for a subtree.
//!
//! Flutter parity: `widgets/directionality.dart` `Directionality`.

use flui_types::layout::{Axis, AxisDirection};
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

/// Resolves an [`AxisDirection`] from a layout/scroll `axis`, its `reverse`
/// flag, and the ambient [`Directionality`] — Flutter's
/// `getAxisDirectionFromAxisReverseAndDirectionality` (`widgets/basic.dart`).
/// Only `Axis::Horizontal` consults `Directionality` (defaulting to `Ltr`
/// with no ancestor, matching every other FLUI widget that reads it) — a
/// vertical caller never registers a dependency on it, so a `Directionality`
/// change never forces an unrelated rebuild. `Axis::Vertical` picks
/// `TopToBottom`/`BottomToTop` from `reverse` alone. `reverse` flips either
/// axis's base direction to its opposite.
///
/// Shared by every FLUI scroll view that needs this exact rule:
/// [`CustomScrollView`](crate::CustomScrollView), [`GridView`](crate::GridView),
/// [`ListView`](crate::ListView), [`PageView`](crate::PageView), and
/// [`SingleChildScrollView`](crate::SingleChildScrollView). [`ListBody`](crate::ListBody)
/// implements the identical rule inline (its own `resolve_axis_direction`)
/// rather than depending on this function, since it resolves through a
/// private render-object widget this module has no reason to know about.
#[must_use]
pub(crate) fn axis_direction_from_axis_reverse_and_directionality(
    ctx: &dyn BuildContext,
    axis: Axis,
    reverse: bool,
) -> AxisDirection {
    match axis {
        Axis::Horizontal => {
            let text_direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
            resolve_horizontal_axis_direction(text_direction, reverse)
        }
        Axis::Vertical => AxisDirection::from_axis(Axis::Vertical, reverse),
    }
}

/// The pure `text_direction` + `reverse` -> `AxisDirection` rule for the
/// horizontal branch of [`axis_direction_from_axis_reverse_and_directionality`],
/// split out so it is unit-testable without a [`BuildContext`].
#[must_use]
fn resolve_horizontal_axis_direction(
    text_direction: TextDirection,
    reverse: bool,
) -> AxisDirection {
    let base = if text_direction.is_rtl() {
        AxisDirection::RightToLeft
    } else {
        AxisDirection::LeftToRight
    };
    if reverse { base.opposite() } else { base }
}

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

    // ------------------------------------------------------------------
    // `resolve_horizontal_axis_direction` — the pure half of
    // `axis_direction_from_axis_reverse_and_directionality`'s Flutter parity
    // rule. Every FLUI scroll view sharing this helper relies on these four
    // combinations resolving correctly, including the `reverse` flip on both
    // the LTR and RTL base cases.
    // ------------------------------------------------------------------

    #[test]
    fn resolve_horizontal_axis_direction_ltr_is_left_to_right() {
        assert_eq!(
            resolve_horizontal_axis_direction(TextDirection::Ltr, false),
            AxisDirection::LeftToRight
        );
    }

    #[test]
    fn resolve_horizontal_axis_direction_rtl_is_right_to_left() {
        assert_eq!(
            resolve_horizontal_axis_direction(TextDirection::Rtl, false),
            AxisDirection::RightToLeft,
            "a horizontal axis under RTL Directionality must resolve to RightToLeft"
        );
    }

    #[test]
    fn resolve_horizontal_axis_direction_ltr_reverse_is_right_to_left() {
        assert_eq!(
            resolve_horizontal_axis_direction(TextDirection::Ltr, true),
            AxisDirection::RightToLeft
        );
    }

    /// `reverse` flips the RTL base direction too: RTL + reverse ends up back
    /// at `LeftToRight`, matching `flipAxisDirection(AxisDirection.left)`.
    #[test]
    fn resolve_horizontal_axis_direction_rtl_reverse_is_left_to_right() {
        assert_eq!(
            resolve_horizontal_axis_direction(TextDirection::Rtl, true),
            AxisDirection::LeftToRight
        );
    }

    /// The vertical axis never consults `text_direction` — mirrors the
    /// oracle's `Axis.vertical` arm, which never touches `Directionality` at
    /// all. `axis_direction_from_axis_reverse_and_directionality`'s vertical
    /// branch delegates straight to `AxisDirection::from_axis`, already
    /// covered at the unit level in `crates/flui-types/src/layout/axis.rs`;
    /// these two pin the specific `Axis::Vertical` outputs this module's
    /// callers rely on.
    #[test]
    fn axis_direction_from_axis_reverse_and_directionality_vertical_false_is_top_to_bottom() {
        assert_eq!(
            AxisDirection::from_axis(Axis::Vertical, false),
            AxisDirection::TopToBottom
        );
    }

    #[test]
    fn axis_direction_from_axis_reverse_and_directionality_vertical_reverse_is_bottom_to_top() {
        assert_eq!(
            AxisDirection::from_axis(Axis::Vertical, true),
            AxisDirection::BottomToTop
        );
    }
}
