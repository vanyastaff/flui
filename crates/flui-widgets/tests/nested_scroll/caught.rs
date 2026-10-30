//! A remainder that reaches an ancestor running motion of its own, and the
//! settle the sequence owes every ancestor it moved: rows of the
//! `nested_scroll_channel` table (ADR-0209 §6a).

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::Vec2;
use flui_interaction::{GestureSettingsSource, RemainderHandoff};
use flui_testing::TouchScript;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    Column, NestedScroll, PageSnap, ScrollCause, ScrollController, ScrollEndReason, Scrollable,
    SizedBox, ViewportFraction, VsyncScope,
};

use crate::common::LaidOut;

use super::{
    DEVICES, Device, Probe, SAMPLE, Seen, at, drag, fling, line, list, mounted, remainder_preset,
    replay_contained, settle, slow,
};

/// A 200-pixel list at the top of a bouncing outer list, `middle` between
/// them and `top` above the outer list, under the remainder rule `handoff`.
fn list_at_the_top_of_a_springing_list(
    outer: &ScrollController,
    inner: &ScrollController,
    middle: &Rc<Probe>,
    top: &Rc<Probe>,
    handoff: RemainderHandoff,
    vsync: &Vsync,
) -> BoxedView {
    let content = Column::new(vec![
        SizedBox::new(300.0, 200.0)
            .child(NestedScroll::new(middle.connection(), list(inner)))
            .boxed(),
        SizedBox::new(300.0, 4800.0).boxed(),
    ]);
    let tree = crate::scroll::FlingProfile {
        provider: GestureSettingsSource::new(remainder_preset(handoff)).provider(),
        child: NestedScroll::new(
            top.connection(),
            Scrollable::new()
                .controller(outer.clone())
                .physics(std::rc::Rc::new(flui_widgets::BouncingScrollPhysics::new()))
                .child(content),
        )
        .boxed(),
    };
    VsyncScope::new(vsync.clone(), tree).boxed()
}

/// The springing layout's controllers and probes, mounted.
struct Springing {
    laid: LaidOut,
    outer: ScrollController,
    inner: ScrollController,
    middle: Rc<Probe>,
}

impl Springing {
    fn new(handoff: RemainderHandoff, inner_at: f64) -> Self {
        let (outer, inner, middle, top, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Probe::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_at_the_top_of_a_springing_list(&outer, &inner, &middle, &top, handoff, &vsync),
            vsync,
        );
        inner.jump_to(inner_at).expect("no build is running");
        laid.tick();
        Self {
            laid,
            outer,
            inner,
            middle,
        }
    }

    /// Drag the outer list down past its top by its own content and let it
    /// go at rest: its spring runs.
    fn pull_past_the_top(&mut self, device: Device) -> f64 {
        self.laid
            .replay(&slow(device, at(150.0, 205.0), at(150.0, 299.0)));
        let pulled = self.outer.pixels();
        assert!(
            pulled < -10.0,
            "{device:?}: the outer list is past its top, at {pulled}"
        );
        self.middle.take();
        pulled
    }
}

const HANDOFFS: [RemainderHandoff; 2] = [
    RemainderHandoff::NextSequence,
    RemainderHandoff::AtEdgeWithinSequence,
];

/// How many remainders `probe` was offered.
fn posts(probe: &Probe) -> usize {
    probe
        .seen()
        .iter()
        .filter(|seen| matches!(seen, Seen::Post(_)))
        .count()
}

/// A remainder that moves an ancestor whose spring runs stops the spring:
/// from then on the ancestor moves by the remainder alone, once, and holds
/// still while the finger does.
pub(crate) fn a_remainder_catches_a_springing_ancestor_and_moves_it_once() {
    for handoff in HANDOFFS {
        for device in DEVICES {
            let mut scene = Springing::new(handoff, 800.0);
            scene.pull_past_the_top(device);
            let script = slow(device, at(150.0, 200.0), at(150.0, 100.0));
            let mut track = Vec::new();
            let (outer, middle) = (scene.outer.clone(), Rc::clone(&scene.middle));
            scene.laid.replay_with(&script, |_, _| {
                track.push((posts(&middle), outer.pixels()));
            });
            let caught = track
                .iter()
                .position(|(posts, _)| *posts > 0)
                .unwrap_or_else(|| panic!("{device:?}/{handoff:?}: a remainder reached it"));
            let lift = track.len() - 1;
            assert!(
                lift - caught >= 3,
                "{device:?}/{handoff:?}: moves follow the first remainder: {track:?}"
            );
            for index in caught + 1..=lift {
                let moved = track[index].1 - track[index - 1].1;
                let finger = if index == lift { 0.0 } else { 10.0 };
                assert!(
                    (moved - finger).abs() < 1e-9,
                    "{device:?}/{handoff:?}: step {index} moves the caught list by the \
                     finger's step alone, {moved} for {finger}: {track:?}"
                );
            }
        }
    }
}

