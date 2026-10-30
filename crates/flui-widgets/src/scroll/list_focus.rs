//! Keyboard focus over a lazy list: one Tab stop, roving focus over its rows.
//!
//! [`FocusableList`] wraps whatever hosts the rows (a `ListView`'s or
//! `GridView`'s viewport, or one sliver under `FocusableSliver`) in the **list
//! node**: an ordinary focusable [`Focus`] registered as a traversal unit
//! (ADR-0026), so Tab stops at the list once and lands on the row it
//! remembers. Each built row is wrapped in a [`ListItemFocus`] marker, found
//! through [`ListFocusScope`] rather than through the viewport, so the same
//! pair serves a box viewport and a single sliver among others. Over a sliver
//! the list node is a sliver-protocol `Focus`, and the keys read the axis from
//! the sliver's committed constraints.
//!
//! The list node's key handler moves focus between rows: arrows, Page Up/Down,
//! Home/End. A target that is built takes focus at once; one that is not is
//! remembered as pending, the list scrolls to its estimated offset, and the
//! row's marker focuses it when it mounts. The marker pins its row
//! ([`KeepAliveHandle::pin`]) while the row holds focus or is the remembered
//! row, so a row the pointer scrolls away stays built, focused and announced
//! (ADR-0265).

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

use flui_foundation::geometry::{Axis, Rect, Vec2};
use flui_foundation::{SaltedKey, ViewKey};
use flui_interaction::focus::{
    EntryTarget, FocusCause, FocusNode, FocusNodeRegistration, FocusSubscription, KeyEventResult,
};
use flui_platform::keyboard::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::view::{ScrollPosition, ViewportOffset as _};
use flui_view::element::ElementKind;
use flui_view::owner::{KeepAliveHandle, KeepAliveLease};
use flui_view::prelude::*;
use flui_view::{BoxedView, impl_inherited_view};

use crate::anchored_box::AnchoredSliver;
use crate::interaction::focus::Focus;
use crate::interaction::focus_reveal::{FocusRevealScope, SharedRevealTarget};
use crate::semantics::{ExcludeSemantics, Semantics, SliverSemantics};
use flui_objects::SubtreeAnchor;
use flui_rendering::semantics::{SemanticsConfiguration, SemanticsRole};

// ============================================================================
// The controller
// ============================================================================

/// One built row: its marker's node and the marker's pin refresh.
struct Row {
    node: Weak<FocusNode>,
    refresh: Weak<dyn Fn()>,
}

/// How the list reaches a row that is not built: the shared scroll position
/// and the extent estimate the virtualizer is seeded with.
#[derive(Clone)]
struct Reach {
    position: ScrollPosition,
    extent_estimate: f64,
}

/// Reads a sliver's committed main axis and whether it runs against the
/// index order on screen.
type AxisSource = Rc<dyn Fn() -> Option<(Axis, bool)>>;

struct Inner {
    /// The list node: the traversal unit Tab stops at.
    node: Rc<FocusNode>,
    /// The markers of the built rows, by logical index.
    rows: RefCell<BTreeMap<usize, Row>>,
    /// The row Tab returns to. A keyed row whose data moves carries it to its
    /// new index: its marker moves with it.
    remembered: Cell<Option<usize>>,
    /// A row asked for while it was not built; its marker focuses it on mount.
    pending: Cell<Option<usize>>,
    item_count: Cell<usize>,
    /// Whether this list sits inside another focusable list, which then
    /// takes the arrow keys this one has no row for.
    nested: Cell<bool>,
    axis: Cell<Axis>,
    /// Whether the main axis runs against the index order on screen (a
    /// horizontal list under right-to-left text).
    reversed: Cell<bool>,
    reach: RefCell<Option<Reach>>,
    /// Reads the axis and direction a list over a sliver was laid out with;
    /// `None` for a list over a box viewport, which takes them from its
    /// config.
    sliver_axis: RefCell<Option<AxisSource>>,
    /// The scope that scrolls an unbuilt row into the band when the list has
    /// no scroll position of its own to set.
    reveal: RefCell<Option<SharedRevealTarget>>,
    /// Whether the rows are tiles of a grid: the cross-axis arrows move
    /// along a row of tiles.
    grid: Cell<bool>,
}

