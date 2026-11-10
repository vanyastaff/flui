//! [`ListView`] — a scrollable list of fixed-height items (static or lazy-built).

use std::fmt;
use std::rc::Rc;

use flui_rendering::view::ScrollPosition;
use flui_view::element::StaticChildren;
use flui_view::prelude::StatelessView;
use flui_view::seq::ViewSeq;
use flui_view::{BoxedView, BuildContext, IntoView, ViewExt};

use crate::localization::axis_direction_from_axis_reverse_and_directionality;
use crate::scroll::list_focus::{FocusableList, ListFocusConfig, ListItemFocus};
use crate::scroll::{
    ScrollView, ScrollViewConfig, Scrollable, ShrinkWrappingViewport, SliverChildBuilderDelegate,
    SliverFixedExtentList, SliverList, Viewport,
};

/// A scrollable list that lays out its children sequentially along its
/// scroll axis.
///
/// The list scrolls itself: drag, fling, wheel, keys and assistive scroll
/// actions move it, through the [`Scrollable`] it composes. Its controller,
/// axis, `reverse` and gesture policy are the [`ScrollView`] builders.
///
/// Two construction modes:
///
/// - **Static** ([`ListView::new`]): all children provided up front as a
///   `ViewSeq`, each at a fixed `item_extent`. Backed by
///   [`SliverFixedExtentList`] — cheap to lay out.
///
/// - **Lazy** ([`ListView::builder`]): children built on demand from a
///   closure, only for the viewport-visible + cache band. Backed by
///   [`SliverList`] (variable-height, element-owned). Wired into both
///   `HeadlessBinding::pump_frame` and the production `UiRuntime::draw_frame`,
///   where the child-manager wiring and its test coverage converge.
///
///   **First-frame settling:** lazy children are built
///   *after* the frame's paint, so the first frame a viewport band appears it
///   paints blank; content lands on the next frame (~16 ms @ 60 fps). The
///   settling frame is automatically scheduled because layout marks the sliver
///   dirty. See [`SliverChildBuilderDelegate`] for the full rationale.
///
/// Both modes compose a [`Viewport`] over their respective sliver, or a
/// [`ShrinkWrappingViewport`] when [`ListView::shrink_wrap`] is enabled. A
/// [`ScrollController`](crate::ScrollController) reads and moves the offset;
/// [`ScrollController::with_initial_scroll_offset`](crate::ScrollController::with_initial_scroll_offset)
/// starts it elsewhere than `0.0`.
///
/// A horizontal axis resolves its `AxisDirection` from the ambient
/// [`Directionality`](crate::Directionality) (`RightToLeft` under an RTL
/// ancestor); the vertical axis never consults it. [`ScrollView::reverse`]
/// flips whichever direction that resolves to.
#[derive(Clone, StatelessView)]
pub struct ListView {
    /// How the list scrolls (see [`ScrollView`]).
    config: ScrollViewConfig,
    /// Per-item extent for the static variant ([`ListView::new`]).
    item_extent: f64,
    /// Per-item extent estimate for the lazy variant ([`ListView::builder`]).
    /// Seeds the virtualizer until real measurements arrive.
    item_extent_estimate: f64,
    shrink_wrap: bool,
    /// Children for the static variant. Empty in the lazy variant.
    /// The static children as handed in — a shared delegate, so a `ListView`
    /// cloned or rebuilt from the same value compares as unchanged and its
    /// resident children are not rebuilt (ADR-0053).
    children: Rc<StaticChildren>,
    /// `children` with the per-item repaint boundary applied, created on
    /// first use so it too keeps its identity across builds of one value.
    children_in_boundaries: std::cell::OnceCell<Rc<StaticChildren>>,
    /// Builder delegate for the lazy variant. `None` in the static variant.
    builder_source: Option<SliverChildBuilderDelegate>,
    /// Wrap each item in a `RepaintBoundary` (default `true`, because children in
    /// a scrolling container do not need to be repainted as the list scrolls).
    /// Set `false` when an item is cheaper to repaint than to composite.
    add_repaint_boundaries: bool,
    /// One Tab stop with arrow-key focus over the rows (`ListView::focusable`).
    focusable: bool,
    /// The list's accessible name.
    semantics_label: Option<std::sync::Arc<str>>,
    /// The accessible name of an empty list.
    empty_label: Option<std::sync::Arc<str>>,
    /// Slides moved items into their new places (a reorderable list's
    /// rows); only the lazy variant honours it.
    placement: Option<flui_objects::Placement>,
}

impl ListView {
    /// A vertical list whose rows are each `item_extent` tall.
    ///
    /// All children are given upfront and laid out at the fixed `item_extent`.
    /// Prefer this constructor when the item count and content are known ahead
    /// of time; use [`ListView::builder`] for large or dynamically-generated
    /// lists.
    pub fn new(item_extent: f64, children: impl ViewSeq) -> Self {
        Self {
            config: ScrollViewConfig::default(),
            item_extent,
            item_extent_estimate: item_extent,
            shrink_wrap: false,
            children: StaticChildren::new(children.into_boxed_vec()),
            children_in_boundaries: std::cell::OnceCell::new(),
            builder_source: None,
            add_repaint_boundaries: true,
            focusable: false,
            semantics_label: None,
            empty_label: None,
            placement: None,
        }
    }