/// A remainder an ancestor past its top cannot take, pointing farther out,
/// leaves its spring running: it comes back to its top while the finger
/// pulls the other way.
pub(crate) fn an_outward_remainder_leaves_the_spring_running() {
    for handoff in HANDOFFS {
        for device in DEVICES {
            let mut scene = Springing::new(handoff, 0.0);
            let pulled = scene.pull_past_the_top(device);
            let script = drag(
                device,
                &line(at(150.0, 120.0), at(150.0, 280.0), 16),
                SAMPLE,
                Duration::from_millis(300),
            );
            let mut track = Vec::new();
            let outer = scene.outer.clone();
            scene
                .laid
                .replay_with(&script, |_, _| track.push(outer.pixels()));
            assert!(
                posts(&scene.middle) > 0,
                "{device:?}/{handoff:?}: the inner list left its pull to the chain"
            );
            assert!(
                track.iter().all(|pixels| *pixels >= pulled),
                "{device:?}/{handoff:?}: nothing pulls it farther out: {track:?}"
            );
            assert_eq!(
                *track.last().expect("a track"),
                0.0,
                "{device:?}/{handoff:?}: its spring brings it back while the finger moves: \
                 {track:?}"
            );
        }
    }
}

/// A press on the inner list holds a springing ancestor still until the
/// inner list's drag takes the contact; then the ancestor settles from
/// where it was held (ADR-0208 rule 4).
pub(crate) fn a_new_contact_holds_a_springing_ancestor_until_the_inner_drag_takes_it() {
    for device in DEVICES {
        let mut scene = Springing::new(RemainderHandoff::NextSequence, 400.0);
        scene.pull_past_the_top(device);
        let script = drag(
            device,
            &line(at(150.0, 200.0), at(150.0, 140.0), 20),
            SAMPLE,
            Duration::from_millis(300),
        );
        let mut track = Vec::new();
        let (outer, middle) = (scene.outer.clone(), Rc::clone(&scene.middle));
        scene.laid.replay_with(&script, |_, _| {
            let dragging = middle
                .steps()
                .iter()
                .any(|step| step.cause == ScrollCause::UserInput);
            track.push((dragging, outer.pixels()));
        });
        let taken = track
            .iter()
            .position(|(dragging, _)| *dragging)
            .unwrap_or_else(|| panic!("{device:?}: the inner list's drag starts"));
        assert!(
            taken > 1,
            "{device:?}: the drag waits for its slop: {track:?}"
        );
        assert!(
            track[..taken]
                .iter()
                .all(|(_, pixels)| *pixels == track[0].1),
            "{device:?}: the ancestor holds still from the press to the drag: {track:?}"
        );
        settle(&mut scene.laid);
        assert_eq!(
            scene.outer.pixels(),
            0.0,
            "{device:?}: then settles on its top"
        );
        assert!(
            scene.inner.pixels() > 400.0,
            "{device:?}: the drag moved the list"
        );
    }
}

/// A vertical pager of three-hundred-pixel pages whose first page is a
/// list, `top` above the pager.
fn list_on_the_first_page_of_a_pager(
    pager: &ScrollController,
    inner: &ScrollController,
    top: &Rc<Probe>,
    vsync: &Vsync,
) -> BoxedView {
    let pages = Column::new(vec![
        SizedBox::new(300.0, 300.0).child(list(inner)).boxed(),
        SizedBox::new(300.0, 900.0).boxed(),
    ]);
    pager_over(pager, top, pages.boxed(), vsync)
}

fn pager_over(
    pager: &ScrollController,
    top: &Rc<Probe>,
    pages: BoxedView,
    vsync: &Vsync,
) -> BoxedView {
    VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(
            top.connection(),
            Scrollable::new()
                .controller(pager.clone())
                .snap_policy(Arc::new(PageSnap::new(ViewportFraction::FULL)))
                .child(pages),
        ),
    )
    .boxed()
}