/// The state one focusable list shares with its rows' markers.
#[derive(Clone)]
pub(crate) struct ListFocusController {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for ListFocusController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListFocusController")
            .field("node", &self.inner.node.id())
            .field("item_count", &self.inner.item_count.get())
            .finish_non_exhaustive()
    }
}

impl ListFocusController {
    fn new(node: Rc<FocusNode>) -> Self {
        Self {
            inner: Rc::new(Inner {
                node,
                rows: RefCell::new(BTreeMap::new()),
                remembered: Cell::new(None),
                pending: Cell::new(None),
                item_count: Cell::new(0),
                nested: Cell::new(false),
                axis: Cell::new(Axis::Vertical),
                reversed: Cell::new(false),
                reach: RefCell::new(None),
                sliver_axis: RefCell::new(None),
                reveal: RefCell::new(None),
                grid: Cell::new(false),
            }),
        }
    }

    fn remembered_index(&self) -> Option<usize> {
        self.inner.remembered.get()
    }

    fn is_remembered(&self, index: usize) -> bool {
        self.remembered_index() == Some(index)
    }

    /// The live marker node of `index`, if that row is built.
    fn row_node(&self, index: usize) -> Option<Rc<FocusNode>> {
        self.inner
            .rows
            .borrow()
            .get(&index)
            .and_then(|row| row.node.upgrade())
    }

    /// Run every built row's pin refresh: `first` before the rest, so a new
    /// pin is taken before the old one is released.
    fn refresh_pins(&self, first: Option<usize>) {
        let (leading, rest): (Vec<_>, Vec<_>) = self
            .inner
            .rows
            .borrow()
            .iter()
            .filter_map(|(index, row)| row.refresh.upgrade().map(|refresh| (*index, refresh)))
            .partition(|(index, _)| Some(*index) == first);
        for (_, refresh) in leading.into_iter().chain(rest) {
            refresh();
        }
    }

    fn remember(&self, index: usize) {
        if self.inner.remembered.replace(Some(index)) != Some(index) {
            self.refresh_pins(Some(index));
        }
    }

    /// Where Tab entering the list lands.
    fn entry(&self) -> EntryTarget {
        let count = self.inner.item_count.get();
        if count == 0 {
            return EntryTarget::Unit;
        }
        let target = self
            .remembered_index()
            .filter(|index| *index < count)
            .or_else(|| self.first_visible_row());
        let Some(target) = target else {
            return EntryTarget::Unit;
        };
        match self
            .row_node(target)
            .and_then(|marker| focus_target(&marker))
        {
            Some(node) => {
                self.remember(target);
                EntryTarget::Node(node)
            }
            None if self.reach_row(target) => {
                self.remember(target);
                EntryTarget::Pending
            }
            None => EntryTarget::Unit,
        }
    }

    /// The first built row whose box lies inside the list's box, or the first
    /// built row.
    fn first_visible_row(&self) -> Option<usize> {
        let list = self.inner.node.rect();
        let rows: Vec<(usize, Rc<FocusNode>)> = self.live_rows();
        rows.iter()
            .find(|(_, node)| contains(list, node.rect(), self.axis().0))
            .or_else(|| rows.first())
            .map(|(index, _)| *index)
    }

    fn live_rows(&self) -> Vec<(usize, Rc<FocusNode>)> {
        self.inner
            .rows
            .borrow()
            .iter()
            .filter_map(|(index, row)| row.node.upgrade().map(|node| (*index, node)))
            .collect()
    }

    /// The row holding focus, or the remembered one when the list node itself
    /// holds it.
    fn current_row(&self) -> Option<usize> {
        self.live_rows()
            .into_iter()
            .find(|(_, node)| node.has_focus())
            .map(|(index, _)| index)
            .or_else(|| self.remembered_index())
    }

