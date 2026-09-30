//! Integration tests for the lazy-sliver backend.
//!
//! Exercises the correctness paths the single-node `LeafBox` harness misses.
//! Each test uses the headless frame driver (`pump_frame`) which, since the
//! child-manager wiring landed, calls `service_child_requests` after `run_frame` — so two `tick` calls
//! are enough to settle a visible window: the first dispatches the child-build
//! request; the second lays out the now-built children.
//!
//! # Frame sequence (per `pump_frame`)
//!
//! 1. `build_scope` — drains the element-level dirty heap.
//! 2. `run_frame`  — layout: the sliver emits pending child requests and a
//!    retain-band signal.
//! 3. `service_child_requests` — drains both buffers, calls each registered
//!    `ChildManager::service` (build new, evict off-band), runs a second
//!    `build_scope` for freshly-scheduled children, marks the sliver dirty,
//!    and finalizes any inactive elements (including sparse children pushed
//!    by `on_unmount` because the host's own `child_ids` stays empty).
//!
//! So: after `lay_out` the sliver has no children; after `tick1` children are
//! built and the sliver is marked dirty; after `tick2` the sliver lays out
//! its real children and reaches a stable state.

use std::sync::Arc;

use crate::common::{LaidOut, lay_out, tight};
use flui_view::ViewExt;
use flui_widgets::prelude::*;

// ============================================================================
// Stateful items — init on entering the band, dispose on leaving it
// ============================================================================

#[derive(Clone, StatefulView)]
struct ProbeItem {
    index: usize,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
}

struct ProbeItemState {
    index: usize,
    log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>>,
}

impl StatefulView for ProbeItem {
    type State = ProbeItemState;
    fn create_state(&self) -> ProbeItemState {
        ProbeItemState {
            index: self.index,
            log: Arc::clone(&self.log),
        }
    }
}

impl ViewState<ProbeItem> for ProbeItemState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        self.log.lock().push((self.index, "init"));
    }
    fn build(&self, _view: &ProbeItem, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(200.0, 48.0)
    }
    fn dispose(&mut self) {
        self.log.lock().push((self.index, "dispose"));
    }
}