    /// A vertical list that lazily builds its items from `builder`.
    ///
    /// Only the children currently visible in the viewport (plus a cache
    /// margin) are built; items that scroll out of the cache window are
    /// disposed. `item_extent_estimate` seeds the virtualizer until real
    /// measurements arrive from laid-out children.
    ///
    /// The `builder` closure receives a logical index and returns the item
    /// view, or `None` when the index is at or past the end of the data
    /// source.
    ///
    /// # Panics
    ///
    /// Panics if `item_extent_estimate` is not finite and positive.
    pub fn builder<F>(item_count: usize, item_extent_estimate: f64, builder: F) -> Self
    where
        F: Fn(usize) -> Option<BoxedView> + 'static,
    {
        Self {
            config: ScrollViewConfig::default(),
            item_extent: item_extent_estimate,
            item_extent_estimate,
            shrink_wrap: false,
            children: StaticChildren::new(Vec::new()),
            children_in_boundaries: std::cell::OnceCell::new(),
            builder_source: Some(SliverChildBuilderDelegate::new(item_count, builder)),
            add_repaint_boundaries: true,
            focusable: false,
            semantics_label: None,
            empty_label: None,
            placement: None,
        }
    }

    /// Wrap each item in a `RepaintBoundary` (default `true`).
    ///
    /// On by default because children in a scrolling container do not need to be
    /// repainted as the list scrolls. Pass `false` when a item is cheaper to repaint than to composite.
    #[must_use]
    pub fn repaint_boundaries(mut self, add: bool) -> Self {
        self.add_repaint_boundaries = add;
        self
    }

    /// Slide moved items into their new places along `placement`.
    pub(crate) fn placement(mut self, placement: flui_objects::Placement) -> Self {
        self.placement = Some(placement);
        self
    }

    /// Map an item's key to its current index in the data source, so a keyed
    /// item whose data moved out of the built band keeps its element state
    /// across insert, remove, and reorder.
    /// Items moving *within* the band are matched by key without it.
    ///
    /// Only meaningful for [`ListView::builder`]; the key handed to `find`
    /// is the item's own key (never the per-item repaint boundary's).
    #[must_use]
    pub fn find_index_by_key(
        mut self,
        find: impl Fn(&dyn flui_foundation::ViewKey) -> Option<usize> + 'static,
    ) -> Self {
        if let Some(delegate) = &mut self.builder_source {
            delegate.find_index_by_key = Some(Rc::new(find));
        }
        self
    }

    /// Whether the list should size itself to its sliver contents in the scroll
    /// axis.
    ///
    /// Defaults to `false`. Use `true` when the parent gives
    /// unbounded main-axis constraints.
    #[must_use]
    pub fn shrink_wrap(mut self, shrink_wrap: bool) -> Self {
        self.shrink_wrap = shrink_wrap;
        self
    }

    /// Make the list one Tab stop with keyboard focus over its rows (default
    /// `false`).
    ///
    /// Tab enters the list on the row it last focused, or on the first visible
    /// one, and leaves it for the next control. Inside it, the arrow keys
    /// along `scroll_direction` move between rows (at the first or last row the
    /// arrow stays in the list; a list inside another focusable list hands it
    /// on to that list), Page Up and Page Down move
    /// to the edge of the visible rows and then a page past it, and Home and
    /// End go to the first and last row. A row's focus goes to its first
    /// focusable descendant, so the row's button activates and shows its ring
    /// as on a click. A row not built yet is scrolled to and focused when it
    /// mounts.
    ///
    /// The focused row stays built and announced while the pointer scrolls it
    /// away (ADR-0265), and an arrow key moves from it.
    #[must_use]
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// The list's accessible name, announced when focus enters it.
    #[must_use]
    pub fn semantics_label(mut self, label: impl Into<std::sync::Arc<str>>) -> Self {
        self.semantics_label = Some(label.into());
        self
    }

    /// The accessible name of the list while it has no rows. A focusable empty
    /// list stays a Tab stop and announces this; without it the list
    /// announces [`semantics_label`](Self::semantics_label).
    #[must_use]
    pub fn empty_label(mut self, label: impl Into<std::sync::Arc<str>>) -> Self {
        self.empty_label = Some(label.into());
        self
    }

    /// The name the list's collection node carries: the empty label while the
    /// list has no rows, when one is set.
    fn collection_label(&self) -> Option<std::sync::Arc<str>> {
        let empty = self.item_count() == 0;
        empty
            .then(|| self.empty_label.clone())
            .flatten()
            .or_else(|| self.semantics_label.clone())
    }

    /// The number of rows the list has.
    fn item_count(&self) -> usize {
        self.builder_source
            .as_ref()
            .map_or(self.children.len(), |delegate| delegate.item_count)
    }
}

