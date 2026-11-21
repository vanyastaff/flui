//! The drag session of a list or grid: what a drag carries, the gap a drop would
//! leave, the per-item records and the state ([`Shared`]) that the list's and the
//! grid's states hold.
//!
//! A "row" here is an item of either: a row of a list, a tile of a grid.

use std::cell::{Cell, RefCell};
#[expect(
    clippy::disallowed_types,
    reason = "keys are hashes of the application's item keys"
)]
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, DrivenController, MotionSpec, Vsync, VsyncUpdate,
};

use flui_foundation::ViewKey;
use flui_foundation::geometry::{Offset, Point, Size};
use flui_interaction::drag::DragView;
use flui_objects::{Placement, PlacementSlot, SubtreeAnchor};
use flui_painting::styling::Color;
use flui_rendering::pipeline::WeakPipelineCell;

use crate::scroll::ScrollView;
use crate::{AnimatedBuilder, PhysicalModel, Transform};
use flui_platform::transfer::data::TransferActions;
use flui_semantics::{Announcer, Urgency};
use flui_view::environment::VsyncScope;
use flui_view::prelude::*;
use flui_view::{EventContext, RebuildHandle, WriterSource};

use crate::localization::{
    BoxedWidgetsLocalizations, DefaultWidgetsLocalizations, WidgetsLocalizations,
};
use crate::{DropTarget, GridView, ListView};

use super::config::{Config, Layout};
use super::row::ReorderRow;
use super::steps::{GridShape, ShapeRecorder};
use super::types::{Reorder, ReorderCause, ReorderPreview};

/// What one drag of a row carries: its list, so only that list's rows take
/// it, and the dragged item's key, so a drop finds where the item is now.
pub(super) struct RowPayload {
    pub(super) list: Rc<Shared>,
    pub(super) key: Rc<dyn ViewKey>,
}

/// How long a dragged item's lift takes.
pub(super) const LIFT: Duration = Duration::from_millis(150);
/// How long a row takes to make room, land or go to its new place.
pub(super) const SLIDE: Duration = Duration::from_millis(250);
/// The elevation and scale the default proxy decorator lifts to.
pub(super) const LIFT_ELEVATION: f64 = 6.0;
pub(super) const LIFT_SCALE: f64 = 1.02;

/// One running drag of a row.
pub(super) struct RowDrag {
    pub(super) key: Rc<dyn ViewKey>,
    /// Where a drop now moves the item. For the lead drag a position in the
    /// order the rows are shown in, which keeps a gap there; for any other
    /// the index of the row it is over.
    pub(super) over: Option<usize>,
    /// Whether a row reported the drag over it since the list last looked:
    /// the rows answer a move before the list around them does.
    pub(super) row_reported: bool,
    /// The row whose report last moved the gap. It slides away under the
    /// pointer, so its reports are not heard until another row reports.
    pub(super) shifted: Option<Rc<dyn ViewKey>>,
    /// The lift the proxy decorator reads.
    pub(super) lift: DrivenController,
    /// How far the proxy was from the dragged item's row when the drag
    /// ended, in the window's space: the row lands from there.
    pub(super) landing: Option<Offset<f64>>,
    /// Where the drag was when it last left the list, for the drop that
    /// follows its own leave; cleared when it is over a row again.
    pub(super) left_over: Option<usize>,
}

/// Where the lead drag shows its item: the rows between `from` and `over`
/// each move one position toward `from`, and the item, painted as a gap,
/// sits at `over`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Gap {
    pub(super) from: usize,
    pub(super) over: usize,
}

