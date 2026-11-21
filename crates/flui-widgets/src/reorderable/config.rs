//! The description shared by the list and the grid: its callbacks and its layout.

#[cfg(doc)]
use super::{ReorderableGrid, ReorderableList};
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use flui_animation::Animation;

use flui_foundation::ValueKey;
use flui_foundation::ViewKey;
use flui_foundation::geometry::Axis;
use flui_interaction::gesture::MultiDragAxis;
use flui_rendering::delegates::SliverGridDelegate;

use flui_view::prelude::*;
use flui_view::{EventContext, EventOutcome};

use crate::interaction::ControlPlacement;

use super::types::{Reorder, ReorderHandles, ReorderPreview, ReorderableItem};

pub(super) type KeyFn = Rc<dyn Fn(usize) -> Rc<dyn ViewKey>>;
pub(super) type ItemFn = Rc<dyn Fn(&ReorderableItem<'_>) -> BoxedView>;
pub(super) type ReorderFn = Rc<dyn Fn(&mut EventContext<'_>, Reorder)>;
pub(super) type PreviewFn = Rc<dyn Fn(&mut EventContext<'_>, ReorderPreview)>;
pub(super) type LabelFn = Rc<dyn Fn(usize) -> String>;
pub(super) type DecoratorFn = Rc<dyn Fn(BoxedView, usize, Rc<dyn Animation<f64>>) -> BoxedView>;

/// How the items are laid out.
#[derive(Clone)]
pub(super) enum Layout {
    /// One after another along the scroll axis.
    List {
        /// The row extent assumed before rows are measured.
        item_extent_estimate: f64,
    },
    /// In rows across the scroll axis, as the delegate lays them out.
    Grid {
        delegate: Rc<dyn SliverGridDelegate>,
    },
}

/// What a [`ReorderableList`] or [`ReorderableGrid`] was configured with.
#[derive(Clone)]
pub(super) struct Config {
    pub(super) item_count: usize,
    pub(super) key_of: KeyFn,
    pub(super) item: ItemFn,
    pub(super) feedback: Option<ItemFn>,
    pub(super) proxy_decorator: Option<DecoratorFn>,
    pub(super) on_reorder: ReorderFn,
    pub(super) on_reorder_preview: Option<PreviewFn>,
    pub(super) item_label: Option<LabelFn>,
    pub(super) handles: ReorderHandles,
    pub(super) long_press: bool,
    pub(super) controls: Option<ControlPlacement>,
    pub(super) scroll_direction: Axis,
    pub(super) layout: Layout,
}

impl Config {
    pub(super) fn new<K, V, R>(
        layout: Layout,
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
        Self {
            item_count,
            key_of: Rc::new(move |index| Rc::new(ValueKey::new(key(index))) as Rc<dyn ViewKey>),
            item: Rc::new(move |row| item(row).into_view().boxed()),
            feedback: None,
            proxy_decorator: None,
            on_reorder: Rc::new(move |ctx, change| on_reorder(ctx, change).report()),
            on_reorder_preview: None,
            item_label: None,
            handles: ReorderHandles::default(),
            long_press: true,
            controls: None,
            scroll_direction: Axis::Vertical,
            layout,
        }
    }

    pub(super) fn feedback<V: IntoView>(
        &mut self,
        feedback: impl Fn(&ReorderableItem<'_>) -> V + 'static,
    ) {
        self.feedback = Some(Rc::new(move |row| feedback(row).into_view().boxed()));
    }

    pub(super) fn on_reorder_preview<F, R>(&mut self, callback: F)
    where
        F: Fn(&mut EventContext<'_>, ReorderPreview) -> R + 'static,
        R: EventOutcome,
    {
        self.on_reorder_preview =
            Some(Rc::new(move |ctx, preview| callback(ctx, preview).report()));
    }

    /// The index of the item keyed `key`, by the keys now.
    pub(super) fn index_of(&self, key: &dyn ViewKey) -> Option<usize> {
        (0..self.item_count).find(|&index| (self.key_of)(index).key_eq(key))
    }

    /// Where a press on an item's handle must move to drag it: along a list,
    /// so the handle is the innermost member on the list's scroll axis, and
    /// any way in a grid, whose tiles move across its rows too.
    pub(super) fn handle_axis(&self) -> MultiDragAxis {
        match (&self.layout, self.scroll_direction) {
            (Layout::Grid { .. }, _) => MultiDragAxis::Free,
            (Layout::List { .. }, Axis::Vertical) => MultiDragAxis::Vertical,
            (Layout::List { .. }, Axis::Horizontal) => MultiDragAxis::Horizontal,
        }
    }
}
