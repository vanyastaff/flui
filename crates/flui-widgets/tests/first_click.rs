//! `FirstClick`: a press marked as the click that brought its window forward reaches the
//! widgets inside a first-click region, and nothing outside one, through the input regions
//! the window's frame published (ADR-0217 §5).

use std::cell::Cell;
use std::rc::Rc;

use flui_foundation::geometry::Point;
use flui_painting::styling::Color;
use flui_platform::EventTime;
use flui_platform::pointer::{
    PointerButton, PointerButtons, PointerEvent, PointerId, PointerKind, PointerPosition,
    PointerPress, PointerRelease, PointerSample, PointerSource,
};
use flui_view::{IntoView, View};
use flui_widgets::{ColoredBox, FirstClick, GestureDetector, Positioned, SizedBox, Stack};

use crate::common::{LaidOut, lay_out, tight};

/// A 100-px square that counts its taps.
fn counted(taps: &Rc<Cell<u32>>) -> impl IntoView {
    let taps = Rc::clone(taps);
    GestureDetector::new()
        .on_tap(move |_| taps.set(taps.get() + 1))
        .child(SizedBox::new(100.0, 100.0).child(ColoredBox::new(Color::rgb(240, 240, 240))))
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
