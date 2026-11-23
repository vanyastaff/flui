//! [`ReorderableGrid`] and its state.

#[cfg(doc)]
use crate::GridView;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use flui_animation::Animation;

use flui_foundation::geometry::Axis;
use flui_rendering::delegates::SliverGridDelegate;

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{EventContext, EventOutcome};

use crate::interaction::ControlPlacement;

use super::config::{Config, Layout};
use super::session::Shared;
use super::types::{Reorder, ReorderHandles, ReorderPreview, ReorderableItem};

/// A lazy grid whose tiles the user moves by dragging them, with their move
/// commands as the drag's alternative — see the module docs.
///
/// The grid lays its tiles out with a [`SliverGridDelegate`], as
/// [`GridView::builder`] does. A tile's handle drags it any way, and on touch
/// and pen a long press on the tile drags it too. Its commands move it to the
/// next tile along its row and by a whole row, by the number of tiles the
/// delegate put in a row at the grid's last layout, so a grid whose width
/// changes the column count moves by its new rows.
///
/// ```rust
/// use std::cell::RefCell;
/// use std::rc::Rc;
///
/// use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
/// use flui_widgets::{ReorderableGrid, Text};
///
/// let photos = Rc::new(RefCell::new(vec!["Beach", "Forest", "Harbour", "Dunes"]));
/// let (keys, labels, model) = (Rc::clone(&photos), Rc::clone(&photos), Rc::clone(&photos));
/// let _grid = ReorderableGrid::builder(
///     SliverGridDelegateWithFixedCrossAxisCount::new(3),
///     photos.borrow().len(),
///     move |i| keys.borrow()[i],
///     move |tile| Text::new(labels.borrow()[tile.index()]),
///     move |_ctx, change| {
///         let mut photos = model.borrow_mut();
///         let photo = photos.remove(change.from);
///         photos.insert(change.to, photo);
///     },
/// );
/// ```
#[derive(Clone)]
pub struct ReorderableGrid {
    config: Config,
}

impl fmt::Debug for ReorderableGrid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("ReorderableGrid");
        debug.field("item_count", &self.config.item_count);
        if let Layout::Grid { delegate } = &self.config.layout {
            debug.field("grid_delegate", delegate);
        }
        debug
            .field("handles", &self.config.handles)
            .field("long_press", &self.config.long_press)
            .field("scroll_direction", &self.config.scroll_direction)
            .finish_non_exhaustive()
    }
}

impl ReorderableGrid {
    /// A grid of `item_count` tiles laid out by `grid_delegate`: `key` names
    /// each item in the app's model, `item` builds its tile, and `on_reorder`
    /// moves it there when the user moves the tile (see [`Reorder`] for what
    /// `to` counts).
    pub fn builder<K, V, R>(
        grid_delegate: impl SliverGridDelegate + 'static,
        item_count: usize,
        key: impl Fn(usize) -> K + 'static,
        item: impl Fn(&ReorderableItem<'_>) -> V + 'static,
        on_reorder: impl Fn(&mut EventContext<'_>, Reorder) -> R + 'static,
    ) -> Self
    where
        K: Clone + Hash + Eq + fmt::Debug + 'static,
        V: IntoView,
        R: EventOutcome,
    {
        let layout = Layout::Grid {
            delegate: Rc::new(grid_delegate),
        };
        Self {
            config: Config::new(layout, item_count, key, item, on_reorder),
        }
    }

    /// What a drag of a tile shows under the pointer; the tile's label
    /// ([`item_label`](Self::item_label)) by default, a plate the tile's size
    /// without one.
    ///
    /// `feedback` builds a second view beside the tile, which stays mounted
    /// while it is dragged: it must not reuse what the tile owns alone, such
    /// as a text field's focus node or a controller.
    #[must_use]
    pub fn feedback<V: IntoView>(
        mut self,
        feedback: impl Fn(&ReorderableItem<'_>) -> V + 'static,
    ) -> Self {
        self.config.feedback(feedback);
        self
    }

    /// Dress what a drag of a tile shows under the pointer as it lifts:
    /// `decorator` gets that view, the tile's index and the lift, which runs
    /// from 0 to 1 over 150 ms as the drag starts (at once under reduced
    /// motion). By default the view rises to elevation 6 and grows to
    /// 102 %.
    #[must_use]
    pub fn proxy_decorator<V: IntoView>(
        mut self,
        decorator: impl Fn(BoxedView, usize, Rc<dyn Animation<f64>>) -> V + 'static,
    ) -> Self {
        self.config.proxy_decorator = Some(Rc::new(move |proxy, index, lift| {
            decorator(proxy, index, lift).into_view().boxed()
        }));
        self
    }

    /// Called as a drag of a tile moves over the tiles: where a drop would
    /// put it now.
    #[must_use]
    pub fn on_reorder_preview<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ReorderPreview) -> R + 'static,
        R: EventOutcome,
    {
        self.config.on_reorder_preview(callback);
        self
    }

    /// Name each tile ("Beach"), for its menu's labels and the announcement
    /// of its move.
    #[must_use]
    pub fn item_label(mut self, label: impl Fn(usize) -> String + 'static) -> Self {
        self.config.item_label = Some(Rc::new(label));
        self
    }

    /// Where the drag handles come from; [`ReorderHandles::Default`] by default.
    #[must_use]
    pub fn handles(mut self, handles: ReorderHandles) -> Self {
        self.config.handles = handles;
        self
    }

    /// Whether a touch or pen long press on a tile drags it, as well as its
    /// handle; on by default. A mouse drags only by the handle.
    #[must_use]
    pub fn long_press(mut self, long_press: bool) -> Self {
        self.config.long_press = long_press;
        self
    }

    /// Show each tile's move commands as controls at `placement` instead of
    /// in a menu opened from its handle.
    #[must_use]
    pub fn visible_controls(mut self, placement: ControlPlacement) -> Self {
        self.config.controls = Some(placement);
        self
    }

    /// The grid's scroll axis; vertical by default, whose rows run across.
    #[must_use]
    pub fn scroll_direction(mut self, axis: Axis) -> Self {
        self.config.scroll_direction = axis;
        self
    }
}

/// Persistent state of a [`ReorderableGrid`].
pub struct ReorderableGridState {
    shared: Rc<Shared>,
}

impl fmt::Debug for ReorderableGridState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReorderableGridState")
            .field("drags", &self.shared.drags.borrow().len())
            .field("shape", &self.shared.grid.get())
            .finish_non_exhaustive()
    }
}

impl View for ReorderableGrid {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for ReorderableGrid {
    type State = ReorderableGridState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        ReorderableGridState {
            shared: Shared::new(&self.config, ctx),
        }
    }
}

impl ViewState<ReorderableGrid> for ReorderableGridState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shared.acquire(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.shared.acquire(ctx);
    }

    fn build(&self, view: &ReorderableGrid, ctx: &dyn BuildContext) -> impl IntoView {
        self.shared.build(&view.config, ctx)
    }

    fn dispose(&mut self) {
        self.shared.dispose();
    }
}
