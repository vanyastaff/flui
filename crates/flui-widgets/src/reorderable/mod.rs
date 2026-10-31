//! [`ReorderableList`] — a lazy list whose rows the user moves (ADR-0211 §9).
//!
//! Moving a row is a drag-and-drop session that never leaves the window
//! ([`DragReach::Window`]): a row's handle starts it at once, and on touch and
//! pen a long press on the row starts it too. Every row is a drop target that
//! takes only drags of its own list, so the row under the pointer is where the
//! dragged row lands, and edge autoscroll comes from the list's scrollable.
//! The dragged row keeps its state while it is scrolled out of the built band:
//! its drag holds the row alive.
//!
//! The same moves have a path without a drag (ADR-0216): each row carries the
//! commands "Move to the start", "Move up", "Move down" and "Move to the end"
//! that apply to it, on the platform's keys while focus is in the row, as
//! assistive technology's actions on the row's node, and for one pointer in a
//! context menu opened from the row's handle, a secondary click or the menu
//! key. A command's move is announced with the row's new position.
//!
//! The list moves nothing itself: it calls [`on_reorder`](Reorder) once, and
//! the app moves the item in its model and rebuilds. Rows are keyed by the
//! model key the list is given, so a row's element, state and focus follow
//! its item to the new position.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use flui_foundation::ValueKey;
use flui_foundation::ViewKey;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::Size;
use flui_interaction::drag::{DragItem, DragReach, DragView};
use flui_interaction::{MultiDragAxis, PointerKindSet};
use flui_objects::SubtreeAnchor;
use flui_painting::styling::{Border, BorderSide, BorderStyle, BoxDecoration, Color};
use flui_rendering::pipeline::WeakPipelineCell;

use crate::__private::AnchoredBox;
use crate::{DecoratedBox, DefaultFocusRingStyle};
use flui_painting::typography::TextDirection;
use flui_platform::data_transfer::TransferActions;
use flui_platform::keyboard::NamedKey;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::semantics::{Announcer, Urgency};
use flui_view::element::ElementKind;
use flui_view::prelude::*;
use flui_view::{EventCx, EventOutcome, WriterSource};

use crate::interaction::actions::{ambient_action_chain, resolve};
use crate::interaction::{
    Action, ActionOutcome, Actions, Alternative, ControlPlacement, ControlSpec,
    GestureAlternatives, GestureArenaScope, GestureCommand, Intent, Shortcuts,
    ShowContextMenuIntent, SingleActivator,
};
use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, Directionality, WidgetsLocalizations,
};
use crate::{
    Center, ColoredBox, Column, ContextMenuArea, Draggable, DropTarget, Expanded, GestureDetector,
    ListView, Listener, Row, Scrollable, SizedBox, Text,
};

/// A row's move, as [`ReorderableList`] reports it once: the item at `from`
/// goes to `to`.
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

/// What moved a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderCause {
    /// A drag of the row, dropped on another row.
    Drag,
    /// A move command: its keys, assistive technology's action or the menu.
    Command,
}

/// Where a drag of a row is while it runs, for a list that shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct ReorderPreview {
    /// Where the dragged item is.
    pub from: usize,
    /// The row the drag is over, which a drop now would move it to; `None`
    /// once it left the rows.
    pub over: Option<usize>,
}

/// Where a row's drag handle comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderHandles {
    /// The list puts a handle before each row's content.
    #[default]
    Default,
    /// Each row places its own, through [`ReorderableItem::handle`].
    Custom,
}

/// One move of a reorderable row, for the platform's keys, assistive
/// technology and the row's menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReorderIntent {
    /// To the first position.
    Start,
    /// One position back in a vertical list.
    Up,
    /// One position forward in a vertical list.
    Down,
    /// To the last position.
    End,
    /// One position to the left in a horizontal list: back in a
    /// left-to-right locale, forward in a right-to-left one.
    Left,
    /// One position to the right in a horizontal list.
    Right,
}

