//! `Carousel` on the keyline model: checked layouts, items masked between keylines, taps that
//! land only where an item shows, the controller's index and moves, `KeylineSnap`, and
//! autoplay with its pause rules.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{MotionOverride, Vsync};
use flui_foundation::geometry::Offset;
use flui_objects::CarouselGeometry;
use flui_painting::Alignment;
use flui_painting::styling::Color;
use flui_testing::a11y::Role;
use flui_testing::{Action, ActionRequest, NodeId, PointerScript, TouchScript, TreeId};
use flui_view::ViewExt;
use flui_view::prelude::*;
use flui_widgets::{
    Carousel, CarouselAutoplay, CarouselAutoplayError, CarouselController, CarouselItemScope,
    CarouselLayout, CarouselLayoutError, ColoredBox, GestureDetector, KeylineSnap, Semantics,
    VsyncScope,
};

use crate::common::{LaidOut, lay_out_animated, tight};

const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 200.0;
const SPACING: f64 = 8.0;

/// The scopes the items were last built with, by index.
type Scopes = Rc<RefCell<Vec<Option<CarouselItemScope>>>>;

/// Records the items' scopes and the taps they take.
#[derive(Default)]
struct Seen {
    scopes: Scopes,
    taps: Rc<RefCell<Vec<usize>>>,
}

impl Seen {
    /// A carousel of `count` coloured items that report to this seen.
    fn carousel(&self, count: usize) -> Carousel {
        self.scopes.borrow_mut().resize(count, None);
        let scopes = Rc::clone(&self.scopes);
        let taps = Rc::clone(&self.taps);
        Carousel::builder(count, move |_ctx, scope, index| {
            scopes.borrow_mut()[index] = Some(scope.clone());
            let taps = Rc::clone(&taps);
            let shade = u8::try_from(index % 12).expect("BUG: below 12") * 20;
            GestureDetector::new()
                .on_tap(move |_cx| taps.borrow_mut().push(index))
                .child(ColoredBox::new(Color::rgb(shade, 40, 60)))
        })
        .item_spacing(SPACING)
    }

    /// The extent item `index` shows along the axis; `None` before it is laid out.
    fn shown(&self, index: usize) -> Option<f64> {
        let scope = self.scopes.borrow()[index].clone()?;
        scope.mask().map(|mask| mask.width())
    }
}

fn mount(view: impl IntoView) -> LaidOut {
    let vsync = Vsync::new();
    let tree = VsyncScope::new(vsync.clone(), view).boxed();
    let mut laid = lay_out_animated(tree, tight(WIDTH, HEIGHT), vsync);
    laid.tick();
    laid
}

fn settle(laid: &mut LaidOut) {
    for _ in 0..120 {
        laid.pump_for(Duration::from_millis(16));
    }
}

/// Runs `span` of frames a quarter second apart.
fn run_for(laid: &mut LaidOut, span: Duration) {
    let frame = Duration::from_millis(250);
    let mut run = Duration::ZERO;
    while run < span {
        laid.pump_for(frame);
        run += frame;
    }
}

fn at(x: f64) -> Offset<f64> {
    Offset::new(x, HEIGHT / 2.0)
}

/// A touch drag from `from` to `to` over ten samples that lifts while still moving.
fn fling(from: f64, to: f64) -> PointerScript {
    let mut script = TouchScript::new("fling").down(1, at(from));
    for step in 1..=10 {
        let t = f64::from(step) / 10.0;
        script = script
            .wait(Duration::from_millis(16))
            .move_to(1, at(from + (to - from) * t));
    }
    script.up(1).build().expect("one whole contact")
}

fn geometry(layout: &CarouselLayout, count: usize) -> CarouselGeometry {
    CarouselGeometry::try_new(layout, WIDTH, SPACING, count).expect("a layout with items to show")
}

