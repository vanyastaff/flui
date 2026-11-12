//! [`ReorderableList`] and [`ReorderableGrid`] — a lazy list and a lazy grid
//! whose items the user moves (ADR-0211 §9).
//!
//! Moving an item is a drag-and-drop session that never leaves the window
//! ([`DragReach::Window`]): an item's handle starts it at once, and on touch
//! and pen a long press on the item starts it too. Every item is a drop target
//! that takes only drags of its own list or grid, and edge autoscroll comes
//! from the scrollable. The dragged item keeps its state while it is scrolled
//! out of the built band: its drag holds the item alive.
//!
//! While a drag runs, the dragged item leaves a gap where a drop would put
//! it, at the item the pointer is over, and the items between make room; what
//! the drag shows lifts from the item's place
//! ([`proxy_decorator`](ReorderableList::proxy_decorator)). A drop or
//! a cancel lands the item in its place from where it was let go, and a move
//! command slides the items the same way: every slide takes 250 ms, and under
//! reduced motion lands on its next frame.
//!
//! The same moves have a path without a drag (ADR-0216): each item carries the
//! commands that apply to it — "Move to the start", "Move to the end" and one
//! step each way, which in a grid are a tile along its row and a whole row —
//! on the platform's keys while focus is in the item, as assistive
//! technology's actions on the item's node, and for one pointer in a context
//! menu opened from the item's handle, a secondary click or the menu key. A
//! command's move is announced with the item's new position.
//!
//! Neither moves anything itself: each calls [`on_reorder`](Reorder) once,
//! and the app moves the item in its model and rebuilds. Items are keyed by
//! the model key they are given, so an item's element, state and focus follow
//! it to the new position.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, DrivenController, MotionSpec, Vsync, VsyncUpdate,
};

use flui_foundation::ValueKey;
use flui_foundation::ViewKey;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Offset, Point, Size};
use flui_interaction::drag::{DragItem, DragReach, DragView};
use flui_interaction::gesture::PanClaim;
use flui_interaction::{MultiDragAxis, PointerKindSet};
use flui_objects::{Placement, PlacementSlot, SubtreeAnchor};
use flui_painting::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_rendering::constraints::SliverConstraints;
use flui_rendering::delegates::{SliverGridDelegate, SliverGridLayout};
use flui_rendering::pipeline::WeakPipelineCell;

use crate::__private::AnchoredBox;
use crate::interaction::FeedbackFrame;
use crate::scroll::ScrollView;
use crate::{
    AnimatedBuilder, DecoratedBox, DefaultFocusRingStyle, Opacity, PhysicalModel, Transform,
};
use flui_painting::typography::TextDirection;
use flui_platform::data_transfer::TransferActions;
use flui_platform::keyboard::NamedKey;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::semantics::{Announcer, Urgency};
use flui_view::element::ElementKind;
use flui_view::environment::GestureArenaScope;
use flui_view::environment::VsyncScope;
use flui_view::prelude::*;
use flui_view::{EventContext, EventOutcome, RebuildHandle, WriterSource};

use crate::interaction::actions::{ambient_action_chain, resolve};
use crate::interaction::{
    Action, ActionOutcome, Actions, Alternative, ControlPlacement, ControlSpec,
    GestureAlternatives, GestureCommand, Intent, Shortcuts, ShowContextMenuIntent, SingleActivator,
};
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, Directionality, WidgetsLocalizations,
};
use crate::{
    Center, ColoredBox, Column, ContextMenuArea, Draggable, DropTarget, Expanded, GestureDetector,
    GridView, ListView, Listener, PanClaimRegion, Row, SizedBox, Text,
};

/// An item's move, as [`ReorderableList`] or [`ReorderableGrid`] reports it
/// once: the item at `from` goes to `to`.
///
/// `to` is the item's index **after** it is removed from `from`, so the move
/// is exactly a `Vec::remove` then a `Vec::insert`:
///
/// ```rust
/// use flui_widgets::Reorder;
///
/// fn apply<T>(items: &mut Vec<T>, change: Reorder) {
///     let item = items.remove(change.from);
///     items.insert(change.to, item);
/// }
///
/// // Dragging "a" onto "d" puts it where "d" was: last.
/// let mut items = vec!["a", "b", "c", "d"];
/// let item = items.remove(0);
/// items.insert(3, item);
/// assert_eq!(items, ["b", "c", "d", "a"]);
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Reorder {
    /// Where the item was.
    pub from: usize,
    /// Where it goes, counted after it left `from`.
    pub to: usize,
    /// What moved it.
    pub cause: ReorderCause,
}

/// What moved an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderCause {
    /// A drag of the item, dropped on another item.
    Drag,
    /// A move command: its keys, assistive technology's action or the menu.
    Command,
}

/// Where a drag of an item is while it runs, for a list or grid that shows
/// it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ReorderPreview {
    /// Where the dragged item is.
    pub from: usize,
    /// Where a drop now would move the item: the position of the item the
    /// drag is over, counted in the order the list shows while it makes
    /// room; `None` once the drag left the list.
    pub over: Option<usize>,
}

/// Where an item's drag handle comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderHandles {
    /// The list puts a handle before each row's content, and the grid one
    /// above each tile's.
    #[default]
    Default,
    /// Each item places its own, through [`ReorderableItem::handle`].
    Custom,
}

/// One move of a reorderable item, for the platform's keys, assistive
/// technology and the item's menu.
///
/// The arrows name the direction the item moves on screen. In a list they
/// move it one position; in a grid the arrows along the grid's rows move it
/// one position too (from a row's end to the next row's start), and the
/// arrows across them by a whole row, onto the last tile when the next row
/// is short.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderIntent {
    /// To the first position.
    Start,
    /// Up: one position back in a vertical list, a row up in a vertical grid,
    /// one tile up in a horizontal grid.
    Up,
    /// Down: one position forward in a vertical list, a row down in a
    /// vertical grid, one tile down in a horizontal grid.
    Down,
    /// To the last position.
    End,
    /// Left: one position in a horizontal list or one tile in a vertical
    /// grid, a row in a horizontal grid; back in a left-to-right locale,
    /// forward in a right-to-left one.
    Left,
    /// Right: the opposite of [`Left`](Self::Left).
    Right,
}

impl Intent for ReorderIntent {}

/// What the item builder of [`ReorderableList`] or [`ReorderableGrid`] knows
/// of the item it builds.
pub struct ReorderableItem<'a> {
    index: usize,
    key: &'a dyn ViewKey,
    handle: &'a HandleSource,
}

impl ReorderableItem<'_> {
    /// The item's index.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// The item's model key.
    #[must_use]
    pub fn key(&self) -> &dyn ViewKey {
        self.key
    }

    /// `child` as the item's drag handle: a press on it that moves drags the
    /// item, and a tap opens the item's menu. Place one in each item of a
    /// list or grid with [`ReorderHandles::Custom`].
    #[must_use]
    pub fn handle(&self, child: impl IntoView) -> BoxedView {
        self.handle.handle(child.into_view().boxed())
    }
}

