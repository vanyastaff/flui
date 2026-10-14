//! The nested scroll channel (ADR-0209): rows of the `nested_scroll_channel`
//! table in `contracts.rs`, each driven by touch and mouse scripts.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::{Axis, Offset, Vec2};
use flui_interaction::{
    GestureProfiles, GestureSettings, GestureSettingsSource, GestureValues, InputSequenceId,
    RemainderHandoff,
};
use flui_platform_api::{EventTime, TargetPlatform};
use flui_testing::{MouseScript, PointerScript, ScriptStep, TouchScript, TrackpadScript};
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    Column, FlingHandoff, FlingStep, ListView, NestedScroll, NestedScrollConnection,
    OverscrollBehavior, PageSnap, Row, ScrollCause, ScrollController, ScrollDevice,
    ScrollEndReason, ScrollMetrics, ScrollStep, Scrollable, SizedBox, SnapPolicy, VsyncScope,
};

use crate::common::{LaidOut, lay_out_animated, tight};

// ---------------------------------------------------------------------------
// A participant that records what reaches it
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Seen {
    Started(InputSequenceId),
    Pre(ScrollStep),
    Post(ScrollStep),
    Released(FlingStep),
    Fling(FlingStep),
    Ended(InputSequenceId, ScrollEndReason),
}

type PreHook = Box<dyn Fn(&ScrollStep) -> Vec2>;
type EndHook = Box<dyn Fn()>;

/// Records every call; takes what its hooks say. Its state is `Rc` and
/// `Cell`, so it also proves a connection may capture both.
#[derive(Default)]
struct Probe {
    seen: RefCell<Vec<Seen>>,
    pre: RefCell<Option<PreHook>>,
    on_end: RefCell<Option<EndHook>>,
    takes_remainder: Cell<bool>,
}

impl Probe {
    fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    fn connection(self: &Rc<Self>) -> Rc<dyn NestedScrollConnection> {
        Rc::clone(self) as Rc<dyn NestedScrollConnection>
    }

    fn set_pre(&self, hook: impl Fn(&ScrollStep) -> Vec2 + 'static) {
        *self.pre.borrow_mut() = Some(Box::new(hook));
    }

    fn take(&self) -> Vec<Seen> {
        self.seen.take()
    }

    fn seen(&self) -> Vec<Seen> {
        self.seen.borrow().clone()
    }

    fn started(&self) -> Vec<InputSequenceId> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Started(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    fn ended(&self) -> Vec<(InputSequenceId, ScrollEndReason)> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Ended(id, reason) => Some((id, reason)),
                _ => None,
            })
            .collect()
    }

    fn steps(&self) -> Vec<ScrollStep> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Pre(step) => Some(step),
                _ => None,
            })
            .collect()
    }

    fn flings(&self) -> usize {
        self.seen()
            .iter()
            .filter(|seen| matches!(seen, Seen::Fling(_)))
            .count()
    }
}

impl NestedScrollConnection for Probe {
    fn sequence_started(&self, sequence: InputSequenceId) {
        self.seen.borrow_mut().push(Seen::Started(sequence));
    }

    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        self.seen.borrow_mut().push(Seen::Pre(*step));
        let hook = self.pre.borrow();
        hook.as_ref().map_or(Vec2::ZERO, |hook| hook(step))
    }

    fn post_scroll(&self, step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
        self.seen.borrow_mut().push(Seen::Post(*step));
        if self.takes_remainder.get() {
            step.available
        } else {
            Vec2::ZERO
        }
    }

    fn released(&self, step: &FlingStep) {
        self.seen.borrow_mut().push(Seen::Released(*step));
    }

    fn post_fling(&self, step: &FlingStep) -> FlingHandoff {
        self.seen.borrow_mut().push(Seen::Fling(*step));
        FlingHandoff::Declined
    }

    fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {
        self.seen.borrow_mut().push(Seen::Ended(sequence, reason));
        let hook = self.on_end.borrow();
        if let Some(hook) = hook.as_ref() {
            hook();
        }
    }
}

// ---------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Touch,
    Mouse,
}

const DEVICES: [Device; 2] = [Device::Touch, Device::Mouse];

const SAMPLE: Duration = Duration::from_millis(16);