#[test]
fn layouts_and_autoplay_are_checked_when_made() {
    use CarouselLayoutError::{
        AlignmentOutOfRange, InvertedRange, NoWeight, NonFinite, NonPositive, TooLarge,
    };
    let cases = [
        (
            "material browse",
            CarouselLayout::multi_browse(40.0, 56.0),
            None,
        ),
        (
            "inverted small range",
            CarouselLayout::multi_browse(56.0, 40.0),
            Some(InvertedRange),
        ),
        (
            "NaN small",
            CarouselLayout::multi_browse(f64::NAN, 56.0),
            Some(NonFinite),
        ),
        (
            "zero small",
            CarouselLayout::multi_browse(0.0, 56.0),
            Some(NonPositive),
        ),
        (
            "huge item",
            CarouselLayout::uncontained(1e8),
            Some(TooLarge),
        ),
        (
            "negative item",
            CarouselLayout::uncontained(-1.0),
            Some(NonPositive),
        ),
        (
            "infinite item",
            CarouselLayout::uncontained(f64::INFINITY),
            Some(NonFinite),
        ),
        (
            "hero NaN",
            CarouselLayout::hero(Alignment::new(f64::NAN, 0.0)),
            Some(NonFinite),
        ),
        (
            "hero MAX",
            CarouselLayout::hero(Alignment::new(f64::MAX, 0.0)),
            Some(AlignmentOutOfRange),
        ),
        (
            "zero weights",
            CarouselLayout::weighted([0.0, 0.0]),
            Some(NoWeight),
        ),
        (
            "negative weight",
            CarouselLayout::weighted([1.0, -1.0]),
            Some(NonPositive),
        ),
        (
            "extreme weights",
            CarouselLayout::weighted([f64::MAX, f64::MAX]),
            None,
        ),
    ];
    for (name, layout, error) in cases {
        assert_eq!(layout.err(), error, "{name}");
    }
    assert_eq!(
        CarouselAutoplay::every(Duration::from_millis(4999)).err(),
        Some(CarouselAutoplayError::TooFast)
    );
    assert!(CarouselAutoplay::every(Duration::from_secs(5)).is_ok());
}

#[test]
fn extreme_weights_give_equal_shares() {
    let layout = CarouselLayout::weighted([f64::MAX, f64::MAX]).expect("finite weights");
    let seen = Seen::default();
    let _laid = mount(seen.carousel(4).layout(layout).item_spacing(0.0));
    assert_eq!(seen.shown(0), Some(WIDTH / 2.0));
    assert_eq!(seen.shown(1), Some(WIDTH / 2.0));
}

#[test]
fn items_are_masked_to_the_keylines_they_pass() {
    let layout = CarouselLayout::default();
    let shape = geometry(&layout, 10);
    let controller = CarouselController::new();
    let seen = Seen::default();
    let mut laid = mount(seen.carousel(10).controller(controller.clone()));

    let (large, medium, small) = (
        seen.shown(0).expect("item 0 shows"),
        seen.shown(1).expect("item 1 shows"),
        seen.shown(2).expect("item 2 shows"),
    );
    assert!(
        large > medium && medium > small && small >= 40.0,
        "{large} {medium} {small}"
    );
    assert!(large <= shape.item_extent());

    // Between two resting offsets every shown item is masked to where the geometry places it,
    // strictly between the extents of the keylines on either side.
    let between = f64::midpoint(shape.rest_offset(1), shape.rest_offset(2));
    controller.jump_to_item(1).expect("no build is running");
    laid.tick();
    assert_eq!(
        seen.shown(1),
        Some(large),
        "item 1 rests on the large keyline"
    );
    let position = shape.place(2, between);
    assert!(position.progress > 0.0 && position.progress < 1.0);
    let travelling = position.mask_extent;
    assert!(travelling > medium && travelling < large, "{travelling}");
}

