//! `assert_gesture_alternatives` passes a row whose every path performs the
//! gesture's intent, and names the path of a row that breaks it.

use flui_foundation::Subscription;
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationBehavior, AnimationController, AnimationStatus, DrivenController,
};
use flui_foundation::geometry::Offset;
use flui_interaction::gesture::GestureSettings;
use flui_painting::styling::Color;
use flui_semantics::SemanticsAnchor;
use flui_testing::gesture_alternatives::{
    AlternativesSubject, GestureAlternativesCheck, assert_gesture_alternatives,
};
use flui_testing::replay::PointerScript;
use flui_testing::widgets::tight;
use flui_view::environment::{GestureArenaScope, VsyncScope};
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    Actions, Alternative, CallbackAction, ColoredBox, Column, CompanionLink, CompanionRegistration,
    CompanionSpec, ControlSpec, Dismissible, GestureAlternatives, GestureCommand, Intent, Listener,
    MenuSpec, RawButton, Semantics, SizedBox,
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
    /// A companion whose button is tall enough and too narrow.
    CompanionTooNarrow,
    /// A companion whose button is wide enough and too short.
    CompanionTooShort,
    /// The menu of `SilentMenu` in a window whose touch minimum is 24 px, so
    /// the control focus reveals is drawn narrower than the check's minimum.
    SmallRevealedControl,
    /// The menu of `SilentMenu` in a 30 px row of a window whose touch minimum
    /// is 48 px, so the control focus reveals is wide enough and too short.
    ShortRowRevealedControl,
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
    /// Its width and height, in logical pixels.
    size: (f64, f64),
}

struct RemoveButtonState {
    anchor: SemanticsAnchor,
    _registration: CompanionRegistration,
}

impl StatefulView for RemoveButton {
    type State = RemoveButtonState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
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
        let body = SizedBox::new(view.size.0, view.size.1);
        match view.reach {
            Reach::Everyone => RawButton::new(semantics.child(body))
                .on_pressed(move |_ctx| removed.set(removed.get() + 1))
                .boxed(),
            Reach::AssistiveOnly => semantics
                .on_click(move |_ctx| removed.set(removed.get() + 1))
                .child(body)
                .boxed(),
            Reach::PointersOnly => semantics
                .child(
                    Listener::new()
                        .on_pointer_down(move |_ctx, _dispatch| removed.set(removed.get() + 1))
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
    _settled: Subscription,
}

impl StatefulView for AnimatedRemoval {
    type State = AnimatedRemovalState;

    fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
        let driven = AnimationController::builder(Duration::from_millis(300))
            .behavior(AnimationBehavior::Preserve)
            .build_on(None);
        let removed = Rc::clone(&self.removed);
        let settled = driven.controller().listen_status(Rc::new(move |status| {
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
        Actions::new(view.child.clone()).action(CallbackAction::new(move |_ctx, _: &Remove| {
            let _ = controller.forward();
        }))
    }

    fn dispose(&mut self) {
        self.driven.dispose();
    }
}

/// `child` in a window whose every pointer, a finger included, needs a target
/// of `min_target` px: the presentation's arena, scoped again with it.
#[derive(Clone, StatelessView)]
struct WindowMinimum {
    child: BoxedView,
    min_target: f64,
}

impl StatelessView for WindowMinimum {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let mut values = GestureSettings::default().values();
        values.min_target = self.min_target;
        let settings = GestureSettings::try_from_values(values).expect("a finite minimum target");
        GestureArenaScope::new(GestureArenaScope::of(ctx), self.child.clone()).settings(settings)
    }
}

impl std::fmt::Debug for WindowMinimum {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("WindowMinimum")
    }
}

fn mount(subject: Subject) -> AlternativesSubject<u32> {
    let removed = Rc::new(Cell::new(0));
    let command = GestureCommand::new("Remove row", Remove);
    let link = CompanionLink::<Remove>::new();
    let alternative = match subject {
        Subject::SilentMenu | Subject::SmallRevealedControl | Subject::ShortRowRevealedControl => {
            Alternative::ContextMenu(MenuSpec::new(command, |_ctx, _request| {}))
        }
        Subject::NoCompanion
        | Subject::Companioned
        | Subject::CompanionIgnoresPointers
        | Subject::CompanionUnderTarget
        | Subject::CompanionTooNarrow
        | Subject::CompanionTooShort
        | Subject::CompanionIgnoresAssistiveTechnology => {
            Alternative::Companion(CompanionSpec::new(command, &link))
        }
        _ => Alternative::VisibleControl(ControlSpec::new(command, SizedBox::new(16.0, 16.0))),
    };
    let alternatives = GestureAlternatives::try_new(alternative).expect("one labelled command");
    let extra = Rc::clone(&removed);
    let mut card = Dismissible::new(
        alternatives.clone(),
        SizedBox::new(300.0, 50.0).child(ColoredBox::new(Color::rgb(10, 20, 30))),
    );
    if matches!(subject, Subject::GestureDoesMore) {
        card = card.on_dismissed(move |_ctx, _direction| extra.set(extra.get() + 10));
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
            .action(CallbackAction::new(move |_ctx, _: &Remove| {
                count.set(count.get() + 1);
            }))
            .boxed()
    };
    let view = match subject {
        Subject::Companioned
        | Subject::CompanionIgnoresPointers
        | Subject::CompanionUnderTarget
        | Subject::CompanionTooNarrow
        | Subject::CompanionTooShort
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
                size: match subject {
                    Subject::CompanionUnderTarget => (10.0, 10.0),
                    Subject::CompanionTooNarrow => (10.0, 48.0),
                    Subject::CompanionTooShort => (48.0, 10.0),
                    _ => (48.0, 48.0),
                },
            }
            .boxed(),
        ])
        .boxed(),
        Subject::SmallRevealedControl => WindowMinimum {
            child: view,
            min_target: 24.0,
        }
        .boxed(),
        // The row sits 30 px down, so the control's edges are not at zero.
        Subject::ShortRowRevealedControl => WindowMinimum {
            child: Column::new(vec![
                SizedBox::new(400.0, 30.0).boxed(),
                SizedBox::new(400.0, 30.0).child(view).boxed(),
            ])
            .boxed(),
            min_target: 48.0,
        }
        .boxed(),
        _ => view,
    };
    AlternativesSubject::new(view, &alternatives, move || removed.get())
}