impl Gap {
    /// The index of the item shown at position `slot`.
    pub(super) fn item_at(gap: Option<Self>, slot: usize) -> usize {
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
    pub(super) fn slot_of(gap: Option<Self>, index: usize) -> usize {
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
pub(super) struct ItemRecord {
    pub(super) key: Rc<dyn ViewKey>,
    pub(super) slot: Rc<PlacementSlot>,
    pub(super) anchor: SubtreeAnchor,
}

/// The items' records by the hash of their keys.
#[expect(
    clippy::disallowed_types,
    reason = "keys are hashes of the application's item keys"
)]
pub(super) type Records = HashMap<u64, Vec<ItemRecord>>;

/// The record of `key` among `records`.
pub(super) fn record_in(records: &Records, key: &dyn ViewKey) -> Option<ItemRecord> {
    records
        .get(&key.key_hash())?
        .iter()
        .find(|record| record.key.key_eq(key))
        .cloned()
}

/// One mounted list or grid, as its rows share it.
pub(super) struct Shared {
    pub(super) view: RefCell<Config>,
    /// How a grid laid its tiles out last; `None` for a list and before a
    /// grid's first layout.
    pub(super) grid: Rc<Cell<Option<GridShape>>>,
    /// The drags of this list's rows that run now, one per dragged item:
    /// two contacts may drag two rows at once, each with its own preview.
    /// The first is the lead, whose item the rows make room for.
    pub(super) drags: RefCell<Vec<RowDrag>>,
    /// The clock every slide of the rows shares: rows making room for a
    /// drag, an item landing, an item a command moved.
    pub(super) clock: RefCell<DrivenController>,
    /// Advances whenever the order the rows are shown in changes; the rows
    /// slide only across a change of it.
    pub(super) generation: Cell<u64>,
    /// Each item's record, rebuilt from the keys by each build.
    pub(super) records: RefCell<Records>,
    pub(super) vsync: RefCell<Option<Vsync>>,
    pub(super) rebuild: RebuildHandle,
    pub(super) writer: WriterSource,
    pub(super) announcer: RefCell<Option<Announcer>>,
    /// The localizations the list was last built with, for its announcements.
    pub(super) strings: RefCell<Option<Rc<BoxedWidgetsLocalizations>>>,
    /// The render tree the rows lay out in, for the size and place of a
    /// dragged row.
    pub(super) pipeline: RefCell<Option<WeakPipelineCell>>,
}

impl Shared {
    pub(super) fn write(&self, invoke: impl FnOnce(&mut EventContext<'_>)) {
        self.writer.write(invoke);
    }

    pub(super) fn schedule_rebuild(&self) {
        self.rebuild.schedule(flui_view::RebuildReason::StateChange);
    }

    /// The order the rows are shown in changed: they slide on the next
    /// layout.
    pub(super) fn reshown(&self) {
        // The generation names no order: the sliver only compares it with the
        // one its last settled pass saw.
        self.generation.set(self.generation.get().wrapping_add(1));
        self.schedule_rebuild();
    }

    /// The laid-out size of the row published into `anchor`, or `None` when it
    /// is not laid out or a layout pass is running.
    pub(super) fn row_size(&self, anchor: &SubtreeAnchor) -> Option<Size> {
        let id = anchor.get()?;
        let owner = self.pipeline.borrow().as_ref()?.upgrade()?;
        owner.try_with_layout(|owner| owner.box_size(id))?
    }

    /// Where the top-left corner of the node published into `anchor` is
    /// painted in the window, or `None` when it is not laid out.
    pub(super) fn painted_at(&self, anchor: &SubtreeAnchor) -> Option<Offset<f64>> {
        let id = anchor.get()?;
        let owner = self.pipeline.borrow().as_ref()?.upgrade()?;
        owner
            .try_with(|owner| owner.local_to_global(id, Point::new(0.0, 0.0), None))
            .flatten()
            .map(|at| Offset::new(at.x, at.y))
            .filter(|at| at.dx.is_finite() && at.dy.is_finite())
    }

    /// The record of the item keyed `key`, while it is in the list.
    pub(super) fn record(&self, key: &dyn ViewKey) -> Option<ItemRecord> {
        record_in(&self.records.borrow(), key)
    }

    /// Where the lead drag shows its item now; `None` while no drag is over
    /// a row, which shows every row where the model has it.
    pub(super) fn gap(&self, view: &Config) -> Option<Gap> {
        let (key, over) = {
            let drags = self.drags.borrow();
            let lead = drags.first()?;
            (Rc::clone(&lead.key), lead.over?)
        };
        let from = view.index_of(&*key)?;
        (over < view.item_count).then_some(Gap { from, over })
    }

    /// Whether `key` is the lead drag's item, which its row shows as a gap.
    pub(super) fn is_gap(&self, key: &dyn ViewKey) -> bool {
        self.drags
            .borrow()
            .first()
            .is_some_and(|lead| lead.key.key_eq(key))
    }

    /// A drag of `key` started: any state left from an earlier drag of it is
    /// forgotten, and its lift begins.
    pub(super) fn started(&self, key: &Rc<dyn ViewKey>) {
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
    pub(super) fn lift(&self, key: &dyn ViewKey) -> Rc<dyn Animation<f64>> {
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
    pub(super) fn proxy(&self, proxy: BoxedView, index: usize, key: &dyn ViewKey) -> BoxedView {
        let lift = self.lift(key);
        let decorator = self.view.borrow().proxy_decorator.clone();
        match decorator {
            Some(decorator) => decorator(proxy, index, lift),
            None => lifted(proxy, lift),
        }
    }

    /// Run `change` on the state of the drag of `key`, if it runs; the cell
    /// is released before the caller runs app code.
    pub(super) fn with_drag<R>(
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
    pub(super) fn hover(
        &self,
        key: &Rc<dyn ViewKey>,
        over: Option<usize>,
        shifted: Option<&Rc<dyn ViewKey>>,
    ) {
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
    pub(super) fn over_row(&self, key: &Rc<dyn ViewKey>, row: &Rc<dyn ViewKey>) {
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
    pub(super) fn take_row_reported(&self, key: &dyn ViewKey) -> bool {
        self.with_drag(key, |drag, _| std::mem::take(&mut drag.row_reported))
            .unwrap_or(false)
    }

    /// The drag of `key` moved over no row inside the list: the lead keeps
    /// its gap, which a drop there takes, and any other is over nothing.
    pub(super) fn over_no_row(&self, key: &Rc<dyn ViewKey>) {
        if self.with_drag(&**key, |_, lead| lead) == Some(false) {
            self.hover(key, None, None);
        }
    }

    /// The drag of `key` was let go over the list: the item goes where the
    /// drag shows it.
    pub(super) fn dropped(&self, ctx: &mut EventContext<'_>, key: &dyn ViewKey) {
        if let Some(Some(to)) = self.with_drag(key, |drag, _| drag.over.or(drag.left_over)) {
            let _ = self.reorder(ctx, key, to, ReorderCause::Drag);
        }
    }

    /// The proxy of the drag of `key` was at `proxy` in the window when the
    /// drag ended: the item's row lands from there. Read before anything the
    /// end changes is laid out, while the row is painted where it was.
    pub(super) fn proxy_ended(&self, key: &dyn ViewKey, proxy: Offset<f64>) {
        let painted = self
            .record(key)
            .and_then(|record| self.painted_at(&record.anchor));
        if let Some(painted) = painted {
            let _ = self.with_drag(key, |drag, _| drag.landing = Some(proxy - painted));
        }
    }

    /// The drag of `key` ended, dropped or not: its row lands from where the
    /// proxy was, the rows close the gap, and the drag is forgotten.
    pub(super) fn ended(&self, key: &Rc<dyn ViewKey>) {
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
    pub(super) fn left(&self, key: &Rc<dyn ViewKey>) {
        let _ = self.with_drag(&**key, |drag, _| {
            drag.left_over = drag.over.or(drag.left_over);
        });
        self.hover(key, None, None);
    }

    /// Move the item keyed `key` to `to`, and announce its new position;
    /// nothing when it is there already.
    pub(super) fn reorder(
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
    pub(super) fn new(view: &Config, ctx: &dyn LifecycleContext) -> Rc<Self> {
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
            records: RefCell::new(Records::default()),
            vsync: RefCell::new(None),
            rebuild: ctx.rebuild_handle(),
            writer: ctx.writer_source(),
            announcer: RefCell::new(None),
            strings: RefCell::new(None),
            pipeline: RefCell::new(None),
        })
    }

    /// Take the capabilities the rows' moves use that an ancestor provides.
    pub(super) fn acquire(&self, ctx: &dyn LifecycleContext) {
        *self.announcer.borrow_mut() = ctx.announcer();
        *self.pipeline.borrow_mut() = ctx.pipeline_owner().map(|owner| owner.downgrade());
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
    pub(super) fn dispose(&self) {
        let drags = std::mem::take(&mut *self.drags.borrow_mut());
        for mut drag in drags {
            drag.lift.dispose();
        }
        self.clock.borrow_mut().dispose();
    }

    /// The items' records in the order the rows are shown in, kept for the
    /// items still in the list and made for the new ones.
    pub(super) fn records_in(&self, view: &Config, gap: Option<Gap>) -> Rc<[Rc<PlacementSlot>]> {
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
    pub(super) fn build(self: &Rc<Self>, view: &Config, ctx: &dyn BuildContext) -> BoxedView {
        *self.strings.borrow_mut() = BoxedWidgetsLocalizations::maybe_of(ctx);
        let _prev = self.view.replace(view.clone());
        let gap = self.gap(view);
        let placement = Placement::new(
            self.clock.borrow().controller().clone(),
            MotionSpec::default().duration(SLIDE),
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
pub(super) fn lifted(proxy: BoxedView, lift: Rc<dyn Animation<f64>>) -> BoxedView {
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