#[test]
fn a_tap_lands_only_where_an_item_shows() {
    let layout = CarouselLayout::default();
    let shape = geometry(&layout, 10);
    let seen = Seen::default();
    let laid = mount(seen.carousel(10));
    // Item 1 is laid out at the large extent about its centre, so its box reaches over the
    // leading part of where item 2 shows; only item 2 takes a tap there.
    let (medium, small) = (shape.place(1, 0.0), shape.place(2, 0.0));
    let x = small.center - small.mask_extent / 2.0 + 4.0;
    assert!(
        x > medium.center + medium.mask_extent / 2.0,
        "past item 1's mask"
    );
    assert!(
        x < medium.center + shape.item_extent() / 2.0,
        "inside item 1's box"
    );
    laid.dispatch_pointer_down(x, HEIGHT / 2.0);
    laid.dispatch_pointer_up(x, HEIGHT / 2.0);
    assert_eq!(*seen.taps.borrow(), [2]);
}

#[test]
fn the_controller_tracks_one_carousel() {
    let controller = CarouselController::new();
    assert_eq!(controller.index(), None, "not taken by a carousel");
    controller.animate_to_item(2).expect("no build is running");
    let seen = Seen::default();
    let mut laid = mount(seen.carousel(10).controller(controller.clone()));
    laid.tick();
    assert_eq!(
        controller.index(),
        Some(2),
        "opened on the item asked for before layout"
    );

    controller
        .jump_to_item(usize::MAX)
        .expect("no build is running");
    laid.tick();
    assert_eq!(controller.index(), Some(9), "past the end is the last item");

    laid.pump_widget(VsyncScope::new(Vsync::new(), ColoredBox::new(Color::BLACK)));
    assert_eq!(controller.index(), None, "let go on unmount");

    let empty = CarouselController::new();
    let mut laid = mount(seen.carousel(0).controller(empty.clone()));
    empty.jump_to_item(3).expect("no build is running");
    laid.tick();
    assert_eq!(empty.index(), None, "an empty carousel has no active item");
}

#[test]
fn a_second_carousel_runs_on_its_own_controller() {
    let controller = CarouselController::new();
    let (first, second) = (Seen::default(), Seen::default());
    let mut laid = mount(flui_widgets::Column::new(vec![
        flui_widgets::Expanded::new(first.carousel(10).controller(controller.clone())).boxed(),
        flui_widgets::Expanded::new(second.carousel(10).controller(controller.clone())).boxed(),
    ]));
    controller.jump_to_item(4).expect("no build is running");
    laid.tick();
    assert_eq!(controller.index(), Some(4));
    let large = first.shown(4).expect("the first carousel moved");
    assert_eq!(
        second.shown(0),
        Some(large),
        "the second stayed on its first item"
    );
}

#[test]
fn index_changes_are_heard_once_each() {
    let controller = CarouselController::new();
    let heard = Rc::new(RefCell::new(Vec::new()));
    {
        let heard = Rc::clone(&heard);
        let again = controller.clone();
        controller.on_index_changed(move |_cx, index| {
            heard.borrow_mut().push(index);
            // Moving again from inside the callback holds no borrow of the controller.
            if index == 3 {
                again.jump_to_item(5).expect("no build is running");
            }
        });
    }
    let seen = Seen::default();
    let mut laid = mount(seen.carousel(10).controller(controller.clone()));
    settle(&mut laid);
    assert_eq!(
        *heard.borrow(),
        [] as [usize; 0],
        "the first item is no change"
    );

    controller.jump_to_item(3).expect("no build is running");
    settle(&mut laid);
    assert_eq!(*heard.borrow(), [3, 5]);
    assert_eq!(controller.index(), Some(5));

    let replaced = Rc::new(RefCell::new(Vec::new()));
    {
        let replaced = Rc::clone(&replaced);
        controller.on_index_changed(move |_cx, index| replaced.borrow_mut().push(index));
    }
    controller.jump_to_item(1).expect("no build is running");
    settle(&mut laid);
    assert_eq!(
        *heard.borrow(),
        [3, 5],
        "the replaced callback hears nothing more"
    );
    assert_eq!(*replaced.borrow(), [1]);
}