    /// Move focus to `index`: at once when it is built, otherwise once it
    /// mounts after the list scrolls to it. `false` when the row cannot be
    /// reached (not built, and the list has no scroll position to move).
    fn move_to(&self, index: usize) -> bool {
        if let Some(node) = self
            .row_node(index)
            .and_then(|marker| focus_target(&marker))
        {
            self.inner.pending.set(None);
            self.remember(index);
            let _ = node.request_focus_with(FocusCause::Traversal);
            return true;
        }
        if !self.reach_row(index) {
            return false;
        }
        self.remember(index);
        let _ = self.inner.node.request_focus_with(FocusCause::Traversal);
        true
    }

    /// Scroll so an unbuilt row gets built, and record it as pending: to the
    /// row's estimated offset when the list owns a scroll position, otherwise
    /// by revealing the row's estimated box through the enclosing scope.
    fn reach_row(&self, index: usize) -> bool {
        if let Some(reach) = self.inner.reach.borrow().clone() {
            let estimate = if self.inner.grid.get() {
                // Tiles share rows, so the offset comes from the built
                // tiles: the estimated row brought to the leading edge.
                let Some(rect) = self.estimated_rect(index) else {
                    return false;
                };
                let list = self.inner.node.rect();
                let distance = match self.axis() {
                    (Axis::Vertical, false) => rect.top() - list.top(),
                    (Axis::Vertical, true) => list.bottom() - rect.bottom(),
                    (Axis::Horizontal, false) => rect.left() - list.left(),
                    (Axis::Horizontal, true) => list.right() - rect.right(),
                };
                reach.position.pixels() + distance
            } else {
                reach.extent_estimate * index as f64
            };
            if !estimate.is_finite() {
                return false;
            }
            let min = reach.position.min_scroll_extent();
            let max = reach.position.max_scroll_extent().max(min);
            reach.position.set_pixels(estimate.clamp(min, max));
            self.inner.pending.set(Some(index));
            return true;
        }
        let Some(rect) = self.estimated_rect(index) else {
            return false;
        };
        let Some(reveal) = self.inner.reveal.borrow().clone() else {
            return false;
        };
        self.inner.pending.set(Some(index));
        reveal.reveal(rect);
        true
    }

    /// Where an unbuilt row would sit: the nearest built row's box, moved
    /// along the main axis by the rows of tiles between them, each as long
    /// as that row.
    fn estimated_rect(&self, index: usize) -> Option<Rect<f64>> {
        let stride = self.stride();
        let (near, node) = self
            .live_rows()
            .into_iter()
            .min_by_key(|(row, _)| row.abs_diff(index))?;
        let rect = node.rect();
        let (axis, reversed) = self.axis();
        let rows = (index / stride) as f64 - (near / stride) as f64;
        let extent = match axis {
            Axis::Vertical => rect.height(),
            Axis::Horizontal => rect.width(),
        };
        let shift = if reversed {
            -rows * extent
        } else {
            rows * extent
        };
        if !shift.is_finite() {
            return None;
        }
        Some(rect.translate(match axis {
            Axis::Vertical => Vec2::new(0.0, shift),
            Axis::Horizontal => Vec2::new(shift, 0.0),
        }))
    }

    /// The main axis and whether it runs against the index order on screen:
    /// a sliver's as last laid out, else the configured ones.
    fn axis(&self) -> (Axis, bool) {
        let source = self.inner.sliver_axis.borrow().clone();
        source
            .and_then(|source| source())
            .unwrap_or_else(|| (self.inner.axis.get(), self.inner.reversed.get()))
    }

    /// The tiles in one row across the main axis: the largest group of built
    /// rows that start at the same main-axis offset (one for a list). The
    /// built band holds whole rows, so a ragged last row does not decide it.
    fn stride(&self) -> usize {
        if !self.inner.grid.get() {
            return 1;
        }
        let (axis, _) = self.axis();
        let mut starts: Vec<f64> = self
            .live_rows()
            .into_iter()
            .map(|(_, node)| {
                let rect = node.rect();
                match axis {
                    Axis::Vertical => rect.top(),
                    Axis::Horizontal => rect.left(),
                }
            })
            .collect();
        starts.sort_by(f64::total_cmp);
        let mut best = 1;
        let mut run = 0;
        let mut previous = f64::NAN;
        for start in starts {
            run = if (start - previous).abs() <= 0.5 {
                run + 1
            } else {
                1
            };
            previous = start;
            best = best.max(run);
        }
        best
    }

