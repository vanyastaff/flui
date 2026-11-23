//! [`ReorderableList`] and its state.

use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use flui_animation::Animation;

use flui_foundation::geometry::Axis;

use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{EventContext, EventOutcome};

use crate::interaction::ControlPlacement;

use super::config::{Config, Layout};
use super::session::Shared;
use super::types::{Reorder, ReorderHandles, ReorderPreview, ReorderableItem};

/// A lazy list whose rows the user moves by dragging them, with their move
/// commands as the drag's alternative — see the module docs.
///
/// ```rust
/// use std::cell::RefCell;
/// use std::rc::Rc;
///
/// use flui_widgets::{ReorderableList, Text};
///
/// let items = Rc::new(RefCell::new(vec!["Milk", "Eggs", "Bread"]));
/// let (keys, labels, model) = (Rc::clone(&items), Rc::clone(&items), Rc::clone(&items));
/// let _list = ReorderableList::builder(
///     items.borrow().len(),
///     move |i| keys.borrow()[i],
///     move |item| Text::new(labels.borrow()[item.index()]),
///     move |_ctx, change| {
///         let mut items = model.borrow_mut();
///         let item = items.remove(change.from);
///         items.insert(change.to, item);
///     },
/// );
/// ```
#[derive(Clone)]
pub struct ReorderableList {
    config: Config,
}

impl fmt::Debug for ReorderableList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReorderableList")
            .field("item_count", &self.config.item_count)
            .field("handles", &self.config.handles)
            .field("long_press", &self.config.long_press)
            .field("scroll_direction", &self.config.scroll_direction)
            .finish_non_exhaustive()
    }
}

impl ReorderableList {
    /// A list of `item_count` rows: `key` names each item in the app's model,
    /// `item` builds its row, and `on_reorder` moves it there when the user
    /// moves the row (see [`Reorder`] for what `to` counts).
    pub fn builder<K, V, R>(
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
        let layout = Layout::List {
            item_extent_estimate: 56.0,
        };
        Self {
            config: Config::new(layout, item_count, key, item, on_reorder),
        }
    }

    /// What a drag of a row shows under the pointer; the row's label
    /// ([`item_label`](Self::item_label)) by default, a plate the row's size
    /// without one.
    ///
    /// `feedback` builds a second view beside the row, which stays mounted
    /// while it is dragged: it must not reuse what the row owns alone, such
    /// as a text field's focus node or a controller.
    #[must_use]
    pub fn feedback<V: IntoView>(
        mut self,
        feedback: impl Fn(&ReorderableItem<'_>) -> V + 'static,
    ) -> Self {
        self.config.feedback(feedback);
        self
    }

    /// Dress what a drag of a row shows under the pointer as it lifts:
    /// `decorator` gets that view, the row's index and the lift, which runs
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

    /// Called as a drag of a row moves over the rows: where a drop would put
    /// it now.
    #[must_use]
    pub fn on_reorder_preview<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventContext<'_>, ReorderPreview) -> R + 'static,
        R: EventOutcome,
    {
        self.config.on_reorder_preview(callback);
        self
    }

    /// Name each row ("Milk"), for its menu's labels and the announcement of
    /// its move.
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

    /// Whether a touch or pen long press on a row drags it, as well as its
    /// handle; on by default. A mouse drags only by the handle.
    #[must_use]
    pub fn long_press(mut self, long_press: bool) -> Self {
        self.config.long_press = long_press;
        self
    }

    /// Show each row's move commands as controls at `placement` instead of in
    /// a menu opened from its handle.
    #[must_use]
    pub fn visible_controls(mut self, placement: ControlPlacement) -> Self {
        self.config.controls = Some(placement);
        self
    }

    /// The list's axis; vertical by default.
    #[must_use]
    pub fn scroll_direction(mut self, axis: Axis) -> Self {
        self.config.scroll_direction = axis;
        self
    }

    /// The row extent the list assumes before rows are measured.
    ///
    /// # Panics
    ///
    /// When the list builds, if `extent` is not finite and positive.
    #[must_use]
    pub fn item_extent_estimate(mut self, extent: f64) -> Self {
        self.config.layout = Layout::List {
            item_extent_estimate: extent,
        };
        self
    }
}

/// Persistent state of a [`ReorderableList`].
pub struct ReorderableListState {
    shared: Rc<Shared>,
}

impl fmt::Debug for ReorderableListState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReorderableListState")
            .field("drags", &self.shared.drags.borrow().len())
            .finish_non_exhaustive()
    }
}

impl View for ReorderableList {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for ReorderableList {
    type State = ReorderableListState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        ReorderableListState {
            shared: Shared::new(&self.config, ctx),
        }
    }
}

impl ViewState<ReorderableList> for ReorderableListState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.shared.acquire(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.shared.acquire(ctx);
    }

    fn build(&self, view: &ReorderableList, ctx: &dyn BuildContext) -> impl IntoView {
        self.shared.build(&view.config, ctx)
    }

    fn dispose(&mut self) {
        self.shared.dispose();
    }
}
