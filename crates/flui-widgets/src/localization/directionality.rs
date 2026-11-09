//! [`Directionality`] — the ambient text/layout direction for a subtree.

use flui_foundation::geometry::Axis;
use flui_painting::typography::TextDirection;
use flui_rendering::constraints::AxisDirection;
use flui_view::prelude::*;
use flui_view::{BoxedView, InheritedView, impl_inherited_view};

/// Provides a [`TextDirection`] to its subtree via FLUI's inherited-data
/// mechanism.
///
/// Descendants that need to mirror layout or convert a physical gesture
/// delta into a logical one (e.g. `Navigator`'s edge-swipe back gesture) read
/// the ambient direction with [`Directionality::of`]/[`Directionality::maybe_of`]
/// instead of hard-coding [`TextDirection::Ltr`].
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
    #[must_use]
    pub fn maybe_of(ctx: &dyn BuildContext) -> Option<TextDirection> {
        ctx.depend_on::<Self, _>(|d| d.direction)
    }

    /// The ambient direction for a render view's configuration, read from
    /// `create_render_object`/`update_render_object` through
    /// [`RenderObjectContext::depend_on`](flui_view::RenderObjectContext::depend_on)
    /// so the render object is reconfigured when the direction changes.
    ///
    /// [`TextDirection::Ltr`] with no `Directionality` ancestor, matching
    /// [`maybe_of`](Self::maybe_of)'s callers, and at mount, where no inherited
    /// read is possible yet: the element's first build reconfigures the render
    /// object with the resolved direction before its first layout.
    #[must_use]
    pub(crate) fn of_render_object(ctx: &flui_view::RenderObjectContext<'_>) -> TextDirection {
        ctx.depend_on::<Self, _>(|d| d.direction)
            .unwrap_or(TextDirection::Ltr)
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
/// flag, and the ambient [`Directionality`].
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
/// and the viewports' cross axis share its horizontal arm, the reading-direction
/// rule, from their render views.
#[must_use]
pub fn axis_direction_from_axis_reverse_and_directionality(
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
/// split out so it is unit-testable without a [`BuildContext`] and shared with
/// the render views that read the direction themselves.
#[must_use]
pub(crate) fn resolve_horizontal_axis_direction(
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

/// Resolve `alignment` to a physical [`Alignment`](flui_painting::Alignment):
/// an absolute geometry is already physical and never calls `text_direction`,
/// so a caller that reads the direction through it registers no dependency for
/// an absolute alignment; a directional one mirrors under right-to-left text.
pub(crate) fn resolve_alignment(
    alignment: flui_painting::AlignmentGeometry,
    text_direction: impl FnOnce() -> TextDirection,
) -> flui_painting::Alignment {
    match alignment {
        flui_painting::AlignmentGeometry::Absolute(alignment) => alignment,
        flui_painting::AlignmentGeometry::Directional(directional) => {
            directional.resolve(text_direction().is_ltr())
        }
    }
}