fn at(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

/// `steps` evenly spaced points after `from`, ending at `to`.
fn line(from: Offset<f64>, to: Offset<f64>, steps: u32) -> Vec<Offset<f64>> {
    (0..=steps)
        .map(|step| {
            let t = f64::from(step) / f64::from(steps);
            Offset::new(
                from.dx + (to.dx - from.dx) * t,
                from.dy + (to.dy - from.dy) * t,
            )
        })
        .collect()
}

/// A contact through `points`, `sample` between moves, held still for
/// `hold` before it lifts.
fn drag(device: Device, points: &[Offset<f64>], sample: Duration, hold: Duration) -> PointerScript {
    let (first, rest) = points.split_first().expect("a drag has a start");
    match device {
        Device::Touch => {
            let mut script = TouchScript::new("drag").down(1, *first);
            for point in rest {
                script = script.wait(sample).move_to(1, *point);
            }
            script.wait(hold).up(1).build().expect("one whole contact")
        }
        Device::Mouse => {
            let mut script = MouseScript::new("drag").move_to(*first).press(*first);
            for point in rest {
                script = script.wait(sample).move_to(*point);
            }
            script
                .wait(hold)
                .release()
                .build()
                .expect("one whole press")
        }
    }
}

/// A drag that comes to rest before it lifts: no fling.
fn slow(device: Device, from: Offset<f64>, to: Offset<f64>) -> PointerScript {
    drag(
        device,
        &line(from, to, 10),
        SAMPLE,
        Duration::from_millis(300),
    )
}

/// A fast drag that lifts while moving.
fn fling(device: Device, from: Offset<f64>, to: Offset<f64>) -> PointerScript {
    drag(
        device,
        &line(from, to, 6),
        Duration::from_millis(8),
        Duration::ZERO,
    )
}

/// A press that lifts where it went down.
fn press(device: Device, point: Offset<f64>) -> PointerScript {
    match device {
        Device::Touch => TouchScript::new("press")
            .down(2, point)
            .wait(Duration::from_millis(50))
            .up(2)
            .build()
            .expect("one whole contact"),
        Device::Mouse => MouseScript::new("press")
            .move_to(point)
            .press(point)
            .wait(Duration::from_millis(50))
            .release()
            .build()
            .expect("one whole press"),
    }
}

/// [`LaidOut::replay`], each step on its own: a step that panics is caught
/// and the rest of the script still runs. Returns the caught messages.
fn replay_contained(laid: &mut LaidOut, script: &PointerScript) -> Vec<String> {
    let mut failures = Vec::new();
    let mut previous = Duration::ZERO;
    for step in script.steps() {
        laid.pump_for(step.at().saturating_sub(previous));
        previous = step.at();
        if let ScriptStep::Pointer(pointer) = step
            && let event = pointer.to_event_at(EventTime::from_nanos(0))
            && let Err(payload) =
                catch_unwind(AssertUnwindSafe(|| laid.dispatch_pointer_event(&event)))
        {
            failures.push(
                payload
                    .downcast_ref::<&str>()
                    .map(ToString::to_string)
                    .or_else(|| payload.downcast_ref::<String>().cloned())
                    .unwrap_or_default(),
            );
        }
    }
    failures
}

fn settle(laid: &mut LaidOut) {
    for _ in 0..120 {
        laid.pump_for(SAMPLE);
    }
}

// ---------------------------------------------------------------------------
// Layouts
// ---------------------------------------------------------------------------

fn list(controller: &ScrollController) -> Scrollable {
    Scrollable::new()
        .controller(controller.clone())
        .child(SizedBox::new(300.0, 1000.0))
}

/// A vertical list on the first page of a horizontal pager, a probe above
/// both.
fn list_in_pager(
    pager: &ScrollController,
    list_controller: &ScrollController,
    probe: &Rc<Probe>,
    vsync: &Vsync,
) -> BoxedView {
    let pages = Row::new(vec![
        SizedBox::new(300.0, 300.0)
            .child(list(list_controller))
            .boxed(),
        SizedBox::new(300.0, 300.0).boxed(),
        SizedBox::new(300.0, 300.0).boxed(),
    ]);
    VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(
            probe.connection(),
            Scrollable::new()
                .controller(pager.clone())
                .scroll_direction(Axis::Horizontal)
                .child(pages),
        ),
    )
    .boxed()
}

/// A 200-pixel list on screen at the top of an outer list scrolled to 600,
/// under `presets`, with `top` above the outer list and `middle` between
/// the two.
fn list_in_list(
    outer: &ScrollController,
    inner: &ScrollController,
    top: &Rc<Probe>,
    middle: &Rc<Probe>,
    presets: Option<GestureProfiles>,
    vsync: &Vsync,
) -> BoxedView {
    let content = Column::new(vec![
        SizedBox::new(300.0, 600.0).boxed(),
        SizedBox::new(300.0, 200.0)
            .child(NestedScroll::new(middle.connection(), list(inner)))
            .boxed(),
        SizedBox::new(300.0, 4800.0).boxed(),
    ]);
    let tree = NestedScroll::new(
        top.connection(),
        Scrollable::new().controller(outer.clone()).child(content),
    )
    .boxed();
    let tree = match presets {
        Some(presets) => crate::scroll::FlingProfile {
            provider: GestureSettingsSource::new(presets).provider(),
            child: tree,
        }
        .boxed(),
        None => tree,
    };
    VsyncScope::new(vsync.clone(), tree).boxed()
}

fn mounted(view: BoxedView, vsync: Vsync) -> LaidOut {
    lay_out_animated(view, tight(300.0, 300.0), vsync)
}

