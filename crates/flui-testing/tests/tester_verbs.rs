//! The scenario `Tester`'s verbs, matchers and reads through its public API (ADR-0400 §2): each
//! gesture reaches the element it names, a matcher lowers to the condition its name says, a
//! strict `Find<One>` holds only of one element, and the reads answer from the published tree.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::Duration;

use flui_foundation::panic::payload_text;
use flui_interaction::pointer::HitTestBehavior;
use flui_platform::TargetPlatform;
use flui_protocol::input::Direction;
use flui_protocol::locator::StateFilter;
use flui_protocol::tree::Node;
use flui_protocol::wait::UnsettledReason;
use flui_protocol::wire::Role;
use flui_semantics::{SemanticsRole, Toggled};
use flui_testing::scenario::matcher::{self, Matcher};
use flui_testing::scenario::{Find, TestError, TestWindow, Tester, find};
use flui_view::prelude::*;
use flui_widgets::{
    ActivityIndicator, Column, EditableText, GestureDetector, ListView, RawButton, Semantics,
    SizedBox, Text, TextEditingController,
};

/// A pad named "Pad" that counts its taps and its long presses.
fn pad() -> (Tester, Rc<Cell<u32>>, Rc<Cell<u32>>) {
    let taps = Rc::new(Cell::new(0));
    let long_presses = Rc::new(Cell::new(0));
    let (tapped, held) = (Rc::clone(&taps), Rc::clone(&long_presses));
    let t = Tester::mount(
        Semantics::new().container(true).label("Pad").child(
            GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .on_tap(move |_| tapped.set(tapped.get() + 1))
                .on_long_press(move |_| held.set(held.get() + 1))
                .child(SizedBox::new(80.0, 40.0)),
        ),
    );
    (t, taps, long_presses)
}

/// A list "Items" of 100 rows 20 px tall, "Item 0" to "Item 99", in a viewport 200 px tall, on
/// a touch platform, whose primary pointer a drag scrolls with.
fn items() -> Tester {
    let rows = ListView::builder(100, 20.0, |index| {
        Some(
            SizedBox::new(200.0, 20.0)
                .child(Text::new(format!("Item {index}")))
                .into_view()
                .boxed(),
        )
    })
    .semantics_label("Items");
    Tester::mount_in(
        Column::new((SizedBox::new(200.0, 200.0).child(rows),)),
        TestWindow::new(200, 300).platform(TargetPlatform::Android),
    )
}

/// Whether the row `name` is built and part of it lies inside the list's viewport.
fn in_view(t: &mut Tester, name: &str) -> bool {
    if t.find_all(&find::text(name).all()).is_empty() {
        return false;
    }
    let list = t.rect(&find::text("Items"));
    let row = t.rect(&find::text(name));
    row.y + row.height > list.y && row.y < list.y + list.height
}

#[test]
fn a_tap_and_a_long_press_reach_the_element_they_name() {
    let (mut t, taps, long_presses) = pad();
    t.tap(&find::text("Pad"));
    assert_eq!((taps.get(), long_presses.get()), (1, 0), "a tap");
    t.long_press(&find::text("Pad"));
    assert_eq!(
        (taps.get(), long_presses.get()),
        (1, 1),
        "a long press, and no tap"
    );
    t.finish();
}

/// A drag presses on the element its first find names and drops on the second's: from "Item 4"
/// onto "Item 1" the list scrolls up by their distance, less at most the slop the recognizer
/// takes. The other way round, at the top of the list, nothing scrolls.
#[test]
fn a_drag_moves_what_it_picks_up_onto_where_it_drops_it() {
    let mut t = items();
    let before = t.rect(&find::text("Item 4"));
    let onto = t.rect(&find::text("Item 1"));
    t.drag(&find::text("Item 4"), &find::text("Item 1"));
    t.settle();
    let after = t.rect(&find::text("Item 4"));
    let scrolled = before.y - after.y;
    let distance = before.y - onto.y;
    assert!(
        scrolled > distance / 2.0 && scrolled <= distance,
        "the list scrolled up by about the drag's distance: {before:?} -> {after:?}, \
         Item 1 at {onto:?}"
    );
    t.finish();

    let mut t = items();
    t.drag(&find::text("Item 1"), &find::text("Item 4"));
    t.settle();
    assert_eq!(
        t.rect(&find::text("Item 4")),
        before,
        "a drag down at the top of the list scrolls nothing"
    );
    t.finish();
}

/// A fling ends fast, so the list runs on past its 80 px stroke: "Item 9", which the stroke
/// alone would leave in view, leaves the viewport.
#[test]
fn a_fling_runs_the_list_on_past_its_stroke() {
    let mut t = items();
    assert!(in_view(&mut t, "Item 9"));
    t.fling(&find::text("Items"), Direction::Up);
    t.settle();
    assert!(!in_view(&mut t, "Item 9"), "Item 9 left the viewport");
    t.finish();
}

