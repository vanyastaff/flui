//! Programmatic requests made while a pass of the channel runs: rows of the
//! `nested_scroll_channel` table (ADR-0209 §3, §7).

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::Vec2;
use flui_interaction::InputSequenceId;
use flui_interaction::events::{Code, Key, NamedKey};
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    NestedScroll, ScrollCause, ScrollController, ScrollDevice, ScrollEndReason, SizedBox,
    VsyncScope,
};

use crate::common::LaidOut;

use super::{
    DEVICES, Keys, Probe, Seen, at, fling, list, mounted, panic_tree, replay_contained, settle,
    slow,
};

/// A list under `probe`.
pub(super) fn list_under(
    probe: &Rc<Probe>,
    controller: &ScrollController,
    vsync: &Vsync,
) -> BoxedView {
    VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(controller)),
    )
    .boxed()
}

/// The programmatic sequences `probe` was offered steps of, in order.
fn programmatic(probe: &Probe) -> Vec<InputSequenceId> {
    let mut ids = Vec::new();
    for step in probe.steps() {
        if step.cause == ScrollCause::Programmatic && !ids.contains(&step.sequence) {
            ids.push(step.sequence);
        }
    }
    ids
}

fn programmatic_steps(probe: &Probe) -> usize {
    probe
        .steps()
        .iter()
        .filter(|step| step.cause == ScrollCause::Programmatic)
        .count()
}

/// The callout a participant asks for a scroll from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Callout {
    Pre,
    Post,
    Released,
    PostFling,
    Ended,
}

impl Callout {
    fn heard(self, seen: &Seen) -> bool {
        match (self, seen) {
            (Self::Pre, Seen::Pre(step)) | (Self::Post, Seen::Post(step)) => {
                step.cause == ScrollCause::UserInput
            }
            (Self::Released, Seen::Released(_))
            | (Self::PostFling, Seen::Fling(_))
            | (Self::Ended, Seen::Ended(..)) => true,
            _ => false,
        }
    }
}

/// A jump a participant asks for from any of its callouts lands when the
/// pass that called it returns, clamped to the extents then: the contact's
/// next delta moves from there, and the jump is a programmatic sequence of
/// its own that starts only once the pass has made the same callout to the
/// participant above the one that asked.
pub(crate) fn a_jump_from_a_participant_lands_when_the_pass_returns() {
    for device in DEVICES {
        for callout in [
            Callout::Pre,
            Callout::Post,
            Callout::Released,
            Callout::PostFling,
            Callout::Ended,
        ] {
            let (controller, near, probe, vsync) = (
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(panic_tree(&controller, &near, &probe, &vsync), vsync);
            // From its end the list leaves its drag to the probe; from short
            // of it a fling offers the residual.
            let (start, target) = match callout {
                Callout::Post => (700.0, 500.0),
                Callout::PostFling => (600.0, 500.0),
                Callout::Ended => (0.0, 5000.0),
                Callout::Pre | Callout::Released => (0.0, 500.0),
            };
            controller.jump_to(start);
            laid.tick();
            probe.take();
            let armed = Rc::new(Cell::new(true));
            let (trigger, reentrant) = (Rc::clone(&armed), controller.clone());
            near.set_observe(move |seen| {
                if callout.heard(seen) && trigger.replace(false) {
                    reentrant.jump_to(target);
                }
            });
            let script = if callout == Callout::PostFling {
                fling(device, at(150.0, 250.0), at(150.0, 160.0))
            } else {
                slow(device, at(150.0, 250.0), at(150.0, 100.0))
            };
            laid.replay(&script);
            settle(&mut laid);
            assert!(!armed.get(), "{device:?}/{callout:?}: the callout ran");
            let pixels = controller.pixels();
            match callout {
                Callout::Pre | Callout::Post => assert!(
                    pixels > 500.0 && pixels < 700.0,
                    "{device:?}/{callout:?}: the contact goes on from the jump, at {pixels}"
                ),
                Callout::Released | Callout::PostFling => {
                    assert_eq!(pixels, 500.0, "{device:?}/{callout:?}: the jump lands");
                }
                Callout::Ended => assert_eq!(
                    pixels, 700.0,
                    "{device:?}/{callout:?}: clamped to the extents where it lands"
                ),
            }
            let seen = probe.seen();
            let asked = seen
                .iter()
                .position(|seen| callout.heard(seen))
                .expect("the participant above heard the same callout");
            let jumps = programmatic(&probe);
            assert_eq!(
                jumps.len(),
                1,
                "{device:?}/{callout:?}: one programmatic sequence: {seen:?}"
            );
            let started = seen
                .iter()
                .position(|seen| *seen == Seen::Started(jumps[0]))
                .expect("the jump's sequence started");
            assert!(
                started > asked,
                "{device:?}/{callout:?}: it starts after the callout that asked: {seen:?}"
            );
            assert!(
                probe
                    .ended()
                    .contains(&(jumps[0], ScrollEndReason::Released)),
                "{device:?}/{callout:?}: and ends released"
            );
        }
    }
}

/// A position listener that jumps while a step publishes the offset does
/// not run the jump inside that step: a listener after it first hears the
/// offset the step published, then the jump's.
pub(crate) fn a_jump_from_a_position_listener_never_nests_inside_a_step() {
    for device in DEVICES {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let mut laid = mounted(list_under(&probe, &controller, &vsync), vsync);
        controller.jump_to(650.0);
        laid.tick();
        probe.take();
        let listenable = controller.listenable();
        let armed = Rc::new(Cell::new(true));
        let (trigger, reentrant) = (Rc::clone(&armed), controller.clone());
        let jumper = listenable.add_listener(Rc::new(move || {
            if reentrant.pixels() >= 700.0 && trigger.replace(false) {
                reentrant.jump_to(100.0);
            }
        }));
        let heard = Rc::new(RefCell::new(Vec::new()));
        let (log, reader) = (Rc::clone(&heard), controller.clone());
        let after =
            listenable.add_listener(Rc::new(move || log.borrow_mut().push(reader.pixels())));
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 100.0)));
        settle(&mut laid);
        listenable.remove_listener(jumper);
        listenable.remove_listener(after);
        assert!(!armed.get(), "{device:?}: the list reached its end");
        let heard = heard.borrow();
        let end = heard
            .iter()
            .position(|pixels| *pixels == 700.0)
            .unwrap_or_else(|| {
                panic!("{device:?}: the step that reached the end was heard: {heard:?}")
            });
        assert_eq!(
            heard.get(end + 1),
            Some(&100.0),
            "{device:?}: then the jump's offset: {heard:?}"
        );
        assert!(
            controller.pixels() > 100.0 && controller.pixels() < 300.0,
            "{device:?}: the contact goes on from the jump, at {}",
            controller.pixels()
        );
        assert_eq!(programmatic(&probe).len(), 1, "{device:?}: one jump");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Requests {
    TwoJumps,
    JumpThenAnimate,
    AnimateThenJump,
}