    /// The tile beside `current` in its row in the on-screen direction of
    /// `toward_end` (right, or down for a horizontal grid), `None` at the
    /// row's edge. Read from the built tiles, so a right-to-left grid moves
    /// the way the arrow points.
    fn beside(
        &self,
        current: usize,
        toward_end: bool,
        stride: usize,
        last: usize,
    ) -> Option<usize> {
        let current_rect = self.row_node(current)?.rect();
        let (axis, _) = self.axis();
        let column = current % stride;
        let neighbours = [
            (column > 0).then(|| current - 1),
            (column + 1 < stride && current < last).then(|| current + 1),
        ];
        neighbours.into_iter().flatten().find(|neighbour| {
            self.row_node(*neighbour).is_some_and(|node| {
                let rect = node.rect();
                let ahead = match axis {
                    Axis::Vertical => rect.left() > current_rect.left(),
                    Axis::Horizontal => rect.top() > current_rect.top(),
                };
                ahead == toward_end
            })
        })
    }

    /// A marker for `index` mounted: if it is the pending row, focus it.
    fn row_mounted(&self, index: usize) {
        if self.inner.pending.get() != Some(index) {
            return;
        }
        if let Some(node) = self
            .row_node(index)
            .and_then(|marker| focus_target(&marker))
        {
            self.inner.pending.set(None);
            let _ = node.request_focus_with(FocusCause::Traversal);
        }
    }

    /// The list node's key handler.
    fn handle_key(&self, event: &KeyEvent) -> KeyEventResult {
        if event.state() != KeyState::Down {
            return KeyEventResult::Ignored;
        }
        let modified = [
            Modifiers::CONTROL,
            Modifiers::ALT,
            Modifiers::META,
            Modifiers::SHIFT,
        ]
        .into_iter()
        .any(|modifier| event.modifiers.contains(modifier));
        if modified {
            return KeyEventResult::Ignored;
        }
        let count = self.inner.item_count.get();
        if count == 0 {
            return KeyEventResult::Ignored;
        }
        let Key::Named(named) = &event.key else {
            return KeyEventResult::Ignored;
        };
        let (axis, reversed) = self.axis();
        let (previous, next) = match (axis, reversed) {
            (Axis::Vertical, false) => (NamedKey::ArrowUp, NamedKey::ArrowDown),
            (Axis::Vertical, true) => (NamedKey::ArrowDown, NamedKey::ArrowUp),
            (Axis::Horizontal, false) => (NamedKey::ArrowLeft, NamedKey::ArrowRight),
            (Axis::Horizontal, true) => (NamedKey::ArrowRight, NamedKey::ArrowLeft),
        };
        let (cross_back, cross_ahead) = match axis {
            Axis::Vertical => (NamedKey::ArrowLeft, NamedKey::ArrowRight),
            Axis::Horizontal => (NamedKey::ArrowUp, NamedKey::ArrowDown),
        };
        let grid = self.inner.grid.get();
        let stride = self.stride();
        let current = self.current_row();
        let last = count - 1;
        let target = match named {
            key if *key == previous => match current {
                Some(index) if index < stride => None,
                Some(index) => Some(index.min(last + stride) - stride),
                None => Some(0),
            },
            // A column past the ragged last row lands on the last tile.
            key if *key == next => match current {
                Some(index) if index / stride >= last / stride => None,
                Some(index) => Some((index + stride).min(last)),
                None => Some(0),
            },
            key if grid && (*key == cross_back || *key == cross_ahead) => match current {
                Some(index) if index <= last => {
                    self.beside(index, *key == cross_ahead, stride, last)
                }
                _ => Some(0),
            },
            NamedKey::Home => (current != Some(0)).then_some(0),
            NamedKey::End => (current != Some(last)).then_some(last),
            NamedKey::PageDown => self.page(current, true),
            NamedKey::PageUp => self.page(current, false),
            _ => return KeyEventResult::Ignored,
        };
        match target {
            Some(index) if self.move_to(index) => KeyEventResult::Handled,
            Some(_) => KeyEventResult::Ignored,
            // At the edge an enclosing list moves on; an outermost list keeps
            // the key, so focus stays on the row instead of leaving the list
            // by direction.
            None if self.inner.nested.get() => KeyEventResult::Ignored,
            None => KeyEventResult::Handled,
        }
    }

