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

/// Resolve an [`AlignmentGeometry`](flui_types::layout::AlignmentGeometry) against
/// the ambient [`Directionality`].
///
/// The one seam at which a directional alignment becomes a physical one. Call
/// it inside a `build`, where a [`BuildContext`] exists, and hand the result to
/// a render-object widget — [`Align`](crate::Align), `OverflowBox`, a
/// [`Stack`](crate::Stack) — which stays physical and knows nothing about
/// reading direction.
///
/// # Why the caller resolves, rather than the widget
///
/// A `RenderView` cannot read `Directionality` at all in this tree, and the
/// reason is worth knowing before reaching for a wrapper: `RenderObjectContext`
/// carries no inherited-dependency access, and — the part that bites — a render
/// element that IS marked dirty never re-pushes its configuration.
/// `RenderBehavior::build_into_views` clears the dirty flag without calling
/// `update_render_object`; the only invocation is on a parent-driven view swap.
/// So a dependency registered on a render element would mark it dirty, rebuild
/// it, and change nothing: an RTL flip would silently do nothing, with no panic
/// and no failing test. FLUI has no equivalent of Flutter's
/// `RenderObjectElement.performRebuild() -> updateRenderObject`.
///
/// [`Flex`](crate::Flex) and [`ListBody`](crate::ListBody) answer this by being
/// public `StatelessView`s over private render views. This function applies the
/// same rule one level up, so a caller who wants a directional alignment pays
/// for it and a caller who does not pays nothing — no widget layer is added to
/// the 60-odd `Align::new` sites, nor inside every aligned
/// [`Container`](crate::Container).
///
/// # An absolute alignment registers no dependency
///
/// By construction rather than by a conditional that could drift: only the
/// `Directional` arm reaches [`Directionality::maybe_of`], so an absolute
/// alignment never becomes a dependent and a direction change never rebuilds
/// it. Same shape, and same reason, as
/// `axis_direction_from_axis_reverse_and_directionality`'s vertical arm below.
///
/// This matters more than it looks: `InheritedDependencies` has no per-rebuild
/// clear, so a registration is monotonic — a widget that depends once keeps
/// depending until it deactivates. A widget that never registers is the only
/// widget that is reliably not a dependent.
///
/// **Not covered by a test, deliberately.** The dependency registry is
/// `pub(crate)` to `flui-view`, and the widget harness cannot separate "did not
/// depend" from "did not rebuild for another reason": swapping the root to flip
/// the direction replaces the child's view as well, so both an absolute and a
/// directional caller rebuild, and an assertion either way would pass for the
/// wrong reason. The property is held by the shape of the `match` — there is no
/// path from the `Absolute` arm to [`Directionality::maybe_of`] — and hoisting
/// that call above the match is the way to break it. A reviewer, not a test, is
/// what catches that today.
///
/// With no [`Directionality`] ancestor the direction defaults to
/// [`TextDirection::Ltr`], matching every other FLUI widget that reads one.
#[must_use]
pub fn resolve_alignment(
    ctx: &dyn BuildContext,
    alignment: impl Into<flui_types::layout::AlignmentGeometry>,
) -> flui_types::Alignment {
    match alignment.into() {
        flui_types::layout::AlignmentGeometry::Absolute(alignment) => alignment,
        flui_types::layout::AlignmentGeometry::Directional(directional) => {
            let text_direction = Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr);
            directional.resolve(text_direction.is_ltr())
        }
    }
}

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
    use flui_types::Alignment;
    use std::cell::Cell;
    use std::rc::Rc;

    /// A directional alignment resolves to opposite edges under the two
    /// directions, through a real mounted `Directionality`.
    ///
    /// The whole point of the seam: `AlignmentGeometry::resolve(is_ltr)` was
    /// already correct and already tested, and the parity corpus called it at
    /// the call site with a literal `false` because "no widget-surface path
    /// reads one" (`tests/parity/align_test.rs`). This is that path.
    #[test]
    fn a_directional_alignment_resolves_against_a_mounted_directionality() {
        use flui_types::layout::AlignmentDirectional;

        #[derive(Clone, StatelessView)]
        struct Probe {
            seen: Rc<Cell<Option<Alignment>>>,
        }

        impl StatelessView for Probe {
            fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
                self.seen.set(Some(resolve_alignment(
                    ctx,
                    AlignmentDirectional::new(-1.0, 0.0),
                )));
                SizedBox::shrink()
            }
        }

        for (direction, expected_x) in [(TextDirection::Ltr, -1.0), (TextDirection::Rtl, 1.0)] {
            let seen = Rc::new(Cell::new(None));
            let _harness = crate::test_harness::mount(Directionality::new(
                direction,
                Probe {
                    seen: Rc::clone(&seen),
                },
            ));
            let resolved = seen.get().expect("the probe must have built");
            assert!(
                (resolved.x - expected_x).abs() < f32::EPSILON,
                "start is the {direction:?} reading edge, so x must be \
                 {expected_x}, got {}",
                resolved.x
            );
        }
    }

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