fn remainder_preset(handoff: RemainderHandoff) -> GestureProfiles {
    let settings = GestureSettings::try_from_values(GestureValues {
        remainder_handoff: handoff,
        ..GestureSettings::default().values()
    })
    .expect("the default values with another remainder rule");
    GestureProfiles::uniform(settings)
}

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// A vertical list on a horizontal pager's page: a vertical drag moves the
/// list, a horizontal or 30° drag the pager, and each step carries motion on
/// its owner's axis only (ADR-0209 C1).
pub(crate) fn list_in_pager_shares_deltas_by_axis() {
    for device in DEVICES {
        let (pager, list_controller, probe, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_in_pager(&pager, &list_controller, &probe, &vsync),
            vsync,
        );

        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 100.0)));
        settle(&mut laid);
        assert!(
            list_controller.pixels() > 100.0,
            "{device:?}: a vertical drag moves the list, got {}",
            list_controller.pixels()
        );
        assert_eq!(pager.pixels(), 0.0, "{device:?}: and not the pager");
        let steps = probe.steps();
        assert!(
            !steps.is_empty(),
            "{device:?}: the probe saw the list's steps"
        );
        assert!(
            steps
                .iter()
                .all(|step| step.available.x == 0.0 && step.available.y > 0.0),
            "{device:?}: a vertical drag offers vertical motion only: {steps:?}"
        );
        let listed = list_controller.pixels();
        probe.take();

        laid.replay(&slow(device, at(250.0, 150.0), at(100.0, 150.0)));
        settle(&mut laid);
        assert!(
            pager.pixels() > 100.0,
            "{device:?}: a horizontal drag moves the pager, got {}",
            pager.pixels()
        );
        assert_eq!(
            list_controller.pixels(),
            listed,
            "{device:?}: and not the list"
        );
        let steps = probe.steps();
        assert!(
            steps
                .iter()
                .all(|step| step.available.y == 0.0 && step.available.x > 0.0),
            "{device:?}: a horizontal drag offers horizontal motion only: {steps:?}"
        );

        pager.jump_to(0.0);
        laid.tick();
        laid.replay(&slow(device, at(250.0, 200.0), at(100.0, 113.4)));
        settle(&mut laid);
        assert!(
            pager.pixels() > 100.0,
            "{device:?}: a drag 30° off horizontal belongs to the pager, got {}",
            pager.pixels()
        );
        assert_eq!(
            list_controller.pixels(),
            listed,
            "{device:?}: not to the list"
        );
    }
}

/// A list at its start with an outer list above: the first delta toward the
/// start latches the outer list, which keeps the whole contact even after it
/// reverses, on every platform's preset (ADR-0209 §5).
pub(crate) fn edge_at_start_latches_ancestor_on_every_preset() {
    let platforms = [
        TargetPlatform::Android,
        TargetPlatform::iOS,
        TargetPlatform::Linux,
        TargetPlatform::MacOS,
        TargetPlatform::Windows,
        TargetPlatform::Fuchsia,
    ];
    for platform in platforms {
        for device in DEVICES {
            let (outer, inner, top, middle, vsync) = (
                ScrollController::new(),
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(
                list_in_list(
                    &outer,
                    &inner,
                    &top,
                    &middle,
                    Some(GestureProfiles::preset(platform)),
                    &vsync,
                ),
                vsync,
            );
            outer.jump_to(600.0);
            laid.tick();
            let mut points = line(at(150.0, 40.0), at(150.0, 160.0), 8);
            points.extend(
                line(at(150.0, 160.0), at(150.0, 20.0), 8)
                    .into_iter()
                    .skip(1),
            );
            let script = drag(device, &points, SAMPLE, Duration::from_millis(300));
            let lowest = Cell::new(outer.pixels());
            laid.replay_with(&script, |_, _| lowest.set(lowest.get().min(outer.pixels())));
            settle(&mut laid);
            assert!(
                lowest.get() < 520.0,
                "{platform:?}/{device:?}: the outer list takes the pull past the inner one's \
                 start, lowest {}",
                lowest.get()
            );
            assert!(
                outer.pixels() > lowest.get() + 60.0,
                "{platform:?}/{device:?}: and keeps the contact when it reverses, at {}",
                outer.pixels()
            );
            assert_eq!(
                inner.pixels(),
                0.0,
                "{platform:?}/{device:?}: the inner list never moves in that sequence"
            );
        }
    }
}

/// With the mid-drag hand-off preset, a latched list that reaches its end
/// re-latches to the nearest ancestor that consumes, within the contact, and
/// never back; with the default preset the ancestor waits for the next
/// contact (ADR-0209 §6).
pub(crate) fn mid_drag_transfer_relatches_only_at_edge_within_the_sequence() {
    for device in DEVICES {
        for handoff in [
            RemainderHandoff::AtEdgeWithinSequence,
            RemainderHandoff::NextSequence,
        ] {
            let (outer, inner, top, middle, vsync) = (
                ScrollController::new(),
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(
                list_in_list(
                    &outer,
                    &inner,
                    &top,
                    &middle,
                    Some(remainder_preset(handoff)),
                    &vsync,
                ),
                vsync,
            );
            outer.jump_to(600.0);
            inner.jump_to(700.0);
            laid.tick();
            let up = line(at(150.0, 190.0), at(150.0, 10.0), 12);
            let up_steps = up.len();
            let mut points = up;
            points.extend(
                line(at(150.0, 10.0), at(150.0, 100.0), 6)
                    .into_iter()
                    .skip(1),
            );
            let script = drag(device, &points, SAMPLE, Duration::from_millis(300));
            // The contact's Down is the first step; the last move up is the
            // step after its last point.
            let turned = Cell::new((0.0, 0.0));
            laid.replay_with(&script, |_, index| {
                if index + 1 == up_steps {
                    turned.set((inner.pixels(), outer.pixels()));
                }
            });
            settle(&mut laid);
            let (inner_at_turn, outer_at_turn) = turned.get();
            assert_eq!(
                inner_at_turn, 800.0,
                "{device:?}/{handoff:?}: the inner list reaches its end"
            );
            if handoff == RemainderHandoff::AtEdgeWithinSequence {
                assert!(
                    outer_at_turn > 600.0,
                    "{device:?}: past the inner end the outer list takes the remainder"
                );
                assert_eq!(
                    inner.pixels(),
                    800.0,
                    "{device:?}: never back to the inner list"
                );
                assert!(
                    outer.pixels() < outer_at_turn,
                    "{device:?}: the outer list keeps the reversed drag"
                );
            } else {
                assert_eq!(outer_at_turn, 600.0, "{device:?}: the outer list waits");
                assert_eq!(outer.pixels(), 600.0, "{device:?}: for the whole contact");
                assert!(
                    inner.pixels() < 800.0,
                    "{device:?}: the latched inner list keeps the reversed drag"
                );
            }
        }
    }
}

/// The binding's sequence is the channel's: wheel notches within the idle
/// time are one sequence, the next one starts exactly when the binding mints
/// it, and each contact is its own (ADR-0209 §5).
pub(crate) fn channel_latch_ends_exactly_when_binding_mints_a_new_sequence() {
    let idle = GestureSettings::default().contactless_latch_idle();
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(&controller)),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    let wheel = MouseScript::new("wheel burst")
        .move_to(at(150.0, 150.0))
        .wheel(1.0)
        .wait(Duration::from_millis(100))
        .wheel(1.0)
        .wait(idle.saturating_sub(Duration::from_millis(1)))
        .wheel(1.0)
        // Frames that let the third notch's eased motion come to rest.
        .wait(Duration::from_millis(100))
        .frame()
        .wait(Duration::from_millis(100))
        .frame()
        .wait(idle.saturating_sub(Duration::from_millis(200)))
        .wheel(1.0)
        .build()
        .expect("a hovering mouse");
    let notches = Cell::new(0);
    laid.replay_with(&wheel, |_, _| notches.set(notches.get() + 1));
    settle(&mut laid);
    let started = probe.started();
    assert_eq!(
        started.len(),
        2,
        "three notches within the idle time are one sequence, the fourth another: {:#?}",
        probe.seen()
    );
    assert_ne!(started[0], started[1]);
    let seen = probe.seen();
    let second = seen
        .iter()
        .position(|seen| *seen == Seen::Started(started[1]))
        .expect("the second sequence started");
    assert_eq!(
        seen[second - 1],
        Seen::Ended(started[0], ScrollEndReason::Released),
        "the first sequence ends exactly where the binding began the next"
    );
    probe.take();

    for device in DEVICES {
        controller.jump_to(0.0);
        laid.tick();
        probe.take();
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 150.0)));
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 150.0)));
        settle(&mut laid);
        let started = probe.started();
        assert_eq!(started.len(), 2, "{device:?}: each contact is a sequence");
        assert_ne!(started[0], started[1]);
        assert_eq!(
            probe.ended(),
            vec![
                (started[0], ScrollEndReason::Released),
                (started[1], ScrollEndReason::Released),
            ],
            "{device:?}: each ends once, released"
        );
    }
}