/// Requests a participant makes in one pass share the controller's one
/// slot: the latest wins, whether it is a jump or an animation, and none
/// starts before the pass that asked for it has called every participant.
pub(crate) fn requests_made_in_one_pass_keep_the_latest() {
    for device in DEVICES {
        for requests in [
            Requests::TwoJumps,
            Requests::JumpThenAnimate,
            Requests::AnimateThenJump,
        ] {
            let (controller, near, probe, vsync) = (
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(panic_tree(&controller, &near, &probe, &vsync), vsync);
            laid.tick();
            let armed = Rc::new(Cell::new(true));
            let (trigger, reentrant) = (Rc::clone(&armed), controller.clone());
            // The nearer participant hears the release first and asks; the
            // farther one hears it after.
            near.set_observe(move |seen| {
                if !matches!(seen, Seen::Released(_)) || !trigger.replace(false) {
                    return;
                }
                let animate = || {
                    reentrant.animate_to(
                        600.0,
                        Duration::from_millis(200),
                        flui_animation::ArcCurve::new(flui_animation::curves::LINEAR),
                    );
                };
                match requests {
                    Requests::TwoJumps => {
                        reentrant.jump_to(300.0);
                        reentrant.jump_to(700.0);
                    }
                    Requests::JumpThenAnimate => {
                        reentrant.jump_to(300.0);
                        animate();
                    }
                    Requests::AnimateThenJump => {
                        animate();
                        reentrant.jump_to(300.0);
                    }
                }
            });
            laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 150.0)));
            settle(&mut laid);
            assert!(
                !armed.get(),
                "{device:?}/{requests:?}: the release was heard"
            );
            let jumps = programmatic(&probe);
            assert_eq!(
                jumps.len(),
                1,
                "{device:?}/{requests:?}: one programmatic sequence: {:?}",
                probe.seen()
            );
            let seen = probe.seen();
            let released = seen
                .iter()
                .position(|seen| matches!(seen, Seen::Released(_)))
                .expect("the farther participant heard the release");
            let started = seen
                .iter()
                .position(|seen| *seen == Seen::Started(jumps[0]))
                .expect("the request's sequence started");
            assert!(
                released < started,
                "{device:?}/{requests:?}: the request waits for the release pass: {seen:?}"
            );
            let (pixels, steps) = (controller.pixels(), programmatic_steps(&probe));
            match requests {
                Requests::TwoJumps => {
                    assert_eq!(pixels, 700.0, "{device:?}: the second jump lands");
                    assert_eq!(steps, 1, "{device:?}: as one step");
                }
                Requests::JumpThenAnimate => {
                    assert_eq!(pixels, 600.0, "{device:?}: the animation runs");
                    assert!(steps > 1, "{device:?}: frame by frame");
                }
                Requests::AnimateThenJump => {
                    assert_eq!(pixels, 300.0, "{device:?}: the jump lands");
                    assert_eq!(steps, 1, "{device:?}: and no run starts");
                }
            }
        }
    }
}

