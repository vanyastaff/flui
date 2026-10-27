//! `SwipeActions`: rows of the `nested_scroll_channel` table in
//! `contracts.rs`. A 300 × 60 row at the top of a 300 × 300 window, Pin at
//! its leading edge and Delete then Archive at its trailing one; each action
//! reached by the swipe is the same command the row's alternatives perform
//! (its matrix rows sit in `gesture_alternatives`).

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_painting::styling::Color;
use flui_painting::typography::TextDirection;
use flui_view::{BoxedView, IntoView, ViewExt};
use flui_widgets::{
    Actions, CallbackAction, ColoredBox, Column, Directionality, GestureCommand, Intent,
    ScrollController, Scrollable, SizedBox, SwipeAction, SwipeActions, SwipeActionsError,
    SwipeAlternative, SwipeEdge, Text, VsyncScope,
};

use crate::anchored_drag::painted_at;
use crate::common::LaidOut;
use crate::nested_scroll::{DEVICES, Device, at, fling, mounted, press, settle, slow};

/// The row's content.
const CONTENT: Color = Color::rgb(20, 120, 200);
/// A second row's content, under the first in a list.
const SECOND: Color = Color::rgb(200, 120, 20);

#[derive(Clone, Copy, Debug)]
struct Pin;
impl Intent for Pin {}

#[derive(Clone, Copy, Debug)]
struct Delete;
impl Intent for Delete {}

#[derive(Clone, Copy, Debug)]
struct Archive;
impl Intent for Archive {}

/// What the actions did, in order.
type Log = Rc<RefCell<Vec<&'static str>>>;

/// How a scene's row is configured.
#[derive(Clone, Copy, Default)]
struct Options {
    full_swipe: bool,
    rtl: bool,
    /// Delete's action panics.
    delete_panics: bool,
}

/// One row whose content is `color`, its actions logging into `log`.
fn row(color: Color, options: Options, log: &Log) -> BoxedView {
    let action = |label: &'static str| Text::new(label);
    let row = SwipeActions::new(
        ColoredBox::new(color).child(SizedBox::new(300.0, 60.0)),
        [SwipeAction::new(
            GestureCommand::new("Pin Alice", Pin),
            action("Pin"),
        )],
        [
            SwipeAction::new(
                GestureCommand::new("Delete Alice", Delete),
                action("Delete"),
            ),
            SwipeAction::new(
                GestureCommand::new("Archive Alice", Archive),
                action("Archive"),
            ),
        ],
        SwipeAlternative::PresentedMenu(Rc::new(|_cx, _request| {})),
    )
    .expect("three labelled actions")
    .target("Alice");
    let row = if options.full_swipe {
        row.full_swipe(SwipeEdge::Trailing)
    } else {
        row
    };
    let (pin, delete, archive) = (Rc::clone(log), Rc::clone(log), Rc::clone(log));
    let panics = options.delete_panics;
    Actions::new(row)
        .action(CallbackAction::new(move |_cx, _: &Pin| {
            pin.borrow_mut().push("Pin");
        }))
        .action(CallbackAction::new(move |_cx, _: &Delete| {
            assert!(!panics, "intentional Delete action panic");
            delete.borrow_mut().push("Delete");
        }))
        .action(CallbackAction::new(move |_cx, _: &Archive| {
            archive.borrow_mut().push("Archive");
        }))
        .boxed()
}

/// The row at the top of the window, in `options`' reading direction.
fn scene(options: Options, log: &Log) -> BoxedView {
    let direction = if options.rtl {
        TextDirection::Rtl
    } else {
        TextDirection::Ltr
    };
    Directionality::new(direction, Column::new(vec![row(CONTENT, options, log)])).boxed()
}

fn mount(view: impl IntoView, vsync: &Vsync) -> LaidOut {
    let mut laid = mounted(VsyncScope::new(vsync.clone(), view).boxed(), vsync.clone());
    laid.tick();
    laid
}

/// Where the row's content is painted: its left edge.
fn content(laid: &LaidOut) -> f64 {
    painted_at(laid, CONTENT).0
}

fn near(value: f64, target: f64) -> bool {
    (value - target).abs() < 0.5
}