/// A fling the inner list cannot carry goes to the middle list and on to
/// the outer one under the sequence that released it; participants hear its
/// end once, after the last run (ADR-0209 §4, C5).
pub(crate) fn fling_handoff_inherits_sequence_across_three_levels() {
    for device in DEVICES {
        let (outer, middle, inner, top, between, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Probe::new(),
            Vsync::new(),
        );
        let middle_view = NestedScroll::new(
            between.connection(),
            Scrollable::new()
                .controller(middle.clone())
                .child(Column::new(vec![
                    SizedBox::new(200.0, 800.0).boxed(),
                    SizedBox::new(200.0, 200.0).child(list(&inner)).boxed(),
                ])),
        );
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(
                top.connection(),
                Scrollable::new()
                    .controller(outer.clone())
                    .child(Column::new(vec![
                        SizedBox::new(200.0, 600.0).boxed(),
                        SizedBox::new(200.0, 200.0).child(middle_view).boxed(),
                        SizedBox::new(200.0, 4800.0).boxed(),
                    ])),
            ),
        )
        .boxed();
        let mut laid = lay_out_animated(tree, tight(200.0, 200.0), vsync);
        outer.jump_to(600.0);
        middle.jump_to(795.0);
        inner.jump_to(650.0);
        laid.tick();
        top.take();
        between.take();

        laid.replay(&fling(device, at(100.0, 150.0), at(100.0, 60.0)));
        // Where the outer list stands in the frame its sequence ended.
        let mut rest = None;
        for _ in 0..400 {
            laid.pump_for(SAMPLE);
            if rest.is_none() && !top.ended().is_empty() {
                rest = Some(outer.pixels());
            }
        }
        let rest = rest.expect("the sequence ended");
        assert_eq!(
            inner.pixels(),
            800.0,
            "{device:?}: the inner list reaches its end"
        );
        assert_eq!(
            middle.pixels(),
            800.0,
            "{device:?}: the middle list carries on to its end"
        );
        assert!(
            outer.pixels() > 600.0,
            "{device:?}: and the outer list takes the rest, at {}",
            outer.pixels()
        );
        assert_eq!(
            outer.pixels(),
            rest,
            "{device:?}: the end comes after the last run moved the outer list"
        );
        let seen = top.seen();
        let end = seen
            .iter()
            .position(|seen| matches!(seen, Seen::Ended(..)))
            .expect("the sequence ended");
        assert!(
            seen[end..].iter().all(|seen| !matches!(seen, Seen::Pre(_))),
            "{device:?}: no step follows the end"
        );
        for probe in [&top, &between] {
            let started = probe.started();
            assert_eq!(
                started.len(),
                1,
                "{device:?}: one sequence: {:?}",
                probe.seen()
            );
            assert_eq!(
                probe.ended(),
                vec![(started[0], ScrollEndReason::Released)],
                "{device:?}: ended once"
            );
            let steps = probe.steps();
            assert!(
                steps.iter().all(|step| step.sequence == started[0]),
                "{device:?}: every run's step carries the releasing sequence"
            );
            assert!(
                steps
                    .iter()
                    .any(|step| step.cause == ScrollCause::SideEffect),
                "{device:?}: the runs are side effects"
            );
        }
        assert!(
            top.steps()
                .iter()
                .any(|step| step.cause == ScrollCause::SideEffect && step.available.y > 0.0),
            "{device:?}: the outer run's own steps reach the participant above it"
        );
    }
}