impl Intent for ReorderIntent {}

/// What [`ReorderableList`]'s item builder knows of the row it builds.
pub struct ReorderableItem<'a> {
    index: usize,
    key: &'a dyn ViewKey,
    handle: &'a HandleSource,
}

impl ReorderableItem<'_> {
    /// The row's index.
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// The row's model key.
    #[must_use]
    pub fn key(&self) -> &dyn ViewKey {
        self.key
    }

    /// `child` as the row's drag handle: a press on it that moves drags the
    /// row, and a tap opens the row's menu. Place one in each row of a list
    /// with [`ReorderHandles::Custom`].
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
type ReorderFn = Rc<dyn Fn(&mut EventCx<'_>, Reorder)>;
type PreviewFn = Rc<dyn Fn(&mut EventCx<'_>, ReorderPreview)>;
type LabelFn = Rc<dyn Fn(usize) -> String>;

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
///     move |_cx, change| {
///         let mut items = model.borrow_mut();
///         let item = items.remove(change.from);
///         items.insert(change.to, item);
///     },
/// );
/// ```
#[derive(Clone)]
pub struct ReorderableList {
    item_count: usize,
    key_of: KeyFn,
    item: ItemFn,
    feedback: Option<ItemFn>,
    on_reorder: ReorderFn,
    on_reorder_preview: Option<PreviewFn>,
    item_label: Option<LabelFn>,
    handles: ReorderHandles,
    long_press: bool,
    controls: Option<ControlPlacement>,
    scroll_direction: Axis,
    item_extent_estimate: f64,
}

impl fmt::Debug for ReorderableList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReorderableList")
            .field("item_count", &self.item_count)
            .field("handles", &self.handles)
            .field("long_press", &self.long_press)
            .field("scroll_direction", &self.scroll_direction)
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
        on_reorder: impl Fn(&mut EventCx<'_>, Reorder) -> R + 'static,
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
            on_reorder: Rc::new(move |cx, change| on_reorder(cx, change).report()),
            on_reorder_preview: None,
            item_label: None,
            handles: ReorderHandles::default(),
            long_press: true,
            controls: None,
            scroll_direction: Axis::Vertical,
            item_extent_estimate: 56.0,
        }
    }

    /// What a drag of a row shows under the pointer; the row's label
    /// ([`item_label`](Self::item_label)) by default, nothing without one.
    ///
    /// `feedback` builds a second view beside the row, which stays mounted
    /// while it is dragged: it must not reuse what the row owns alone, such
    /// as a text field's focus node or a controller.
    #[must_use]
    pub fn feedback<V: IntoView>(
        mut self,
        feedback: impl Fn(&ReorderableItem<'_>) -> V + 'static,
    ) -> Self {
        self.feedback = Some(Rc::new(move |row| feedback(row).into_view().boxed()));
        self
    }

    /// Called as a drag of a row moves over the rows: where a drop would put
    /// it now.
    #[must_use]
    pub fn on_reorder_preview<F, R>(mut self, callback: F) -> Self
    where
        F: Fn(&mut EventCx<'_>, ReorderPreview) -> R + 'static,
        R: EventOutcome,
    {
        self.on_reorder_preview = Some(Rc::new(move |cx, preview| callback(cx, preview).report()));
        self
    }

    /// Name each row ("Milk"), for its menu's labels and the announcement of
    /// its move.
    #[must_use]
    pub fn item_label(mut self, label: impl Fn(usize) -> String + 'static) -> Self {
        self.item_label = Some(Rc::new(label));
        self
    }

    /// Where the drag handles come from; [`ReorderHandles::Default`] by default.
    #[must_use]
    pub fn handles(mut self, handles: ReorderHandles) -> Self {
        self.handles = handles;
        self
    }

    /// Whether a touch or pen long press on a row drags it, as well as its
    /// handle; on by default. A mouse drags only by the handle.
    #[must_use]
    pub fn long_press(mut self, long_press: bool) -> Self {
        self.long_press = long_press;
        self
    }

    /// Show each row's move commands as controls at `placement` instead of in
    /// a menu opened from its handle.
    #[must_use]
    pub fn visible_controls(mut self, placement: ControlPlacement) -> Self {
        self.controls = Some(placement);
        self
    }

    /// The list's axis; vertical by default.
    #[must_use]
    pub fn scroll_direction(mut self, axis: Axis) -> Self {
        self.scroll_direction = axis;
        self
    }

    /// The row extent the list assumes before rows are measured.
    ///
    /// # Panics
    ///
    /// When the list builds, if `extent` is not finite and positive.
    #[must_use]
    pub fn item_extent_estimate(mut self, extent: f64) -> Self {
        self.item_extent_estimate = extent;
        self
    }

    /// The index of the item keyed `key`, by the list's keys now.
    fn index_of(&self, key: &dyn ViewKey) -> Option<usize> {
        (0..self.item_count).find(|&index| (self.key_of)(index).key_eq(key))
    }
}

// ============================================================================
// The list
// ============================================================================

/// What one drag of a row carries: its list, so only that list's rows take
/// it, and the dragged item's key, so a drop finds where the item is now.
struct RowPayload {
    list: Rc<Shared>,
    key: Rc<dyn ViewKey>,
}

/// One mounted list, as its rows share it.
struct Shared {
    view: RefCell<ReorderableList>,
    /// The row the live drag is over.
    over: Cell<Option<usize>>,
    /// Whether a row reported the drag over it since the list last looked:
    /// the rows answer a move before the list around them does.
    row_reported: Cell<bool>,
    writer: RefCell<Option<WriterSource>>,
    announcer: RefCell<Option<Announcer>>,
    /// The localizations the list was last built with, for its announcements.
    strings: RefCell<Option<std::sync::Arc<BoxedWidgetsLocalizations>>>,
    /// The render tree the rows lay out in, for the size of a dragged row.
    pipeline: RefCell<Option<WeakPipelineCell>>,
}

impl Shared {
    fn write(&self, invoke: impl FnOnce(&mut EventCx<'_>)) {
        let writer = self.writer.borrow().clone();
        if let Some(writer) = writer {
            writer.write(invoke);
        }
    }

    /// The laid-out size of the row published into `anchor`, or `None` when it
    /// is not laid out or a layout pass is running.
    fn row_size(&self, anchor: &SubtreeAnchor) -> Option<Size> {
        let id = anchor.get()?;
        let owner = self.pipeline.borrow().as_ref()?.upgrade()?;
        owner.try_with_layout(|owner| owner.box_size(id))?
    }

    /// Where the model has the item keyed `key` now: an earlier move may have
    /// changed it since the rows were built. The view is cloned out of its
    /// cell first, since `key_of` is app code.
    fn index_now(&self, key: &dyn ViewKey) -> Option<usize> {
        let view = self.view.borrow().clone();
        view.index_of(key)
    }

    /// A drag of `key` is over `over` now.
    fn hover(&self, key: &dyn ViewKey, over: Option<usize>) {
        if self.over.replace(over) == over {
            return;
        }
        let view = self.view.borrow().clone();
        let (Some(preview), Some(from)) = (view.on_reorder_preview.clone(), view.index_of(key))
        else {
            return;
        };
        self.write(|cx| preview(cx, ReorderPreview { from, over }));
    }

    /// Move the item keyed `key` to `to`, and announce its new position;
    /// nothing when it is there already.
    fn reorder(
        &self,
        cx: &mut EventCx<'_>,
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
        (view.on_reorder)(cx, Reorder { from, to, cause });
        let announcer = self.announcer.borrow().clone();
        if let Some(announcer) = announcer {
            let _ = announcer.announce(message, Urgency::Polite);
        }
        true
    }
}

/// Persistent state of a [`ReorderableList`].
pub struct ReorderableListState {
    shared: Rc<Shared>,
}

impl fmt::Debug for ReorderableListState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReorderableListState")
            .field("over", &self.shared.over.get())
            .finish_non_exhaustive()
    }
}

impl View for ReorderableList {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

impl StatefulView for ReorderableList {
    type State = ReorderableListState;

    fn create_state(&self) -> Self::State {
        ReorderableListState {
            shared: Rc::new(Shared {
                view: RefCell::new(self.clone()),
                over: Cell::new(None),
                row_reported: Cell::new(false),
                writer: RefCell::new(None),
                announcer: RefCell::new(None),
                strings: RefCell::new(None),
                pipeline: RefCell::new(None),
            }),
        }
    }
}

impl ReorderableListState {
    fn acquire(&self, ctx: &dyn LifecycleContext) {
        *self.shared.writer.borrow_mut() = Some(ctx.writer_source());
        *self.shared.announcer.borrow_mut() = ctx.announcer();
        *self.shared.pipeline.borrow_mut() = ctx.pipeline_owner().map(|owner| owner.downgrade());
    }
}

impl ViewState<ReorderableList> for ReorderableListState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.acquire(ctx);
    }

    fn build(&self, view: &ReorderableList, ctx: &dyn BuildContext) -> impl IntoView {
        *self.shared.strings.borrow_mut() = BoxedWidgetsLocalizations::maybe_of(ctx);
        let _prev = self.shared.view.replace(view.clone());
        let shared = Rc::clone(&self.shared);
        let list = view.clone();
        let left = Rc::clone(&self.shared);
        let scrollable = Scrollable::new()
            .scroll_direction(view.scroll_direction)
            .viewport_builder(Rc::new(move |position| {
                let shared = Rc::clone(&shared);
                let rows = list.clone();
                let keys = list.clone();
                ListView::builder(list.item_count, list.item_extent_estimate, move |index| {
                    (index < rows.item_count).then(|| {
                        ReorderRow {
                            index,
                            key: (rows.key_of)(index),
                            list: Rc::clone(&shared),
                        }
                        .boxed()
                    })
                })
                .scroll_direction(list.scroll_direction)
                .find_index_by_key(move |key| keys.index_of(key))
                .focusable(true)
                .position(position)
                .boxed()
            }))
            .boxed();
        // The rows report where a drag is. The list, which answers each move
        // after the rows under it, reports "over none" for a move no row
        // answered (the space after the last row) and when the drag leaves
        // it; a row's own leave would report it between two rows.
        let between = Rc::clone(&self.shared);
        DropTarget::bare(
            move |drag: &DragView<'_>| {
                if let Some(payload) = drag.value::<RowPayload>()
                    && Rc::ptr_eq(&payload.list, &between)
                    && !between.row_reported.replace(false)
                {
                    between.hover(&*payload.key, None);
                }
                None
            },
            move |_hover| scrollable.clone(),
        )
        .on_leave(move |drag| {
            if let Some(payload) = drag.value::<RowPayload>()
                && Rc::ptr_eq(&payload.list, &left)
            {
                left.row_reported.set(false);
                left.hover(&*payload.key, None);
            }
        })
    }
}