/// A swipe toward the reading start reveals the trailing actions, one toward
/// the reading end the leading one: a release past halfway or a fling leaves
/// them shown, a short release or a fling back closes the row, and tapping a
/// revealed action closes the row and performs its command.
pub(crate) fn swipe_actions_reveal_settle_and_perform_a_tapped_action() {
    for device in DEVICES {
        let (vsync, log) = (Vsync::new(), Log::default());
        let mut laid = mount(scene(Options::default(), &log), &vsync);
        let play = |laid: &mut LaidOut, script| {
            laid.replay(&script);
            settle(laid);
            content(laid)
        };
        assert!(
            near(
                play(&mut laid, slow(device, at(250.0, 30.0), at(220.0, 30.0))),
                0.0
            ),
            "{device:?}: short of halfway the row closes again"
        );
        let opened = play(&mut laid, slow(device, at(250.0, 30.0), at(120.0, 30.0)));
        assert!(
            near(opened, -160.0),
            "{device:?}: past halfway Delete and Archive stay shown, at {opened}"
        );
        // Delete is the trailing edge's first action: at the row's edge.
        assert!(near(play(&mut laid, press(device, at(260.0, 30.0))), 0.0));
        assert_eq!(*log.borrow(), ["Delete"], "{device:?}: the tap deleted");
        assert!(
            near(
                play(&mut laid, fling(device, at(250.0, 30.0), at(220.0, 30.0))),
                -160.0
            ),
            "{device:?}: a fling opens the row"
        );
        assert!(
            near(
                play(&mut laid, fling(device, at(100.0, 30.0), at(130.0, 30.0))),
                0.0
            ),
            "{device:?}: a fling back closes it"
        );
        assert!(
            near(
                play(&mut laid, slow(device, at(50.0, 30.0), at(150.0, 30.0))),
                80.0
            ),
            "{device:?}: a swipe toward the reading end shows Pin"
        );
        assert!(near(play(&mut laid, press(device, at(40.0, 30.0))), 0.0));
        assert_eq!(
            *log.borrow(),
            ["Delete", "Pin"],
            "{device:?}: the tap pinned"
        );
    }
}

/// With a full swipe, a release far past the trailing actions performs the
/// edge's first action once and closes the row; without one, the same
/// release leaves the actions shown.
pub(crate) fn swipe_actions_full_swipe_performs_the_first_action_on_release() {
    for device in DEVICES {
        for full_swipe in [true, false] {
            let (vsync, log) = (Vsync::new(), Log::default());
            let options = Options {
                full_swipe,
                ..Options::default()
            };
            let mut laid = mount(scene(options, &log), &vsync);
            laid.replay(&slow(device, at(270.0, 30.0), at(5.0, 30.0)));
            settle(&mut laid);
            let (expected_log, expected_at): (&[&str], f64) = if full_swipe {
                (&["Delete"], 0.0)
            } else {
                (&[], -160.0)
            };
            assert_eq!(
                *log.borrow(),
                expected_log,
                "{device:?}, full swipe {full_swipe}: what the release performed"
            );
            let at_rest = content(&laid);
            assert!(
                near(at_rest, expected_at),
                "{device:?}, full swipe {full_swipe}: the row rests at {at_rest}"
            );
        }
    }
}

/// Under right-to-left text the trailing actions are at the left: a swipe to
/// the right reveals them, Delete outermost at the row's left edge.
pub(crate) fn swipe_actions_mirror_right_to_left() {
    for device in DEVICES {
        let (vsync, log) = (Vsync::new(), Log::default());
        let options = Options {
            rtl: true,
            ..Options::default()
        };
        let mut laid = mount(scene(options, &log), &vsync);
        laid.replay(&slow(device, at(50.0, 30.0), at(180.0, 30.0)));
        settle(&mut laid);
        assert!(
            near(content(&laid), 160.0),
            "{device:?}: the content moved right, at {}",
            content(&laid)
        );
        laid.replay(&press(device, at(40.0, 30.0)));
        settle(&mut laid);
        assert_eq!(
            *log.borrow(),
            ["Delete"],
            "{device:?}: Delete is at the left"
        );
        assert!(near(content(&laid), 0.0), "{device:?}: and the row closed");
    }
}