/// A jump that lands ends the run the scrollable drives `Preempted`, before
/// the jump's own sequence starts, and the run offers no residual; it never
/// ends a contact or a key that is still held, which go on from the new
/// offset.
pub(crate) fn a_landing_jump_preempts_the_run_and_keeps_live_input() {
    for device in DEVICES {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let mut laid = mounted(list_under(&probe, &controller, &vsync), vsync);
        laid.tick();
        let armed = Rc::new(Cell::new(true));
        let (trigger, reentrant) = (Rc::clone(&armed), controller.clone());
        probe.set_observe(move |seen| {
            if let Seen::Pre(step) = seen
                && step.cause == ScrollCause::SideEffect
                && trigger.replace(false)
            {
                reentrant.jump_to(100.0);
            }
        });
        laid.replay(&fling(device, at(150.0, 250.0), at(150.0, 150.0)));
        settle(&mut laid);
        assert!(!armed.get(), "{device:?}: the fling ran");
        assert_eq!(
            controller.pixels(),
            100.0,
            "{device:?}: the jump lands and the fling moves no more"
        );
        let flung = probe.started()[0];
        let jumps = programmatic(&probe);
        assert_eq!(jumps.len(), 1, "{device:?}: one jump");
        let seen = probe.seen();
        let preempted = seen
            .iter()
            .position(|seen| *seen == Seen::Ended(flung, ScrollEndReason::Preempted))
            .unwrap_or_else(|| panic!("{device:?}: the fling ends preempted: {seen:?}"));
        let started = seen
            .iter()
            .position(|seen| *seen == Seen::Started(jumps[0]))
            .expect("the jump's sequence started");
        assert!(
            preempted < started,
            "{device:?}: before the jump starts: {seen:?}"
        );
        assert_eq!(probe.flings(), 0, "{device:?}: no residual is offered");

        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let mut laid = mounted(list_under(&probe, &controller, &vsync), vsync);
        laid.tick();
        let armed = Rc::new(Cell::new(true));
        let (trigger, reentrant) = (Rc::clone(&armed), controller.clone());
        probe.set_observe(move |seen| {
            if let Seen::Pre(step) = seen
                && step.cause == ScrollCause::UserInput
                && trigger.replace(false)
            {
                reentrant.jump_to(500.0);
            }
        });
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 100.0)));
        settle(&mut laid);
        assert!(
            controller.pixels() > 500.0,
            "{device:?}: the contact goes on from the jump, at {}",
            controller.pixels()
        );
        let contact = probe.started()[0];
        let jumps = programmatic(&probe);
        assert_eq!(jumps.len(), 1, "{device:?}: one jump");
        assert_eq!(
            probe.ended(),
            vec![
                (jumps[0], ScrollEndReason::Released),
                (contact, ScrollEndReason::Released)
            ],
            "{device:?}: the jump does not end the contact, which ends released"
        );
    }

    let down = Key::Named(NamedKey::ArrowDown);
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(&controller).autofocus(true)),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    laid.tick();
    let keys = Keys::of(&laid);
    let armed = Rc::new(Cell::new(true));
    let (trigger, reentrant) = (Rc::clone(&armed), controller.clone());
    probe.set_observe(move |seen| {
        if let Seen::Pre(step) = seen
            && step.device == ScrollDevice::Keyboard
            && trigger.replace(false)
        {
            reentrant.jump_to(300.0);
        }
    });
    assert!(
        keys.down(Code::ArrowDown, down.clone()),
        "the key is handled"
    );
    assert_eq!(controller.pixels(), 300.0, "the jump lands");
    let held = probe.started()[0];
    assert!(
        !probe.ended().iter().any(|(id, _)| *id == held),
        "the held key's sequence stays open: {:?}",
        probe.seen()
    );
    keys.up(Code::ArrowDown, down);
    settle(&mut laid);
    assert!(
        probe.ended().contains(&(held, ScrollEndReason::Released)),
        "its release ends it"
    );
}