// ============================================================================
// A row
// ============================================================================

/// Makes a row's drag handles: each one drags the row it was made for.
struct HandleSource {
    list: Rc<Shared>,
    key: Rc<dyn ViewKey>,
    feedback: BoxedView,
    menu: bool,
    /// The list's axis, which a press on the handle moves along to drag.
    axis: Axis,
}

impl HandleSource {
    /// A draggable of the row, started by `devices` with `trigger`.
    fn draggable(&self, child: BoxedView, devices: PointerKindSet) -> Draggable {
        let list = Rc::clone(&self.list);
        let key = Rc::clone(&self.key);
        let started = Rc::clone(&self.list);
        let (ended, ended_key) = (Rc::clone(&self.list), Rc::clone(&self.key));
        let feedback = self.feedback.clone();
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
        .feedback(move || feedback.clone())
        .on_drag_started(move |_cx| started.over.set(None))
        // A drag that ends anywhere, dropped or not, is over no row now.
        .on_drag_end(move |_cx, _outcome| {
            ended.row_reported.set(false);
            ended.hover(&*ended_key, None);
        })
    }

    fn handle(&self, child: BoxedView) -> BoxedView {
        // All of the handle's box takes the press, not only what it paints,
        // and a press that moves along the list drags the row: on the list's
        // axis the handle is deeper than the list's own scroll, so it wins.
        let child = Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .child(child)
            .boxed();
        let axis = match self.axis {
            Axis::Vertical => MultiDragAxis::Vertical,
            Axis::Horizontal => MultiDragAxis::Horizontal,
        };
        let handle = self.draggable(child, PointerKindSet::all()).axis(axis);
        if self.menu {
            DragHandle {
                child: handle.boxed(),
            }
            .boxed()
        } else {
            handle.boxed()
        }
    }
}