    /// Page Down (or Up): first to the last (first) fully visible row of
    /// tiles, then a page of rows past it, in the same column. `None` at the
    /// edge.
    fn page(&self, current: Option<usize>, forward: bool) -> Option<usize> {
        let count = self.inner.item_count.get();
        let last = count.checked_sub(1)?;
        let stride = self.stride();
        let list = self.inner.node.rect();
        let (axis, _) = self.axis();
        let mut visible: Vec<usize> = self
            .live_rows()
            .into_iter()
            .filter(|(_, node)| contains(list, node.rect(), axis))
            .map(|(index, _)| index / stride)
            .collect();
        visible.dedup();
        let page = visible.len().max(1);
        let current = current.unwrap_or(0).min(last);
        let (row, column) = (current / stride, current % stride);
        let last_row = last / stride;
        let target_row = if forward {
            if row >= last_row {
                return None;
            }
            match visible.last() {
                Some(&edge) if edge > row => edge,
                _ => row.saturating_add(page - 1).max(row + 1).min(last_row),
            }
        } else {
            if row == 0 {
                return None;
            }
            match visible.first() {
                Some(&edge) if edge < row => edge,
                _ => row.saturating_sub(page - 1).min(row - 1),
            }
        };
        Some(
            target_row
                .saturating_mul(stride)
                .saturating_add(column)
                .min(last),
        )
    }
}

/// Whether `inner` lies inside `outer` along `axis`: a row is fully visible
/// when its main-axis span is inside the list's.
fn contains(outer: Rect<f64>, inner: Rect<f64>, axis: Axis) -> bool {
    const SLACK: f64 = 0.5;
    match axis {
        Axis::Vertical => {
            inner.top() >= outer.top() - SLACK && inner.bottom() <= outer.bottom() + SLACK
        }
        Axis::Horizontal => {
            inner.left() >= outer.left() - SLACK && inner.right() <= outer.right() + SLACK
        }
    }
}

/// The node a row's focus goes to: the first focusable descendant of its
/// marker, so the row's button receives `ActivateIntent` and draws its ring
/// as it does on a click, or the marker itself for a row with none.
fn focus_target(marker: &Rc<FocusNode>) -> Option<Rc<FocusNode>> {
    marker
        .descendants()
        .find(|node| node.can_request_focus())
        .or_else(|| marker.can_request_focus().then(|| Rc::clone(marker)))
}

// ============================================================================
// The list node
// ============================================================================

/// Provides a focusable list's controller to its rows' markers.
#[derive(Clone)]
pub(crate) struct ListFocusScope {
    controller: ListFocusController,
    child: BoxedView,
}

impl std::fmt::Debug for ListFocusScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListFocusScope").finish_non_exhaustive()
    }
}

impl InheritedView for ListFocusScope {
    type Data = ListFocusController;

    fn data(&self) -> &Self::Data {
        &self.controller
    }

    fn child(&self) -> &dyn View {
        &self.child
    }

    fn update_should_notify(&self, old: &Self) -> bool {
        !Rc::ptr_eq(&self.controller.inner, &old.controller.inner)
    }
}

impl_inherited_view!(ListFocusScope);

/// What a focusable list announces and how it reaches unbuilt rows.
#[derive(Clone, Debug)]
pub(crate) struct ListFocusConfig {
    pub(crate) item_count: usize,
    pub(crate) axis: Axis,
    pub(crate) reversed: bool,
    pub(crate) position: Option<ScrollPosition>,
    pub(crate) extent_estimate: f64,
    /// The name an empty list announces.
    pub(crate) empty_list_label: Option<std::sync::Arc<str>>,
    /// The rows are a sliver's children, not a box viewport's; read once,
    /// when the list mounts.
    pub(crate) sliver: bool,
    /// The rows are grid tiles: the cross-axis arrows move along a row.
    pub(crate) grid: bool,
}