#[test]
fn single_advance_moves_one_item_from_where_the_drag_began() {
    let layout = CarouselLayout::uncontained(100.0).expect("a valid extent");
    let shape = geometry(&layout, 10);
    let cases = [
        // (start item, drag from, drag to, expected item)
        (0, 340.0_f64, 20.0, 1),
        (4, 20.0, 340.0, 3),
    ];
    for (start, from, to, expected) in cases {
        let controller = CarouselController::new();
        controller.jump_to_item(start).expect("no build is running");
        let seen = Seen::default();
        let mut laid = mount(
            seen.carousel(10)
                .layout(layout.clone())
                .snap(KeylineSnap::SingleAdvance)
                .controller(controller.clone()),
        );
        laid.tick();
        assert!(
            (to - from).abs() > 2.0 * shape.stride(),
            "the drag crosses more than two items"
        );
        laid.replay(&fling(from, to));
        settle(&mut laid);
        assert_eq!(controller.index(), Some(expected), "from item {start}");
    }
}

#[test]
fn autoplay_steps_and_wraps_until_paused() {
    let controller = CarouselController::new();
    let heard = Rc::new(RefCell::new(Vec::new()));
    {
        let heard = Rc::clone(&heard);
        controller.on_index_changed(move |_cx, index| heard.borrow_mut().push(index));
    }
    let seen = Seen::default();
    let mut laid = mount(
        seen.carousel(3)
            .layout(CarouselLayout::full_screen())
            .controller(controller.clone())
            .autoplay(CarouselAutoplay::every(Duration::from_secs(5)).expect("long enough")),
    );
    // A step every five seconds, each a short animation the next period starts after.
    run_for(&mut laid, Duration::from_secs(4));
    assert_eq!(
        *heard.borrow(),
        [] as [usize; 0],
        "nothing before the first period"
    );
    run_for(&mut laid, Duration::from_secs(13));
    // The wrap scrolls back over item 1, which is active on the way, as with any scroll.
    assert_eq!(
        *heard.borrow(),
        [1, 2, 1, 0],
        "the last item wraps to the first"
    );

    controller.pause();
    run_for(&mut laid, Duration::from_secs(12));
    assert_eq!(*heard.borrow(), [1, 2, 1, 0], "no step while paused");
    controller.resume();
    run_for(&mut laid, Duration::from_secs(6));
    assert_eq!(
        *heard.borrow(),
        [1, 2, 1, 0, 1],
        "a full period after resuming"
    );
}

#[test]
fn reduced_motion_stops_autoplay_and_moves_at_once() {
    let controller = CarouselController::new();
    let heard = Rc::new(RefCell::new(Vec::new()));
    {
        let heard = Rc::clone(&heard);
        controller.on_index_changed(move |_cx, index| heard.borrow_mut().push(index));
    }
    let seen = Seen::default();
    let mut laid = mount(
        seen.carousel(3)
            .layout(CarouselLayout::full_screen())
            .controller(controller.clone())
            .autoplay(CarouselAutoplay::every(Duration::from_secs(5)).expect("long enough")),
    );
    laid.set_motion_override(MotionOverride::Reduce);
    laid.tick();
    run_for(&mut laid, Duration::from_secs(12));
    assert_eq!(
        *heard.borrow(),
        [] as [usize; 0],
        "no autoplay under reduced motion"
    );

    controller.animate_to_item(2).expect("no build is running");
    laid.tick();
    assert_eq!(controller.index(), Some(2), "the move takes one frame");
}

fn labelled(count: usize) -> Carousel {
    Carousel::builder(count, |_ctx, _scope, index| {
        Semantics::new()
            .label(format!("item {index}"))
            .child(ColoredBox::new(Color::BLACK))
    })
    .semantic_label("Photos")
}

