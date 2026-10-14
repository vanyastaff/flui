//! `AnimatedList`, `AnimatedGrid` and `SliverAnimatedList`: the keyed diff
//! through mounted trees — what an exit paints, which element a returning
//! key gets, where moved items are mid-slide — read back from the laid-out
//! opacity and offsets of each row.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, tight};
use flui_animation::{MotionOverride, MotionSpec, Stagger, StaggerOrigin, Vsync, curves};
use flui_foundation::RenderId;
use flui_rendering::delegates::SliverGridDelegateWithFixedCrossAxisCount;
use flui_view::prelude::{BuildContext, LifecycleContext, StatefulView};
use flui_view::{IntoView, ViewState};
use flui_widgets::motion::{Effects, Transition};
use flui_widgets::{AnimatedGrid, AnimatedList, SizedBox, Text, VsyncScope};

/// Every row's height, and a grid tile's side.
const ROW: f64 = 40.0;

fn ms(millis: u64) -> Duration {
    Duration::from_millis(millis)
}

/// How often each row's state was created and disposed, by id.
#[derive(Default)]
struct Lives {
    inits: RefCell<HashMap<u32, usize>>,
    disposes: RefCell<HashMap<u32, usize>>,
}

impl Lives {
    fn inits(&self, id: u32) -> usize {
        self.inits.borrow().get(&id).copied().unwrap_or(0)
    }

    fn disposes(&self, id: u32) -> usize {
        self.disposes.borrow().get(&id).copied().unwrap_or(0)
    }
}

/// A stateful row labelled `row{id}`, `extent` tall, counting its state's
/// lifetime.
#[derive(Clone, StatefulView)]
struct Row {
    id: u32,
    extent: f64,
    lives: Rc<Lives>,
}

struct RowState {
    id: u32,
    lives: Rc<Lives>,
}

impl StatefulView for Row {
    type State = RowState;

    fn create_state(&self) -> RowState {
        RowState {
            id: self.id,
            lives: Rc::clone(&self.lives),
        }
    }
}

impl ViewState<Row> for RowState {
    fn init_state(&mut self, _ctx: &dyn LifecycleContext) {
        *self.lives.inits.borrow_mut().entry(self.id).or_default() += 1;
    }

    fn build(&self, view: &Row, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(ROW, view.extent).child(Text::new(format!("row{}", self.id)))
    }

    fn dispose(&mut self) {
        *self.lives.disposes.borrow_mut().entry(self.id).or_default() += 1;
    }
}

/// A linear 100 ms fade, so a sample's opacity is its elapsed share.
fn fade() -> Transition {
    Transition::fade().duration(ms(100)).curve(curves::LINEAR)
}

/// A linear 100 ms slide.
fn slide() -> Option<MotionSpec> {
    Some(
        MotionSpec::default()
            .with_duration(ms(100))
            .with_curve(curves::LINEAR),
    )
}

/// A list of `ids` in a 100 × `height` viewport.
struct Fixture {
    registry: Vsync,
    lives: Rc<Lives>,
    height: f64,
    placement: Option<MotionSpec>,
}

impl Fixture {
    fn new(height: f64) -> Self {
        Self {
            registry: Vsync::new(),
            lives: Rc::default(),
            height,
            placement: slide(),
        }
    }

    fn list(&self, ids: &[u32]) -> VsyncScope {
        let lives = Rc::clone(&self.lives);
        VsyncScope::new(
            self.registry.clone(),
            AnimatedList::new(
                ids.to_vec(),
                |id| *id,
                move |id| Row {
                    id: *id,
                    extent: ROW,
                    lives: Rc::clone(&lives),
                },
            )
            .item_extent_estimate(ROW)
            .transition(fade())
            .placement(self.placement.clone()),
        )
    }

    fn mount(&self, ids: &[u32]) -> LaidOut {
        lay_out_animated(
            self.list(ids),
            tight(100.0, self.height),
            self.registry.clone(),
        )
    }
}

fn row(laid: &LaidOut, id: u32) -> Option<RenderId> {
    laid.find_text(&format!("row{id}"))
}

/// Where row `id` is painted, from the top of the list.
fn dy(laid: &LaidOut, id: u32) -> f64 {
    let node = row(laid, id).unwrap_or_else(|| panic!("row{id} is mounted"));
    laid.absolute_offset(node).dy
}

