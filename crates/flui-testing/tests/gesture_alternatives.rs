//! `assert_gesture_alternatives` passes a row whose every path performs the
//! gesture's intent, and names the path of a row that breaks it.

use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationBehavior, AnimationController, AnimationStatus, DrivenController,
    StatusSubscription,
};
use flui_foundation::geometry::Offset;
use flui_painting::styling::Color;
use flui_testing::replay::PointerScript;
use flui_testing::widgets::tight;
use flui_testing::{AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives};
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    Actions, Alternative, CallbackAction, ColoredBox, Column, CompanionLink, CompanionRegistration,
    CompanionSpec, ControlSpec, Dismissible, GestureAlternatives, GestureCommand, Intent, Listener,
    MenuSpec, RawButton, Semantics, SemanticsAnchor, SizedBox, VsyncScope,
};

/// Count one removal.
struct Remove;
impl Intent for Remove {}

/// How the subject is broken, if it is.
#[derive(Clone, Copy)]
enum Subject {
    /// Every path performs `Remove`.
    Conforming,
    /// The drag also does something the alternatives do not.
    GestureDoesMore,
    /// Nothing answers `Remove`.
    Unbound,
    /// The menu opens but shows nothing assistive technology can reach.
    SilentMenu,
    /// Removal waits for an animation that plays under reduced motion too.
    IgnoresReducedMotion,
    /// The alternatives rely on a companion, and none is registered.
    NoCompanion,
    /// A companion backs them with a button that every pointer can press.
    Companioned,
    /// A companion registers a node that assistive technology can click and
    /// no pointer can press.
    CompanionIgnoresPointers,
    /// A companion whose button is too small to hit.
    CompanionUnderTarget,
    /// A companion registers a node that every pointer can press and
    /// assistive technology cannot click.
    CompanionIgnoresAssistiveTechnology,
}

/// Who can activate a companion's node.
#[derive(Clone, Copy)]
enum Reach {
    Everyone,
    /// Assistive technology clicks it; no pointer presses it.
    AssistiveOnly,
    /// A pointer presses it; assistive technology has no click for it.
    PointersOnly,
}

/// Registers on a link while mounted, naming a 48 px node that removes the row
/// when it is activated.
#[derive(Clone, StatefulView)]
struct RemoveButton {
    link: CompanionLink<Remove>,
    removed: Rc<Cell<u32>>,
    reach: Reach,
    /// Its side, in logical pixels.
    side: f64,
}

struct RemoveButtonState {
    anchor: SemanticsAnchor,
    _registration: CompanionRegistration,
}

impl StatefulView for RemoveButton {
    type State = RemoveButtonState;

    fn create_state(&self) -> Self::State {
        let anchor = SemanticsAnchor::new();
        let named = anchor.clone();
        RemoveButtonState {
            _registration: self.link.register(move |_| Some(named.clone())),
            anchor,
        }
    }
}

impl ViewState<RemoveButton> for RemoveButtonState {
    fn build(&self, view: &RemoveButton, _ctx: &dyn BuildContext) -> impl IntoView {
        let removed = Rc::clone(&view.removed);
        let semantics = Semantics::new()
            .label("Remove row")
            .anchor(self.anchor.clone());
        let body = SizedBox::new(view.side, view.side);
        match view.reach {
            Reach::Everyone => RawButton::new(semantics.child(body))
                .on_press(move |_cx| removed.set(removed.get() + 1))
                .boxed(),
            Reach::AssistiveOnly => semantics
                .on_click(move |_cx| removed.set(removed.get() + 1))
                .child(body)
                .boxed(),
            Reach::PointersOnly => semantics
                .child(
                    Listener::new()
                        .on_pointer_down(move |_cx, _dispatch| removed.set(removed.get() + 1))
                        .child(body),
                )
                .boxed(),
        }
    }
}

impl std::fmt::Debug for RemoveButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoveButton")
    }
}

impl std::fmt::Debug for RemoveButtonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RemoveButtonState")
    }
}

/// Performs `Remove` once a 300 ms animation that ignores reduced motion
/// (`AnimationBehavior::Preserve`) has finished.
#[derive(Clone, StatefulView)]
struct AnimatedRemoval {
    child: BoxedView,
    removed: Rc<Cell<u32>>,
}

struct AnimatedRemovalState {
    driven: DrivenController,
    _settled: StatusSubscription,
}

impl StatefulView for AnimatedRemoval {
    type State = AnimatedRemovalState;

    fn create_state(&self) -> Self::State {
        let driven = AnimationController::builder(Duration::from_millis(300))
            .behavior(AnimationBehavior::Preserve)
            .build_on(None);
        let removed = Rc::clone(&self.removed);
        let settled = driven.controller().subscribe_status(Rc::new(move |status| {
            if status == AnimationStatus::ForwardSettled {
                removed.set(removed.get() + 1);
            }
        }));
        AnimatedRemovalState {
            driven,
            _settled: settled,
        }
    }
}

impl ViewState<AnimatedRemoval> for AnimatedRemovalState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.driven
            .rebind(VsyncScope::maybe_of(ctx).as_ref())
            .expect("a registry seat");
    }

    fn build(&self, view: &AnimatedRemoval, _ctx: &dyn BuildContext) -> impl IntoView {
        let controller = self.driven.controller().clone();
        Actions::new(view.child.clone()).action(CallbackAction::new(move |_cx, _: &Remove| {
            let _ = controller.forward();
        }))
    }

    fn dispose(&mut self) {
        self.driven.dispose();
    }
}