/// Swipe rows in a vertical list share each contact by axis: a horizontal
/// swipe opens the row under it and leaves the list, a vertical drag on a row
/// scrolls the list and leaves the rows closed.
pub(crate) fn swipe_rows_in_a_vertical_list_share_contacts_by_axis() {
    for device in DEVICES {
        let (vsync, log, list) = (Vsync::new(), Log::default(), ScrollController::new());
        let rows = Column::new(vec![
            row(CONTENT, Options::default(), &log),
            row(SECOND, Options::default(), &log),
            SizedBox::new(300.0, 1000.0).boxed(),
        ]);
        let mut laid = mount(
            Scrollable::new().controller(list.clone()).child(rows),
            &vsync,
        );
        laid.replay(&slow(device, at(250.0, 90.0), at(120.0, 90.0)));
        settle(&mut laid);
        assert!(
            near(painted_at(&laid, SECOND).0, -160.0) && near(content(&laid), 0.0),
            "{device:?}: the horizontal swipe opened the second row only"
        );
        assert_eq!(list.pixels(), 0.0, "{device:?}: the list did not move");
        laid.replay(&slow(device, at(150.0, 55.0), at(150.0, 0.0)));
        settle(&mut laid);
        assert!(
            list.pixels() > 20.0,
            "{device:?}: a vertical drag on a row scrolls the list, at {}",
            list.pixels()
        );
        assert!(
            near(content(&laid), 0.0),
            "{device:?}: and the row it went down on stays closed"
        );
        assert!(log.borrow().is_empty());
    }
}

/// A touch that goes down in the window's system-gesture strip at either
/// side is the operating system's edge swipe (going back): the row does not
/// take it, and asks the OS to exclude nothing. A mouse there, or a touch
/// just inside the strip, swipes the row.
pub(crate) fn swipe_actions_leave_the_system_edges_to_the_os() {
    use flui_platform::input_regions::InputRegionKind;
    let rows: [(&str, Device, f64, f64, f64); 5] = [
        ("a touch at the left edge", Device::Touch, 8.0, 150.0, 0.0),
        (
            "a touch at the right edge",
            Device::Touch,
            292.0,
            150.0,
            0.0,
        ),
        ("a mouse at the left edge", Device::Mouse, 8.0, 150.0, 80.0),
        (
            "a touch inside the left strip",
            Device::Touch,
            30.0,
            150.0,
            80.0,
        ),
        (
            "a touch inside the right strip",
            Device::Touch,
            270.0,
            120.0,
            -160.0,
        ),
    ];
    for (name, device, from, to, rest) in rows {
        let (vsync, log) = (Vsync::new(), Log::default());
        let mut laid = mount(scene(Options::default(), &log), &vsync);
        laid.replay(&slow(device, at(from, 30.0), at(to, 30.0)));
        settle(&mut laid);
        assert!(
            near(content(&laid), rest),
            "{name}: the row rests at {}, not {rest}",
            content(&laid)
        );
        let excluded = laid.window().input_region_updates().iter().any(|update| {
            update
                .iter()
                .any(|(kind, _)| *kind == InputRegionKind::SystemGestureExclusion)
        });
        assert!(!excluded, "{name}: the row excludes no system gesture");
    }
}

/// Unmounting a row in the middle of a full swipe performs nothing; the row
/// mounted again rests closed and swipes on.
pub(crate) fn swipe_actions_unmounted_mid_swipe_perform_nothing() {
    let (vsync, log) = (Vsync::new(), Log::default());
    let options = Options {
        full_swipe: true,
        ..Options::default()
    };
    let mut laid = mount(scene(options, &log), &vsync);
    laid.dispatch_pointer_down(290.0, 30.0);
    for step in 1..=10 {
        laid.dispatch_pointer_move_after(
            290.0 - f64::from(step) * 25.0,
            30.0,
            Duration::from_millis(16),
        );
    }
    laid.pump();
    assert!(
        content(&laid) < -200.0,
        "the swipe is past its actions: {}",
        content(&laid)
    );
    laid.pump_widget(VsyncScope::new(vsync.clone(), SizedBox::new(300.0, 60.0)));
    laid.dispatch_pointer_up(40.0, 30.0);
    settle(&mut laid);
    assert!(
        log.borrow().is_empty(),
        "the unmounted row performed nothing"
    );
    laid.pump_widget(VsyncScope::new(vsync, scene(options, &log)));
    settle(&mut laid);
    assert!(near(content(&laid), 0.0), "the row mounts closed");
    laid.replay(&slow(Device::Touch, at(270.0, 30.0), at(5.0, 30.0)));
    settle(&mut laid);
    assert_eq!(*log.borrow(), ["Delete"], "and its full swipe works");
}