fn on_a_page(pixels: f64) -> bool {
    let page = (pixels / 300.0).round() * 300.0;
    (pixels - page).abs() < 0.5
}

/// The release of a `Scrollable` whose remainder moved its ancestor settles
/// the ancestor under the same sequence, with the release velocity on the
/// axis it holds: a pager lands on a page, the next one for a fling, and a
/// bouncing list caught past its top comes back to it.
pub(crate) fn a_caught_ancestor_settles_at_release_with_the_latched_velocity() {
    for device in DEVICES {
        for flung in [false, true] {
            let (pager, inner, top, vsync) = (
                ScrollController::new(),
                ScrollController::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(
                list_on_the_first_page_of_a_pager(&pager, &inner, &top, &vsync),
                vsync,
            );
            inner.jump_to(700.0).expect("no build is running");
            laid.tick();
            top.take();
            let script = if flung {
                fling(device, at(150.0, 250.0), at(150.0, 180.0))
            } else {
                slow(device, at(150.0, 250.0), at(150.0, 150.0))
            };
            let dragged = Cell::new(0.0);
            laid.replay_with(&script, |_, _| dragged.set(pager.pixels()));
            assert!(
                dragged.get() > 0.0 && !on_a_page(dragged.get()),
                "{device:?}/flung={flung}: the remainder moved the pager off its page, to {}",
                dragged.get()
            );
            settle(&mut laid);
            assert_eq!(
                pager.pixels(),
                if flung { 300.0 } else { 0.0 },
                "{device:?}/flung={flung}: the release lands it on a page"
            );
            assert_eq!(inner.pixels(), 700.0, "{device:?}: the list runs nothing");
            let contact = top.started()[0];
            assert_eq!(
                top.ended()
                    .iter()
                    .filter(|(id, _)| *id == contact)
                    .copied()
                    .collect::<Vec<_>>(),
                vec![(contact, ScrollEndReason::Released)],
                "{device:?}/flung={flung}: the settle runs under the contact's sequence, \
                 which ends once: {:?}",
                top.seen()
            );
        }

        // A pull that latches the pager and then reverses: the list could
        // fling the way the finger left, but the pager holds the axis, so
        // the release settles the pager alone. A participant between the
        // two hears no step of a run of the list's.
        let (pager, inner, middle, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Vsync::new(),
        );
        let pages = Column::new(vec![
            SizedBox::new(300.0, 300.0)
                .child(NestedScroll::new(middle.connection(), list(&inner)))
                .boxed(),
            SizedBox::new(300.0, 900.0).boxed(),
        ]);
        let mut laid = mounted(
            pager_over(&pager, &Probe::new(), pages.boxed(), &vsync),
            vsync,
        );
        inner.jump_to(700.0).expect("no build is running");
        laid.tick();
        let mut points = line(at(150.0, 250.0), at(150.0, 150.0), 10);
        points.extend(
            line(at(150.0, 150.0), at(150.0, 230.0), 4)
                .into_iter()
                .skip(1),
        );
        laid.replay(&drag(
            device,
            &points,
            Duration::from_millis(8),
            Duration::ZERO,
        ));
        settle(&mut laid);
        assert_eq!(
            pager.pixels(),
            0.0,
            "{device:?}: the pager settles back on its page"
        );
        assert_eq!(inner.pixels(), 700.0, "{device:?}: the list never moves");
        assert!(
            !middle
                .steps()
                .iter()
                .any(|step| step.cause == ScrollCause::SideEffect),
            "{device:?}: the list runs no fling of its own: {:?}",
            middle.seen()
        );

        for handoff in HANDOFFS {
            let mut scene = Springing::new(handoff, 800.0);
            scene.pull_past_the_top(device);
            let script = drag(
                device,
                &line(at(150.0, 200.0), at(150.0, 176.0), 8),
                SAMPLE,
                Duration::from_millis(300),
            );
            let held = Cell::new(0.0);
            let outer = scene.outer.clone();
            scene
                .laid
                .replay_with(&script, |_, _| held.set(outer.pixels()));
            assert!(
                held.get() < 0.0 && posts(&scene.middle) > 0,
                "{device:?}/{handoff:?}: caught past its top, at {}",
                held.get()
            );
            settle(&mut scene.laid);
            assert_eq!(
                scene.outer.pixels(),
                0.0,
                "{device:?}/{handoff:?}: the release settles it on its top"
            );
        }
    }
}

/// A sequence that moved an ancestor and ends without a release (cancelled,
/// or its list unmounted mid-drag) settles the ancestor from rest before it
/// ends, and every participant hears the end once.
pub(crate) fn a_caught_ancestor_settles_when_the_sequence_ends_without_release() {
    for unmount in [false, true] {
        let (pager, inner, top, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_on_the_first_page_of_a_pager(&pager, &inner, &top, &vsync),
            vsync.clone(),
        );
        inner.jump_to(700.0).expect("no build is running");
        laid.tick();
        top.take();
        let mut script = TouchScript::new("pull").down(1, at(150.0, 250.0));
        for point in line(at(150.0, 250.0), at(150.0, 150.0), 10)
            .into_iter()
            .skip(1)
        {
            script = script.wait(SAMPLE).move_to(1, point);
        }
        let script = if unmount {
            script.wait(SAMPLE).up(1)
        } else {
            script.wait(SAMPLE).cancel(1)
        }
        .build()
        .expect("one whole contact");
        let last_move = script.steps().len() - 2;
        let dragged = Cell::new(0.0);
        laid.replay_with(&script, |laid, index| {
            if index == last_move {
                dragged.set(pager.pixels());
                if unmount {
                    laid.pump_widget(pager_over(
                        &pager,
                        &top,
                        SizedBox::new(300.0, 1200.0).boxed(),
                        &vsync,
                    ));
                }
            }
        });
        assert!(
            dragged.get() > 0.0 && !on_a_page(dragged.get()),
            "unmount={unmount}: the remainder moved the pager off its page, to {}",
            dragged.get()
        );
        settle(&mut laid);
        assert_eq!(pager.pixels(), 0.0, "unmount={unmount}: it rests on a page");
        let contact = top.started()[0];
        assert_eq!(
            top.ended().iter().filter(|(id, _)| *id == contact).count(),
            1,
            "unmount={unmount}: the sequence ends once: {:?}",
            top.seen()
        );
    }
}

/// A finger on the caught ancestor itself takes it over: the inner list's
/// release leaves it to that finger, and that finger's release settles it.
/// Touch only: one mouse has one contact.
pub(crate) fn a_caught_ancestor_touched_directly_follows_its_own_contact() {
    let mut scene = Springing::new(RemainderHandoff::NextSequence, 800.0);
    scene.pull_past_the_top(Device::Touch);
    let mut script = TouchScript::new("two fingers").down(1, at(150.0, 200.0));
    for point in line(at(150.0, 200.0), at(150.0, 150.0), 5)
        .into_iter()
        .skip(1)
    {
        script = script.wait(SAMPLE).move_to(1, point);
    }
    script = script.wait(SAMPLE).down(2, at(150.0, 290.0));
    for point in line(at(150.0, 290.0), at(150.0, 230.0), 6)
        .into_iter()
        .skip(1)
    {
        script = script.wait(SAMPLE).move_to(2, point);
    }
    script = script.wait(SAMPLE).up(1);
    for point in line(at(150.0, 230.0), at(150.0, 200.0), 3)
        .into_iter()
        .skip(1)
    {
        script = script.wait(SAMPLE).move_to(2, point);
    }
    let script = script
        .wait(Duration::from_millis(300))
        .up(2)
        .build()
        .expect("two whole contacts");
    // The Down, five moves, the second Down and its six moves come before
    // the inner list lifts.
    let lifted = 1 + 5 + 1 + 6;
    let mut track = Vec::new();
    let outer = scene.outer.clone();
    scene
        .laid
        .replay_with(&script, |_, _| track.push(outer.pixels()));
    assert!(
        posts(&scene.middle) > 0,
        "the inner list's remainder reached the outer one"
    );
    assert_eq!(
        track[lifted],
        track[lifted - 1],
        "the inner list's release starts nothing on the outer one, which its own finger \
         holds: {track:?}"
    );
    assert!(
        track[lifted + 1] > track[lifted],
        "which it goes on following: {track:?}"
    );
    let lifted_outer = *track.last().expect("a track");
    settle(&mut scene.laid);
    assert!(
        lifted_outer >= 0.0 && scene.outer.pixels() == lifted_outer,
        "its own finger brought it into its range, where its release leaves it: {track:?}"
    );
}

/// An ancestor a remainder moved and that a rebuild retires mid-drag hands
/// what the sequence owes it to its successor: the drag goes on moving the
/// successor, and the release settles it (ADR-0209 §8).
pub(crate) fn a_caught_ancestor_retired_mid_drag_is_settled_by_its_successor() {
    for device in DEVICES {
        let (pager, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_on_the_first_page_of_a_pager(&pager, &inner, &Probe::new(), &vsync),
            vsync.clone(),
        );
        inner.jump_to(700.0).expect("no build is running");
        laid.tick();
        let script = slow(device, at(150.0, 280.0), at(150.0, 120.0));
        let (rebuilt_at, before, after) = (5, Cell::new(0.0), Cell::new(0.0));
        laid.replay_with(&script, |laid, index| {
            if index == rebuilt_at {
                before.set(pager.pixels());
                // A new connection above the pager gives it a new chain
                // node: its owner retires into a successor on the same
                // position.
                laid.pump_widget(list_on_the_first_page_of_a_pager(
                    &pager,
                    &inner,
                    &Probe::new(),
                    &vsync,
                ));
            }
            after.set(pager.pixels());
        });
        assert!(
            before.get() > 0.0 && after.get() > before.get(),
            "{device:?}: the drag goes on moving the pager after the rebuild, {} then {}",
            before.get(),
            after.get()
        );
        settle(&mut laid);
        assert!(
            on_a_page(pager.pixels()),
            "{device:?}: the release settles the successor on a page, at {}",
            pager.pixels()
        );
    }
}

/// A participant above a caught ancestor that panics does not cost the
/// ancestor its settle: the release still lands the pager on a page, and the
/// first failure is the one resumed (ADR-0127).
pub(crate) fn a_panic_above_a_caught_ancestor_keeps_its_settle(device: Device) {
    let (pager, inner, top, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        Probe::new(),
        Vsync::new(),
    );
    let mut laid = mounted(
        list_on_the_first_page_of_a_pager(&pager, &inner, &top, &vsync),
        vsync,
    );
    inner.jump_to(700.0).expect("no build is running");
    laid.tick();
    let (steps, moved) = (Cell::new(0), pager.clone());
    top.set_pre(move |step| {
        if step.cause == ScrollCause::UserInput && moved.pixels() > 0.0 {
            steps.set(steps.get() + 1);
            assert!(steps.get() != 2, "first participant failure");
        }
        Vec2::ZERO
    });
    let failures = replay_contained(&mut laid, &slow(device, at(150.0, 250.0), at(150.0, 150.0)));
    settle(&mut laid);
    assert_eq!(
        failures,
        vec!["first participant failure".to_owned()],
        "{device:?}: the first failure resumes, once"
    );
    assert!(
        pager.pixels() == 0.0 || pager.pixels() == 300.0,
        "{device:?}: the caught pager still settles on a page, at {}",
        pager.pixels()
    );

    // The pager's own listener fails while the pager publishes the one
    // remainder of the contact: the sequence still owes the pager its
    // settle. A touch whose moves stay under its slop until the last but one
    // has a single step; a mouse's slop is too small for that.
    if device != Device::Touch {
        return;
    }
    let (pager, inner, top, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        Probe::new(),
        Vsync::new(),
    );
    let mut laid = mounted(
        list_on_the_first_page_of_a_pager(&pager, &inner, &top, &vsync),
        vsync,
    );
    inner.jump_to(700.0).expect("no build is running");
    laid.tick();
    let (armed, moved) = (Rc::new(Cell::new(true)), pager.clone());
    let trigger = Rc::clone(&armed);
    pager.listenable().add_listener(Rc::new(move || {
        let fails = moved.pixels() > 0.0 && trigger.replace(false);
        assert!(!fails, "first participant failure");
    }));
    let script = drag(
        device,
        &[
            at(150.0, 250.0),
            at(150.0, 245.0),
            at(150.0, 240.0),
            at(150.0, 235.0),
            at(150.0, 226.0),
            at(150.0, 220.0),
        ],
        SAMPLE,
        Duration::from_millis(300),
    );
    let failures = replay_contained(&mut laid, &script);
    assert_eq!(
        failures,
        vec!["first participant failure".to_owned()],
        "{device:?}: the listener's failure resumes, pager at {}, list at {}",
        pager.pixels(),
        inner.pixels()
    );
    let moved = pager.pixels();
    assert!(
        moved > 0.0 && !on_a_page(moved),
        "{device:?}: the one remainder moved the pager off its page, to {moved}"
    );
    settle(&mut laid);
    assert_eq!(
        pager.pixels(),
        0.0,
        "{device:?}: and the release settles it on its page"
    );
}