/// A handle whose tap opens its row's menu.
#[derive(Clone)]
struct DragHandle {
    child: BoxedView,
}

impl View for DragHandle {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for DragHandle {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let chain = ambient_action_chain(ctx);
        GestureDetector::new()
            .on_tap(move |cx| {
                if let Some(open) = chain
                    .as_ref()
                    .and_then(|chain| resolve(chain, &ShowContextMenuIntent))
                {
                    open.invoke(cx, &ShowContextMenuIntent);
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
/// minimum) at the row's reading start in a vertical list, or on top in a
/// horizontal one.
///
/// The handle comes before the content because keyboard focus in the row
/// reveals the row's menu control at the trailing edge, where it would
/// cover a handle.
fn beside_handle(content: BoxedView, handle: BoxedView, side: f64, axis: Axis) -> BoxedView {
    let handle = SizedBox::new(side, side).child(handle).boxed();
    let content = Expanded::new(content).boxed();
    match axis {
        Axis::Vertical => Row::new(vec![handle, content]).boxed(),
        Axis::Horizontal => Column::new(vec![handle, content]).boxed(),
    }
}

/// One row: its moves, its menu or controls, the drop target it is and the
/// long press that drags it.
#[derive(Clone)]
struct ReorderRow {
    index: usize,
    key: Rc<dyn ViewKey>,
    list: Rc<Shared>,
}

impl View for ReorderRow {
    fn create_element(&self) -> ElementKind {
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
        let bare_source = HandleSource {
            list: Rc::clone(&self.list),
            key: Rc::clone(&self.key),
            feedback: SizedBox::shrink().boxed(),
            menu,
            axis: view.scroll_direction,
        };
        let anchor = SubtreeAnchor::new();
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
                beside_handle(content, handle, side, view.scroll_direction)
            }
            ReorderHandles::Custom => content,
        };
        let content = if view.long_press {
            source
                .draggable(content, PointerKindSet::TOUCH | PointerKindSet::PEN)
                .trigger(flui_interaction::drag::DragTrigger::LongPress)
                .boxed()
        } else {
            content
        };

        // The row publishes its render node, so a drag without a label shows
        // a plate of the row's size.
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
                // Where the model has this row now, as for the drop.
                hover.row_reported.set(true);
                if let Some(over) = hover.index_now(&*row) {
                    hover.hover(&*payload.key, Some(over));
                }
                Some(TransferActions::MOVE)
            },
            move |_hover| content.clone(),
        )
        .on_drop({
            let list = Rc::clone(&self.list);
            let dropped_on = Rc::clone(&self.key);
            move |cx, drag, _at, _action| {
                if let Some(payload) = drag.value::<RowPayload>()
                    && let Some(to) = list.index_now(&*dropped_on)
                {
                    let _ = list.reorder(cx, &*payload.key, to, ReorderCause::Drag);
                }
            }
        });

        let localizations = BoxedWidgetsLocalizations::maybe_of(ctx);
        let strings: &dyn WidgetsLocalizations = localizations
            .as_deref()
            .map_or(&DefaultWidgetsLocalizations, |boxed| &**boxed);
        let moves = moves(strings, &view, index, rtl);
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
        Actions::new(row).action(RowMoves {
            list: Rc::clone(&self.list),
            key: Rc::clone(&self.key),
            rtl,
        })
    }
}

/// What a drag of a row without a label shows: a plate the row's size in the
/// focus ring's colors, so the drag is seen; nothing while the row is not
/// laid out.
#[derive(Clone)]
struct RowPlate {
    list: Rc<Shared>,
    anchor: SubtreeAnchor,
}

impl View for RowPlate {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl StatelessView for RowPlate {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let Some(size) = self.list.row_size(&self.anchor) else {
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

/// Every move of a row of `view`, in menu order, with whether it applies to
/// the row at `index`: Start and back need a row before it, forward and End
/// one after it.
fn moves(
    strings: &dyn WidgetsLocalizations,
    view: &ReorderableList,
    index: usize,
    rtl: bool,
) -> Vec<(GestureCommand, ReorderIntent, SingleActivator, bool)> {
    let last = view.item_count.saturating_sub(1);
    let key = |named: NamedKey| SingleActivator::named(named).alt();
    let (back, back_key, forward, forward_key) = match (view.scroll_direction, rtl) {
        (Axis::Vertical, _) => (
            (strings.reorder_item_up(), ReorderIntent::Up),
            NamedKey::ArrowUp,
            (strings.reorder_item_down(), ReorderIntent::Down),
            NamedKey::ArrowDown,
        ),
        (Axis::Horizontal, false) => (
            (strings.reorder_item_left(), ReorderIntent::Left),
            NamedKey::ArrowLeft,
            (strings.reorder_item_right(), ReorderIntent::Right),
            NamedKey::ArrowRight,
        ),
        (Axis::Horizontal, true) => (
            (strings.reorder_item_right(), ReorderIntent::Right),
            NamedKey::ArrowRight,
            (strings.reorder_item_left(), ReorderIntent::Left),
            NamedKey::ArrowLeft,
        ),
    };
    let (after_start, before_end) = (index > 0, index < last);
    [
        (
            strings.reorder_item_to_start(),
            ReorderIntent::Start,
            NamedKey::Home,
            after_start,
        ),
        (back.0, back.1, back_key, after_start),
        (forward.0, forward.1, forward_key, before_end),
        (
            strings.reorder_item_to_end(),
            ReorderIntent::End,
            NamedKey::End,
            before_end,
        ),
    ]
    .into_iter()
    .map(|(label, intent, named, applies)| {
        (
            GestureCommand::new(label, intent),
            intent,
            key(named),
            applies,
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
        let view = self.list.view.borrow().clone();
        let index = view.index_of(&*self.key)?;
        let last = view.item_count.checked_sub(1)?;
        let back = index.checked_sub(1);
        let forward = (index < last).then_some(index + 1);
        let target = match intent {
            ReorderIntent::Start => Some(0),
            ReorderIntent::End => Some(last),
            ReorderIntent::Left if self.rtl => forward,
            ReorderIntent::Right if self.rtl => back,
            ReorderIntent::Up | ReorderIntent::Left => back,
            ReorderIntent::Down | ReorderIntent::Right => forward,
        };
        target.filter(|&target| target != index)
    }
}

impl Action<ReorderIntent> for RowMoves {
    fn is_enabled(&self, intent: &ReorderIntent) -> bool {
        self.target(*intent).is_some()
    }

    fn invoke(&self, cx: &mut EventCx<'_>, intent: &ReorderIntent) -> ActionOutcome {
        match self.target(*intent) {
            Some(to) if self.list.reorder(cx, &*self.key, to, ReorderCause::Command) => {
                ActionOutcome::Performed
            }
            _ => ActionOutcome::NotPerformed,
        }
    }
}
