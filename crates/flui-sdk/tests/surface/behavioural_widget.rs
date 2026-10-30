//! A behavioural widget a package writes against the SDK alone: flinging a
//! row to the left archives it and flinging it to the right pins it, and its
//! alternatives archive and pin it the same way, through one intent per
//! command (ADR-0216). `assert_gesture_alternatives` drives every path; the
//! second command's gesture, which that check does not drive, has its own
//! test, as does the system-edge strip the row leaves to the operating system.

use std::cell::Cell;
use std::rc::Rc;

use flui_sdk::geometry::{Axis, Offset, Size};
use flui_sdk::painting::Color;
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, ViewExt};
use flui_sdk::widgets::{
    Actions, Alternative, CallbackAction, ColoredBox, ContactAdmission, ControlSpec,
    DragEndDetails, DragSlot, GestureAlternatives, GestureCommand, GestureDetector, Intent,
    SingleActivator, SizedBox,
};
use flui_testing::replay::PointerScript;
use flui_testing::widgets::lay_out;
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};

/// Archive the row.
struct Archive;
impl Intent for Archive {}

/// Pin the row.
struct Pin;
impl Intent for Pin {}

/// Flinging the row to the left performs its first command, to the right
/// its second. A touch going down in the window's system-gesture strip is
/// left to the operating system's edge swipe.
#[derive(Clone, StatelessView)]
struct FlingToFile {
    alternatives: GestureAlternatives,
    child: BoxedView,
}

impl StatelessView for FlingToFile {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let left = self.alternatives.handle(ctx);
        let right = self
            .alternatives
            .commands()
            .find(|command| command.performs::<Pin>())
            .expect("the row has a pin command")
            .handle(ctx);
        let detector = GestureDetector::new()
            .admission(ContactAdmission::outside_system_edges(
                ctx,
                Axis::Horizontal,
            ))
            .drag(
                DragSlot::horizontal().on_end(move |cx, details: DragEndDetails| {
                    if details.primary_velocity < -300.0 {
                        let _ = left.perform(cx);
                    } else if details.primary_velocity > 300.0 {
                        let _ = right.perform(cx);
                    }
                }),
            )
            .child(self.child.clone());
        self.alternatives.clone().child(detector)
    }
}

impl std::fmt::Debug for FlingToFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FlingToFile")
    }
}

/// How often the row was archived and pinned.
type Filed = (u32, u32);

fn row() -> AlternativesSubject<Filed> {
    let (view, alternatives, model) = filing_row();
    AlternativesSubject::new(view, &alternatives, model)
}

/// The row, its alternatives and a reading of how often it was filed.
fn filing_row() -> (Actions, GestureAlternatives, impl Fn() -> Filed + 'static) {
    let (archived, pinned) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
    let alternatives = GestureAlternatives::try_new(Alternative::VisibleControl(
        ControlSpec::new(
            GestureCommand::new("Archive row", Archive)
                .key(SingleActivator::character("e").control().shift()),
            SizedBox::new(16.0, 16.0),
        )
        .control(
            GestureCommand::new("Pin row", Pin),
            SizedBox::new(16.0, 16.0),
        ),
    ))
    .expect("two labelled commands");
    let (archive, pin) = (Rc::clone(&archived), Rc::clone(&pinned));
    let view = Actions::new(FlingToFile {
        alternatives: alternatives.clone(),
        child: SizedBox::new(300.0, 50.0)
            .child(ColoredBox::new(Color::rgb(10, 20, 30)))
            .boxed(),
    })
    .action(CallbackAction::new(move |_cx, _: &Archive| {
        archive.set(archive.get() + 1);
    }))
    .action(CallbackAction::new(move |_cx, _: &Pin| {
        pin.set(pin.get() + 1);
    }));
    (view, alternatives, move || (archived.get(), pinned.get()))
}

/// How often a fresh row was filed after `gesture`.
fn after(gesture: &PointerScript) -> Filed {
    let (view, _alternatives, model) = filing_row();
    let mut laid = lay_out(view, BoxConstraints::tight(Size::new(400.0, 100.0)));
    laid.replay(gesture);
    laid.pump_for(std::time::Duration::from_secs(1));
    model()
}

#[test]
fn a_package_gesture_widget_reaches_its_intent_on_every_path() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "FlingToFile",
        BoxConstraints::tight(Size::new(400.0, 100.0)),
        PointerScript::swipe(Offset::new(250.0, 25.0), Offset::new(40.0, 25.0)),
        row,
    ));
}

#[test]
fn a_package_gesture_widget_performs_its_second_command_from_its_gesture() {
    assert_eq!(
        after(&PointerScript::swipe(
            Offset::new(100.0, 25.0),
            Offset::new(320.0, 25.0)
        )),
        (0, 1),
        "the fling to the right performs the second command through the chain"
    );
    assert_eq!(
        after(&PointerScript::swipe(
            Offset::new(2.0, 25.0),
            Offset::new(220.0, 25.0)
        )),
        (0, 0),
        "a touch from the window's edge strip is the operating system's"
    );
}