fn act(laid: &mut LaidOut, node: NodeId, action: Action) {
    laid.invoke_semantics_action(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node,
        data: None,
    })
    .expect("the request resolves");
    settle(laid);
}

#[test]
fn items_are_numbered_and_scroll_actions_stop_at_the_ends() {
    let controller = CarouselController::new();
    let mut laid = mount(labelled(5).controller(controller.clone()));
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("published semantics");
    for index in 0..3 {
        let item = tree
            .find_by_label(&format!("item {index}"))
            .expect("a shown item");
        let set = (item.position_in_set(), item.size_of_containing_set());
        assert_eq!(set, (Some(index), Some(5)), "item {index}");
    }
    let view = tree.find_by_label("Photos").expect("the scroll view");
    assert!(
        !view.supports_action(Action::ScrollLeft),
        "nothing before the first item"
    );
    assert!(view.supports_action(Action::ScrollRight));

    controller.jump_to_item(4).expect("no build is running");
    laid.tick();
    let tree = laid.a11y_tree().expect("republished semantics");
    let view = tree.find_by_label("Photos").expect("the scroll view");
    assert!(view.supports_action(Action::ScrollLeft));
    assert!(
        !view.supports_action(Action::ScrollRight),
        "nothing after the last item"
    );
}

#[test]
fn the_pause_control_is_a_labelled_button_that_toggles() {
    let controller = CarouselController::new();
    let mut laid = mount(
        labelled(5)
            .controller(controller.clone())
            .autoplay(CarouselAutoplay::every(Duration::from_secs(5)).expect("long enough")),
    );
    laid.enable_semantics();
    laid.tick();
    let tree = laid.a11y_tree().expect("published semantics");
    let button = tree
        .find_by_role_and_label(Role::Button, "Pause autoplay")
        .expect("the pause control is a button");
    let first_item = tree.find_by_label("item 0").expect("item 0");
    let order: Vec<NodeId> = tree.nodes().map(|node| node.id()).collect();
    let position = |id| order.iter().position(|node| *node == id);
    assert!(
        position(button.id()) < position(first_item.id()),
        "the control reads before the items"
    );
    let button = button.id();

    act(&mut laid, button, Action::Click);
    assert!(controller.is_paused());
    let tree = laid.a11y_tree().expect("republished semantics");
    let button = tree
        .find_by_role_and_label(Role::Button, "Resume autoplay")
        .expect("the control now resumes")
        .id();
    act(&mut laid, button, Action::Click);
    assert!(!controller.is_paused());
}

fn autoplaying(controller: &CarouselController, count: usize) -> Carousel {
    labelled(count)
        .layout(CarouselLayout::full_screen())
        .controller(controller.clone())
        .autoplay(CarouselAutoplay::every(Duration::from_secs(5)).expect("long enough"))
}

#[test]
fn input_on_the_carousel_pauses_autoplay_for_good() {
    type Touch = fn(&mut LaidOut);
    let cases: [(&str, Touch); 2] = [
        ("a press", |laid| {
            laid.dispatch_pointer_down(WIDTH / 2.0, HEIGHT / 2.0);
            laid.dispatch_pointer_up(WIDTH / 2.0, HEIGHT / 2.0);
        }),
        ("a hover", |laid| {
            laid.dispatch_pointer_hover(WIDTH / 2.0, HEIGHT / 2.0);
        }),
    ];
    for (name, touch) in cases {
        let controller = CarouselController::new();
        let mut laid = mount(autoplaying(&controller, 3));
        touch(&mut laid);
        laid.tick();
        assert!(controller.is_paused(), "{name}");
        run_for(&mut laid, Duration::from_secs(12));
        assert_eq!(controller.index(), Some(0), "{name} keeps it still");
    }
}