/// Platform inertia is the platform's fling: a trackpad's momentum steps
/// reach participants as `Momentum` and never offer `post_fling`, where a
/// touch or mouse fling to the same edge does (ADR-0209 §4).
pub(crate) fn momentum_never_offers_post_fling() {
    for device in DEVICES {
        let (outer, inner, top, middle, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Probe::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_in_list(&outer, &inner, &top, &middle, None, &vsync),
            vsync,
        );
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        laid.replay(&fling(device, at(150.0, 150.0), at(150.0, 60.0)));
        settle(&mut laid);
        assert!(
            middle.flings() > 0,
            "{device:?}: a fling to the inner end offers its residual"
        );

        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        top.take();
        middle.take();
        let trackpad = TrackpadScript::new("pan with momentum", at(150.0, 100.0))
            .pan(Offset::new(0.0, 40.0), 4, SAMPLE)
            .momentum(Offset::new(0.0, 600.0), 12, SAMPLE)
            .build()
            .expect("a whole pan");
        laid.replay(&trackpad);
        settle(&mut laid);
        assert_eq!(
            inner.pixels(),
            800.0,
            "momentum carries the inner list to its end"
        );
        assert_eq!(
            middle.flings() + top.flings(),
            0,
            "inertia never offers post_fling"
        );
        assert!(
            top.steps()
                .iter()
                .chain(middle.steps().iter())
                .any(|step| step.cause == ScrollCause::Momentum),
            "the momentum phases reach participants as Momentum: {:?}",
            middle.seen()
        );
    }
}

/// A press on a list whose ancestor runs the list's handed-off fling stops
/// that run, and the fling's sequence ends `Preempted` (ADR-0209 §4).
pub(crate) fn new_touch_preempts_handed_off_fling() {
    for device in DEVICES {
        let (outer, inner, top, middle, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Probe::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_in_list(&outer, &inner, &top, &middle, None, &vsync),
            vsync,
        );
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        top.take();
        laid.replay(&fling(device, at(150.0, 150.0), at(150.0, 60.0)));
        let released = top.started()[0];
        for _ in 0..30 {
            if outer.pixels() > 610.0 {
                break;
            }
            laid.pump_for(SAMPLE);
        }
        assert!(
            outer.pixels() > 600.0,
            "{device:?}: the outer list runs the residual"
        );
        laid.replay(&press(device, at(150.0, 100.0)));
        let stopped = outer.pixels();
        settle(&mut laid);
        assert_eq!(
            outer.pixels(),
            stopped,
            "{device:?}: the press stopped the run"
        );
        assert!(
            top.ended()
                .contains(&(released, ScrollEndReason::Preempted)),
            "{device:?}: the fling's sequence ends preempted: {:?}",
            top.ended()
        );
    }
}

/// User input takes the offset from a programmatic animation: the
/// animation's sequence ends `Preempted` and the animation stops
/// (ADR-0209 §3).
pub(crate) fn user_input_preempts_programmatic_scroll() {
    for device in DEVICES {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(probe.connection(), list(&controller)),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        controller.animate_to(
            700.0,
            Duration::from_secs(1),
            flui_animation::ArcCurve::new(flui_animation::curves::LINEAR),
        );
        for _ in 0..12 {
            laid.pump_for(SAMPLE);
        }
        let animated = probe.started();
        assert_eq!(animated.len(), 1, "{device:?}: the animation is a sequence");
        let steps = probe.steps();
        assert!(
            !steps.is_empty()
                && steps
                    .iter()
                    .all(|step| step.cause == ScrollCause::Programmatic
                        && step.device == ScrollDevice::Programmatic),
            "{device:?}: its steps are programmatic: {steps:?} {:?}",
            probe.seen()
        );
        assert!(controller.pixels() > 0.0 && controller.pixels() < 700.0);

        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 200.0)));
        let after_drag = controller.pixels();
        settle(&mut laid);
        assert_eq!(
            controller.pixels(),
            after_drag,
            "{device:?}: the animation no longer moves the list"
        );
        assert!(
            probe
                .ended()
                .contains(&(animated[0], ScrollEndReason::Preempted)),
            "{device:?}: the animation's sequence ends preempted: {:?}",
            probe.ended()
        );
        assert!(
            probe.started().len() >= 2,
            "{device:?}: the drag is a sequence of its own"
        );
    }
}