/// A `StatefulView` item — a composite top-level child — mounts (`init_state`)
/// when its index enters the band and is disposed exactly once when the band
/// moves away. Every disposed index was initialised first, and no index is
/// initialised twice while resident. (Same-frame timing is pinned by the test
/// above; this one pins the lifecycle pairing.)
pub(crate) fn lazy_list_view_builder_stateful_items_init_and_dispose_with_the_band() {
    const ITEM_COUNT: usize = 100;
    const ITEM_EXTENT: f64 = 48.0;
    let log: Arc<parking_lot::Mutex<Vec<(usize, &'static str)>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));
    let list = {
        let log = Arc::clone(&log);
        move |offset: f64| {
            let log = Arc::clone(&log);
            ListView::builder(ITEM_COUNT, ITEM_EXTENT, move |i| {
                (i < ITEM_COUNT).then(|| {
                    ProbeItem {
                        index: i,
                        log: Arc::clone(&log),
                    }
                    .boxed()
                })
            })
            .repaint_boundaries(false)
            .offset(offset)
        }
    };
    let mut laid = lay_out(list(0.0), tight(200.0, 200.0));

    let inits_at_start: Vec<usize> = log
        .lock()
        .iter()
        .filter(|(_, what)| *what == "init")
        .map(|(i, _)| *i)
        .collect();
    assert!(
        inits_at_start.contains(&0) && inits_at_start.contains(&4),
        "visible items must have initialised state in the bootstrap frame; got {inits_at_start:?}"
    );
    assert!(
        !inits_at_start.contains(&50),
        "an off-band item must not have been created; got {inits_at_start:?}"
    );
    assert!(
        log.lock().iter().all(|(_, what)| *what != "dispose"),
        "nothing is disposed before the band moves"
    );

    laid.pump_widget(list(50.0 * ITEM_EXTENT));
    let entries = log.lock().clone();
    let disposed: Vec<usize> = entries
        .iter()
        .filter(|(_, what)| *what == "dispose")
        .map(|(i, _)| *i)
        .collect();
    assert!(
        disposed.contains(&0) && disposed.contains(&4),
        "items that left the band must be disposed in the frame that moved it; got {disposed:?}"
    );
    let inits_after: Vec<usize> = entries
        .iter()
        .filter(|(_, what)| *what == "init")
        .map(|(i, _)| *i)
        .collect();
    assert!(
        inits_after.contains(&50) && inits_after.contains(&54),
        "items of the new band must initialise in that same frame; got {inits_after:?}"
    );
    for index in &disposed {
        assert!(
            inits_after.contains(index),
            "index {index} was disposed without ever being initialised"
        );
    }
    // Resident set discipline: no index is initialised twice without a
    // dispose in between.
    let mut live = std::collections::HashSet::new();
    for (i, what) in &entries {
        match *what {
            "init" => assert!(
                live.insert(*i),
                "index {i} initialised twice while resident"
            ),
            "dispose" => assert!(live.remove(i), "index {i} disposed while not resident"),
            _ => unreachable!(),
        }
    }
}

// ============================================================================
// Estimate adaptation — a wrong seed estimate must not cost a frame per band
// generation, nor trip the frame's pass bound
// ============================================================================

/// The geometry of the tree's single `RenderSliverList`.
fn sliver_list_geometry(laid: &LaidOut) -> flui_rendering::constraints::SliverGeometry {
    let slivers = laid.find_all_by_render_type("RenderSliverList");
    assert_eq!(slivers.len(), 1, "expected exactly one RenderSliverList");
    laid.sliver_geometry(slivers[0])
}

/// Content whose measured extents keep falling faster than the mean can
/// follow — every item past the entry point is half the height of the one
/// before — cannot settle inside the frame's lazy-band budget. That is the
/// deferral path, and it must stay a deferral: no `BUG:` panic in a debug
/// build, and the band completes over the following frames exactly as the
/// old post-paint service path would have.
pub(crate) fn lazy_list_view_builder_pathological_extents_defer_instead_of_panicking() {
    const ITEM_COUNT: usize = 400;
    const ENTRY: usize = 25;
    const SEED: f64 = 200.0;
    let height_of = |i: usize| -> f64 {
        if i < ENTRY {
            SEED
        } else {
            (SEED / 2_f64.powi((i - ENTRY) as i32 + 1)).max(0.25)
        }
    };
    let mut laid = lay_out(
        ListView::builder(ITEM_COUNT, SEED, move |i| {
            (i < ITEM_COUNT).then(|| SizedBox::new(200.0, height_of(i)).boxed())
        })
        .offset(ENTRY as f64 * SEED),
        tight(200.0, 600.0),
    );
    // Whatever the first frame managed, the following frames finish the band.
    for _ in 0..12 {
        laid.tick();
    }
    // Past the entry point the remaining 375 items sum to under 200 px, so a
    // settled band reaches the list's end: every one of them is resident
    // (one boundary + one box each, plus the viewport and the sliver). The
    // 25 items above the entry are never measured — they stay hinted — so the
    // total extent is deliberately not asserted here.
    let resident_items = laid.render_node_count().saturating_sub(2) / 2;
    assert!(
        resident_items >= ITEM_COUNT - ENTRY,
        "the band must reach the list's end over a few frames; resident items={resident_items}"
    );
    let geometry = sliver_list_geometry(&laid);
    assert!(
        geometry.scroll_extent.is_finite() && geometry.scroll_extent > 0.0,
        "geometry must stay sane while the band settles; {geometry:?}"
    );
}

/// The lazy-band pass budget is a deferral, never a panic. With the budget
/// forced to a single pass, the frame that mounts a 20× over-estimated list
/// services exactly one band generation inside its fixpoint; the widened
/// band is picked up by the frame's post-paint safety net — built, but not
/// laid out until the next frame — and that next frame completes it. The
/// default budget settles the same scene in one frame (the test above), so
/// the knob is what makes this path observable.
pub(crate) fn lazy_list_view_builder_exhausted_pass_budget_defers_the_rest_to_the_next_frame() {
    const ITEM_COUNT: usize = 1000;
    const SEED_ESTIMATE: f64 = 200.0;
    const ACTUAL: f64 = 10.0;
    let list = || {
        ListView::builder(ITEM_COUNT, SEED_ESTIMATE, |i| {
            (i < ITEM_COUNT).then(|| {
                SizedBox::new(200.0, ACTUAL)
                    .child(Text::new(format!("item{i}")))
                    .boxed()
            })
        })
        .repaint_boundaries(false)
    };
    let mut laid = lay_out(SizedBox::square(10.0), tight(200.0, 600.0));
    laid.with_build_owner_mut(|owner| owner.set_lazy_band_pass_budget_for_test(1));

    laid.pump_widget(list());
    // `try_size`: a deferred item exists in the render tree (the safety net
    // built it) but has no geometry until the next frame lays it out.
    let laid_out = |laid: &LaidOut, i: usize| -> Option<f64> {
        laid.find_text(&format!("item{i}"))
            .and_then(|id| laid.try_size(id))
            .map(|size| size.height)
    };
    assert!(
        laid_out(&laid, 0).is_some_and(|h| h > 0.0),
        "the first band generation is serviced and laid out within the budget"
    );
    let deferred = laid_out(&laid, 30);
    assert!(
        deferred.is_none_or(|h| h == 0.0),
        "an item the widened band requested past the budget must not be laid out \
         this frame (deferred, not panicked); item30 height={deferred:?}"
    );

    laid.tick();
    assert!(
        laid_out(&laid, 30).is_some_and(|h| h > 0.0),
        "the deferred generation completes on the next frame"
    );
}

// ============================================================================
// Keyed identity — insert, reorder, duplicate keys, and a GlobalKey graft
// ============================================================================

/// A keyed item whose state records which data id it was created for and
/// how many times `init_state` ran across the whole test.
#[derive(Clone)]
struct KeyedRow {
    id: u32,
    key: flui_foundation::ValueKey<u32>,
    inits: Arc<parking_lot::Mutex<Vec<u32>>>,
}

struct KeyedRowState {
    born_as: u32,
    log: Arc<parking_lot::Mutex<Vec<u32>>>,
}

impl StatefulView for KeyedRow {
    type State = KeyedRowState;
    fn create_state(&self) -> KeyedRowState {
        KeyedRowState {
            born_as: self.id,
            log: Arc::clone(&self.inits),
        }
    }
}

impl ViewState<KeyedRow> for KeyedRowState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        // One entry per STATE created: a preserved element never adds a
        // second entry for its id, a remounted one does.
        self.log.lock().push(self.born_as);
    }
    fn build(&self, _view: &KeyedRow, _ctx: &dyn BuildContext) -> impl IntoView {
        // The paragraph carries the STATE's id (what the element was born as),
        // so a remounted element reads differently from a preserved one.
        SizedBox::new(200.0, 48.0).child(Text::new(format!("row{}", self.born_as)))
    }
}