/// The opacity row `id` is painted with: the product of every animated
/// opacity above it.
fn opacity(laid: &LaidOut, id: u32) -> f64 {
    let node = row(laid, id).unwrap_or_else(|| panic!("row{id} is mounted"));
    let fades = laid.find_all_by_render_type("RenderAnimatedOpacity");
    let mut alpha = 1.0;
    let mut current = Some(node);
    while let Some(id) = current {
        if fades.contains(&id) {
            alpha *= laid.opacity(id);
        }
        current = laid
            .pipeline_owner()
            .with(|owner| owner.render_tree().parent(id));
    }
    alpha
}

fn within(actual: f64, low: f64, high: f64, what: &str) {
    assert!(
        actual > low && actual < high,
        "{what}: expected strictly between {low} and {high}, got {actual}"
    );
}

fn close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected} ± {tolerance}, got {actual}"
    );
}

/// A removed item stays where it was and fades out; once its exit has run it
/// is gone, its state disposed, and the item after it slides up.
pub(crate) fn a_removed_item_plays_its_exit_and_then_drops() {
    let fixture = Fixture::new(400.0);
    let mut laid = fixture.mount(&[1, 2, 3]);
    close(dy(&laid, 3), 2.0 * ROW, 1e-9, "row3 mounts third");

    laid.pump_widget(fixture.list(&[1, 3]));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    close(opacity(&laid, 2), 0.5, 0.05, "the leaving row at half time");
    close(dy(&laid, 2), ROW, 1e-9, "the leaving row holds its place");
    close(
        dy(&laid, 3),
        2.0 * ROW,
        1e-9,
        "the next row waits behind it",
    );
    assert_eq!(fixture.lives.disposes(2), 0, "still mounted mid-exit");

    laid.pump_for(ms(60));
    assert!(row(&laid, 2).is_none(), "the exit ran out: the row is gone");
    assert_eq!(fixture.lives.disposes(2), 1, "and its state disposed");
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    within(dy(&laid, 3), ROW, 2.0 * ROW, "the next row slides up");
    laid.pump_for(ms(100));
    close(dy(&laid, 3), ROW, 1e-9, "and lands in the gap");
    assert_eq!(fixture.lives.inits(3), 1, "the next row kept its state");
}

/// A key that comes back mid-exit takes back its element and turns back
/// from the opacity its exit reached.
pub(crate) fn a_key_returning_mid_exit_reclaims_its_element() {
    let fixture = Fixture::new(400.0);
    let mut laid = fixture.mount(&[1, 2, 3]);
    laid.pump_widget(fixture.list(&[1, 3]));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    let seam = opacity(&laid, 2);
    within(seam, 0.4, 0.6, "half way out");

    laid.pump_widget(fixture.list(&[1, 2, 3]));
    laid.pump_for(Duration::ZERO);
    close(
        opacity(&laid, 2),
        seam,
        1e-9,
        "the return starts at the seam",
    );
    laid.pump_for(ms(20));
    let next = opacity(&laid, 2);
    assert!(
        next > seam && next - seam < 0.3,
        "rising from the seam: {seam} -> {next}"
    );
    laid.pump_for(ms(100));
    close(opacity(&laid, 2), 1.0, 1e-9, "fully back");
    assert_eq!(
        fixture.lives.disposes(2),
        0,
        "the element was never disposed"
    );
    assert_eq!(fixture.lives.inits(2), 1, "nor created again");
    close(dy(&laid, 2), ROW, 1e-9, "in its old place");
}

/// Swapped items slide past each other: mid-slide both sit strictly between
/// their old and new places, and they land exactly. Without placement they
/// jump.
pub(crate) fn swapped_items_slide_between_their_places() {
    for (placement, sliding) in [(slide(), true), (None, false)] {
        let mut fixture = Fixture::new(400.0);
        fixture.placement = placement;
        let mut laid = fixture.mount(&[1, 2]);
        laid.pump_widget(fixture.list(&[2, 1]));
        laid.pump_for(Duration::ZERO);
        laid.pump_for(ms(50));
        if sliding {
            within(dy(&laid, 1), 0.0, ROW, "row1 slides down");
            within(dy(&laid, 2), 0.0, ROW, "row2 slides up");
        } else {
            close(dy(&laid, 1), ROW, 1e-9, "row1 jumps");
            close(dy(&laid, 2), 0.0, 1e-9, "row2 jumps");
        }
        laid.pump_for(ms(100));
        close(dy(&laid, 1), ROW, 1e-9, "row1 lands");
        close(dy(&laid, 2), 0.0, 1e-9, "row2 lands");
        assert_eq!(
            (fixture.lives.inits(1), fixture.lives.inits(2)),
            (1, 1),
            "moved rows keep their state"
        );
    }
}