/// An action that panics leaves the row closed: the close is committed
/// before the command runs, whether a tap or a full swipe performed it, and
/// the next swipe works.
pub(crate) fn a_panicking_swipe_action_leaves_the_row_closed() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let (vsync, log) = (Vsync::new(), Log::default());
    let options = Options {
        full_swipe: true,
        delete_panics: true,
        ..Options::default()
    };
    let mut laid = mount(scene(options, &log), &vsync);
    laid.replay(&slow(Device::Touch, at(250.0, 30.0), at(120.0, 30.0)));
    settle(&mut laid);
    assert!(near(content(&laid), -160.0), "open");
    let tapped = catch_unwind(AssertUnwindSafe(|| {
        laid.replay(&press(Device::Touch, at(260.0, 30.0)));
    }));
    let payload = tapped.expect_err("the tapped action's panic reaches the caller");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some("intentional Delete action panic")
    );
    settle(&mut laid);
    assert!(near(content(&laid), 0.0), "the tap closed the row first");

    let swiped = catch_unwind(AssertUnwindSafe(|| {
        laid.replay(&slow(Device::Touch, at(270.0, 30.0), at(5.0, 30.0)));
        settle(&mut laid);
    }));
    let payload = swiped.expect_err("the full swipe's panic reaches the frame");
    assert_eq!(
        payload.downcast_ref::<&str>().copied(),
        Some("intentional Delete action panic")
    );
    settle(&mut laid);
    assert!(
        near(content(&laid), 0.0),
        "the full swipe closed the row first"
    );

    laid.replay(&slow(Device::Touch, at(250.0, 30.0), at(120.0, 30.0)));
    settle(&mut laid);
    laid.replay(&press(Device::Touch, at(180.0, 30.0)));
    settle(&mut laid);
    assert_eq!(*log.borrow(), ["Archive"], "the row works on");
}

/// A cancelled contact undoes its swipe: past the full swipe it performs
/// nothing and the row returns closed; from an open row it returns open.
pub(crate) fn a_cancelled_swipe_returns_where_it_began() {
    let cancelled = |from: f64, to: f64| {
        let mut script = flui_testing::TouchScript::new("cancelled").down(1, at(from, 30.0));
        for point in crate::nested_scroll::line(at(from, 30.0), at(to, 30.0), 8) {
            script = script.wait(Duration::from_millis(16)).move_to(1, point);
        }
        script.cancel(1).build().expect("one whole contact")
    };
    let (vsync, log) = (Vsync::new(), Log::default());
    let options = Options {
        full_swipe: true,
        ..Options::default()
    };
    let mut laid = mount(scene(options, &log), &vsync);
    laid.replay(&cancelled(270.0, 5.0));
    settle(&mut laid);
    assert!(
        log.borrow().is_empty(),
        "a cancelled full swipe performs nothing"
    );
    assert!(near(content(&laid), 0.0), "and returns the row closed");
    laid.replay(&slow(Device::Touch, at(250.0, 30.0), at(120.0, 30.0)));
    settle(&mut laid);
    laid.replay(&cancelled(100.0, 20.0));
    settle(&mut laid);
    assert!(
        near(content(&laid), -160.0),
        "a cancelled drag of an open row leaves it open, at {}",
        content(&laid)
    );
}

/// A row needs an action, and its commands must be told apart.
pub(crate) fn swipe_actions_refuse_rows_without_distinct_actions() {
    let menu = || SwipeAlternative::PresentedMenu(Rc::new(|_cx, _request| {}));
    let none = SwipeActions::new(SizedBox::shrink(), [], [], menu());
    assert_eq!(none.err(), Some(SwipeActionsError::NoActions));
    let twice = SwipeActions::new(
        SizedBox::shrink(),
        [SwipeAction::new(
            GestureCommand::new("Delete", Delete),
            SizedBox::shrink(),
        )],
        [SwipeAction::new(
            GestureCommand::new("Delete", Archive),
            SizedBox::shrink(),
        )],
        menu(),
    );
    assert!(matches!(twice, Err(SwipeActionsError::Alternatives(_))));
}
