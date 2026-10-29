//! [`AnimatedList`] and [`AnimatedGrid`] — scroll views over a
//! [`SliverAnimatedList`].

use std::fmt;
use std::hash::Hash;

use flui_animation::{MotionSpec, Stagger};
use flui_foundation::geometry::Axis;
use flui_rendering::delegates::SliverGridDelegate;
use flui_rendering::view::ScrollPosition;
use flui_view::prelude::StatelessView;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use super::SliverAnimatedList;
use crate::localization::axis_direction_from_axis_reverse_and_directionality;
use crate::motion::{Effects, Transition};
use crate::scroll::{ShrinkWrappingViewport, Viewport};

/// Where the composed viewport takes its scroll offset from, as
/// [`ListView`](crate::ListView) does.
#[derive(Clone, Debug)]
enum OffsetSource {
    Fixed(f64),
    Position(ScrollPosition),
}

/// The viewport both scroll views compose.
#[derive(Clone, Debug)]
struct Scrolling {
    direction: Axis,
    offset: OffsetSource,
    shrink_wrap: bool,
}

impl Default for Scrolling {
    fn default() -> Self {
        Self {
            direction: Axis::Vertical,
            offset: OffsetSource::Fixed(0.0),
            shrink_wrap: false,
        }
    }
}

impl Scrolling {
    fn wrap(&self, ctx: &dyn BuildContext, sliver: BoxedView) -> BoxedView {
        let axis_direction =
            axis_direction_from_axis_reverse_and_directionality(ctx, self.direction, false);
        if self.shrink_wrap {
            let viewport = ShrinkWrappingViewport::new((sliver,)).axis_direction(axis_direction);
            match &self.offset {
                OffsetSource::Fixed(pixels) => viewport.offset(*pixels),
                OffsetSource::Position(position) => viewport.position(position.clone()),
            }
            .boxed()
        } else {
            let viewport = Viewport::new((sliver,)).axis_direction(axis_direction);
            match &self.offset {
                OffsetSource::Fixed(pixels) => viewport.offset(*pixels),
                OffsetSource::Position(position) => viewport.position(position.clone()),
            }
            .boxed()
        }
    }
}

/// The configuration methods [`AnimatedList`] and [`AnimatedGrid`] share:
/// the sliver's motion and the viewport's scrolling.
macro_rules! scroll_view_methods {
    ($view:ident) => {
        impl<T, K> $view<T, K>
        where
            T: 'static,
            K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
        {
            /// How items enter and leave — see
            /// [`SliverAnimatedList::transition`].
            #[must_use]
            pub fn transition(mut self, transition: Transition) -> Self {
                self.sliver = self.sliver.transition(transition);
                self
            }

            /// How moved items slide into place — see
            /// [`SliverAnimatedList::placement`].
            #[must_use]
            pub fn placement(mut self, placement: Option<MotionSpec>) -> Self {
                self.sliver = self.sliver.placement(placement);
                self
            }

            /// A staggered entrance for the items the view mounts with — see
            /// [`SliverAnimatedList::stagger`].
            #[must_use]
            pub fn stagger(mut self, effects: Effects, stagger: Stagger) -> Self {
                self.sliver = self.sliver.stagger(effects, stagger);
                self
            }

            /// The scroll axis (default [`Axis::Vertical`]). A horizontal
            /// axis follows the ambient [`Directionality`](crate::Directionality).
            #[must_use]
            pub fn scroll_direction(mut self, direction: Axis) -> Self {
                self.scrolling.direction = direction;
                self
            }

            /// A programmatic scroll offset in logical pixels; exclusive with
            /// [`position`](Self::position), the last call wins.
            #[must_use]
            pub fn offset(mut self, offset: f64) -> Self {
                self.scrolling.offset = OffsetSource::Fixed(offset);
                self
            }

            /// A shared scroll position — see [`Viewport::position`];
            /// exclusive with [`offset`](Self::offset), the last call wins.
            #[must_use]
            pub fn position(mut self, position: ScrollPosition) -> Self {
                self.scrolling.offset = OffsetSource::Position(position);
                self
            }

            /// Size to the content along the scroll axis (default `false`),
            /// for a parent with unbounded main-axis constraints.
            #[must_use]
            pub fn shrink_wrap(mut self, shrink_wrap: bool) -> Self {
                self.scrolling.shrink_wrap = shrink_wrap;
                self
            }
        }

        impl<T, K> Clone for $view<T, K> {
            fn clone(&self) -> Self {
                Self {
                    sliver: self.sliver.clone(),
                    scrolling: self.scrolling.clone(),
                }
            }
        }

        impl<T, K: fmt::Debug> fmt::Debug for $view<T, K> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($view))
                    .field("sliver", &self.sliver)
                    .field("scrolling", &self.scrolling)
                    .finish()
            }
        }

        impl<T, K> StatelessView for $view<T, K>
        where
            T: 'static,
            K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
        {
            fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
                self.scrolling.wrap(ctx, self.sliver.clone().boxed())
            }
        }
    };
}