/// The list node around a list's rows: one Tab stop, the arrow keys, and the
/// list's accessible name.
#[derive(Clone)]
pub(crate) struct FocusableList {
    config: ListFocusConfig,
    child: BoxedView,
}

impl FocusableList {
    pub(crate) fn new(config: ListFocusConfig, child: impl IntoView) -> Self {
        Self {
            config,
            child: BoxedView::new(child.into_view()),
        }
    }
}

impl std::fmt::Debug for FocusableList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FocusableList")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl View for FocusableList {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }
}

pub(crate) struct FocusableListState {
    controller: ListFocusController,
    /// Keeps the traversal entry registered while the list is mounted.
    _entry: FocusNodeRegistration,
    /// Whether the rows sit in a sliver rather than a box viewport; fixed
    /// for the state's life, as the protocol of the child is.
    sliver: bool,
    /// Publishes the sliver's render node, so the keys read its axis.
    anchor: SubtreeAnchor,
}

impl StatefulView for FocusableList {
    type State = FocusableListState;

    fn create_state(&self) -> FocusableListState {
        let node = FocusNode::new();
        let controller = ListFocusController::new(Rc::clone(&node));
        apply_config(&controller, &self.config);
        let entry_controller = Rc::downgrade(&controller.inner);
        let entry = node.register_traversal_entry(Rc::new(move || {
            entry_controller
                .upgrade()
                .map_or(EntryTarget::Unit, |inner| {
                    ListFocusController { inner }.entry()
                })
        }));
        FocusableListState {
            controller,
            _entry: entry,
            sliver: self.config.sliver,
            anchor: SubtreeAnchor::new(),
        }
    }
}

fn apply_config(controller: &ListFocusController, config: &ListFocusConfig) {
    let inner = &controller.inner;
    inner.item_count.set(config.item_count);
    inner.axis.set(config.axis);
    inner.reversed.set(config.reversed);
    inner.grid.set(config.grid);
    *inner.reach.borrow_mut() = config.position.clone().map(|position| Reach {
        position,
        extent_estimate: config.extent_estimate,
    });
    if inner
        .pending
        .get()
        .is_some_and(|index| index >= config.item_count)
    {
        inner.pending.set(None);
    }
}