/// Two items with one key are a caller bug, named with the key.
pub(crate) fn a_duplicate_key_panics_naming_it() {
    let failure = catch_unwind(AssertUnwindSafe(|| {
        AnimatedList::new([7_u32, 3, 7], |id| *id, |_| SizedBox::new(1.0, 1.0))
    }))
    .expect_err("a duplicate key must be refused");
    let message = flui_foundation::panic::payload_text(failure.as_ref()).unwrap_or_default();
    assert!(
        message.starts_with("BUG:") && message.contains("key 7"),
        "the panic names the duplicate key: {message}"
    );
}

/// Only an item laid out before and after a change slides: one brought back
/// from beyond the cache band appears in its place, even though it was laid
/// out somewhere else long ago, while the items it displaced slide.
pub(crate) fn an_item_entering_the_band_takes_its_place_without_sliding() {
    let fixture = Fixture::new(100.0);
    let order = |head: &[u32], tail: &[u32]| -> Vec<u32> {
        let mut ids = head.to_vec();
        ids.extend((0..59).filter(|id| !head.contains(id)));
        ids.extend(tail);
        ids
    };
    // Row 59 starts on screen, then leaves the band for the far end.
    let mut laid = fixture.mount(&order(&[59], &[]));
    close(dy(&laid, 59), 0.0, 1e-9, "row59 starts first");
    laid.pump_widget(fixture.list(&order(&[], &[59])));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(200));
    assert!(row(&laid, 59).is_none(), "row59 is far beyond the band");

    // It comes back second, from where nothing on screen saw it.
    laid.pump_widget(fixture.list(&order(&[0, 59], &[])));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    close(dy(&laid, 59), ROW, 1e-9, "row59 appears at its place");
    within(
        dy(&laid, 1),
        ROW,
        2.0 * ROW,
        "row1, laid out before, slides",
    );
}

/// Placement follows the roster, not every layout: an item that grows moves
/// the items after it at once.
pub(crate) fn a_resized_item_moves_its_followers_at_once() {
    let registry = Vsync::new();
    let lives = Rc::new(Lives::default());
    let list = |first: f64| {
        let lives = Rc::clone(&lives);
        VsyncScope::new(
            registry.clone(),
            AnimatedList::new(
                [(1_u32, first), (2, ROW)],
                |row| row.0,
                move |row| Row {
                    id: row.0,
                    extent: row.1,
                    lives: Rc::clone(&lives),
                },
            )
            .item_extent_estimate(ROW)
            .transition(fade())
            .placement(slide()),
        )
    };
    let mut laid = lay_out_animated(list(ROW), tight(100.0, 400.0), registry.clone());
    laid.pump_widget(list(1.5 * ROW));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(10));
    close(
        dy(&laid, 2),
        1.5 * ROW,
        1e-9,
        "row2 follows the resize at once",
    );
}

/// Under reduced motion an insertion shows, a removal drops and a move lands
/// on the next frame; at full motion the same frame is mid-flight.
pub(crate) fn reduced_motion_lands_every_change_on_the_next_frame() {
    for (policy, reduced) in [
        (MotionOverride::Full, false),
        (MotionOverride::Reduce, true),
    ] {
        let fixture = Fixture::new(400.0);
        let mut laid = fixture.mount(&[1, 2, 3]);
        laid.set_motion_override(policy);
        // 2 leaves, 4 enters, 1 and 3 swap.
        laid.pump_widget(fixture.list(&[3, 4, 1]));
        laid.pump_for(Duration::ZERO);
        laid.pump_for(ms(10));
        let what = |case: &str| format!("{policy:?}: {case}");
        if reduced {
            assert!(row(&laid, 2).is_none(), "{}", what("the removal dropped"));
            close(opacity(&laid, 4), 1.0, 1e-9, &what("the insertion shows"));
            close(dy(&laid, 3), 0.0, 1e-9, &what("the move landed"));
        } else {
            assert!(
                row(&laid, 2).is_some(),
                "{}",
                what("the removal is leaving")
            );
            within(
                opacity(&laid, 4),
                0.0,
                1.0,
                &what("the insertion is entering"),
            );
            within(dy(&laid, 3), 0.0, 2.0 * ROW, &what("the move is sliding"));
        }
    }
}