/// `scroll_into_view` scrolls a row built past the viewport into it, and `scroll_until_visible`
/// scrolls a scrollable a viewport at a time until a row not built yet shows.
#[test]
fn a_scroll_brings_the_element_it_names_into_view() {
    let mut t = items();
    assert!(
        !t.find_all(&find::text("Item 14").all()).is_empty() && !in_view(&mut t, "Item 14"),
        "Item 14 is built past the viewport"
    );
    t.scroll_into_view(&find::text("Item 14"));
    t.settle();
    assert!(in_view(&mut t, "Item 14"));

    assert!(
        t.find_all(&find::text("Item 60").all()).is_empty(),
        "Item 60 is not built"
    );
    t.scroll_until_visible(&find::text("Items"), &find::text("Item 60"));
    t.settle();
    assert!(in_view(&mut t, "Item 60"));
    t.finish();
}

#[test]
fn typed_text_is_committed_into_the_field_it_names() {
    let field = EditableText::new(
        TextEditingController::new(),
        flui_interaction::focus::FocusNode::with_debug_label("title"),
    )
    .semantic_label("Title");
    let mut t = Tester::mount(SizedBox::new(300.0, 40.0).child(field));
    let title = find::role(Role::TextInput);
    t.type_text(&title, "Notes");
    t.expect(&title, matcher::value("Notes"));
    t.expect(&title, matcher::focused());
    t.finish();
}

/// "Once"; two "Twice"; a button "Go" and a disabled one "Off"; a checked box "Agree"; and
/// "Volume", with a value and a description.
fn catalog() -> Tester {
    Tester::mount(Column::new((
        Text::new("Once"),
        Text::new("Twice"),
        Text::new("Twice"),
        RawButton::new(Text::new("Go")).on_pressed(|_| {}),
        RawButton::new(Text::new("Off")),
        Semantics::new()
            .container(true)
            .label("Agree")
            .checkbox(Toggled::On)
            .child(SizedBox::new(20.0, 20.0)),
        Semantics::new()
            .container(true)
            .label("Volume")
            .role(SemanticsRole::SpinButton)
            .value("7 of 10")
            .hint("Turn it up")
            .child(SizedBox::new(20.0, 20.0)),
    )))
}

/// What a row of the matcher table expects of: one element, or every match.
enum Subject {
    One(Find),
    Many(Find<flui_testing::scenario::find::Many>),
}

/// Each matcher on the catalog, holding or not as its name says. A strict `Find<One>` holds
/// only of one element: "Twice" matches two, so neither `exists` nor `name` holds of it until a
/// pick or `all` says which.
#[test]
fn each_matcher_holds_exactly_where_its_name_says() {
    use Subject::{Many, One};

    let mut disabled = StateFilter::default();
    disabled.disabled = Some(true);
    let rows: Vec<(&str, Subject, Matcher, bool)> = vec![
        (
            "one exists",
            One(find::text("Once")),
            matcher::exists(),
            true,
        ),
        (
            "two are not one",
            One(find::text("Twice")),
            matcher::exists(),
            false,
        ),
        (
            "two exist",
            Many(find::text("Twice").all()),
            matcher::exists(),
            true,
        ),
        (
            "the first of two",
            One(find::text("Twice").all().first()),
            matcher::exists(),
            true,
        ),
        (
            "a count",
            Many(find::text("Twice").all()),
            matcher::count(2),
            true,
        ),
        (
            "a wrong count",
            Many(find::text("Twice").all()),
            matcher::count(3),
            false,
        ),
        ("gone", One(find::text("Never")), matcher::gone(), true),
        ("not gone", One(find::text("Once")), matcher::gone(), false),
        (
            "a name of one",
            One(find::text("Once")),
            matcher::name("Once"),
            true,
        ),
        (
            "a name of two is not of one",
            One(find::text("Twice")),
            matcher::name("Twice"),
            false,
        ),
        (
            "a name of all",
            Many(find::text("Twice").all()),
            matcher::name("Twice"),
            true,
        ),
        ("enabled", One(find::button("Go")), matcher::enabled(), true),
        (
            "disabled",
            One(find::button("Off")),
            matcher::disabled(),
            true,
        ),
        (
            "not disabled",
            One(find::button("Go")),
            matcher::disabled(),
            false,
        ),
        (
            "a state",
            One(find::button("Off")),
            matcher::state(disabled),
            true,
        ),
        (
            "checked",
            One(find::text("Agree")),
            matcher::checked(),
            true,
        ),
        (
            "not checked",
            One(find::text("Once")),
            matcher::checked(),
            false,
        ),
        (
            "a value",
            One(find::text("Volume")),
            matcher::value("7 of 10"),
            true,
        ),
        (
            "a wrong value",
            One(find::text("Volume")),
            matcher::value("7"),
            false,
        ),
        (
            "a value containing",
            One(find::text("Volume")),
            matcher::value_contains("of"),
            true,
        ),
        (
            "a description",
            One(find::text("Volume")),
            matcher::description("Turn it up"),
            true,
        ),
        ("visible", One(find::text("Once")), matcher::visible(), true),
        (
            "not focused",
            One(find::text("Once")),
            matcher::focused(),
            false,
        ),
        (
            "all",
            One(find::button("Go")),
            matcher::all([matcher::exists(), matcher::enabled()]),
            true,
        ),
        (
            "not all",
            One(find::button("Go")),
            matcher::all([matcher::enabled(), matcher::disabled()]),
            false,
        ),
        (
            "any",
            One(find::button("Go")),
            matcher::any([matcher::disabled(), matcher::enabled()]),
            true,
        ),
        (
            "not any",
            One(find::button("Go")),
            matcher::any([matcher::disabled(), matcher::checked()]),
            false,
        ),
    ];
    let mut t = catalog();
    let wrong: Vec<&str> = rows
        .into_iter()
        .filter_map(|(row, subject, matcher, holds)| {
            let held = match &subject {
                One(find) => t.try_expect(find, matcher),
                Many(find) => t.try_expect(find, matcher),
            }
            .is_ok();
            (held != holds).then_some(row)
        })
        .collect();
    assert_eq!(
        wrong,
        Vec::<&str>::new(),
        "rows whose matcher held otherwise"
    );
    t.finish();
}