/// A participant's answer outside what it was offered is clamped: wrong
/// signs and NaN take nothing, a larger or infinite answer takes all
/// (ADR-0209 §7).
pub(crate) fn participant_result_is_clamped() {
    for device in DEVICES {
        let moved = |answer: fn(Vec2) -> Vec2| {
            let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
            let tree = VsyncScope::new(
                vsync.clone(),
                NestedScroll::new(probe.connection(), list(&controller)),
            )
            .boxed();
            let mut laid = mounted(tree, vsync);
            controller.jump_to(300.0);
            laid.tick();
            probe.set_pre(move |step| answer(step.available));
            laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 100.0)));
            settle(&mut laid);
            assert!(
                controller.pixels().is_finite(),
                "{device:?}: the offset stays finite"
            );
            controller.pixels() - 300.0
        };
        let taken_nothing = moved(|_| Vec2::ZERO);
        assert!(
            taken_nothing > 100.0,
            "{device:?}: the baseline drag moves the list"
        );
        assert_eq!(
            moved(|offered| Vec2::ZERO - offered),
            taken_nothing,
            "{device:?}: wrong sign"
        );
        assert_eq!(
            moved(|_| Vec2::new(f64::NAN, f64::NAN)),
            taken_nothing,
            "{device:?}: NaN"
        );
        assert_eq!(
            moved(|offered| Vec2::new(offered.x * 10.0, offered.y * 10.0)),
            0.0,
            "{device:?}: larger than offered"
        );
        assert_eq!(
            moved(|_| Vec2::new(f64::INFINITY, f64::INFINITY)),
            0.0,
            "{device:?}: infinite"
        );
    }
}

/// A scroll a participant dispatches into the chain that called it is
/// refused; outside the participant the same scroll goes through
/// (ADR-0209 §7).
pub(crate) fn reentrant_dispatch_is_refused() {
    for device in DEVICES {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(probe.connection(), list(&controller)),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        let reentrant = controller.clone();
        probe.set_pre(move |_| {
            reentrant.jump_to(500.0);
            Vec2::ZERO
        });
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 100.0)));
        settle(&mut laid);
        assert!(
            controller.pixels() > 100.0 && controller.pixels() < 200.0,
            "{device:?}: only the drag moved the list, at {}",
            controller.pixels()
        );
        *probe.pre.borrow_mut() = None;
        controller.jump_to(500.0);
        assert_eq!(
            controller.pixels(),
            500.0,
            "{device:?}: outside a participant it jumps"
        );
    }
}