impl flui_view::View for KeyedRow {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
    fn key(&self) -> Option<&dyn flui_foundation::ViewKey> {
        Some(&self.key)
    }
}

type Data = Arc<parking_lot::Mutex<Vec<u32>>>;

fn keyed_list(
    data: &Data,
    inits: &Arc<parking_lot::Mutex<Vec<u32>>>,
    with_callback: bool,
) -> ListView {
    let snapshot: Vec<u32> = data.lock().clone();
    let count = snapshot.len();
    let inits = Arc::clone(inits);
    let rows = snapshot.clone();
    let list = ListView::builder(count, 48.0, move |i| {
        rows.get(i).map(|&id| {
            KeyedRow {
                id,
                key: flui_foundation::ValueKey::new(id),
                inits: Arc::clone(&inits),
            }
            .boxed()
        })
    });
    if with_callback {
        let rows = snapshot;
        list.find_index_by_key(move |key| {
            key.as_any()
                .downcast_ref::<flui_foundation::ValueKey<u32>>()
                .and_then(|k| rows.iter().position(|id| id == k.value()))
        })
    } else {
        list
    }
}

/// The id every on-stage row's STATE was born as, in list order.
fn born_ids(laid: &LaidOut, ids: &[u32]) -> Vec<u32> {
    let mut found: Vec<(f64, u32)> = ids
        .iter()
        .filter_map(|&id| {
            laid.find_text(&format!("row{id}"))
                .map(|node| (laid.absolute_offset(node).dy, id))
        })
        .collect();
    found.sort_by(|a, b| a.0.total_cmp(&b.0));
    found.into_iter().map(|(_, id)| id).collect()
}

/// A keyed row whose data moves far away while the viewport jumps to its
/// new place in the same frame keeps its state: the reconcile relocates it
/// before the band eviction judges it by its NEW index. Evicting first
/// destroyed it and mounted a fresh row at the destination.
pub(crate) fn lazy_list_view_builder_keyed_row_moving_with_the_viewport_keeps_state() {
    const EXTENT: f64 = 48.0;
    let data: Data = Arc::new(parking_lot::Mutex::new((0..100).collect()));
    let inits = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let mut laid = lay_out(keyed_list(&data, &inits, true), tight(200.0, 200.0));
    assert_eq!(born_ids(&laid, &[0, 1, 2]), vec![0, 1, 2]);

    // Row 1 moves to index 60; the viewport jumps there in the same frame.
    {
        let mut d = data.lock();
        let row = d.remove(1);
        d.insert(60, row);
    }
    laid.pump_widget(keyed_list(&data, &inits, true).offset(58.0 * EXTENT));
    let top = laid
        .absolute_offset(
            laid.find_text("row1")
                .expect("row 1 must be resident at its new index"),
        )
        .dy;
    assert!(
        (0.0..200.0).contains(&top),
        "row 1 is on screen at its new index; top={top}"
    );
    // Its state is the one it was born with: the element moved, it was not
    // remounted — a remount would read `row1` too (the id is the data), so
    // the evidence is the state log: exactly one state ever created for id 1.
    let states_for_1 = inits.lock().iter().filter(|&&id| id == 1).count();
    assert_eq!(
        states_for_1, 1,
        "row 1's state must be created once and carried to its new index"
    );
}