/// Whether `node` or a node under it is named `name`.
fn names(node: &Node, name: &str) -> bool {
    node.name.as_deref() == Some(name) || node.children.iter().any(|child| names(child, name))
}

/// The reads answer from the published tree: every match of a `Find<Many>`, the one match's
/// rectangle, the whole tree, and the settled frame.
#[test]
fn the_reads_answer_from_the_published_tree() {
    let mut t = catalog();
    assert_eq!(t.find_all(&find::text("Twice").all()).len(), 2);
    let once = t.rect(&find::text("Once"));
    assert!(
        once.width > 0.0 && once.height > 0.0 && once.x >= 0.0 && once.y >= 0.0,
        "{once:?}"
    );
    let tree = t.snapshot();
    assert!(
        tree.roots.iter().any(|root| names(root, "Volume")),
        "{}",
        flui_protocol::tree::outline(&tree.roots)
    );
    assert!(
        matches!(
            t.try_read(&find::text("Twice")),
            Err(TestError::Ambiguous { matched: 2, .. })
        ),
        "a strict read of two"
    );
    let settled = t.wait_settled();
    assert_eq!(
        settled.frame,
        flui_protocol::client::Connection::now(t.connection()).frame
    );
    t.finish();
}

/// `settle` ends on a still frame and fails, naming the ticker, on a repeating animation;
/// `freeze_at` holds that animation's frame at an instant and names what still runs.
#[test]
fn settle_names_a_repeating_animation_and_freeze_at_holds_its_frame() {
    let mut t = catalog();
    let still = t.settle().frame();
    assert_eq!(
        still,
        flui_protocol::client::Connection::now(t.connection()).frame
    );
    t.finish();

    let mut t = Tester::mount(ActivityIndicator::new().label("Loading"));
    let Err(TestError::NotSettled { diagnosis, .. }) =
        t.try_settle().map(|settled| settled.frame())
    else {
        panic!("a repeating animation never settles");
    };
    assert!(
        diagnosis
            .unsettled
            .iter()
            .any(|cause| cause.reason == UnsettledReason::Ticker),
        "{diagnosis}"
    );
    drop(diagnosis);
    let before = flui_protocol::client::Connection::now(t.connection()).frame;
    let frozen = t.freeze_at(Duration::from_millis(500));
    assert!(frozen.frame() > before, "the clock ran to the instant");
    assert!(
        frozen
            .running()
            .iter()
            .any(|cause| cause.reason == UnsettledReason::Ticker),
        "the ticker still runs at the frozen frame"
    );
    t.finish_unchecked("a repeating animation never settles");
}

/// Soft expectations inside `checks` are kept, not raised, so the body runs on; their list is
/// raised once when it returns.
#[test]
fn checks_raise_every_soft_failure_once_the_body_returns() {
    let mut t = catalog();
    let reached = Cell::new(false);
    let payload = catch_unwind(AssertUnwindSafe(|| {
        t.checks(|c| {
            c.expect(&find::text("Missing"), matcher::exists());
            c.expect(&find::text("Once"), matcher::exists());
            c.expect(&find::text("Absent"), matcher::exists());
            reached.set(true);
        });
    }))
    .expect_err("the soft failures are raised");
    let message = payload_text(&*payload).unwrap_or_default().to_owned();
    assert!(reached.get(), "a soft failure does not stop the body");
    assert!(
        message.contains("2 soft failure(s)")
            && message.contains("Missing")
            && message.contains("Absent"),
        "{message}"
    );
}