/// One row as the list hands it to its sliver: in a focusable list, inside
/// its focus marker, which carries the row's key; then inside its repaint
/// boundary.
fn wrap_row(index: usize, row: BoxedView, focusable: bool, boundary: bool) -> BoxedView {
    match (focusable, boundary) {
        (false, false) => row,
        (false, true) => super::sliver_list::wrap_in_repaint_boundary(row),
        (true, boundary) => BoxedView::new(ListItemFocus::new(index, row, boundary)),
    }
}

impl fmt::Debug for ListView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("ListView");
        s.field("config", &self.config)
            .field("shrink_wrap", &self.shrink_wrap)
            .field("focusable", &self.focusable)
            .field("semantics_label", &self.semantics_label)
            .field("empty_label", &self.empty_label)
            .field("placement", &self.placement);
        if self.builder_source.is_some() {
            s.field("item_extent_estimate", &self.item_extent_estimate);
            s.field("builder_source", &self.builder_source);
        } else {
            s.field("item_extent", &self.item_extent)
                .field("children", &self.children.len())
                .field("add_repaint_boundaries", &self.add_repaint_boundaries)
                .field(
                    "children_in_boundaries",
                    &self.children_in_boundaries.get().map(|c| c.len()),
                );
        }
        s.finish()
    }
}

impl ScrollView for ListView {
    fn scroll_config_mut(&mut self) -> &mut ScrollViewConfig {
        &mut self.config
    }
}

impl StatelessView for ListView {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let axis_direction = axis_direction_from_axis_reverse_and_directionality(
            ctx,
            self.config.scroll_direction,
            self.config.reverse,
        );
        let sliver = self.sliver();
        let shrink_wrap = self.shrink_wrap;
        let focus = self.focusable.then(|| ListFocusConfig {
            item_count: self.item_count(),
            axis: self.config.scroll_direction,
            reversed: axis_direction.is_reversed(),
            position: None,
            extent_estimate: self.item_extent_estimate,
            empty_list_label: self.collection_label(),
            sliver: false,
            grid: false,
        });
        Scrollable::with_config(
            self.config.clone(),
            Rc::new(move |position: ScrollPosition| {
                let viewport = if shrink_wrap {
                    ShrinkWrappingViewport::new(position.clone(), (sliver.clone(),))
                        .axis_direction(axis_direction)
                        .boxed()
                } else {
                    Viewport::new(position.clone(), (sliver.clone(),))
                        .axis_direction(axis_direction)
                        .boxed()
                };
                match &focus {
                    None => viewport,
                    Some(focus) => {
                        let config = ListFocusConfig {
                            position: Some(position),
                            ..focus.clone()
                        };
                        FocusableList::new(config, viewport).boxed()
                    }
                }
            }),
        )
    }
}

impl ListView {
    /// The list's sliver: its rows, each in its focus marker and repaint
    /// boundary as configured. Built once per list value, so the static rows
    /// keep their identity across builds (ADR-0053).
    fn sliver(&self) -> BoxedView {
        if let Some(ref delegate) = self.builder_source {
            let builder = match (self.focusable, self.add_repaint_boundaries) {
                (false, false) => Rc::clone(&delegate.builder),
                (false, true) => {
                    super::sliver_list::wrap_builder_in_repaint_boundaries(&delegate.builder)
                }
                (true, boundary) => {
                    let inner = Rc::clone(&delegate.builder);
                    let wrapped: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(move |index| {
                        inner(index).map(|row| wrap_row(index, row, true, boundary))
                    });
                    wrapped
                }
            };
            {
                let sliver =
                    SliverList::new(delegate.item_count, self.item_extent_estimate, builder);
                let sliver = match self.collection_label() {
                    Some(label) => sliver.semantics_label(label),
                    None => sliver,
                };
                let sliver = match &self.placement {
                    Some(placement) => sliver.placement(placement.clone()),
                    None => sliver,
                };
                match &delegate.find_index_by_key {
                    Some(find) => {
                        let find = Rc::clone(find);
                        sliver.find_index_by_key(move |key| find(key))
                    }
                    None => sliver,
                }
            }
            .boxed()
        } else {
            // Static children go through the same lazy path as a builder:
            // only the window is built, keyed children are found by the
            // delegate's key map, everything outside the window is evicted.
            let children = if self.focusable {
                // The marker needs the row's index, which a mapping does not
                // see, so the rows are wrapped once per list value.
                Rc::clone(self.children_in_boundaries.get_or_init(|| {
                    StaticChildren::new(
                        self.children
                            .children()
                            .iter()
                            .enumerate()
                            .map(|(index, row)| {
                                wrap_row(index, row.clone(), true, self.add_repaint_boundaries)
                            })
                            .collect(),
                    )
                }))
            } else if self.add_repaint_boundaries {
                Rc::clone(self.children_in_boundaries.get_or_init(|| {
                    StaticChildren::mapped(
                        self.children.children().to_vec(),
                        super::sliver_list::wrap_in_repaint_boundary,
                    )
                }))
            } else {
                Rc::clone(&self.children)
            };
            let sliver = SliverFixedExtentList::over(self.item_extent, children);
            match self.collection_label() {
                Some(label) => sliver.semantics_label(label),
                None => sliver,
            }
            .boxed()
        }
    }
}