fn mount(subject: Subject) -> AlternativesSubject<u32> {
    let removed = Rc::new(Cell::new(0));
    let command = GestureCommand::new("Remove row", Remove);
    let link = CompanionLink::<Remove>::new();
    let alternative = match subject {
        Subject::SilentMenu => Alternative::ContextMenu(MenuSpec::new(command, |_cx, _request| {})),
        Subject::NoCompanion
        | Subject::Companioned
        | Subject::CompanionIgnoresPointers
        | Subject::CompanionUnderTarget
        | Subject::CompanionIgnoresAssistiveTechnology => {
            Alternative::Companion(CompanionSpec::new(command, &link))
        }
        _ => Alternative::VisibleControl(ControlSpec::new(command, SizedBox::new(16.0, 16.0))),
    };
    let alternatives = GestureAlternatives::try_new(alternative).expect("one labelled command");
    let extra = Rc::clone(&removed);
    let mut card = Dismissible::new(
        SizedBox::new(300.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
        alternatives.clone(),
    );
    if matches!(subject, Subject::GestureDoesMore) {
        card = card.on_dismissed(move |_cx, _direction| extra.set(extra.get() + 10));
    }
    let count = Rc::clone(&removed);
    let view = if matches!(subject, Subject::Unbound) {
        card.boxed()
    } else if matches!(subject, Subject::IgnoresReducedMotion) {
        AnimatedRemoval {
            child: card.boxed(),
            removed: Rc::clone(&removed),
        }
        .boxed()
    } else {
        Actions::new(card)
            .action(CallbackAction::new(move |_cx, _: &Remove| {
                count.set(count.get() + 1);
            }))
            .boxed()
    };
    let view = match subject {
        Subject::Companioned
        | Subject::CompanionIgnoresPointers
        | Subject::CompanionUnderTarget
        | Subject::CompanionIgnoresAssistiveTechnology => Column::new(vec![
            view,
            RemoveButton {
                link,
                removed: Rc::clone(&removed),
                reach: match subject {
                    Subject::CompanionIgnoresPointers => Reach::AssistiveOnly,
                    Subject::CompanionIgnoresAssistiveTechnology => Reach::PointersOnly,
                    _ => Reach::Everyone,
                },
                side: if matches!(subject, Subject::CompanionUnderTarget) {
                    10.0
                } else {
                    48.0
                },
            }
            .boxed(),
        ])
        .boxed(),
        _ => view,
    };
    AlternativesSubject::new(view, &alternatives, move || removed.get())
}

fn check(subject: Subject) -> GestureAlternativesCheck<u32> {
    GestureAlternativesCheck::new(
        "row",
        tight(400.0, 100.0),
        PointerScript::swipe(Offset::new(250.0, 25.0), Offset::new(10.0, 25.0)),
        move || mount(subject),
    )
}

/// The failure message of a check that must fail.
fn failure(subject: Subject) -> String {
    let payload = catch_unwind(AssertUnwindSafe(|| {
        assert_gesture_alternatives(&check(subject));
    }))
    .expect_err("a broken subject fails the check");
    payload
        .downcast_ref::<String>()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn the_check_passes_a_conforming_row_and_names_each_broken_path() {
    assert_gesture_alternatives(&check(Subject::Conforming));

    let more = failure(Subject::GestureDoesMore);
    assert!(
        more.contains("by assistive technology: the model is 1, by gesture 11"),
        "a gesture with an effect of its own is caught:\n{more}"
    );

    let unbound = failure(Subject::Unbound);
    assert!(
        unbound.contains("by gesture: the model did not change from 0"),
        "an intent nothing answers is caught:\n{unbound}"
    );

    let animated = failure(Subject::IgnoresReducedMotion);
    for source in ["reduced motion", "an animator duration scale of 0"] {
        assert!(
            animated.contains(&format!(
                "by assistive technology under {source}: the model did not change from 0"
            )),
            "an alternative that animates under {source} is caught:\n{animated}"
        );
    }
    assert!(
        !animated.contains("by assistive technology: "),
        "and it conforms with motion on:\n{animated}"
    );

    let silent = failure(Subject::SilentMenu);
    assert!(
        silent.contains("its menu, opened by a secondary click, item chosen by assistive technology: the path could not be driven"),
        "a menu assistive technology cannot reach is caught:\n{silent}"
    );

    assert_gesture_alternatives(&check(Subject::Companioned));
    let dead = failure(Subject::CompanionIgnoresPointers);
    for device in ["Mouse", "Touch", "Virtual"] {
        assert!(
            dead.contains(&format!(
                "by its companion's node, pressed with {device}: the model did not change from 0"
            )),
            "a companion no {device} pointer can press is caught:\n{dead}"
        );
    }
    assert!(
        !dead.contains("clicked by assistive technology"),
        "while assistive technology still reaches it:\n{dead}"
    );

    let deaf = failure(Subject::CompanionIgnoresAssistiveTechnology);
    assert!(
        deaf.contains(
            "its companion's node, clicked by assistive technology: the path could not be driven"
        ),
        "a companion assistive technology cannot click is caught:\n{deaf}"
    );

    let tiny = failure(Subject::CompanionUnderTarget);
    assert!(
        tiny.contains("its companion's node is 10×10, under the"),
        "a companion too small to hit is caught:\n{tiny}"
    );

    let companionless = failure(Subject::NoCompanion);
    assert!(
        companionless.contains(
            "\"Remove row\": the alternatives declare a companion, but no companion is registered on the link"
        ),
        "alternatives that rely on a companion nobody mounted are caught:\n{companionless}"
    );
}