impl fmt::Debug for ReorderableItem<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReorderableItem")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

type KeyFn = Rc<dyn Fn(usize) -> Rc<dyn ViewKey>>;
type ItemFn = Rc<dyn Fn(&ReorderableItem<'_>) -> BoxedView>;
type ReorderFn = Rc<dyn Fn(&mut EventContext<'_>, Reorder)>;
type PreviewFn = Rc<dyn Fn(&mut EventContext<'_>, ReorderPreview)>;
type LabelFn = Rc<dyn Fn(usize) -> String>;
type DecoratorFn = Rc<dyn Fn(BoxedView, usize, Rc<dyn Animation<f64>>) -> BoxedView>;

/// How the items are laid out.
#[derive(Clone)]
enum Layout {
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
struct Config {
    item_count: usize,
    key_of: KeyFn,
    item: ItemFn,
    feedback: Option<ItemFn>,
    proxy_decorator: Option<DecoratorFn>,
    on_reorder: ReorderFn,
    on_reorder_preview: Option<PreviewFn>,
    item_label: Option<LabelFn>,
    handles: ReorderHandles,
    long_press: bool,
    controls: Option<ControlPlacement>,
    scroll_direction: Axis,
    layout: Layout,
}

impl Config {
    fn new<K, V, R>(
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

    fn feedback<V: IntoView>(&mut self, feedback: impl Fn(&ReorderableItem<'_>) -> V + 'static) {
        self.feedback = Some(Rc::new(move |row| feedback(row).into_view().boxed()));
    }

    fn on_reorder_preview<F, R>(&mut self, callback: F)
    where
        F: Fn(&mut EventContext<'_>, ReorderPreview) -> R + 'static,
        R: EventOutcome,
    {
        self.on_reorder_preview =
            Some(Rc::new(move |ctx, preview| callback(ctx, preview).report()));
    }

    /// The index of the item keyed `key`, by the keys now.
    fn index_of(&self, key: &dyn ViewKey) -> Option<usize> {
        (0..self.item_count).find(|&index| (self.key_of)(index).key_eq(key))
    }

    /// Where a press on an item's handle must move to drag it: along a list,
    /// so the handle is the innermost member on the list's scroll axis, and
    /// any way in a grid, whose tiles move across its rows too.
    fn handle_axis(&self) -> MultiDragAxis {
        match (&self.layout, self.scroll_direction) {
            (Layout::Grid { .. }, _) => MultiDragAxis::Free,
            (Layout::List { .. }, Axis::Vertical) => MultiDragAxis::Vertical,
            (Layout::List { .. }, Axis::Horizontal) => MultiDragAxis::Horizontal,
        }
    }
}

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
/// use flui_widgets::{ReorderableGrid, SliverGridDelegateWithFixedCrossAxisCount, Text};
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

// ============================================================================
// The grid's rows
// ============================================================================

/// How a grid laid its tiles out at its last layout: what its row moves need.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct GridShape {
    /// The tiles in one row across the scroll axis.
    stride: usize,
    /// Whether the index order runs against the screen along the scroll axis
    /// (a horizontal grid in a right-to-left locale).
    main_reversed: bool,
    /// Whether it runs against the screen across it.
    cross_reversed: bool,
}

impl GridShape {
    /// The shape assumed before the grid is laid out: no row moves, and the
    /// tiles in the screen's order.
    const UNKNOWN: Self = Self {
        stride: 1,
        main_reversed: false,
        cross_reversed: false,
    };
}

/// The app's grid delegate, which also records the shape of each layout it
/// gives, so the row moves go by the rows on screen.
#[derive(Debug)]
struct ShapeRecorder {
    inner: Rc<dyn SliverGridDelegate>,
    shape: Rc<Cell<Option<GridShape>>>,
}

impl SliverGridDelegate for ShapeRecorder {
    fn get_layout(&self, constraints: SliverConstraints) -> SliverGridLayout {
        let layout = self.inner.get_layout(constraints);
        self.shape.set(Some(GridShape {
            stride: layout.cross_axis_count.max(1),
            main_reversed: constraints.axis_direction.is_reversed(),
            cross_reversed: layout.reverse_cross_axis,
        }));
        layout
    }

    fn should_relayout(&self, old_delegate: &dyn SliverGridDelegate) -> bool {
        match old_delegate.as_any().downcast_ref::<Self>() {
            Some(old) => {
                !Rc::ptr_eq(&self.shape, &old.shape) || self.inner.should_relayout(&*old.inner)
            }
            None => true,
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// How far a move goes from an item's index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Toward the first item, by this many.
    Back(usize),
    /// Toward the last item, by this many; past the last, onto it, when the
    /// item is not in the last row.
    Forward(usize),
}

/// The step an arrow's move takes along a list, in the list's reading
/// direction; `None` for Start and End, which go to a place, not by a step.
fn list_step(intent: ReorderIntent, rtl: bool) -> Option<Step> {
    match intent {
        ReorderIntent::Start | ReorderIntent::End => None,
        ReorderIntent::Left if rtl => Some(Step::Forward(1)),
        ReorderIntent::Right if rtl => Some(Step::Back(1)),
        ReorderIntent::Up | ReorderIntent::Left => Some(Step::Back(1)),
        ReorderIntent::Down | ReorderIntent::Right => Some(Step::Forward(1)),
    }
}

/// The step an arrow's move of a tile takes in a grid shaped `shape` on the
/// `axis`: the arrows along its rows go to the next tile, those across them
/// by a row, each the way it points on screen. `None` for Start and End, and
/// for a row move in a grid of one tile per row, which would only repeat the
/// move along it.
fn grid_step(intent: ReorderIntent, axis: Axis, shape: GridShape) -> Option<Step> {
    let toward = |forward_on_screen: bool, reversed: bool, by: usize| {
        if forward_on_screen == reversed {
            Step::Back(by)
        } else {
            Step::Forward(by)
        }
    };
    let row = |forward_on_screen: bool| {
        (shape.stride > 1).then(|| toward(forward_on_screen, shape.main_reversed, shape.stride))
    };
    let along = |forward_on_screen: bool| Some(toward(forward_on_screen, shape.cross_reversed, 1));
    match (axis, intent) {
        (_, ReorderIntent::Start | ReorderIntent::End) => None,
        (Axis::Vertical, ReorderIntent::Up) | (Axis::Horizontal, ReorderIntent::Left) => row(false),
        (Axis::Vertical, ReorderIntent::Down) | (Axis::Horizontal, ReorderIntent::Right) => {
            row(true)
        }
        (Axis::Vertical, ReorderIntent::Left) | (Axis::Horizontal, ReorderIntent::Up) => {
            along(false)
        }
        (Axis::Vertical, ReorderIntent::Right) | (Axis::Horizontal, ReorderIntent::Down) => {
            along(true)
        }
    }
}

/// Where `intent` moves the item at `index` of `view`, laid out as `shape`
/// when it is a grid; `None` when the move does not apply there.
fn move_target(
    view: &Config,
    shape: Option<GridShape>,
    rtl: bool,
    index: usize,
    intent: ReorderIntent,
) -> Option<usize> {
    let last = view.item_count.checked_sub(1)?;
    if index > last {
        return None;
    }
    let target = match intent {
        ReorderIntent::Start => 0,
        ReorderIntent::End => last,
        _ => {
            let step = match view.layout {
                Layout::List { .. } => list_step(intent, rtl),
                Layout::Grid { .. } => grid_step(
                    intent,
                    view.scroll_direction,
                    shape.unwrap_or(GridShape::UNKNOWN),
                ),
            }?;
            match step {
                Step::Back(by) => index.checked_sub(by)?,
                // A row down from a row that is not the last lands on the
                // tile below, or on the last tile when the next row is short.
                Step::Forward(by) => {
                    if index / by >= last / by {
                        return None;
                    }
                    index.saturating_add(by).min(last)
                }
            }
        }
    };
    (target != index).then_some(target)
}

// ============================================================================
// The list or grid
// ============================================================================
//
// From here on a "row" is an item of either: a row of a list, a tile of a
// grid.

/// What one drag of a row carries: its list, so only that list's rows take
/// it, and the dragged item's key, so a drop finds where the item is now.
struct RowPayload {
    list: Rc<Shared>,
    key: Rc<dyn ViewKey>,
}

/// How long a dragged item's lift takes.
const LIFT: Duration = Duration::from_millis(150);
/// How long a row takes to make room, land or go to its new place.
const SLIDE: Duration = Duration::from_millis(250);
/// The elevation and scale the default proxy decorator lifts to.
const LIFT_ELEVATION: f64 = 6.0;
const LIFT_SCALE: f64 = 1.02;

/// One running drag of a row.
struct RowDrag {
    key: Rc<dyn ViewKey>,
    /// Where a drop now moves the item. For the lead drag a position in the
    /// order the rows are shown in, which keeps a gap there; for any other
    /// the index of the row it is over.
    over: Option<usize>,
    /// Whether a row reported the drag over it since the list last looked:
    /// the rows answer a move before the list around them does.
    row_reported: bool,
    /// The row whose report last moved the gap. It slides away under the
    /// pointer, so its reports are not heard until another row reports.
    shifted: Option<Rc<dyn ViewKey>>,
    /// The lift the proxy decorator reads.
    lift: DrivenController,
    /// How far the proxy was from the dragged item's row when the drag
    /// ended, in the window's space: the row lands from there.
    landing: Option<Offset<f64>>,
    /// Where the drag was when it last left the list, for the drop that
    /// follows its own leave; cleared when it is over a row again.
    left_over: Option<usize>,
}

/// Where the lead drag shows its item: the rows between `from` and `over`
/// each move one position toward `from`, and the item, painted as a gap,
/// sits at `over`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Gap {
    from: usize,
    over: usize,
}

impl Gap {
    /// The index of the item shown at position `slot`.
    fn item_at(gap: Option<Self>, slot: usize) -> usize {
        match gap {
            Some(Self { from, over }) if slot == over => from,
            Some(Self { from, over }) if from < over && (from..over).contains(&slot) => slot + 1,
            Some(Self { from, over }) if over < from && (over + 1..=from).contains(&slot) => {
                slot - 1
            }
            _ => slot,
        }
    }

    /// The position item `index` is shown at.
    fn slot_of(gap: Option<Self>, index: usize) -> usize {
        match gap {
            Some(Self { from, over }) if index == from => over,
            Some(Self { from, over }) if from < over && (from + 1..=over).contains(&index) => {
                index - 1
            }
            Some(Self { from, over }) if over < from && (over..from).contains(&index) => index + 1,
            _ => index,
        }
    }
}

/// What the list keeps for one item while the item is in it: the record its
/// slides read, and the node its row's content is published in. Both follow
/// the item wherever it is shown.
#[derive(Clone)]
struct ItemRecord {
    key: Rc<dyn ViewKey>,
    slot: Rc<PlacementSlot>,
    anchor: SubtreeAnchor,
}

/// The items' records by the hash of their keys.
type Records = HashMap<u64, Vec<ItemRecord>>;

/// The record of `key` among `records`.
fn record_in(records: &Records, key: &dyn ViewKey) -> Option<ItemRecord> {
    records
        .get(&key.key_hash())?
        .iter()
        .find(|record| record.key.key_eq(key))
        .cloned()
}

/// One mounted list or grid, as its rows share it.
struct Shared {
    view: RefCell<Config>,
    /// How a grid laid its tiles out last; `None` for a list and before a
    /// grid's first layout.
    grid: Rc<Cell<Option<GridShape>>>,
    /// The drags of this list's rows that run now, one per dragged item:
    /// two contacts may drag two rows at once, each with its own preview.
    /// The first is the lead, whose item the rows make room for.
    drags: RefCell<Vec<RowDrag>>,
    /// The clock every slide of the rows shares: rows making room for a
    /// drag, an item landing, an item a command moved.
    clock: RefCell<DrivenController>,
    /// Advances whenever the order the rows are shown in changes; the rows
    /// slide only across a change of it.
    generation: Cell<u64>,
    /// Each item's record, rebuilt from the keys by each build.
    records: RefCell<Records>,
    vsync: RefCell<Option<Vsync>>,
    rebuild: RefCell<Option<RebuildHandle>>,
    writer: RefCell<Option<WriterSource>>,
    announcer: RefCell<Option<Announcer>>,
    /// The localizations the list was last built with, for its announcements.
    strings: RefCell<Option<Rc<BoxedWidgetsLocalizations>>>,
    /// The render tree the rows lay out in, for the size and place of a
    /// dragged row.
    pipeline: RefCell<Option<WeakPipelineCell>>,
}

impl Shared {
    fn write(&self, invoke: impl FnOnce(&mut EventContext<'_>)) {
        let writer = self.writer.borrow().clone();
        if let Some(writer) = writer {
            writer.write(invoke);
        }
    }

    fn schedule_rebuild(&self) {
        let rebuild = self.rebuild.borrow().clone();
        if let Some(rebuild) = rebuild {
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        }
    }

    /// The order the rows are shown in changed: they slide on the next
    /// layout.
    fn reshown(&self) {
        // The generation names no order: the sliver only compares it with the
        // one its last settled pass saw.
        self.generation.set(self.generation.get().wrapping_add(1));
        self.schedule_rebuild();
    }

    /// The laid-out size of the row published into `anchor`, or `None` when it
    /// is not laid out or a layout pass is running.
    fn row_size(&self, anchor: &SubtreeAnchor) -> Option<Size> {
        let id = anchor.get()?;
        let owner = self.pipeline.borrow().as_ref()?.upgrade()?;
        owner.try_with_layout(|owner| owner.box_size(id))?
    }

    /// Where the top-left corner of the node published into `anchor` is
    /// painted in the window, or `None` when it is not laid out.
    fn painted_at(&self, anchor: &SubtreeAnchor) -> Option<Offset<f64>> {
        let id = anchor.get()?;
        let owner = self.pipeline.borrow().as_ref()?.upgrade()?;
        owner
            .try_with(|owner| owner.local_to_global(id, Point::new(0.0, 0.0), None))
            .flatten()
            .map(|at| Offset::new(at.x, at.y))
            .filter(|at| at.dx.is_finite() && at.dy.is_finite())
    }

    /// The record of the item keyed `key`, while it is in the list.
    fn record(&self, key: &dyn ViewKey) -> Option<ItemRecord> {
        record_in(&self.records.borrow(), key)
    }

    /// Where the lead drag shows its item now; `None` while no drag is over
    /// a row, which shows every row where the model has it.
    fn gap(&self, view: &Config) -> Option<Gap> {
        let (key, over) = {
            let drags = self.drags.borrow();
            let lead = drags.first()?;
            (Rc::clone(&lead.key), lead.over?)
        };
        let from = view.index_of(&*key)?;
        (over < view.item_count).then_some(Gap { from, over })
    }

    /// Whether `key` is the lead drag's item, which its row shows as a gap.
    fn is_gap(&self, key: &dyn ViewKey) -> bool {
        self.drags
            .borrow()
            .first()
            .is_some_and(|lead| lead.key.key_eq(key))
    }

    /// A drag of `key` started: any state left from an earlier drag of it is
    /// forgotten, and its lift begins.
    fn started(&self, key: &Rc<dyn ViewKey>) {
        let lift = {
            let vsync = self.vsync.borrow();
            AnimationController::builder(LIFT)
                .debug_label("Reorderable.lift")
                .build_on(vsync.as_ref())
        };
        if let Err(error) = lift.controller().forward() {
            tracing::warn!(%error, "a reorderable item's lift did not start; it shows lifted");
            let _ = lift.controller().set_value(1.0);
        }
        let replaced = {
            let mut drags = self.drags.borrow_mut();
            let replaced = drags
                .iter()
                .position(|drag| drag.key.key_eq(&**key))
                .map(|at| drags.remove(at));
            drags.push(RowDrag {
                key: Rc::clone(key),
                over: None,
                row_reported: false,
                shifted: None,
                lift,
                landing: None,
                left_over: None,
            });
            replaced
        };
        if let Some(mut replaced) = replaced {
            replaced.lift.dispose();
        }
        // The row becomes the gap the proxy leaves.
        self.schedule_rebuild();
    }

    /// The lift of the drag of `key`; at rest, lifted, when it has none.
    fn lift(&self, key: &dyn ViewKey) -> Rc<dyn Animation<f64>> {
        let lift = self
            .drags
            .borrow()
            .iter()
            .find(|drag| drag.key.key_eq(key))
            .map(|drag| drag.lift.controller().clone());
        match lift {
            Some(lift) => Rc::new(lift),
            None => Rc::new(flui_animation::UNIT_END_ANIMATION),
        }
    }

    /// What a drag of the item at `index`, keyed `key`, shows under the
    /// pointer: `proxy` dressed by the decorator.
    fn proxy(&self, proxy: BoxedView, index: usize, key: &dyn ViewKey) -> BoxedView {
        let lift = self.lift(key);
        let decorator = self.view.borrow().proxy_decorator.clone();
        match decorator {
            Some(decorator) => decorator(proxy, index, lift),
            None => lifted(proxy, lift),
        }
    }

    /// Run `change` on the state of the drag of `key`, if it runs; the cell
    /// is released before the caller runs app code.
    fn with_drag<R>(
        &self,
        key: &dyn ViewKey,
        change: impl FnOnce(&mut RowDrag, bool) -> R,
    ) -> Option<R> {
        let mut drags = self.drags.borrow_mut();
        let at = drags.iter().position(|drag| drag.key.key_eq(key))?;
        Some(change(&mut drags[at], at == 0))
    }

    /// The drag of `key` is over `over` now; the preview hears of each
    /// change, per dragged item, and a change of the lead's moves the gap.
    fn hover(&self, key: &Rc<dyn ViewKey>, over: Option<usize>, shifted: Option<&Rc<dyn ViewKey>>) {
        let Some((changed, lead, replaced)) = self.with_drag(&**key, |drag, lead| {
            let changed = std::mem::replace(&mut drag.over, over) != over;
            if over.is_some() {
                drag.left_over = None;
            }
            let replaced = if changed {
                std::mem::replace(&mut drag.shifted, shifted.cloned())
            } else {
                None
            };
            (changed, lead, replaced)
        }) else {
            return;
        };
        // Dropped once the cell is released: a key's destructor is app code.
        drop(replaced);
        if !changed {
            return;
        }
        if lead {
            self.reshown();
        }
        let view = self.view.borrow().clone();
        let (Some(preview), Some(from)) = (view.on_reorder_preview.clone(), view.index_of(&**key))
        else {
            return;
        };
        self.write(|ctx| preview(ctx, ReorderPreview { from, over }));
    }

    /// A drag of `key` is over the row of the item keyed `row`.
    ///
    /// The lead drag puts its item where that row is shown, and the rows
    /// between make room; over its own row, the gap, it stays where it is.
    /// Any other drag is over the row's item.
    fn over_row(&self, key: &Rc<dyn ViewKey>, row: &Rc<dyn ViewKey>) {
        let view = self.view.borrow().clone();
        let gap = self.gap(&view);
        let Some(item) = view.index_of(&**row) else {
            return;
        };
        let Some((report, cleared)) = self.with_drag(&**key, |drag, lead| {
            drag.row_reported = true;
            if !lead {
                return (Some((Some(item), None)), None);
            }
            if row.key_eq(&**key) {
                let cleared = drag.shifted.take();
                return (Some((drag.over.or(Some(item)), None)), cleared);
            }
            if drag
                .shifted
                .as_ref()
                .is_some_and(|shifted| shifted.key_eq(&**row))
            {
                return (None, None);
            }
            (
                Some((Some(Gap::slot_of(gap, item)), Some(Rc::clone(row)))),
                None,
            )
        }) else {
            return;
        };
        // Dropped once the cell is released: a key's destructor is app code.
        drop(cleared);
        if let Some((over, shifted)) = report {
            self.hover(key, over, shifted.as_ref());
        }
    }

    /// Whether a row answered the drag of `key` since the list last asked.
    fn take_row_reported(&self, key: &dyn ViewKey) -> bool {
        self.with_drag(key, |drag, _| std::mem::take(&mut drag.row_reported))
            .unwrap_or(false)
    }

    /// The drag of `key` moved over no row inside the list: the lead keeps
    /// its gap, which a drop there takes, and any other is over nothing.
    fn over_no_row(&self, key: &Rc<dyn ViewKey>) {
        if self.with_drag(&**key, |_, lead| lead) == Some(false) {
            self.hover(key, None, None);
        }
    }

    /// The drag of `key` was let go over the list: the item goes where the
    /// drag shows it.
    fn dropped(&self, ctx: &mut EventContext<'_>, key: &dyn ViewKey) {
        if let Some(Some(to)) = self.with_drag(key, |drag, _| drag.over.or(drag.left_over)) {
            let _ = self.reorder(ctx, key, to, ReorderCause::Drag);
        }
    }

    /// The proxy of the drag of `key` was at `proxy` in the window when the
    /// drag ended: the item's row lands from there. Read before anything the
    /// end changes is laid out, while the row is painted where it was.
    fn proxy_ended(&self, key: &dyn ViewKey, proxy: Offset<f64>) {
        let painted = self
            .record(key)
            .and_then(|record| self.painted_at(&record.anchor));
        if let Some(painted) = painted {
            let _ = self.with_drag(key, |drag, _| drag.landing = Some(proxy - painted));
        }
    }

    /// The drag of `key` ended, dropped or not: its row lands from where the
    /// proxy was, the rows close the gap, and the drag is forgotten.
    fn ended(&self, key: &Rc<dyn ViewKey>) {
        let preview = self
            .with_drag(&**key, |drag, _| drag.over.is_some())
            .unwrap_or(false);
        let ended = {
            let mut drags = self.drags.borrow_mut();
            drags
                .iter()
                .position(|drag| drag.key.key_eq(&**key))
                .map(|at| drags.remove(at))
        };
        let Some(mut ended) = ended else {
            return;
        };
        ended.lift.dispose();
        self.reshown();
        if let (Some(landing), Some(record)) = (ended.landing, self.record(&**key)) {
            record.slot.displace(landing, self.generation.get());
        }
        // The preview hears that the drag is over no row now.
        let view = self.view.borrow().clone();
        if preview
            && let (Some(callback), Some(from)) =
                (view.on_reorder_preview.clone(), view.index_of(&**key))
        {
            self.write(|ctx| callback(ctx, ReorderPreview { from, over: None }));
        }
    }

    /// The drag of `key` left the list: over no row, and the gap closes.
    /// Where it was is kept for a drop: the drag machinery tells every
    /// target but the one dropped on that the drag left before the drop.
    fn left(&self, key: &Rc<dyn ViewKey>) {
        let _ = self.with_drag(&**key, |drag, _| {
            drag.left_over = drag.over.or(drag.left_over);
        });
        self.hover(key, None, None);
    }

    /// Move the item keyed `key` to `to`, and announce its new position;
    /// nothing when it is there already.
    fn reorder(
        &self,
        ctx: &mut EventContext<'_>,
        key: &dyn ViewKey,
        to: usize,
        cause: ReorderCause,
    ) -> bool {
        let view = self.view.borrow().clone();
        let Some(from) = view.index_of(key) else {
            return false;
        };
        if from == to || to >= view.item_count {
            return false;
        }
        // Named before the app's model moves the item.
        let label = view.item_label.as_ref().map(|label| label(from));
        let message = {
            let strings = self.strings.borrow();
            match strings.as_deref() {
                Some(strings) => {
                    strings.reorder_item_moved(label.as_deref(), to + 1, view.item_count)
                }
                None => DefaultWidgetsLocalizations.reorder_item_moved(
                    label.as_deref(),
                    to + 1,
                    view.item_count,
                ),
            }
        };
        // The rows slide to the order the app's rebuild gives them.
        self.reshown();
        (view.on_reorder)(ctx, Reorder { from, to, cause });
        let announcer = self.announcer.borrow().clone();
        if let Some(announcer) = announcer {
            let _ = announcer.announce(message, Urgency::Polite);
        }
        true
    }
}

impl Shared {
    fn new(view: &Config) -> Rc<Self> {
        let clock = AnimationController::builder(SLIDE)
            .debug_label("Reorderable.slide")
            .build_on(None);
        // At rest until the first slide.
        let _ = clock.controller().set_value(1.0);
        Rc::new(Self {
            view: RefCell::new(view.clone()),
            grid: Rc::new(Cell::new(None)),
            drags: RefCell::new(Vec::new()),
            clock: RefCell::new(clock),
            generation: Cell::new(0),
            records: RefCell::new(HashMap::new()),
            vsync: RefCell::new(None),
            rebuild: RefCell::new(None),
            writer: RefCell::new(None),
            announcer: RefCell::new(None),
            strings: RefCell::new(None),
            pipeline: RefCell::new(None),
        })
    }

    /// Take the capabilities the rows' moves use.
    fn acquire(&self, ctx: &dyn LifecycleContext) {
        *self.writer.borrow_mut() = Some(ctx.writer_source());
        *self.announcer.borrow_mut() = ctx.announcer();
        *self.pipeline.borrow_mut() = ctx.pipeline_owner().map(|owner| owner.downgrade());
        *self.rebuild.borrow_mut() = Some(ctx.rebuild_handle());
        let vsync = VsyncScope::maybe_of(ctx);
        let rebound = {
            let mut clock = self.clock.borrow_mut();
            let mut drags = self.drags.borrow_mut();
            VsyncUpdate::run(|update| {
                update.rebind_controller(&mut clock, vsync.as_ref());
                for drag in drags.iter_mut() {
                    update.rebind_controller(&mut drag.lift, vsync.as_ref());
                }
            })
        };
        // Logged once the cells are released: a subscriber is app code.
        if let Err(error) = rebound {
            tracing::error!(%error, "a reorderable list lost its frame registry");
        }
        *self.vsync.borrow_mut() = vsync;
    }

    /// Stop every clock: the list leaves the tree.
    fn dispose(&self) {
        let drags = std::mem::take(&mut *self.drags.borrow_mut());
        for mut drag in drags {
            drag.lift.dispose();
        }
        self.clock.borrow_mut().dispose();
    }

    /// The items' records in the order the rows are shown in, kept for the
    /// items still in the list and made for the new ones.
    fn records_in(&self, view: &Config, gap: Option<Gap>) -> Rc<[Rc<PlacementSlot>]> {
        let previous = std::mem::take(&mut *self.records.borrow_mut());
        let mut records = Records::with_capacity(view.item_count);
        let slots = (0..view.item_count)
            .map(|slot| {
                let key = (view.key_of)(Gap::item_at(gap, slot));
                let record = record_in(&previous, &*key).unwrap_or_else(|| ItemRecord {
                    key: Rc::clone(&key),
                    slot: Rc::new(PlacementSlot::new()),
                    anchor: SubtreeAnchor::new(),
                });
                let placement = Rc::clone(&record.slot);
                records.entry(key.key_hash()).or_default().push(record);
                placement
            })
            .collect();
        *self.records.borrow_mut() = records;
        // Dropped once the cell is released: a key's destructor is app code.
        drop(previous);
        slots
    }

    /// The list or grid of `view`'s rows in its scrollable, as one drop
    /// target around them.
    fn build(self: &Rc<Self>, view: &Config, ctx: &dyn BuildContext) -> BoxedView {
        *self.strings.borrow_mut() = BoxedWidgetsLocalizations::maybe_of(ctx);
        let _prev = self.view.replace(view.clone());
        let gap = self.gap(view);
        let placement = Placement::new(
            self.clock.borrow().controller().clone(),
            MotionSpec::default().with_duration(SLIDE),
            self.records_in(view, gap),
            self.generation.get(),
        );
        let shared = Rc::clone(self);
        let config = view.clone();
        let rows = config.clone();
        let keys = config.clone();
        let grid = Rc::clone(&shared.grid);
        let row = move |slot: usize| {
            (slot < rows.item_count).then(|| {
                let index = Gap::item_at(gap, slot);
                let key = (rows.key_of)(index);
                let record = shared.record(&*key);
                ReorderRow {
                    index,
                    hidden: shared.is_gap(&*key),
                    anchor: record.map_or_else(SubtreeAnchor::new, |record| record.anchor),
                    key,
                    list: Rc::clone(&shared),
                }
                .boxed()
            })
        };
        let find =
            move |key: &dyn ViewKey| keys.index_of(key).map(|index| Gap::slot_of(gap, index));
        let scrollable = match &config.layout {
            Layout::List {
                item_extent_estimate,
            } => ListView::builder(config.item_count, *item_extent_estimate, row)
                .scroll_direction(config.scroll_direction)
                .find_index_by_key(find)
                .placement(placement)
                .focusable(true)
                .boxed(),
            Layout::Grid { delegate } => {
                let recorder = ShapeRecorder {
                    inner: Rc::clone(delegate),
                    shape: grid,
                };
                GridView::builder(recorder, config.item_count, row)
                    .scroll_direction(config.scroll_direction)
                    .find_index_by_key(find)
                    .placement(placement)
                    .focusable(true)
                    .boxed()
            }
        };
        // The rows report where a drag is. The list, which answers each move
        // after the rows under it, hears a move no row answered (the space
        // after the last row) and the drag leaving it; a row's own leave
        // would report it between two rows. A drop the rows do not take
        // lands where the lead drag shows its item.
        let between = Rc::clone(self);
        let dropped = Rc::clone(self);
        let left = Rc::clone(self);
        DropTarget::bare(
            move |drag: &DragView<'_>| {
                let payload = drag.value::<RowPayload>()?;
                if !Rc::ptr_eq(&payload.list, &between) {
                    return None;
                }
                if !between.take_row_reported(&*payload.key) {
                    between.over_no_row(&payload.key);
                }
                between
                    .with_drag(&*payload.key, |drag, _| drag.over.is_some())
                    .unwrap_or(false)
                    .then_some(TransferActions::MOVE)
            },
            move |_hover| scrollable.clone(),
        )
        .on_drop(move |ctx, drag, _at, _action| {
            if let Some(payload) = drag.value::<RowPayload>()
                && Rc::ptr_eq(&payload.list, &dropped)
            {
                dropped.dropped(ctx, &*payload.key);
            }
        })
        .on_leave(move |drag| {
            if let Some(payload) = drag.value::<RowPayload>()
                && Rc::ptr_eq(&payload.list, &left)
            {
                left.left(&payload.key);
            }
        })
        .boxed()
    }
}

/// The default proxy decorator: `proxy` rises to elevation 6 and grows to
/// 102 % as `lift` runs.
fn lifted(proxy: BoxedView, lift: Rc<dyn Animation<f64>>) -> BoxedView {
    let listenable: Rc<dyn flui_foundation::Listenable> = lift.clone();
    AnimatedBuilder::new(listenable, move || {
        let t = lift.value();
        let t = if t.is_finite() {
            t.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let scale = 1.0 + (LIFT_SCALE - 1.0) * t;
        PhysicalModel::new(Color::TRANSPARENT)
            .elevation(LIFT_ELEVATION * t)
            .child(
                Transform::scale(scale, scale)
                    .alignment(flui_painting::Alignment::CENTER)
                    .child(proxy.clone()),
            )
    })
    .boxed()
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ReorderableListState {
            shared: Shared::new(&self.config),
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

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        ReorderableGridState {
            shared: Shared::new(&self.config),
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

// ============================================================================
// A row
// ============================================================================

/// Makes a row's drag handles: each one drags the row it was made for.
struct HandleSource {
    list: Rc<Shared>,
    key: Rc<dyn ViewKey>,
    index: usize,
    feedback: BoxedView,
    /// Where the row's content is published: the box the proxy shows and
    /// the row lands in.
    content: SubtreeAnchor,
    menu: bool,
    /// Where a press on the handle moves to drag ([`Config::handle_axis`]).
    axis: MultiDragAxis,
}

impl HandleSource {
    /// A draggable of the row, started by `devices`; its child sits in the
    /// row's content at the place `child_offset` reads as a drag starts.
    fn draggable(
        &self,
        child: BoxedView,
        devices: PointerKindSet,
        child_offset: Rc<dyn Fn() -> Offset<f64>>,
    ) -> Draggable {
        let list = Rc::clone(&self.list);
        let key = Rc::clone(&self.key);
        let (started, started_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (ended, ended_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (shown, shown_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (landed, landed_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let (feedback, index) = (self.feedback.clone(), self.index);
        Draggable::bare(
            move || {
                vec![DragItem::new().with_value(RowPayload {
                    list: Rc::clone(&list),
                    key: Rc::clone(&key),
                })]
            },
            child,
        )
        .devices(devices)
        .reach(DragReach::Window)
        .feedback(move || shown.proxy(feedback.clone(), index, &*shown_key))
        .feedback_frame(FeedbackFrame {
            child_offset,
            ended: Rc::new(move |proxy| landed.proxy_ended(&*landed_key, proxy)),
        })
        .on_drag_started(move |_ctx| started.started(&started_key))
        // A drag that ends anywhere, dropped or not, is over no row now.
        .on_drag_end(move |_ctx, _outcome| {
            ended.ended(&ended_key);
        })
    }

    fn handle(&self, child: BoxedView) -> BoxedView {
        // All of the handle's box takes the press, not only what it paints,
        // and a press that moves along the list drags the row: on the list's
        // axis the handle is deeper than the list's own scroll, so it wins.
        // A grid's handle drags on a press that moves any way, and keeps the
        // contact from the grid's scroll, which would take any motion along
        // its axis from a free drag (ADR-0208 rule 1).
        let child = Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .child(child)
            .boxed();
        // The proxy lifts from the row's content, not from the handle: the
        // handle's place in the content is read as the drag starts.
        let at = SubtreeAnchor::new();
        let child = AnchoredBox::new(at.clone(), child).boxed();
        let (list, content) = (Rc::clone(&self.list), self.content.clone());
        let child_offset =
            Rc::new(
                move || match (list.painted_at(&at), list.painted_at(&content)) {
                    (Some(handle), Some(content)) => handle - content,
                    _ => Offset::ZERO,
                },
            );
        let handle = self
            .draggable(child, PointerKindSet::all(), child_offset)
            .axis(self.axis)
            .boxed();
        let handle = match self.axis {
            MultiDragAxis::Free => PanClaimRegion::new(PanClaim::None, handle).boxed(),
            MultiDragAxis::Horizontal | MultiDragAxis::Vertical => handle,
        };
        if self.menu {
            DragHandle { child: handle }.boxed()
        } else {
            handle
        }
    }
}

/// A handle whose tap opens its row's menu.
#[derive(Clone)]
struct DragHandle {
    child: BoxedView,
}

impl View for DragHandle {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for DragHandle {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let chain = ambient_action_chain(ctx);
        GestureDetector::new()
            .on_tap(move |ctx| {
                if let Some(open) = chain
                    .as_ref()
                    .and_then(|chain| resolve(chain, &ShowContextMenuIntent))
                {
                    open.invoke(ctx, &ShowContextMenuIntent);
                }
            })
            .child(self.child.clone())
    }
}

/// The handle the list puts before a row's content: three bars, centred in
/// a square as large as the platform's smallest touch target.
fn default_handle() -> BoxedView {
    let bar = || {
        SizedBox::new(18.0, 2.0)
            .child(ColoredBox::new(Color::rgb(0x49, 0x45, 0x4F)))
            .boxed()
    };
    let gap = || SizedBox::new(18.0, 3.0).boxed();
    Center::new()
        .child(
            Column::new(vec![bar(), gap(), bar(), gap(), bar()])
                .main_axis_size(flui_objects::MainAxisSize::Min),
        )
        .boxed()
}

/// `content` after `handle`, which sits in a square `side` wide (the touch
/// minimum) at the row's reading start in a vertical list, and on top in a
/// horizontal list or a grid's tile.
///
/// The handle comes before the content because keyboard focus in the row
/// reveals the row's menu control at the trailing edge, where it would
/// cover a handle.
fn beside_handle(content: BoxedView, handle: BoxedView, side: f64, view: &Config) -> BoxedView {
    let handle = SizedBox::new(side, side).child(handle).boxed();
    let content = Expanded::new(content).boxed();
    match (&view.layout, view.scroll_direction) {
        (Layout::List { .. }, Axis::Vertical) => Row::new(vec![handle, content]).boxed(),
        (Layout::List { .. }, Axis::Horizontal) | (Layout::Grid { .. }, _) => {
            Column::new(vec![handle, content]).boxed()
        }
    }
}

/// One row: its moves, its menu or controls, the drop target it is and the
/// long press that drags it.
#[derive(Clone)]
struct ReorderRow {
    /// The item's index in the app's model, wherever the row is shown.
    index: usize,
    key: Rc<dyn ViewKey>,
    list: Rc<Shared>,
    /// Whether the row is the gap the lead drag's item leaves: laid out and
    /// still a drop target, but not painted.
    hidden: bool,
    /// Where the row's content is published, kept for the item.
    anchor: SubtreeAnchor,
}

impl View for ReorderRow {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        Some(&*self.key)
    }
}

impl StatelessView for ReorderRow {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let view = self.list.view.borrow().clone();
        let index = self.index;
        let menu = view.controls.is_none();
        let item = |handle: &HandleSource| {
            (view.item)(&ReorderableItem {
                index,
                key: &*self.key,
                handle,
            })
        };
        // What the drag shows under the pointer. Never the row's own content
        // built again: that would mount a second copy of whatever the row
        // owns alone (a field's focus node, a controller).
        let anchor = self.anchor.clone();
        let bare_source = HandleSource {
            list: Rc::clone(&self.list),
            key: Rc::clone(&self.key),
            index,
            feedback: SizedBox::shrink().boxed(),
            content: anchor.clone(),
            menu,
            axis: view.handle_axis(),
        };
        let feedback = match &view.feedback {
            Some(feedback) => feedback(&ReorderableItem {
                index,
                key: &*self.key,
                handle: &bare_source,
            }),
            None => match view.item_label.as_ref().map(|label| label(index)) {
                Some(label) => Text::new(label).boxed(),
                None => RowPlate {
                    list: Rc::clone(&self.list),
                    anchor: anchor.clone(),
                    measured: Rc::default(),
                }
                .boxed(),
            },
        };
        let source = HandleSource {
            feedback,
            ..bare_source
        };
        let content = item(&source);
        let rtl = Directionality::maybe_of(ctx) == Some(TextDirection::Rtl);
        let content = match view.handles {
            ReorderHandles::Default => {
                let side = GestureArenaScope::settings_or_default_of(ctx)
                    .min_target_for(flui_interaction::PointerKind::Touch);
                let handle = source.handle(default_handle());
                beside_handle(content, handle, side, &view)
            }
            ReorderHandles::Custom => content,
        };
        let content = if view.long_press {
            source
                .draggable(
                    content,
                    PointerKindSet::TOUCH | PointerKindSet::PEN,
                    Rc::new(|| Offset::ZERO),
                )
                .trigger(flui_interaction::drag::DragTrigger::LongPress)
                .boxed()
        } else {
            content
        };

        // The row publishes its render node, so a drag without a label shows
        // a plate of the row's size, the proxy lifts from it and the row
        // lands from where the proxy was let go.
        let content = AnchoredBox::new(anchor, content).boxed();
        let token = Rc::clone(&self.list);
        let hover = Rc::clone(&self.list);
        let row = Rc::clone(&self.key);
        let target = DropTarget::bare(
            move |drag: &DragView<'_>| {
                let payload = drag.value::<RowPayload>()?;
                if !Rc::ptr_eq(&payload.list, &token) {
                    return None;
                }
                hover.over_row(&payload.key, &row);
                Some(TransferActions::MOVE)
            },
            move |_hover| content.clone(),
        )
        .on_drop({
            let list = Rc::clone(&self.list);
            move |ctx, drag, _at, _action| {
                if let Some(payload) = drag.value::<RowPayload>()
                    && Rc::ptr_eq(&payload.list, &list)
                {
                    list.dropped(ctx, &*payload.key);
                }
            }
        });

        let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
        let strings: &dyn WidgetsLocalizations = localizations
            .as_deref()
            .map_or(&DefaultWidgetsLocalizations, |boxed| &**boxed);
        let moves = moves(strings, &view, self.list.grid.get(), index, rtl);
        // The menu, controls and assistive actions offer the moves that apply
        // now; the keys are bound for every move, and the action decides when
        // it runs from where the model has the row then.
        let commands = moves
            .iter()
            .filter(|(_, _, _, applies)| *applies)
            .map(|(command, intent, key, _)| (command.clone(), *intent, key.clone()))
            .collect::<Vec<_>>();
        let label = view.item_label.as_ref().map(|label| label(index));
        let row = match commands.split_first() {
            None => target.boxed(),
            Some(((first, _, _), more)) if menu => {
                let area = ContextMenuArea::try_new(
                    first.clone(),
                    more.iter().map(|(command, _, _)| command.clone()),
                )
                .expect("BUG: the move commands have distinct labels and keys");
                match &label {
                    Some(label) => area.target(label.clone()),
                    None => area,
                }
                .child(target)
                .boxed()
            }
            Some(((first, intent, _), more)) => {
                let placement = view.controls.unwrap_or_default();
                let spec = more.iter().fold(
                    ControlSpec::new(first.clone(), Text::new(arrow(*intent))).placement(placement),
                    |spec, (command, intent, _)| {
                        spec.control(command.clone(), Text::new(arrow(*intent)))
                    },
                );
                let alternatives = GestureAlternatives::try_new(Alternative::VisibleControl(spec))
                    .expect("BUG: the move commands have distinct labels and keys");
                match &label {
                    Some(label) => alternatives.target(label.clone()),
                    None => alternatives,
                }
                .child(target)
                .boxed()
            }
        };
        // The moves' Alt keys are bound by the row, not as the commands'
        // keys: ADR-0216 §1 keeps arrows, Home and End from gesture commands
        // for text editing, and a text field inside the row still takes them
        // first, since it handles its keys before they reach the row.
        let row = moves
            .iter()
            .fold(Shortcuts::new(row), |shortcuts, (_, intent, key, _)| {
                shortcuts.shortcut(key.clone(), *intent)
            });
        let row = Actions::new(row).action(RowMoves {
            list: Rc::clone(&self.list),
            key: Rc::clone(&self.key),
            rtl,
        });
        // Always the same wrapper, so the row keeps its state when it becomes
        // the gap and when it lands.
        Opacity::new(if self.hidden { 0.0 } else { 1.0 }).child(row)
    }
}

/// What a drag of a row without a label shows: a plate the row's size in the
/// focus ring's colors, so the drag is seen; nothing while the row is not
/// laid out.
#[derive(Clone)]
struct RowPlate {
    list: Rc<Shared>,
    anchor: SubtreeAnchor,
    /// The row's size as last measured: a rebuild of the row during the
    /// drag publishes it under a new anchor, and the plate keeps its size.
    measured: Rc<std::cell::Cell<Option<Size>>>,
}

impl View for RowPlate {
    fn create_element(self: std::rc::Rc<Self>) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for RowPlate {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let measured = self.list.row_size(&self.anchor).or(self.measured.get());
        self.measured.set(measured);
        let Some(size) = measured else {
            return SizedBox::shrink().boxed();
        };
        let style = DefaultFocusRingStyle::of(ctx);
        let plate = BoxDecoration::new()
            .set_color(Some(style.inner.with_alpha(0xCC)))
            .set_border(Some(Border::all(BorderSide::new(
                style.outer,
                style.outer_width.max(1.0),
                BorderStyle::Solid,
            ))));
        SizedBox::new(size.width, size.height)
            .child(DecoratedBox::new(plate))
            .boxed()
    }
}

/// The glyph a visible control for `intent` shows.
fn arrow(intent: ReorderIntent) -> &'static str {
    match intent {
        ReorderIntent::Start => "\u{2912}",
        ReorderIntent::Up => "\u{2191}",
        ReorderIntent::Down => "\u{2193}",
        ReorderIntent::End => "\u{2913}",
        ReorderIntent::Left => "\u{2190}",
        ReorderIntent::Right => "\u{2192}",
    }
}

/// Every move of a row of `view` (laid out as `shape` when it is a grid), in
/// menu order, with whether it applies to the row at `index`: a list's moves
/// along it, a grid's along its rows and across them.
fn moves(
    strings: &dyn WidgetsLocalizations,
    view: &Config,
    shape: Option<GridShape>,
    index: usize,
    rtl: bool,
) -> Vec<(GestureCommand, ReorderIntent, SingleActivator, bool)> {
    let up = (
        strings.reorder_item_up(),
        ReorderIntent::Up,
        NamedKey::ArrowUp,
    );
    let down = (
        strings.reorder_item_down(),
        ReorderIntent::Down,
        NamedKey::ArrowDown,
    );
    let left = (
        strings.reorder_item_left(),
        ReorderIntent::Left,
        NamedKey::ArrowLeft,
    );
    let right = (
        strings.reorder_item_right(),
        ReorderIntent::Right,
        NamedKey::ArrowRight,
    );
    let steps = match (&view.layout, view.scroll_direction, rtl) {
        (Layout::List { .. }, Axis::Vertical, _) => vec![up, down],
        (Layout::List { .. }, Axis::Horizontal, false) => vec![left, right],
        (Layout::List { .. }, Axis::Horizontal, true) => vec![right, left],
        (Layout::Grid { .. }, _, _) => vec![up, down, left, right],
    };
    let start = (
        strings.reorder_item_to_start(),
        ReorderIntent::Start,
        NamedKey::Home,
    );
    let end = (
        strings.reorder_item_to_end(),
        ReorderIntent::End,
        NamedKey::End,
    );
    std::iter::once(start)
        .chain(steps)
        .chain(std::iter::once(end))
        .map(|(label, intent, named)| {
            (
                GestureCommand::new(label, intent),
                intent,
                SingleActivator::named(named).alt(),
                move_target(view, shape, rtl, index, intent).is_some(),
            )
        })
        .collect::<Vec<_>>()
}

/// Performs a row's moves.
struct RowMoves {
    list: Rc<Shared>,
    key: Rc<dyn ViewKey>,
    rtl: bool,
}

impl RowMoves {
    fn target(&self, intent: ReorderIntent) -> Option<usize> {
        // The row's index now, not when it was built: the app may have moved
        // its model since, by an earlier command delivered before a rebuild.
        // The key is resolved outside the borrow, since `key_of` is app code.
        // A grid's rows are those of its last layout.
        let view = self.list.view.borrow().clone();
        let index = view.index_of(&*self.key)?;
        move_target(&view, self.list.grid.get(), self.rtl, index, intent)
    }
}

impl Action<ReorderIntent> for RowMoves {
    fn is_enabled(&self, intent: &ReorderIntent) -> bool {
        self.target(*intent).is_some()
    }

    fn invoke(&self, ctx: &mut EventContext<'_>, intent: &ReorderIntent) -> ActionOutcome {
        match self.target(*intent) {
            Some(to)
                if self
                    .list
                    .reorder(ctx, &*self.key, to, ReorderCause::Command) =>
            {
                ActionOutcome::Performed
            }
            _ => ActionOutcome::NotPerformed,
        }
    }
}