impl ViewState<FocusableList> for FocusableListState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let inner = &self.controller.inner;
        let nested = ctx.get::<ListFocusScope, _>(|_| ()).is_some();
        inner.nested.set(nested);
        *inner.reveal.borrow_mut() = FocusRevealScope::of(ctx);
        if self.sliver {
            let anchor = self.anchor.clone();
            let owner = ctx.pipeline_owner();
            let source: AxisSource = Rc::new(move || {
                let render_id = anchor.get()?;
                owner
                    .as_ref()?
                    .try_with_layout(|owner| owner.sliver_layout(render_id))
                    .flatten()
                    .map(|(constraints, _)| {
                        let reversed = matches!(
                            constraints.axis_direction,
                            AxisDirection::BottomToTop | AxisDirection::RightToLeft
                        );
                        (constraints.axis(), reversed)
                    })
            });
            *inner.sliver_axis.borrow_mut() = Some(source);
        }
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        *self.controller.inner.reveal.borrow_mut() = FocusRevealScope::of(ctx);
    }

    fn did_update_view(&mut self, _old: &FocusableList, view: &FocusableList) {
        apply_config(&self.controller, &view.config);
    }

    fn build(&self, view: &FocusableList, _ctx: &dyn BuildContext) -> impl IntoView {
        // A list with rows hands focus to a row, whose button is the node a
        // screen reader follows, so the list node publishes nothing; the rows'
        // collection node below carries the list's name. An empty list keeps
        // focus on itself and is published as that collection: one named,
        // focusable list node, with the empty sliver's own node excluded. The
        // structure is the same either way, so a list that empties or fills
        // keeps its viewport.
        let empty = view.config.item_count == 0;
        let keys = self.controller.clone();
        let scope = |child: BoxedView| ListFocusScope {
            controller: self.controller.clone(),
            child,
        };
        if self.sliver {
            // The same structure over a sliver, with the sliver protocol's
            // annotation and an anchor the arrow keys read the axis from.
            let mut configuration = SemanticsConfiguration::default();
            if empty {
                configuration.set_semantics_boundary(true);
                configuration.set_role(SemanticsRole::List);
                if let Some(label) = &view.config.empty_list_label {
                    configuration.set_label(label.to_string());
                }
            }
            let child = AnchoredSliver::new(
                self.anchor.clone(),
                SliverSemantics::new()
                    .exclude_semantics(empty)
                    .child(view.child.clone()),
            );
            return SliverSemantics::from_configuration(configuration)
                .child(
                    Focus::new(scope(BoxedView::new(child.into_view())))
                        .sliver()
                        .focus_node(Rc::clone(&self.controller.inner.node))
                        .include_semantics(empty)
                        .debug_label("list")
                        .on_key_event(move |_cx, event| keys.handle_key(event)),
                )
                .boxed();
        }
        let mut list = Semantics::new();
        if empty {
            list = list.container(true).role(SemanticsRole::List);
            if let Some(label) = &view.config.empty_list_label {
                list = list.label(label.to_string());
            }
        }
        list.child(
            Focus::new(scope(BoxedView::new(
                ExcludeSemantics::new()
                    .excluding(empty)
                    .child(view.child.clone())
                    .into_view(),
            )))
            .focus_node(Rc::clone(&self.controller.inner.node))
            .include_semantics(empty)
            .debug_label("list")
            .on_key_event(move |_cx, event| keys.handle_key(event)),
        )
        .boxed()
    }
}

// ============================================================================
// The row marker
// ============================================================================

/// The marker around one built row of a focusable list. Not a Tab stop: focus
/// goes to the row's first focusable descendant. It carries the row's key,
/// salted, so the lazy sliver reconciles the row by its own key.
#[derive(Clone)]
pub(crate) struct ListItemFocus {
    index: usize,
    salted_key: Option<SaltedKey>,
    child: BoxedView,
}

impl ListItemFocus {
    /// The marker of row `index`, with the row inside a repaint boundary when
    /// `repaint_boundary` is set. The boundary goes inside the marker and
    /// keyless: the marker carries the row's key, and a boundary outside it
    /// would salt that key twice.
    pub(crate) fn new(index: usize, row: BoxedView, repaint_boundary: bool) -> Self {
        let salted_key = row.key().map(SaltedKey::new);
        let child = if repaint_boundary {
            BoxedView::new(crate::paint::RepaintBoundary::new().child(row))
        } else {
            row
        };
        Self {
            index,
            salted_key,
            child,
        }
    }
}

impl std::fmt::Debug for ListItemFocus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ListItemFocus")
            .field("index", &self.index)
            .finish_non_exhaustive()
    }
}

impl View for ListItemFocus {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateful(self)
    }

    fn key(&self) -> Option<&dyn ViewKey> {
        self.salted_key.as_ref().map(|key| key as &dyn ViewKey)
    }
}

pub(crate) struct ListItemFocusState {
    index: usize,
    node: Rc<FocusNode>,
    controller: Option<ListFocusController>,
    keep_alive: Option<KeepAliveHandle>,
    pin: Rc<RefCell<Option<KeepAliveLease>>>,
    /// The pin refresh the controller calls; the controller holds it weakly.
    refresh: Rc<dyn Fn()>,
    subscription: Option<FocusSubscription>,
    post_frame: Option<flui_view::PostFrameHandle>,
}

impl StatefulView for ListItemFocus {
    type State = ListItemFocusState;

    fn create_state(&self) -> ListItemFocusState {
        let node = FocusNode::new();

        ListItemFocusState {
            index: self.index,
            node,
            controller: None,
            keep_alive: None,
            pin: Rc::new(RefCell::new(None)),
            refresh: Rc::new(|| {}),
            subscription: None,
            post_frame: None,
        }
    }
}