#[test]
fn the_screen_reader_entering_the_carousel_pauses_autoplay() {
    let controller = CarouselController::new();
    let mut laid = mount(autoplaying(&controller, 3));
    laid.enable_semantics();
    laid.tick();
    let item = laid
        .a11y_tree()
        .expect("published semantics")
        .find_by_label("item 0")
        .expect("item 0")
        .id();

    laid.move_assistive_focus(Some(item));
    laid.tick();
    assert!(controller.is_paused(), "the cursor on an item pauses it");
    laid.move_assistive_focus(None);
    run_for(&mut laid, Duration::from_secs(12));
    assert_eq!(controller.index(), Some(0), "leaving does not resume it");
}

#[test]
fn autoplay_takes_no_step_when_every_item_fits() {
    let controller = CarouselController::new();
    let mut laid = mount(
        labelled(2)
            .layout(CarouselLayout::weighted([1.0, 1.0]).expect("positive weights"))
            .controller(controller.clone())
            .autoplay(CarouselAutoplay::every(Duration::from_secs(5)).expect("long enough")),
    );
    run_for(&mut laid, Duration::from_secs(12));
    assert_eq!(controller.index(), Some(0));
}

#[test]
fn right_to_left_puts_the_first_item_at_the_right() {
    let layout = CarouselLayout::default();
    let shape = geometry(&layout, 10);
    let seen = Seen::default();
    let laid = mount(flui_widgets::Directionality::new(
        flui_painting::typography::TextDirection::Rtl,
        seen.carousel(10),
    ));
    let first = shape.place(0, 0.0);
    let x = WIDTH - first.center;
    laid.dispatch_pointer_down(x, HEIGHT / 2.0);
    laid.dispatch_pointer_up(x, HEIGHT / 2.0);
    assert_eq!(*seen.taps.borrow(), [0]);
}

#[test]
fn every_layout_rests_in_order_and_masks_within_the_item() {
    let layouts = [
        ("browse", CarouselLayout::default()),
        (
            "browse with a large extent",
            CarouselLayout::default()
                .with_large_extent(186.0)
                .expect("a valid extent"),
        ),
        (
            "uncontained",
            CarouselLayout::uncontained(150.0).expect("a valid extent"),
        ),
        (
            "hero at the start",
            CarouselLayout::hero(Alignment::new(-1.0, 0.0)).expect("in range"),
        ),
        (
            "centred hero",
            CarouselLayout::hero(Alignment::new(0.0, 0.0)).expect("in range"),
        ),
        (
            "weighted",
            CarouselLayout::weighted([1.0, 7.0, 1.0]).expect("positive weights"),
        ),
        ("full screen", CarouselLayout::full_screen()),
    ];
    for (name, layout) in layouts {
        for count in [1, 2, 3, 10] {
            let shape = geometry(&layout, count);
            let max = shape.max_offset();
            let rests: Vec<f64> = (0..count).map(|index| shape.rest_offset(index)).collect();
            assert_eq!(
                rests[0], 0.0,
                "{name} × {count}: the first item rests at the start"
            );
            assert!(
                rests.windows(2).all(|pair| pair[0] <= pair[1]),
                "{name} × {count}: rests in order {rests:?}"
            );
            assert!(
                rests.iter().all(|rest| (0.0..=max).contains(rest)),
                "{name} × {count}"
            );
            for (index, rest) in rests.iter().enumerate() {
                let nearest = shape.nearest_item(*rest).expect("items");
                assert_eq!(rests[nearest], *rest, "{name} × {count}: item {index}");
            }
            for step in 0..=20 {
                let offset = max * f64::from(step) / 20.0;
                for index in 0..count {
                    let placed = shape.place(index, offset);
                    assert!(
                        placed.mask_extent.is_finite()
                            && (0.0..=shape.item_extent()).contains(&placed.mask_extent),
                        "{name} × {count}: item {index} at {offset}: {placed:?}"
                    );
                    assert!((0.0..=1.0).contains(&placed.progress), "{name}: {placed:?}");
                }
            }
        }
    }
}