/// A grid's tiles slide between cells across both axes.
pub(crate) fn grid_tiles_slide_between_cells() {
    let registry = Vsync::new();
    let lives = Rc::new(Lives::default());
    let grid = |ids: &[u32]| {
        let lives = Rc::clone(&lives);
        VsyncScope::new(
            registry.clone(),
            AnimatedGrid::new(
                Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2)),
                ids.to_vec(),
                |id| *id,
                move |id| Row {
                    id: *id,
                    extent: ROW,
                    lives: Rc::clone(&lives),
                },
            )
            .transition(fade())
            .placement(slide()),
        )
    };
    // Two 40 px columns: tiles are 40 × 40.
    let mut laid = lay_out_animated(grid(&[1, 2, 3]), tight(2.0 * ROW, 400.0), registry.clone());
    let at = |laid: &LaidOut, id: u32| {
        let node = row(laid, id).unwrap_or_else(|| panic!("tile {id} is mounted"));
        laid.absolute_offset(node)
    };
    assert_eq!(
        (at(&laid, 3).dx, at(&laid, 3).dy),
        (0.0, ROW),
        "tile 3 starts below tile 1"
    );

    // Tile 3 moves to the first cell; 1 and 2 shift one cell along.
    laid.pump_widget(grid(&[3, 1, 2]));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(50));
    let three = at(&laid, 3);
    within(three.dy, 0.0, ROW, "tile 3 rises");
    close(three.dx, 0.0, 1e-9, "in its column");
    let two = at(&laid, 2);
    within(two.dx, 0.0, ROW, "tile 2 crosses columns");
    within(two.dy, 0.0, ROW, "and drops a row");
    laid.pump_for(ms(100));
    let two = at(&laid, 2);
    assert_eq!((two.dx, two.dy), (0.0, ROW), "tile 2 lands in its new cell");
}

/// The items a staggered list mounts with arrive one stagger step apart.
pub(crate) fn a_staggered_list_mounts_its_items_one_after_another() {
    let registry = Vsync::new();
    let lives = Rc::new(Lives::default());
    let entrance = Effects::new()
        .duration(ms(100))
        .curve(curves::LINEAR)
        .fade_in();
    let list = VsyncScope::new(
        registry.clone(),
        AnimatedList::new(
            [0_u32, 1, 2],
            |id| *id,
            move |id| Row {
                id: *id,
                extent: ROW,
                lives: Rc::clone(&lives),
            },
        )
        .item_extent_estimate(ROW)
        .stagger(entrance, Stagger::new(ms(100), StaggerOrigin::First)),
    );
    let mut laid = lay_out_animated(list, tight(100.0, 400.0), registry.clone());
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(150));
    close(opacity(&laid, 0), 1.0, 1e-9, "the first arrived");
    close(opacity(&laid, 1), 0.5, 0.05, "the second is half way");
    close(opacity(&laid, 2), 0.0, 1e-9, "the third waits its turn");
}

/// An item whose destructor panics when its exit drops it: the failure
/// propagates once the roster is committed, and the list keeps working.
pub(crate) fn a_panicking_item_drop_leaves_the_list_working() {
    struct Item {
        id: u32,
        armed: Rc<Cell<bool>>,
    }
    impl Drop for Item {
        fn drop(&mut self) {
            if self.armed.get() && self.id == 2 {
                self.armed.set(false);
                panic!("item 2 drop panics");
            }
        }
    }
    let registry = Vsync::new();
    let lives = Rc::new(Lives::default());
    let armed = Rc::new(Cell::new(false));
    let list = |ids: &[u32]| {
        let lives = Rc::clone(&lives);
        let items: Vec<Item> = ids
            .iter()
            .map(|&id| Item {
                id,
                armed: Rc::clone(&armed),
            })
            .collect();
        VsyncScope::new(
            registry.clone(),
            AnimatedList::new(
                items,
                |item| item.id,
                move |item| Row {
                    id: item.id,
                    extent: ROW,
                    lives: Rc::clone(&lives),
                },
            )
            .item_extent_estimate(ROW)
            .transition(fade())
            .placement(slide()),
        )
    };
    let mut laid = lay_out_animated(list(&[1, 2, 3]), tight(100.0, 400.0), registry.clone());
    laid.pump_widget(list(&[1, 3]));
    armed.set(true);
    laid.pump_for(Duration::ZERO);
    let outcome = catch_unwind(AssertUnwindSafe(|| laid.pump_for(ms(200))));
    assert!(!armed.get(), "the dropped item's destructor ran");
    drop(outcome);
    laid.pump_for(ms(20));
    assert!(row(&laid, 2).is_none(), "the dropped item stays dropped");

    laid.pump_widget(list(&[1, 3, 4]));
    laid.pump_for(Duration::ZERO);
    laid.pump_for(ms(200));
    close(
        opacity(&laid, 4),
        1.0,
        1e-9,
        "a later insertion still enters",
    );
    close(dy(&laid, 4), 2.0 * ROW, 1e-9, "after the survivors");
}