fn check(subject: Subject) -> GestureAlternativesCheck<u32> {
    let (height, row_y) = if matches!(subject, Subject::ShortRowRevealedControl) {
        (60.0, 45.0)
    } else {
        (100.0, 25.0)
    };
    GestureAlternativesCheck::new(
        "row",
        tight(400.0, height),
        PointerScript::swipe(Offset::new(250.0, row_y), Offset::new(10.0, row_y)),
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
    for device in ["Mouse", "Touch", "Virtual"] {
        assert!(
            more.contains(&format!(
                "by its control, pressed with {device}: the model is 1, by gesture 11"
            )),
            "and each pointer's press of the visible control is compared:\n{more}"
        );
    }

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
    assert!(
        !silent.contains("the control focus reveals is"),
        "the control focus reveals meets the minimum at the default settings:\n{silent}"
    );
    for (subject, size) in [
        (Subject::SmallRevealedControl, "26×100"),
        (Subject::ShortRowRevealedControl, "48×30"),
    ] {
        let small = failure(subject);
        assert!(
            small.contains(&format!(
                "the control focus reveals is {size}, under the 44 minimum target"
            )),
            "a revealed control under the minimum on one axis is caught:\n{small}"
        );
    }

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
    for (subject, size) in [
        (Subject::CompanionTooNarrow, "10×48"),
        (Subject::CompanionTooShort, "48×10"),
    ] {
        let short = failure(subject);
        assert!(
            short.contains(&format!("its companion's node is {size}, under the")),
            "a companion under the minimum on one axis is caught:\n{short}"
        );
    }

    let companionless = failure(Subject::NoCompanion);
    assert!(
        companionless.contains(
            "\"Remove row\": the alternatives declare a companion, but no companion is registered on the link"
        ),
        "alternatives that rely on a companion nobody mounted are caught:\n{companionless}"
    );
}