/// A participant that asks for a jump and then panics loses neither: the
/// jump lands, the first failure resumes once, and the next sequence
/// includes the participant again (ADR-0127).
pub(crate) fn a_jump_asked_before_a_participant_panics_still_lands() {
    for device in DEVICES {
        for competing in [false, true] {
            let (controller, near, far, vsync) = (
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(panic_tree(&controller, &near, &far, &vsync), vsync);
            let armed = Rc::new(Cell::new(true));
            let (trigger, reentrant) = (Rc::clone(&armed), controller.clone());
            near.set_pre(move |_| {
                if trigger.replace(false) {
                    reentrant.jump_to(500.0);
                    panic!("first participant failure");
                }
                Vec2::ZERO
            });
            if competing {
                *near.on_end.borrow_mut() = Some(Box::new(|| panic!("second participant failure")));
            }
            let failures =
                replay_contained(&mut laid, &slow(device, at(150.0, 250.0), at(150.0, 100.0)));
            settle(&mut laid);
            assert_eq!(
                failures,
                vec!["first participant failure".to_owned()],
                "{device:?}/competing={competing}: the first failure resumes, once"
            );
            assert!(
                controller.pixels() > 500.0,
                "{device:?}/competing={competing}: the jump landed and the contact went on \
                 from it, at {}",
                controller.pixels()
            );

            *near.on_end.borrow_mut() = None;
            controller.jump_to(0.0);
            laid.tick();
            near.take();
            assert!(
                replay_contained(&mut laid, &slow(device, at(150.0, 250.0), at(150.0, 100.0)))
                    .is_empty(),
                "{device:?}: the next sequence runs clean"
            );
            settle(&mut laid);
            assert_eq!(
                near.started().len(),
                1,
                "{device:?}: the participant is back"
            );
            assert_ne!(
                near.steps().len(),
                0,
                "{device:?}: it is offered steps again"
            );
        }
    }
}

/// A mounted list whose position listener jumps one pixel on from every
/// change it hears, while `remaining` lasts.
struct JumpingOnEveryChange {
    laid: LaidOut,
    controller: ScrollController,
    probe: Rc<Probe>,
    remaining: Rc<Cell<u32>>,
    vsync: Vsync,
}

impl JumpingOnEveryChange {
    fn new(budget: u32) -> Self {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let mut laid = mounted(list_under(&probe, &controller, &vsync), vsync.clone());
        laid.tick();
        let remaining = Rc::new(Cell::new(budget));
        let (left, reentrant) = (Rc::clone(&remaining), controller.clone());
        controller.listenable().add_listener(Rc::new(move || {
            if left.get() > 0 {
                left.set(left.get() - 1);
                reentrant.jump_to(reentrant.pixels() + 1.0);
            }
        }));
        Self {
            laid,
            controller,
            probe,
            remaining,
            vsync,
        }
    }
}

/// A listener that jumps on every change cannot spin the pass: after a
/// bounded number of jumps the dispatch returns, and the target asked for
/// last waits and lands on the next notification.
pub(crate) fn a_jump_feedback_loop_is_bounded_and_keeps_its_target() {
    let scene = JumpingOnEveryChange::new(50);
    scene.controller.jump_to(100.0);
    let landed = scene.controller.pixels();
    scene.remaining.set(0);
    assert!(
        landed > 100.0 && landed < 150.0,
        "the jumps stop short of the listener's budget, at {landed}"
    );
    assert_eq!(
        programmatic(&scene.probe).len() as f64,
        landed - 100.0 + 1.0,
        "each landed jump is a sequence"
    );
    scene
        .controller
        .update_dimensions(300.0, 0.0, 700.0)
        .expect("ordered extents");
    assert_eq!(
        scene.controller.pixels(),
        landed + 1.0,
        "the waiting target lands on the next notification"
    );
}

/// A jump still waiting when its scrollable unmounts is not dropped: the
/// controller writes it, clamped, with no owner left.
pub(crate) fn a_jump_pending_across_an_unmount_writes_the_position() {
    let mut scene = JumpingOnEveryChange::new(50);
    scene.controller.jump_to(100.0);
    let landed = scene.controller.pixels();
    scene.remaining.set(0);
    scene.laid.pump_widget(VsyncScope::new(
        scene.vsync.clone(),
        SizedBox::new(300.0, 300.0),
    ));
    assert_eq!(
        scene.controller.pixels(),
        landed + 1.0,
        "the waiting target is written when the scrollable goes"
    );
}
