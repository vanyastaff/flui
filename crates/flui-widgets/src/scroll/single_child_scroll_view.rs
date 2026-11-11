//! [`SingleChildScrollView`] — makes a single child scrollable along one axis.

use std::rc::Rc;

use flui_rendering::view::ScrollPosition;
use flui_view::prelude::StatelessView;
use flui_view::{BuildContext, Child, IntoView, ViewExt};

use crate::localization::axis_direction_from_axis_reverse_and_directionality;
use crate::scroll::{ScrollView, ScrollViewConfig, Scrollable, SliverToBoxAdapter, Viewport};

/// A box that lets its single child be larger than the available space along
/// its scroll axis, and scrolls it.
///
/// Composes a [`Scrollable`] over a [`Viewport`] over a
/// [`SliverToBoxAdapter`]: the child is laid out unbounded on the scroll axis
/// and the viewport clips the overflow. Drag, fling, wheel, keys and
/// assistive scroll actions move it; its controller, axis, `reverse` and
/// gesture policy are the [`ScrollView`] builders.
///
/// `AxisDirection` is resolved as follows: a horizontal axis reads the
/// ambient [`Directionality`](crate::Directionality) (`RightToLeft` under an
/// RTL ancestor, defaulting to `LeftToRight` with no ancestor), and
/// [`ScrollView::reverse`] flips whichever base direction that resolves to —
/// including the vertical axis, which never consults `Directionality` itself.
///
/// # Example
///
/// ```rust
/// use flui_widgets::{ScrollController, ScrollView, SingleChildScrollView, SizedBox};
///
/// let controller = ScrollController::new();
/// let view = SingleChildScrollView::new()
///     .controller(controller)
///     .child(SizedBox::new(300.0, 1400.0));
/// ```
#[derive(Clone, Debug, Default, StatelessView)]
pub struct SingleChildScrollView {
    config: ScrollViewConfig,
    child: Child,
}

impl SingleChildScrollView {
    /// A vertical scroll view with no child yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the scrollable child.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }
}

impl ScrollView for SingleChildScrollView {
    fn scroll_config_mut(&mut self) -> &mut ScrollViewConfig {
        &mut self.config
    }
}

impl StatelessView for SingleChildScrollView {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = axis_direction_from_axis_reverse_and_directionality(
            ctx,
            self.config.scroll_direction,
            self.config.reverse,
        );
        let child = self.child.clone();
        Scrollable::with_config(
            self.config.clone(),
            Rc::new(move |position: ScrollPosition| {
                let adapter = match child.clone().into_inner() {
                    Some(boxed) => SliverToBoxAdapter::new().child(boxed),
                    None => SliverToBoxAdapter::new(),
                };
                Viewport::new(position, (adapter,))
                    .axis_direction(axis_direction)
                    .boxed()
            }),
        )
    }
}
