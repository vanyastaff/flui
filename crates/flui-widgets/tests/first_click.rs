//! `FirstClick`: a press marked as the click that brought its window forward reaches the
//! widgets its hit path runs through inside a `FirstClick` box, and nothing else: not a
//! widget beside the box, nor one painted over it.

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::geometry::Point;
use flui_painting::styling::Color;
use flui_platform::EventTime;
use flui_platform::input::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerKind, PointerPosition,
    PointerPress, PointerRelease, PointerSample, PointerSource,
};
use flui_view::{IntoView, View};
use flui_widgets::HitTestBehavior;
use flui_widgets::{ColoredBox, FirstClick, GestureDetector, Positioned, SizedBox, Stack};

use crate::common::{LaidOut, lay_out, tight};

/// A 100-px square that counts its taps.
fn counted(taps: &Rc<Cell<u32>>) -> impl IntoView {
    let taps = Rc::clone(taps);
    GestureDetector::new()
        .on_tap(move |_| taps.set(taps.get() + 1))
        .child(SizedBox::new(100.0, 100.0).child(ColoredBox::new(Color::rgb(240, 240, 240))))
}

/// A 100-px square that paints nothing and counts its taps through a translucent detector:
/// a hit on it lets the press through to whatever lies below.
fn translucent(taps: &Rc<Cell<u32>>) -> impl IntoView {
    let taps = Rc::clone(taps);
    GestureDetector::new()
        .behavior(HitTestBehavior::Translucent)
        .on_tap(move |_| taps.set(taps.get() + 1))
        .child(SizedBox::new(100.0, 100.0))
}

/// The two squares side by side: `accepting` on the left, inside a first-click region
/// while `accepts`, and `plain` on the right.
fn squares(accepting: &Rc<Cell<u32>>, plain: &Rc<Cell<u32>>, accepts: bool) -> impl View {
    Stack::new((
        Positioned::new(
            FirstClick::new()
                .accepting(accepts)
                .child(counted(accepting)),
        )
        .left(0.0)
        .top(0.0),
        Positioned::new(counted(plain)).left(100.0).top(0.0),
    ))
}

/// A mouse click at `x`, on contact `contact`, marked as the first click when `first`.
fn click(laid: &LaidOut, contact: u64, x: f64, first: bool) {
    let pointer = PointerSource::new(
        PointerId::try_from(contact).expect("contact ids start at one"),
        PointerKind::Mouse,
    );
    let sample = |ms: u64| {
        PointerSample::new(
            EventTime::from_nanos(ms * 1_000_000),
            PointerPosition::try_new(Point::new(x, 50.0)).expect("a finite position"),
        )
    };
    let press = PointerPress::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(contact * 100),
    );
    let press = if first {
        press.with_first_click()
    } else {
        press
    };
    laid.dispatch_pointer_event(&PointerEvent::Down(press));
    laid.dispatch_pointer_event(&PointerEvent::Up(PointerRelease::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(contact * 100 + 10),
    )));
}

pub(crate) fn a_first_click_reaches_only_the_widgets_that_accept_it() {
    let accepting = Rc::new(Cell::new(0));
    let plain = Rc::new(Cell::new(0));
    let mut laid = lay_out(squares(&accepting, &plain, true), tight(200.0, 100.0));

    click(&laid, 1, 50.0, true);
    assert_eq!(accepting.get(), 1, "the first click acts inside the region");

    click(&laid, 2, 150.0, true);
    assert_eq!(
        plain.get(),
        0,
        "outside every first-click region the first click only activated the window"
    );

    click(&laid, 3, 150.0, false);
    assert_eq!(
        plain.get(),
        1,
        "a press into an active window acts anywhere"
    );

    // The region withdrawn, the same first click only activates there too.
    laid.pump_widget(squares(&accepting, &plain, false));
    click(&laid, 4, 50.0, true);
    assert_eq!(
        accepting.get(),
        1,
        "a region no longer accepting refuses it"
    );
    click(&laid, 5, 50.0, false);
    assert_eq!(accepting.get(), 2);
}

/// A widget painted over a `FirstClick` box takes the hit, so the first click there reaches
/// neither it nor the box under it; an ordinary press reaches the widget on top.
pub(crate) fn a_widget_painted_over_a_first_click_box_does_not_take_the_first_click() {
    let under = Rc::new(Cell::new(0));
    let over = Rc::new(Cell::new(0));
    let laid = lay_out(
        Stack::new((
            Positioned::new(FirstClick::new().child(counted(&under)))
                .left(0.0)
                .top(0.0),
            Positioned::new(counted(&over)).left(0.0).top(0.0),
        )),
        tight(200.0, 100.0),
    );

    click(&laid, 1, 50.0, true);
    assert_eq!(
        (under.get(), over.get()),
        (0, 0),
        "the box's region holds the point, but the covering widget took the hit"
    );

    click(&laid, 2, 50.0, false);
    assert_eq!(
        (under.get(), over.get()),
        (0, 1),
        "a press into an active window"
    );
}

/// A translucent target inside a `FirstClick` box lets the hit through, and the box still
/// joins the path: the first click reaches it.
pub(crate) fn a_translucent_target_inside_a_first_click_box_takes_the_first_click() {
    let taps = Rc::new(Cell::new(0));
    let laid = lay_out(
        Stack::new(
            (Positioned::new(FirstClick::new().child(translucent(&taps)))
                .left(0.0)
                .top(0.0),),
        ),
        tight(200.0, 100.0),
    );

    click(&laid, 1, 50.0, true);
    assert_eq!(
        taps.get(),
        1,
        "the translucent target inside the box takes the first click"
    );
}

/// A translucent widget painted over a `FirstClick` box lets the press through to the box:
/// the first click reaches the box's widget and not the widget on top, which an ordinary
/// press reaches as well.
pub(crate) fn a_translucent_widget_over_a_first_click_box_leaves_it_the_first_click() {
    let under = Rc::new(Cell::new(0));
    let over = Rc::new(Cell::new(0));
    let laid = lay_out(
        Stack::new((
            Positioned::new(FirstClick::new().child(counted(&under)))
                .left(0.0)
                .top(0.0),
            Positioned::new(translucent(&over)).left(0.0).top(0.0),
        )),
        tight(200.0, 100.0),
    );

    click(&laid, 1, 50.0, true);
    assert_eq!(
        (under.get(), over.get()),
        (1, 0),
        "only the box's subtree sees the first click"
    );
}
