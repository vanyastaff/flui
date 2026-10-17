//! A behavioural widget a package writes against the SDK alone: its fling
//! archives a row, and its alternatives archive it the same way, through one
//! intent (ADR-0216). `assert_gesture_alternatives` drives every path.

use std::cell::Cell;
use std::rc::Rc;

use flui_sdk::geometry::{Offset, Size};
use flui_sdk::painting::Color;
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, ViewExt};
use flui_sdk::widgets::{
    Actions, Alternative, CallbackAction, ColoredBox, ControlSpec, DragEndDetails, DragSlot,
    GestureAlternatives, GestureCommand, GestureDetector, Intent, SingleActivator, SizedBox,
};
use flui_testing::replay::PointerScript;
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};

/// Archive the row.
struct Archive;
impl Intent for Archive {}

/// Flinging the row to the left archives it.
#[derive(Clone, StatelessView)]
struct FlingToArchive {
    alternatives: GestureAlternatives,
    child: BoxedView,
}

impl StatelessView for FlingToArchive {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let archive = self.alternatives.handle(ctx);
        let detector = GestureDetector::new()
            .drag(
                DragSlot::horizontal().on_end(move |cx, details: DragEndDetails| {
                    if details.primary_velocity < -300.0 {
                        let _ = archive.perform(cx);
                    }
                }),
            )
            .child(self.child.clone());
        self.alternatives.clone().child(detector)
    }
}

impl std::fmt::Debug for FlingToArchive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FlingToArchive")
    }
}

fn row() -> AlternativesSubject<u32> {
    let archived = Rc::new(Cell::new(0));
    let alternatives = GestureAlternatives::new(Alternative::VisibleControl(ControlSpec::new(
        GestureCommand::new("Archive row", Archive)
            .key(SingleActivator::character("e").control().shift()),
        SizedBox::new(16.0, 16.0),
    )))
    .expect("one labelled command");
    let count = Rc::clone(&archived);
    let view = Actions::new(FlingToArchive {
        alternatives: alternatives.clone(),
        child: SizedBox::new(300.0, 50.0)
            .child(ColoredBox::new(Color::rgb(10, 20, 30)))
            .boxed(),
    })
    .action(CallbackAction::new(move |_cx, _: &Archive| {
        count.set(count.get() + 1);
    }));
    AlternativesSubject::new(view, &alternatives, move || archived.get())
}

#[test]
fn a_package_gesture_widget_reaches_its_intent_on_every_path() {
    assert_gesture_alternatives(&GestureAlternativesCheck::new(
        "FlingToArchive",
        BoxConstraints::tight(Size::new(400.0, 100.0)),
        PointerScript::swipe(Offset::new(250.0, 25.0), Offset::new(10.0, 25.0)),
        row,
    ));
}