/// A scrollable list over keyed items that animates every change to them:
/// a [`SliverAnimatedList`] in a viewport, as [`ListView`](crate::ListView)
/// is a lazy sliver list in one.
///
/// Rebuild it with new items; insertions enter, removals leave from where
/// they were, a removal undone mid-exit turns back with its state, and moved
/// items slide to their new places. See [`SliverAnimatedList`] for the whole
/// contract.
///
/// ```
/// use flui_widgets::prelude::*;
/// use flui_widgets::AnimatedList;
///
/// let names = vec!["ada", "grace", "barbara"];
/// let list = AnimatedList::new(names, |name| *name, |name| Text::new(*name));
/// # let _ = list;
/// ```
#[derive(StatelessView)]
pub struct AnimatedList<T, K> {
    sliver: SliverAnimatedList<T, K>,
    scrolling: Scrolling,
}

impl<T, K> AnimatedList<T, K>
where
    T: 'static,
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    /// A list of `items`, each keyed by `key` and shown by `builder`.
    ///
    /// # Panics
    ///
    /// When two items share a key, with a `BUG:` message naming it.
    pub fn new<V: IntoView>(
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        builder: impl Fn(&T) -> V + 'static,
    ) -> Self {
        Self {
            sliver: SliverAnimatedList::new(items, key, builder),
            scrolling: Scrolling::default(),
        }
    }

    /// The extent assumed for an item not yet measured — see
    /// [`SliverAnimatedList::item_extent_estimate`].
    #[must_use]
    pub fn item_extent_estimate(mut self, estimate: f64) -> Self {
        self.sliver = self.sliver.item_extent_estimate(estimate);
        self
    }
}

scroll_view_methods!(AnimatedList);

/// A scrollable grid over keyed items that animates every change to them:
/// tiles enter, leave and slide between cells exactly as the items of an
/// [`AnimatedList`] do, laid out by a [`SliverGridDelegate`] as
/// [`GridView::builder`](crate::GridView::builder) lays its tiles out.
///
/// ```
/// use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
/// use flui_widgets::prelude::*;
/// use flui_widgets::AnimatedGrid;
///
/// let grid = AnimatedGrid::new(
///     SliverGridDelegateWithFixedCrossAxisCount::new(3),
///     1..=9_u32,
///     |n| *n,
///     |n| Text::new(n.to_string()),
/// );
/// # let _ = grid;
/// ```
#[derive(StatelessView)]
pub struct AnimatedGrid<T, K> {
    sliver: SliverAnimatedList<T, K>,
    scrolling: Scrolling,
}

impl<T, K> AnimatedGrid<T, K>
where
    T: 'static,
    K: Clone + Eq + Hash + fmt::Debug + Send + Sync + 'static,
{
    /// A grid of `items` laid out by `grid_delegate`, each keyed by `key`
    /// and shown by `builder`.
    ///
    /// # Panics
    ///
    /// When two items share a key, with a `BUG:` message naming it.
    pub fn new<V: IntoView>(
        grid_delegate: impl SliverGridDelegate + 'static,
        items: impl IntoIterator<Item = T>,
        key: impl Fn(&T) -> K,
        builder: impl Fn(&T) -> V + 'static,
    ) -> Self {
        Self {
            sliver: SliverAnimatedList::grid(grid_delegate, items, key, builder),
            scrolling: Scrolling::default(),
        }
    }
}

scroll_view_methods!(AnimatedGrid);