/// A participant that panics consumes the whole step, leaves the sequence
/// with `Cancelled`, and the first failure resumes; a second panic of the
/// same participant does not replace it; the next sequence includes it again
/// (ADR-0209 §7, ADR-0127).
pub(crate) fn participant_panic_consumes_all_and_next_sequence_recovers() {
    for device in DEVICES {
        let baseline = {
            let (controller, near, far, vsync) = (
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(panic_tree(&controller, &near, &far, &vsync), vsync);
            assert_eq!(
                replay_contained(&mut laid, &slow(device, at(150.0, 250.0), at(150.0, 100.0))),
                Vec::<String>::new()
            );
            settle(&mut laid);
            controller.pixels()
        };
        for competing in [false, true] {
            let (controller, near, far, vsync) = (
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(panic_tree(&controller, &near, &far, &vsync), vsync);
            let armed = Rc::new(Cell::new(true));
            let trigger = Rc::clone(&armed);
            near.set_pre(move |_| {
                assert!(!trigger.replace(false), "first participant failure");
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
            let sequence = far.started()[0];
            assert_eq!(
                near.ended(),
                vec![(sequence, ScrollEndReason::Cancelled)],
                "{device:?}: the panicking participant leaves the sequence"
            );
            assert_eq!(
                far.ended(),
                vec![(sequence, ScrollEndReason::Released)],
                "{device:?}: the others finish it"
            );
            let moved = controller.pixels();
            assert!(
                moved > 0.0 && moved < baseline,
                "{device:?}: the failed step moved nothing, the rest did: {moved} of {baseline}"
            );
            let near_steps = near
                .steps()
                .iter()
                .filter(|step| step.sequence == sequence)
                .count();
            assert_eq!(near_steps, 1, "{device:?}: a closed link is skipped");

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
            assert_eq!(controller.pixels(), baseline, "{device:?}: and moves fully");
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

fn panic_tree(
    controller: &ScrollController,
    near: &Rc<Probe>,
    far: &Rc<Probe>,
    vsync: &Vsync,
) -> BoxedView {
    VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(
            far.connection(),
            NestedScroll::new(near.connection(), list(controller)),
        ),
    )
    .boxed()
}

/// Anchor correction keeps content still while layout measures items above
/// it: the viewport applies it inside layout, so it is no step, starts or
/// ends no sequence, and the offset moves by more than the steps offered
/// (ADR-0209 §3, ADR-0051).
pub(crate) fn anchor_correction_bypasses_the_channel() {
    for device in DEVICES {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        // Item 98 sits in the cache band above the viewport; it grows by 200
        // in the middle of the drag.
        let grown = Rc::new(Cell::new(100.0));
        let grow = Rc::new(flui_foundation::ChangeNotifier::new());
        let (height, listenable) = (
            Rc::clone(&grown),
            Rc::clone(&grow) as Rc<dyn flui_foundation::Listenable>,
        );
        let list_view = Scrollable::new()
            .controller(controller.clone())
            .viewport_builder(Rc::new(move |position| {
                let (height, listenable) = (Rc::clone(&height), Rc::clone(&listenable));
                ListView::builder(200, 100.0, move |index| {
                    (index < 200).then(|| {
                        if index == 98 {
                            let height = Rc::clone(&height);
                            flui_widgets::AnimatedBuilder::new(Rc::clone(&listenable), move || {
                                SizedBox::new(300.0, height.get())
                            })
                            .boxed()
                        } else {
                            SizedBox::new(300.0, 100.0).boxed()
                        }
                    })
                })
                .position(position)
                .boxed()
            }));
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(probe.connection(), list_view),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        controller.jump_to(10_000.0);
        for _ in 0..4 {
            laid.tick();
        }
        probe.take();
        let before = controller.pixels();
        laid.replay_with(
            &slow(device, at(150.0, 40.0), at(150.0, 160.0)),
            |_, index| {
                if index == 5 {
                    grown.set(300.0);
                    grow.notify_listeners();
                }
            },
        );
        settle(&mut laid);
        let offered: f64 = probe.steps().iter().map(|step| step.available.y).sum();
        let moved = controller.pixels() - before;
        assert!(
            offered < -100.0,
            "{device:?}: the drag offered its motion: {offered}"
        );
        assert!(
            (moved - offered).abs() > 1.0,
            "{device:?}: measuring the items above corrected the offset outside the \
             channel: moved {moved}, offered {offered}"
        );
        let started = probe.started();
        assert_eq!(started.len(), 1, "{device:?}: one sequence, the drag's");
        assert!(
            probe
                .steps()
                .iter()
                .all(|step| step.cause == ScrollCause::UserInput && step.sequence == started[0]),
            "{device:?}: no step stands for the correction"
        );
        assert_eq!(
            probe.ended(),
            vec![(started[0], ScrollEndReason::Released)],
            "{device:?}: the correction neither ended nor split the sequence"
        );
    }
}

/// A release on a snapping scrollable settles on the policy's target: the
/// nearest page from rest, the next one past half a page or after a fling,
/// an extent for a target that is not finite (ADR-0209 §9).
pub(crate) fn snap_policy_targets() {
    #[derive(Debug)]
    struct Broken(f64);
    impl SnapPolicy for Broken {
        fn target(&self, _metrics: &ScrollMetrics, _velocity: f64) -> f64 {
            self.0
        }
    }
    for device in DEVICES {
        let rests = |policy: Arc<dyn SnapPolicy>, script: PointerScript| {
            let (controller, vsync) = (ScrollController::new(), Vsync::new());
            let pages = Row::new(vec![
                SizedBox::new(300.0, 300.0).boxed(),
                SizedBox::new(300.0, 300.0).boxed(),
                SizedBox::new(300.0, 300.0).boxed(),
            ]);
            let tree = VsyncScope::new(
                vsync.clone(),
                Scrollable::new()
                    .controller(controller.clone())
                    .scroll_direction(Axis::Horizontal)
                    .snap_policy(policy)
                    .child(pages),
            )
            .boxed();
            let mut laid = mounted(tree, vsync);
            laid.replay(&script);
            settle(&mut laid);
            controller.pixels()
        };
        let pages = || Arc::new(PageSnap::new(1.0)) as Arc<dyn SnapPolicy>;
        let near = |pixels: f64, target: f64| (pixels - target).abs() < 1.0;
        let short = rests(pages(), slow(device, at(250.0, 150.0), at(150.0, 150.0)));
        assert!(
            near(short, 0.0),
            "{device:?}: a short drag returns to its page, at {short}"
        );
        let long = rests(pages(), slow(device, at(280.0, 150.0), at(80.0, 150.0)));
        assert!(
            near(long, 300.0),
            "{device:?}: past half a page, the next one, at {long}"
        );
        let flung = rests(pages(), fling(device, at(250.0, 150.0), at(190.0, 150.0)));
        assert!(
            near(flung, 300.0),
            "{device:?}: a fling commits to the next page, at {flung}"
        );
        let broken = rests(
            Arc::new(Broken(f64::NAN)),
            slow(device, at(280.0, 150.0), at(80.0, 150.0)),
        );
        assert!(
            near(broken, 0.0),
            "{device:?}: NaN rests on an extent, at {broken}"
        );
        let beyond = rests(
            Arc::new(Broken(f64::INFINITY)),
            slow(device, at(280.0, 150.0), at(80.0, 150.0)),
        );
        assert!(
            near(beyond, 600.0),
            "{device:?}: infinity rests on the far extent, at {beyond}"
        );
    }
}

/// The step, fling, cause, device, end-reason, hand-off, overscroll and snap
/// types are `Send` data a future compositor-side scroller can carry; a
/// connection is not required to be (ADR-0209 §10).
pub(crate) fn scroll_channel_data_is_send() {
    fn send_sync<T: Send + Sync>() {}
    send_sync::<ScrollStep>();
    send_sync::<FlingStep>();
    send_sync::<ScrollCause>();
    send_sync::<ScrollDevice>();
    send_sync::<ScrollEndReason>();
    send_sync::<FlingHandoff>();
    send_sync::<OverscrollBehavior>();
    send_sync::<PageSnap>();
    send_sync::<Arc<dyn SnapPolicy>>();
}

/// `Contain` keeps a scrollable's remainder and fling residual: nothing
/// above it is offered either (ADR-0209 §6).
pub(crate) fn contained_scrollable_keeps_its_remainder() {
    for device in DEVICES {
        let (outer, inner, top, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Vsync::new(),
        );
        top.takes_remainder.set(true);
        let content = Column::new(vec![
            SizedBox::new(300.0, 600.0).boxed(),
            SizedBox::new(300.0, 200.0)
                .child(list(&inner).overscroll_behavior(OverscrollBehavior::Contain))
                .boxed(),
            SizedBox::new(300.0, 4800.0).boxed(),
        ]);
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(
                top.connection(),
                Scrollable::new().controller(outer.clone()).child(content),
            ),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        outer.jump_to(600.0);
        inner.jump_to(650.0);
        laid.tick();
        top.take();
        laid.replay(&fling(device, at(150.0, 190.0), at(150.0, 40.0)));
        settle(&mut laid);
        assert_eq!(
            inner.pixels(),
            800.0,
            "{device:?}: the inner list reaches its end"
        );
        assert_eq!(outer.pixels(), 600.0, "{device:?}: nothing goes past it");
        assert!(
            !top.seen()
                .iter()
                .any(|seen| matches!(seen, Seen::Post(_) | Seen::Fling(_))),
            "{device:?}: the participant above is never offered a remainder: {:?}",
            top.seen()
        );
        assert!(
            !top.steps().is_empty(),
            "{device:?}: it still sees the steps first"
        );
    }
}

/// A reveal is a programmatic step and an assistive scroll action a user
/// step from the assistive device: both go through the chain like any other
/// offset change (ADR-0209 §3, C8).
pub(crate) fn reveal_and_assistive_scroll_go_through_the_chain() {
    use flui_testing::a11y::{Action, ActionRequest, Role, TreeId};
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let target = flui_widgets::Semantics::new()
        .container(true)
        .role(flui_widgets::SemanticsRole::Button)
        .label("reveal target")
        .child(SizedBox::new(40.0, 40.0));
    let content = Column::new(vec![
        SizedBox::new(200.0, 600.0).boxed(),
        target.boxed(),
        SizedBox::new(200.0, 600.0).boxed(),
    ]);
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(
            probe.connection(),
            Scrollable::new()
                .controller(controller.clone())
                .child(content),
        ),
    )
    .boxed();
    let mut laid = lay_out_animated(tree, tight(200.0, 200.0), vsync);
    laid.enable_semantics();
    laid.tick();
    let request = |laid: &LaidOut, action: Action, node| {
        laid.invoke_semantics_action(ActionRequest {
            action,
            target_tree: TreeId::ROOT,
            target_node: node,
            data: None,
        })
        .expect("the request resolves");
    };
    let target = laid
        .a11y_tree()
        .expect("published semantics")
        .find_by_label("reveal target")
        .expect("the offscreen target is addressable")
        .id();
    request(&laid, Action::ScrollIntoView, target);
    settle(&mut laid);
    assert!(
        controller.pixels() > 400.0,
        "the reveal scrolled to the target"
    );
    let steps = probe.steps();
    assert!(
        !steps.is_empty()
            && steps
                .iter()
                .all(|step| step.cause == ScrollCause::Programmatic
                    && step.device == ScrollDevice::Programmatic),
        "a reveal offers programmatic steps: {steps:?}"
    );
    let revealed = probe.started();
    assert_eq!(
        probe.ended(),
        vec![(revealed[0], ScrollEndReason::Released)],
        "the reveal is one sequence"
    );
    probe.take();

    let before = controller.pixels();
    let scroll_view = laid
        .a11y_tree()
        .expect("republished semantics")
        .find(Role::ScrollView)
        .expect("the scroll view is published")
        .id();
    request(&laid, Action::ScrollUp, scroll_view);
    settle(&mut laid);
    assert!(
        controller.pixels() < before,
        "the assistive action scrolled"
    );
    let steps = probe.steps();
    assert!(
        !steps.is_empty()
            && steps.iter().all(|step| step.cause == ScrollCause::UserInput
                && step.device == ScrollDevice::Assistive),
        "an assistive action offers user steps from the assistive device: {steps:?}"
    );
}

/// A connection takes nothing it did not opt into: every default answer is
/// zero or `Declined`, for every cause, `Programmatic` included
/// (ADR-0209 §2).
pub(crate) fn connection_defaults_take_nothing() {
    struct Silent;
    impl NestedScrollConnection for Silent {}
    let sequence = flui_foundation::IdMint::<InputSequenceId>::new()
        .next()
        .expect("a fresh mint issues");
    let silent = Silent;
    let devices = [
        ScrollDevice::Pointer {
            kind: flui_platform_api::pointer::PointerKind::Touch,
            device: None,
        },
        ScrollDevice::Keyboard,
        ScrollDevice::Assistive,
        ScrollDevice::Programmatic,
    ];
    for cause in [
        ScrollCause::UserInput,
        ScrollCause::Momentum,
        ScrollCause::SideEffect,
        ScrollCause::Programmatic,
    ] {
        for device in devices {
            let step = ScrollStep::for_test(sequence, cause, device, Vec2::new(3.0, -4.0));
            assert_eq!(silent.pre_scroll(&step), Vec2::ZERO, "{cause:?}/{device:?}");
            assert_eq!(
                silent.post_scroll(&step, Vec2::new(1.0, 1.0)),
                Vec2::ZERO,
                "{cause:?}/{device:?}"
            );
            let fling = FlingStep::for_test(
                sequence,
                device,
                flui_interaction::Velocity::from_components(100.0, -50.0),
            );
            assert_eq!(silent.post_fling(&fling), FlingHandoff::Declined);
        }
    }
}