impl ListItemFocusState {
    /// Build the pin refresh: pinned while the row holds focus or is the
    /// remembered row.
    fn make_refresh(&self) -> Rc<dyn Fn()> {
        let node = Rc::downgrade(&self.node);
        let controller = self.controller.as_ref().map(|c| Rc::downgrade(&c.inner));
        let keep_alive = self.keep_alive.clone();
        let pin = Rc::clone(&self.pin);
        let index = self.index;
        Rc::new(move || {
            let focused = node.upgrade().is_some_and(|node| node.has_focus());
            let remembered = controller
                .as_ref()
                .and_then(Weak::upgrade)
                .is_some_and(|inner| ListFocusController { inner }.is_remembered(index));
            let wants = focused || remembered;
            let held = pin.borrow().is_some();
            if wants && !held {
                if let Some(keep_alive) = &keep_alive {
                    let lease = keep_alive.pin();
                    *pin.borrow_mut() = Some(lease);
                }
            } else if !wants && held {
                // Released outside the borrow: the lease's drop notifies the
                // slivers watching the pins.
                let lease = pin.borrow_mut().take();
                drop(lease);
            }
        })
    }

    /// Enter the controller's registry under the current index, and follow
    /// the row's focus with the pin.
    fn register(&mut self) {
        self.refresh = self.make_refresh();
        let refresh = Rc::downgrade(&self.refresh);
        self.subscription = Some(self.node.subscribe(Rc::new(move || {
            if let Some(refresh) = refresh.upgrade() {
                refresh();
            }
        })));
        if let Some(controller) = &self.controller {
            controller.inner.rows.borrow_mut().insert(
                self.index,
                Row {
                    node: Rc::downgrade(&self.node),
                    refresh: Rc::downgrade(&self.refresh),
                },
            );
        }
        (self.refresh)();
    }

    fn unregister(&mut self) {
        if let Some(controller) = &self.controller {
            let mut rows = controller.inner.rows.borrow_mut();
            let ours = rows
                .get(&self.index)
                .is_some_and(|row| Weak::ptr_eq(&row.node, &Rc::downgrade(&self.node)));
            if ours {
                rows.remove(&self.index);
            }
        }
    }

    /// After the row's subtree mounts, focus it if the list is waiting for it.
    fn schedule_mounted(&self) {
        let (Some(post_frame), Some(controller)) = (&self.post_frame, &self.controller) else {
            return;
        };
        if controller.inner.pending.get() != Some(self.index) {
            return;
        }
        let controller = Rc::downgrade(&controller.inner);
        let index = self.index;
        if let Err(error) = post_frame.schedule(move |_| {
            if let Some(inner) = controller.upgrade() {
                ListFocusController { inner }.row_mounted(index);
            }
        }) {
            tracing::debug!(?error, "a pending list row could not schedule its focus");
        }
    }
}

impl ViewState<ListItemFocus> for ListItemFocusState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.keep_alive = Some(ctx.keep_alive_handle());
        self.post_frame = ctx.post_frame_handle();
        self.controller = ctx.get::<ListFocusScope, _>(|scope| scope.controller.clone());
        self.register();
        self.schedule_mounted();
    }

    fn did_update_view(&mut self, _old: &ListItemFocus, view: &ListItemFocus) {
        if view.index != self.index {
            let was_remembered = self
                .controller
                .as_ref()
                .is_some_and(|controller| controller.is_remembered(self.index));
            self.unregister();
            self.index = view.index;
            if was_remembered && let Some(controller) = &self.controller {
                controller.inner.remembered.set(Some(view.index));
            }
            self.register();
            self.schedule_mounted();
        }
    }

    fn build(&self, view: &ListItemFocus, _ctx: &dyn BuildContext) -> impl IntoView {
        Focus::new(view.child.clone())
            .focus_node(Rc::clone(&self.node))
            .skip_traversal(true)
            .include_semantics(false)
            .debug_label("list row")
    }

    fn dispose(&mut self) {
        self.subscription = None;
        self.unregister();
        let lease = self.pin.borrow_mut().take();
        drop(lease);
    }
}
