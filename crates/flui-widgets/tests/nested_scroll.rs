//! The nested scroll channel (ADR-0209): rows of the `nested_scroll_channel`
//! table in `contracts.rs`, each driven by touch and mouse scripts.

use std::cell::{Cell, RefCell};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::Vsync;
use flui_foundation::geometry::{Axis, Offset, Point, Vec2};
use flui_interaction::{
    GestureProfiles, GestureSettings, GestureSettingsSource, GestureValues, InputSequenceId,
    RemainderHandoff,
};
use flui_platform::{EventTime, TargetPlatform};
use flui_testing::{MouseScript, PointerScript, ScriptStep, TouchScript, TrackpadScript};
use flui_view::{BoxedView, ViewExt};
use flui_widgets::{
    AnchorSnap, AnchorSnapError, Column, FlingHandoff, FlingStep, ListView, NestedScroll,
    NestedScrollConnection, OverscrollBehavior, PageSnap, Row, ScrollCause, ScrollController,
    ScrollDevice, ScrollEndReason, ScrollMetrics, ScrollStep, Scrollable, SizedBox, SnapPolicy,
    Transform, ViewportFraction, VsyncScope,
};

use crate::common::{LaidOut, lay_out_animated, tight};

pub(crate) mod caught;
pub(crate) mod requests;

// ---------------------------------------------------------------------------
// A participant that records what reaches it
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Seen {
    Started(InputSequenceId),
    Pre(ScrollStep),
    Post(ScrollStep),
    Released(FlingStep),
    Fling(FlingStep),
    Ended(InputSequenceId, ScrollEndReason),
}

type PreHook = Box<dyn Fn(&ScrollStep) -> Vec2>;
type PostHook = Box<dyn Fn(&ScrollStep) -> Vec2>;
type EndHook = Box<dyn Fn()>;
type Observer = Box<dyn Fn(&Seen)>;

/// Records every call; takes what its hooks say. Its state is `Rc` and
/// `Cell`, so it also proves a connection may capture both.
#[derive(Default)]
pub(crate) struct Probe {
    seen: RefCell<Vec<Seen>>,
    pre: RefCell<Option<PreHook>>,
    /// Answers `post_scroll` in place of `takes_remainder`.
    post: RefCell<Option<PostHook>>,
    on_end: RefCell<Option<EndHook>>,
    /// Told of every call once it is recorded, from inside the callout.
    observe: RefCell<Option<Observer>>,
    takes_remainder: Cell<bool>,
    /// Takes no part in programmatic sequences; a probe accepts them unless
    /// told otherwise, so it records every sequence of its chain.
    declines_programmatic: Cell<bool>,
}

impl Probe {
    pub(crate) fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// A probe that leaves programmatic sequences to the rest of the chain.
    pub(crate) fn declining_programmatic() -> Rc<Self> {
        let probe = Self::new();
        probe.declines_programmatic.set(true);
        probe
    }

    fn record(&self, seen: Seen) {
        self.seen.borrow_mut().push(seen.clone());
        let observer = self.observe.borrow();
        if let Some(observer) = observer.as_ref() {
            observer(&seen);
        }
    }

    pub(crate) fn set_observe(&self, observer: impl Fn(&Seen) + 'static) {
        *self.observe.borrow_mut() = Some(Box::new(observer));
    }

    pub(crate) fn connection(self: &Rc<Self>) -> Rc<dyn NestedScrollConnection> {
        Rc::clone(self) as Rc<dyn NestedScrollConnection>
    }

    fn set_pre(&self, hook: impl Fn(&ScrollStep) -> Vec2 + 'static) {
        *self.pre.borrow_mut() = Some(Box::new(hook));
    }

    fn set_post(&self, hook: impl Fn(&ScrollStep) -> Vec2 + 'static) {
        *self.post.borrow_mut() = Some(Box::new(hook));
    }

    /// The remainders offered to `post_scroll`.
    fn posts(&self) -> Vec<ScrollStep> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Post(step) => Some(step),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn take(&self) -> Vec<Seen> {
        self.seen.take()
    }

    pub(crate) fn seen(&self) -> Vec<Seen> {
        self.seen.borrow().clone()
    }

    pub(crate) fn started(&self) -> Vec<InputSequenceId> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Started(id) => Some(id),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn ended(&self) -> Vec<(InputSequenceId, ScrollEndReason)> {
        self.seen()
            .into_iter()
            .filter_map(|seen| match seen {
                Seen::Ended(id, reason) => Some((id, reason)),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn steps(&self) -> Vec<ScrollStep> {
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
    fn accepts_programmatic(&self) -> bool {
        !self.declines_programmatic.get()
    }

    fn sequence_started(&self, sequence: InputSequenceId) {
        self.record(Seen::Started(sequence));
    }

    fn pre_scroll(&self, step: &ScrollStep) -> Vec2 {
        self.record(Seen::Pre(*step));
        let hook = self.pre.borrow();
        hook.as_ref().map_or(Vec2::ZERO, |hook| hook(step))
    }

    fn post_scroll(&self, step: &ScrollStep, _consumed_by_child: Vec2) -> Vec2 {
        self.record(Seen::Post(*step));
        if let Some(hook) = self.post.borrow().as_ref() {
            return hook(step);
        }
        if self.takes_remainder.get() {
            step.available
        } else {
            Vec2::ZERO
        }
    }

    fn released(&self, step: &FlingStep) {
        self.record(Seen::Released(*step));
    }

    fn post_fling(&self, step: &FlingStep) -> FlingHandoff {
        self.record(Seen::Fling(*step));
        FlingHandoff::Declined
    }

    fn sequence_ended(&self, sequence: InputSequenceId, reason: ScrollEndReason) {
        self.record(Seen::Ended(sequence, reason));
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
pub(crate) enum Device {
    Touch,
    Mouse,
}

pub(crate) const DEVICES: [Device; 2] = [Device::Touch, Device::Mouse];

const SAMPLE: Duration = Duration::from_millis(16);

pub(crate) fn at(x: f64, y: f64) -> Offset<f64> {
    Offset::new(x, y)
}

/// `steps` evenly spaced points after `from`, ending at `to`.
pub(crate) fn line(from: Offset<f64>, to: Offset<f64>, steps: u32) -> Vec<Offset<f64>> {
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
pub(crate) fn drag(
    device: Device,
    points: &[Offset<f64>],
    sample: Duration,
    hold: Duration,
) -> PointerScript {
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
pub(crate) fn slow(device: Device, from: Offset<f64>, to: Offset<f64>) -> PointerScript {
    drag(
        device,
        &line(from, to, 10),
        SAMPLE,
        Duration::from_millis(300),
    )
}

/// A fast drag that lifts while moving.
pub(crate) fn fling(device: Device, from: Offset<f64>, to: Offset<f64>) -> PointerScript {
    drag(
        device,
        &line(from, to, 6),
        Duration::from_millis(8),
        Duration::ZERO,
    )
}

/// A press that lifts where it went down.
pub(crate) fn press(device: Device, point: Offset<f64>) -> PointerScript {
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
pub(crate) fn replay_contained(laid: &mut LaidOut, script: &PointerScript) -> Vec<String> {
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

pub(crate) fn settle(laid: &mut LaidOut) {
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

pub(crate) fn mounted(view: BoxedView, vsync: Vsync) -> LaidOut {
    lay_out_animated(view, tight(300.0, 300.0), vsync)
}

pub(crate) fn remainder_preset(handoff: RemainderHandoff) -> GestureProfiles {
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

/// Every step names where its input points and the presentation's pixel
/// ratio (ADR-0209 §1): a contact's steps follow the finger, a fling's keep
/// the release's point and ratio, a wheel's point at the pointer, and a
/// programmatic step points at the middle of its leaf.
pub(crate) fn steps_carry_the_focal_point_and_the_device_pixel_ratio() {
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(&controller)),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    laid.set_scale_factor(2.0);
    probe.take();
    let by_cause = |cause: ScrollCause| {
        probe
            .steps()
            .into_iter()
            .filter(move |step| step.cause == cause)
            .collect::<Vec<_>>()
    };

    laid.replay(&fling(Device::Touch, at(150.0, 250.0), at(150.0, 160.0)));
    settle(&mut laid);
    let contact = by_cause(ScrollCause::UserInput);
    assert!(!contact.is_empty(), "the drag offered steps");
    for step in &contact {
        assert_eq!(step.focal_point.x, 150.0, "{step:?}");
        assert!((160.0..=250.0).contains(&step.focal_point.y), "{step:?}");
        assert_eq!(step.device_pixel_ratio, 2.0, "{step:?}");
    }
    let lifted = contact.last().expect("a last drag step").focal_point;
    assert_eq!(
        lifted,
        Point::new(150.0, 160.0),
        "the last step is where the finger lifted"
    );
    let run = by_cause(ScrollCause::SideEffect);
    assert!(!run.is_empty(), "the fling ran");
    assert!(
        run.iter()
            .all(|step| step.focal_point == lifted && step.device_pixel_ratio == 2.0),
        "the fling keeps the release's point and ratio: {run:?}"
    );
    let released = probe
        .seen()
        .into_iter()
        .find_map(|seen| match seen {
            Seen::Released(step) => Some(step),
            _ => None,
        })
        .expect("the contact was released");
    assert_eq!(released.focal_point, lifted);
    assert_eq!(released.device_pixel_ratio, 2.0);

    controller.jump_to(0.0).expect("no build is running");
    laid.tick();
    probe.take();
    laid.replay(
        &MouseScript::new("notch")
            .move_to(at(120.0, 80.0))
            .wheel(1.0)
            .build()
            .expect("a hovering mouse"),
    );
    settle(&mut laid);
    let wheel = by_cause(ScrollCause::UserInput);
    assert!(!wheel.is_empty(), "the notch offered steps");
    assert!(
        wheel
            .iter()
            .all(|step| step.focal_point == Point::new(120.0, 80.0)),
        "a wheel step points at the pointer: {wheel:?}"
    );

    probe.take();
    controller.jump_to(300.0).expect("no build is running");
    laid.tick();
    let jump = by_cause(ScrollCause::Programmatic);
    assert_eq!(jump.len(), 1, "the jump is one step");
    assert_eq!(
        jump[0].focal_point,
        Point::new(150.0, 150.0),
        "a step without a position points at the middle of the leaf"
    );
    assert_eq!(jump[0].device_pixel_ratio, 2.0);
}

/// Each participant is offered a step in its own plane (ADR-0209 §1): above
/// and below a scaling or a rotating `Transform`, the participants see one
/// motion on screen in their own coordinates — its delta, its focal point
/// and its release velocity — and the list moves by what reaches it in its
/// own. A plane that cannot be crossed offers its participants nothing and
/// publishes nothing that is not finite.
pub(crate) fn each_participant_is_offered_the_step_in_its_own_plane() {
    let mount = |transform: Transform, controller: &ScrollController| {
        let (outer, inner, vsync) = (Probe::new(), Probe::new(), Vsync::new());
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(
                outer.connection(),
                transform.child(NestedScroll::new(inner.connection(), list(controller))),
            ),
        )
        .boxed();
        (mounted(tree, vsync), outer, inner)
    };
    let pre = |probe: &Probe| {
        probe
            .steps()
            .into_iter()
            .filter(|step| step.cause == ScrollCause::UserInput)
            .collect::<Vec<_>>()
    };
    let released = |probe: &Probe| {
        probe
            .seen()
            .into_iter()
            .find_map(|seen| match seen {
                Seen::Released(step) => Some(step.available.pixels_per_second),
                _ => None,
            })
            .expect("the contact was released")
    };
    let close = |a: f64, b: f64| (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0);

    // Twice the size about the centre: the screen's motion is twice the
    // list's.
    let controller = ScrollController::new();
    let (mut laid, outer, inner) = mount(Transform::scale(2.0, 2.0), &controller);
    laid.replay(&fling(Device::Touch, at(150.0, 280.0), at(150.0, 160.0)));
    let (above, below) = (pre(&outer), pre(&inner));
    assert!(!below.is_empty(), "the drag reached the list");
    assert_eq!(above.len(), below.len(), "one step each per dispatch");
    for (above, below) in above.iter().zip(&below) {
        assert_eq!(
            above.available,
            below.available * 2.0,
            "{above:?} {below:?}"
        );
        assert_eq!(
            above.focal_point,
            Point::new(
                150.0 + 2.0 * (below.focal_point.x - 150.0),
                150.0 + 2.0 * (below.focal_point.y - 150.0),
            ),
            "the focal point is the same point on screen"
        );
    }
    let moved: f64 = below.iter().map(|step| step.available.y).sum();
    assert_eq!(
        controller.pixels(),
        moved,
        "the list moved by its own steps"
    );
    let (above, below) = (released(&outer), released(&inner));
    assert!(below.dy > 0.0, "the release flings the list on");
    assert_eq!(above.dy, below.dy * 2.0, "a velocity crosses the plane too");

    // A quarter turn: a horizontal swipe on screen scrolls the vertical
    // list, and the participant above sees it horizontal.
    let controller = ScrollController::new();
    let (mut laid, outer, inner) = mount(
        Transform::rotation(std::f64::consts::FRAC_PI_2),
        &controller,
    );
    laid.replay(&slow(Device::Touch, at(100.0, 150.0), at(200.0, 150.0)));
    let (above, below) = (pre(&outer), pre(&inner));
    assert!(!below.is_empty(), "the swipe reached the list");
    assert_eq!(above.len(), below.len());
    for (above, below) in above.iter().zip(&below) {
        assert_eq!(
            below.available.x, 0.0,
            "the list sees its own axis: {below:?}"
        );
        assert_eq!(above.available.y, 0.0, "the screen sees a swipe: {above:?}");
        assert!(
            close(above.available.x, -below.available.y),
            "{above:?} {below:?}"
        );
        // Local (x, y) shows at (150 - (y - 150), 150 + (x - 150)).
        assert!(close(above.focal_point.x, 300.0 - below.focal_point.y));
        assert!(close(above.focal_point.y, below.focal_point.x));
    }
    let moved: f64 = below.iter().map(|step| step.available.y).sum();
    assert!(moved > 0.0, "the swipe scrolled the list");
    assert!(
        close(controller.pixels(), moved),
        "the list moved by its own steps"
    );

    // Squashed flat: the leaf's plane cannot be crossed, so a jump moves the
    // list alone and the participants are offered nothing.
    let controller = ScrollController::new();
    let (mut laid, outer, inner) = mount(Transform::scale(0.0, 1.0), &controller);
    outer.take();
    inner.take();
    controller.jump_to(100.0).expect("no build is running");
    laid.tick();
    assert_eq!(controller.pixels(), 100.0, "the jump moves the list");
    assert!(
        outer.steps().is_empty() && inner.steps().is_empty(),
        "no participant is offered a step it cannot see: {:?} {:?}",
        outer.seen(),
        inner.seen()
    );
    assert_eq!(outer.ended().len(), 1, "the jump is still a sequence");
}

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

        pager.jump_to(0.0).expect("no build is running");
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
            outer.jump_to(600.0).expect("no build is running");
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
            outer.jump_to(600.0).expect("no build is running");
            inner.jump_to(700.0).expect("no build is running");
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
        controller.jump_to(0.0).expect("no build is running");
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

/// A wheel sequence that no further packet follows ends at its latch's
/// idle deadline, with no input to end it (ADR-0209 §5): a packet before
/// the deadline moves it on, every participant hears the end exactly once,
/// a panic in one of them or the leaf's removal neither loses nor repeats
/// it, the next packet starts a new sequence, and a phased trackpad pan
/// ends with its end phase or, left open, at the longer phased idle time.
/// A contact held still past both idle times ends only when it lifts.
pub(crate) fn an_idle_wheel_sequence_ends_at_its_latch_deadline() {
    let idle = GestureSettings::default().contactless_latch_idle();
    let wheel = || {
        MouseScript::new("notch")
            .move_to(at(150.0, 150.0))
            .wheel(1.0)
            .build()
            .expect("a hovering mouse")
    };
    let (controller, outer, inner, vsync) = (
        ScrollController::new(),
        Probe::new(),
        Probe::new(),
        Vsync::new(),
    );
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(
            outer.connection(),
            NestedScroll::new(inner.connection(), list(&controller)),
        ),
    )
    .boxed();
    let mut laid = mounted(tree, vsync.clone());
    let pump = |laid: &mut LaidOut, mut span: Duration| {
        while span > Duration::ZERO {
            let frame = span.min(SAMPLE);
            laid.pump_for(frame);
            span -= frame;
        }
    };

    laid.replay(&wheel());
    let first = outer.started();
    assert_eq!(first.len(), 1, "the notch starts a sequence");
    pump(&mut laid, idle / 2);
    laid.replay(&wheel());
    // Past the first notch's deadline, short of the second's.
    pump(&mut laid, idle.saturating_sub(Duration::from_millis(1)));
    assert_eq!(
        outer.started(),
        first,
        "the second notch is a step of the same sequence"
    );
    assert_eq!(
        outer.ended(),
        Vec::new(),
        "a notch before the deadline moves it on"
    );
    assert!(controller.pixels() > 0.0, "both notches scrolled the list");
    pump(&mut laid, Duration::from_millis(2));
    assert_eq!(
        outer.ended(),
        vec![(first[0], ScrollEndReason::Released)],
        "the sequence ends at the deadline, with no input"
    );
    assert_eq!(inner.ended(), outer.ended(), "every participant hears it");
    pump(&mut laid, idle * 2);
    assert_eq!(outer.ended().len(), 1, "it ends once");

    laid.replay(&wheel());
    let started = outer.started();
    assert_eq!(started.len(), 2, "the next notch starts a new sequence");
    assert_ne!(started[0], started[1]);
    pump(&mut laid, idle + SAMPLE);
    assert_eq!(
        outer.ended(),
        vec![
            (started[0], ScrollEndReason::Released),
            (started[1], ScrollEndReason::Released),
        ],
        "a repeated sequence ends at its own deadline"
    );

    // A participant that panics at the end leaves the others' end and
    // gets none twice.
    outer.take();
    inner.take();
    let failed = Rc::new(Cell::new(false));
    {
        let failed = Rc::clone(&failed);
        *inner.on_end.borrow_mut() = Some(Box::new(move || {
            failed.set(true);
            panic!("a participant fails at the end");
        }));
    }
    laid.replay(&wheel());
    // The frame driver contains the failure or resumes it; either way the
    // end was delivered.
    let _ = catch_unwind(AssertUnwindSafe(|| pump(&mut laid, idle + SAMPLE)));
    assert!(failed.get(), "the participant failed at the end");
    inner.on_end.borrow_mut().take();
    pump(&mut laid, idle * 2);
    let id = outer.started()[0];
    assert_eq!(outer.ended(), vec![(id, ScrollEndReason::Released)]);
    assert_eq!(inner.ended(), vec![(id, ScrollEndReason::Released)]);

    // A leaf removed before the deadline ends its sequence once, cancelled;
    // the deadline adds nothing.
    outer.take();
    laid.replay(&wheel());
    let id = outer.started()[0];
    laid.pump_widget(VsyncScope::new(
        vsync,
        NestedScroll::new(
            outer.connection(),
            NestedScroll::new(inner.connection(), SizedBox::new(300.0, 300.0)),
        ),
    ));
    laid.tick();
    pump(&mut laid, idle * 2);
    assert_eq!(outer.ended(), vec![(id, ScrollEndReason::Cancelled)]);

    // A phased pan ends with its end phase, not at the longer phased idle.
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(&controller)),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    laid.replay(
        &TrackpadScript::new("pan", at(150.0, 150.0))
            .pan(Offset::new(0.0, 40.0), 4, SAMPLE)
            .build()
            .expect("a whole pan"),
    );
    let pan = probe.started();
    assert_eq!(pan.len(), 1);
    assert_eq!(
        probe.ended(),
        vec![(pan[0], ScrollEndReason::Released)],
        "the end phase ends the pan's sequence"
    );

    // A phased pan whose end phase never comes ends at the longer phased
    // idle time, not at the wheel's.
    let phased = GestureSettings::default().phased_latch_idle();
    assert!(phased > idle, "the phased idle time is the longer one");
    probe.take();
    laid.replay(
        &TrackpadScript::new("pan without its end phase", at(150.0, 150.0))
            .unended_pan(Offset::new(0.0, 40.0), 4, SAMPLE)
            .build()
            .expect("a pan left open"),
    );
    let open = probe.started();
    assert_eq!(open.len(), 1, "the open pan is a sequence");
    pump(&mut laid, idle + SAMPLE);
    assert_eq!(
        probe.ended(),
        Vec::new(),
        "past the wheel's idle time the open pan is still live"
    );
    pump(&mut laid, phased.saturating_sub(idle));
    assert_eq!(
        probe.ended(),
        vec![(open[0], ScrollEndReason::Released)],
        "it ends at the phased idle time, with no input"
    );

    // A contact held still past both idle times stays one sequence: only
    // its lift ends it.
    for device in DEVICES {
        probe.take();
        let mut points = line(at(150.0, 250.0), at(150.0, 200.0), 4);
        let rest = *points.last().expect("a drag has an end");
        points.push(rest);
        laid.replay(&drag(device, &points, SAMPLE, phased * 2));
        settle(&mut laid);
        let contact = probe.started();
        assert_eq!(contact.len(), 1, "{device:?}: the contact is one sequence");
        assert_eq!(
            probe.ended(),
            vec![(contact[0], ScrollEndReason::Released)],
            "{device:?}: it ends once"
        );
        let seen = probe.seen();
        let lifted = seen
            .iter()
            .position(|seen| matches!(seen, Seen::Released(_)));
        let ended = seen.iter().position(|seen| matches!(seen, Seen::Ended(..)));
        assert!(
            lifted.is_some() && lifted < ended,
            "{device:?}: at its lift, not at an idle time while held: {seen:?}"
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
        outer.jump_to(600.0).expect("no build is running");
        middle.jump_to(795.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
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
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        laid.replay(&fling(device, at(150.0, 150.0), at(150.0, 60.0)));
        settle(&mut laid);
        assert!(
            middle.flings() > 0,
            "{device:?}: a fling to the inner end offers its residual"
        );

        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        top.take();
        middle.take();
        let trackpad = TrackpadScript::new("pan with momentum", at(150.0, 100.0))
            .pan(Offset::new(0.0, 40.0), 4, SAMPLE)
            .momentum(Offset::new(0.0, 600.0), 12, SAMPLE)
            .build()
            .expect("a whole pan");
        // Momentum offers split by where the inner list was when the
        // participant above it was asked: with room left, or at its end.
        let offered = Rc::new(Cell::new((0.0, 0_u32)));
        {
            let (offered, inner) = (Rc::clone(&offered), inner.clone());
            middle.set_observe(move |seen| {
                if let Seen::Pre(step) = seen
                    && step.cause == ScrollCause::Momentum
                {
                    let (moving, at_end) = offered.get();
                    offered.set(if inner.pixels() < 800.0 {
                        (moving + step.available.y, at_end)
                    } else {
                        (moving, at_end + 1)
                    });
                }
            });
        }
        laid.replay(&trackpad);
        settle(&mut laid);
        assert_eq!(
            inner.pixels(),
            800.0,
            "momentum carries the inner list to its end"
        );
        // The fling above is the control that makes "never" mean something:
        // the same probe records the offer when a fling makes one. Wheel
        // packets are clamped to the extent, so momentum offers the list only
        // what it has left; at its end the list still offers each packet to
        // the participant above it, which is asked before the list.
        let (moving, at_end) = offered.get();
        assert_eq!(
            moving,
            800.0 - 690.0,
            "{device:?}: momentum ran the inner list from where the pan left it to its end"
        );
        assert!(
            at_end > 0,
            "{device:?}: the participant above the list at its end still had its turn"
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
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
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

/// New input on the list whose fling an ancestor carries on stops that run
/// even when the list takes the input whole and nothing reaches the
/// ancestor: a wheel notch or an assistive scroll back the other way. The
/// fling's sequence ends `Preempted` before the new one starts (ADR-0209 §4,
/// C5). A key needs no row of its own here: the first key after the
/// fling's pointer contact shows the focus, whose reveal already stops the
/// ancestor.
pub(crate) fn new_wheel_or_assistive_scroll_preempts_handed_off_fling() {
    use flui_testing::a11y::{Action, ActionRequest, Role, TreeId};
    #[derive(Debug, Clone, Copy)]
    enum Input {
        Wheel,
        Assistive,
    }
    for input in [Input::Wheel, Input::Assistive] {
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
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.enable_semantics();
        laid.tick();
        top.take();
        laid.replay(&fling(Device::Mouse, at(150.0, 150.0), at(150.0, 60.0)));
        let released = top.started()[0];
        for _ in 0..30 {
            if outer.pixels() > 610.0 {
                break;
            }
            laid.pump_for(SAMPLE);
        }
        assert!(
            outer.pixels() > 600.0,
            "{input:?}: the outer list runs the residual"
        );
        let at_end = inner.pixels();
        match input {
            Input::Wheel => laid.replay(
                &MouseScript::new("wheel up")
                    .move_to(at(150.0, 100.0))
                    .wheel(-1.0)
                    .build()
                    .expect("a hovering mouse"),
            ),
            Input::Assistive => {
                let inner_view = laid
                    .a11y_tree()
                    .expect("published semantics")
                    .find_all(Role::ScrollView)
                    .get(1)
                    .expect("the inner scroll view follows the outer one")
                    .id();
                laid.invoke_semantics_action(ActionRequest {
                    action: Action::ScrollUp,
                    target_tree: TreeId::ROOT,
                    target_node: inner_view,
                    data: None,
                })
                .expect("the request resolves");
            }
        }
        let stopped = outer.pixels();
        settle(&mut laid);
        assert!(
            inner.pixels() < at_end,
            "{input:?}: the inner list takes the input back up"
        );
        assert_eq!(
            outer.pixels(),
            stopped,
            "{input:?}: the new input stopped the handed-off run"
        );
        let seen = top.seen();
        let ended = seen
            .iter()
            .position(|seen| *seen == Seen::Ended(released, ScrollEndReason::Preempted));
        let next = seen
            .iter()
            .position(|seen| matches!(seen, Seen::Started(id) if *id != released));
        assert!(
            ended.is_some() && ended < next,
            "{input:?}: the fling's sequence ends preempted before the next starts: {seen:?}"
        );
    }
}

/// A hard-edged physics whose run glides to `short_by` pixels before the
/// edge it reports a boundary velocity for.
#[derive(Debug)]
struct GlideShort {
    short_by: f64,
}

/// A linear glide from `from` to `to` over a quarter second.
struct Glide {
    from: f64,
    to: f64,
}

impl flui_animation::Simulation for Glide {
    fn x(&self, time: f64) -> f64 {
        self.from + (self.to - self.from) * (time / 0.25).min(1.0)
    }

    fn dx(&self, time: f64) -> f64 {
        if time < 0.25 {
            (self.to - self.from) / 0.25
        } else {
            0.0
        }
    }

    fn is_done(&self, time: f64) -> bool {
        time >= 0.25
    }
}

impl flui_widgets::ScrollPhysics for GlideShort {
    fn apply_boundary_conditions(&self, metrics: &ScrollMetrics, proposed_pixels: f64) -> f64 {
        proposed_pixels.clamp(metrics.min_scroll_extent, metrics.max_scroll_extent)
    }

    fn create_ballistic_simulation(
        &self,
        metrics: &ScrollMetrics,
        velocity_px_per_sec: f64,
    ) -> Option<Box<dyn flui_animation::Simulation>> {
        (velocity_px_per_sec > 0.0).then(|| {
            Box::new(Glide {
                from: metrics.pixels,
                to: metrics.max_scroll_extent - self.short_by,
            }) as Box<dyn flui_animation::Simulation>
        })
    }

    fn boundary_velocity(&self, _metrics: &ScrollMetrics, velocity_px_per_sec: f64) -> Option<f64> {
        (velocity_px_per_sec > 0.0).then_some(1000.0)
    }
}

/// On a list fifty million pixels long, a run that comes to rest 10 px
/// before its edge has not reached it and hands no residual on; one that
/// rests on the edge does. The edge test is half a device pixel, not a share
/// of the list's length.
pub(crate) fn a_run_short_of_the_edge_of_a_long_list_hands_nothing_on() {
    let flings_after = |short_by: f64| {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(
                probe.connection(),
                Scrollable::new()
                    .controller(controller.clone())
                    .physics(Rc::new(GlideShort { short_by }))
                    .child(SizedBox::new(300.0, 5.0e7)),
            ),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        let end = controller.max_scroll_extent();
        controller
            .jump_to(end - 500.0)
            .expect("no build is running");
        laid.tick();
        laid.replay(&fling(Device::Touch, at(150.0, 250.0), at(150.0, 150.0)));
        settle(&mut laid);
        assert_eq!(
            controller.pixels(),
            end - short_by,
            "the run rests where it glides"
        );
        probe.flings()
    };
    assert_eq!(
        flings_after(10.0),
        0,
        "10 px short of the edge hands nothing on"
    );
    assert_eq!(flings_after(0.0), 1, "on the edge, the residual goes up");
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
        controller
            .animate_to(
                700.0,
                Duration::from_secs(1),
                flui_animation::SharedCurve::new(flui_animation::curves::LINEAR),
            )
            .expect("no build is running");
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

/// A participant takes part in a programmatic sequence only by accepting
/// programmatic scrolls (ADR-0209 §2): one that declines, however greedy,
/// hears nothing of a jump or an animation, which move the list whole,
/// while one that accepts hears each as a sequence; user input still
/// reaches the declining one.
pub(crate) fn a_participant_declining_programmatic_scrolls_hears_none_of_them() {
    for device in DEVICES {
        let (controller, accepting, greedy, vsync) = (
            ScrollController::new(),
            Probe::new(),
            Probe::declining_programmatic(),
            Vsync::new(),
        );
        // Takes every step it is offered, whatever its cause.
        greedy.set_pre(|step| step.available);
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(
                accepting.connection(),
                NestedScroll::new(greedy.connection(), list(&controller)),
            ),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);

        controller.jump_to(200.0).expect("no build is running");
        laid.tick();
        assert_eq!(
            controller.pixels(),
            200.0,
            "{device:?}: the jump moves the list"
        );
        controller
            .animate_to(
                400.0,
                Duration::from_millis(200),
                flui_animation::SharedCurve::new(flui_animation::curves::LINEAR),
            )
            .expect("no build is running");
        settle(&mut laid);
        assert_eq!(
            controller.pixels(),
            400.0,
            "{device:?}: the animation moves the list"
        );
        assert_eq!(
            greedy.take(),
            Vec::new(),
            "{device:?}: the declining participant hears nothing of either"
        );
        let started = accepting.started();
        assert_eq!(
            started.len(),
            2,
            "{device:?}: the accepting one hears the jump and the animation: {:?}",
            accepting.seen()
        );
        assert_eq!(
            accepting.ended(),
            vec![
                (started[0], ScrollEndReason::Released),
                (started[1], ScrollEndReason::Released),
            ],
            "{device:?}"
        );
        assert!(
            accepting
                .steps()
                .iter()
                .all(|step| step.device == ScrollDevice::Programmatic),
            "{device:?}"
        );
        accepting.take();

        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 150.0)));
        settle(&mut laid);
        assert_eq!(
            controller.pixels(),
            400.0,
            "{device:?}: the greedy participant takes the drag whole"
        );
        assert!(
            greedy
                .steps()
                .iter()
                .any(|step| step.cause == ScrollCause::UserInput),
            "{device:?}: user input reaches it"
        );
    }
}

/// A list rebuilt mid-fling under a new chain above it, as a build that
/// writes `NestedScroll::new(Rc::new(..), ..)` does each frame, keeps its
/// fling: the run goes on, bounces past the end and rests exactly on it,
/// and its sequence, snapshotted once, ends once for the connection it
/// started under; the next drag is heard by the current one.
pub(crate) fn a_new_chain_above_keeps_the_leaf_run() {
    for device in DEVICES {
        let (controller, vsync) = (ScrollController::new(), Vsync::new());
        let physics: flui_widgets::SharedScrollPhysics =
            Rc::new(flui_widgets::BouncingScrollPhysics::new());
        let tree = |probe: &Rc<Probe>| {
            VsyncScope::new(
                vsync.clone(),
                NestedScroll::new(
                    probe.connection(),
                    list(&controller).physics(physics.clone()),
                ),
            )
            .boxed()
        };
        let first = Probe::new();
        let mut laid = mounted(tree(&first), vsync.clone());
        controller.jump_to(450.0).expect("no build is running");
        laid.tick();
        first.take();
        laid.replay(&fling(device, at(150.0, 250.0), at(150.0, 150.0)));
        let released = first.started();
        assert_eq!(released.len(), 1, "{device:?}: the fling's sequence");

        let mut furthest = controller.pixels();
        let mut current = Rc::clone(&first);
        for frame in 0..240 {
            let before = controller.pixels();
            let rebuilt = Probe::new();
            laid.pump_widget(tree(&rebuilt));
            current = rebuilt;
            laid.pump_for(SAMPLE);
            if frame < 3 {
                assert!(
                    controller.pixels() > before,
                    "{device:?} frame {frame}: the list keeps flinging, at {}",
                    controller.pixels()
                );
            }
            furthest = furthest.max(controller.pixels());
        }
        assert!(
            furthest > 701.0,
            "{device:?}: the fling bounces past the end; furthest {furthest:.3}"
        );
        assert_eq!(controller.pixels(), 700.0, "{device:?}: rests on the end");
        assert_eq!(
            first
                .ended()
                .iter()
                .filter(|(id, _)| *id == released[0])
                .count(),
            1,
            "{device:?}: the sequence ends once for its own chain: {:?}",
            first.seen()
        );

        laid.replay(&slow(device, at(150.0, 100.0), at(150.0, 200.0)));
        assert_eq!(
            current.started().len(),
            1,
            "{device:?}: the next drag is the current chain's"
        );
    }
}

/// An ancestor rebuilt with a fresh physics value mid-drag, as a build that
/// writes `.physics(Rc::new(..))` does, stays the same link of the drag's
/// chain: the remainder of the list inside it goes on moving it.
pub(crate) fn a_fresh_ancestor_physics_mid_drag_keeps_taking_the_remainder() {
    let (outer, inner, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        Vsync::new(),
    );
    let mut laid = mounted(
        list_at_the_top_of_a_bouncing_list(&outer, &inner, &vsync),
        vsync.clone(),
    );
    inner.jump_to(800.0).expect("no build is running");
    laid.tick();
    laid.dispatch_pointer_down(150.0, 190.0);
    for y in [170.0, 150.0, 130.0] {
        laid.dispatch_pointer_move(150.0, y);
    }
    let before = outer.pixels();
    assert!(
        before > 0.0,
        "the remainder moves the ancestor, at {before}"
    );
    laid.pump_widget(list_at_the_top_of_a_bouncing_list(&outer, &inner, &vsync));
    for y in [110.0, 90.0] {
        laid.dispatch_pointer_move(150.0, y);
    }
    assert_eq!(
        outer.pixels(),
        before + 40.0,
        "the rebuilt ancestor keeps taking the remainder"
    );
    laid.dispatch_pointer_up(150.0, 90.0);
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
            controller.jump_to(300.0).expect("no build is running");
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

        // The same for `post_scroll`: a list at its top leaves a drag toward
        // the start to `near`, and what `near` is taken to have consumed
        // shows in what is left for `far` above it.
        let left_above = |answer: fn(Vec2) -> Vec2| {
            let (controller, near, far, vsync) = (
                ScrollController::new(),
                Probe::new(),
                Probe::new(),
                Vsync::new(),
            );
            let mut laid = mounted(panic_tree(&controller, &near, &far, &vsync), vsync);
            near.set_post(move |step| answer(step.available));
            laid.replay(&slow(device, at(150.0, 100.0), at(150.0, 250.0)));
            settle(&mut laid);
            assert_eq!(
                controller.pixels(),
                0.0,
                "{device:?}: the list stays at its top"
            );
            assert!(
                far.posts()
                    .iter()
                    .all(|step| step.available.x.is_finite() && step.available.y.is_finite()),
                "{device:?}: nothing above sees a value that is not finite: {:?}",
                far.seen()
            );
            far.posts().iter().map(|step| step.available.y).sum::<f64>()
        };
        let taken_nothing = left_above(|_| Vec2::ZERO);
        assert!(
            taken_nothing < -100.0,
            "{device:?}: the baseline remainder reaches the far participant, {taken_nothing}"
        );
        assert_eq!(
            left_above(|offered| Vec2::ZERO - offered),
            taken_nothing,
            "{device:?}: post_scroll, wrong sign"
        );
        assert_eq!(
            left_above(|_| Vec2::new(f64::NAN, f64::NAN)),
            taken_nothing,
            "{device:?}: post_scroll, NaN"
        );
        assert_eq!(
            left_above(|offered| Vec2::new(offered.x * 10.0, offered.y * 10.0)),
            0.0,
            "{device:?}: post_scroll, larger than offered"
        );
        assert_eq!(
            left_above(|_| Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY)),
            0.0,
            "{device:?}: post_scroll, infinite"
        );
    }
}

/// A step that is not finite never enters the chain: a jump to NaN moves
/// nothing and reaches no participant, even one that opts into programmatic
/// steps and answers with what fits, and the next jump goes through. Finite
/// values make one too: a jump across extents near `f64::MAX` overflows the
/// distance; no participant is offered it, and the jump still lands.
pub(crate) fn a_step_that_is_not_finite_is_dropped_at_the_channel() {
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(&controller)),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    controller.jump_to(300.0).expect("no build is running");
    laid.tick();
    probe.set_pre(|step| {
        if step.cause == ScrollCause::Programmatic {
            // What fits of the step, as a participant with room for 50
            // would answer: `NaN.min(50)` is 50.
            Vec2::new(0.0, step.available.y.min(50.0))
        } else {
            Vec2::ZERO
        }
    });
    probe.take();
    let jumped = catch_unwind(AssertUnwindSafe(|| controller.jump_to(f64::NAN)));
    assert!(jumped.is_ok(), "a jump to NaN does not panic");
    assert_eq!(controller.pixels(), 300.0, "and moves nothing");
    assert!(
        probe
            .steps()
            .iter()
            .all(|step| step.available.y.is_finite()),
        "no participant sees a step that is not finite: {:?}",
        probe.seen()
    );
    *probe.pre.borrow_mut() = None;
    controller.jump_to(120.0).expect("no build is running");
    assert_eq!(controller.pixels(), 120.0, "the next jump goes through");

    // Every operand finite, the distance not: `f64::MAX - -f64::MAX`.
    probe.take();
    controller
        .update_dimensions(300.0, -f64::MAX, f64::MAX)
        .expect("ordered extents");
    controller.set_pixels(-f64::MAX);
    let jumped = catch_unwind(AssertUnwindSafe(|| controller.jump_to(f64::MAX)));
    assert!(
        jumped.is_ok(),
        "a jump whose distance overflows does not panic"
    );
    assert!(
        probe
            .steps()
            .iter()
            .all(|step| step.available.x.is_finite() && step.available.y.is_finite()),
        "no participant sees the overflowed step: {:?}",
        probe.seen()
    );
    assert_eq!(
        controller.pixels(),
        f64::MAX,
        "the jump still lands, though no step could carry it"
    );
}

/// User input a participant dispatches into the chain that called it is
/// refused: a key a participant sends to the scrollable its drag focused
/// moves nothing, and the same key outside the participant scrolls
/// (ADR-0209 §7).
pub(crate) fn a_participant_cannot_dispatch_input_into_its_own_chain() {
    let down = Key::Named(NamedKey::ArrowDown);
    for device in DEVICES {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let mut laid = mounted(requests::list_under(&probe, &controller, &vsync), vsync);
        let keys = Rc::new(Keys::of(&laid));
        let (sender, key) = (Rc::clone(&keys), down.clone());
        let handled = Rc::new(Cell::new(None));
        let (answer, steps) = (Rc::clone(&handled), Cell::new(0));
        probe.set_pre(move |step| {
            // A later step of the drag, once the drag has given the
            // scrollable the keys.
            if step.cause == ScrollCause::UserInput {
                steps.set(steps.get() + 1);
                if steps.get() == 3 {
                    answer.set(Some(sender.down(Code::ArrowDown, key.clone())));
                    sender.up(Code::ArrowDown, key.clone());
                }
            }
            Vec2::ZERO
        });
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 100.0)));
        settle(&mut laid);
        assert!(
            handled.get().is_some(),
            "{device:?}: the participant sent the key"
        );
        assert!(
            controller.pixels() > 100.0 && controller.pixels() < 200.0,
            "{device:?}: only the drag moved the list, at {}",
            controller.pixels()
        );
        assert!(
            probe
                .steps()
                .iter()
                .all(|step| step.device != ScrollDevice::Keyboard),
            "{device:?}: no key step entered the chain: {:?}",
            probe.seen()
        );
        *probe.pre.borrow_mut() = None;
        let before = controller.pixels();
        assert!(keys.down(Code::ArrowDown, down.clone()), "{device:?}");
        keys.up(Code::ArrowDown, down.clone());
        assert!(
            controller.pixels() > before,
            "{device:?}: outside a participant the key scrolls"
        );
    }
    a_participant_cannot_dispatch_an_assistive_scroll_into_its_own_chain();
}

/// The same for an assistive scroll action on the list sent from inside a
/// drag step: the list moves exactly as far as the drag alone moves it, no
/// sequence of the action enters the chain, and the action scrolls once
/// sent from outside.
fn a_participant_cannot_dispatch_an_assistive_scroll_into_its_own_chain() {
    use flui_testing::a11y::{Action, ActionRequest, Role, TreeId};
    fn scroll_down(laid: &LaidOut) {
        let view = laid
            .a11y_tree()
            .expect("published semantics")
            .find_all(Role::ScrollView)
            .first()
            .expect("the list is a scroll view")
            .id();
        laid.invoke_semantics_action(ActionRequest {
            action: Action::ScrollDown,
            target_tree: TreeId::ROOT,
            target_node: view,
            data: None,
        })
        .expect("the request resolves");
    }
    // The same drag with and without the participant's action.
    let mut baseline = None;
    for from_inside in [false, true] {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let mut laid = mounted(requests::list_under(&probe, &controller, &vsync), vsync);
        laid.enable_semantics();
        laid.tick();
        let laid = Rc::new(RefCell::new(laid));
        let (sender, steps, sent) = (Rc::clone(&laid), Cell::new(0), Rc::new(Cell::new(false)));
        let was_sent = Rc::clone(&sent);
        probe.set_pre(move |step| {
            if from_inside && step.cause == ScrollCause::UserInput {
                steps.set(steps.get() + 1);
                if steps.get() == 3 {
                    scroll_down(&sender.borrow());
                    was_sent.set(true);
                }
            }
            Vec2::ZERO
        });
        {
            let laid = laid.borrow();
            laid.dispatch_pointer_down(150.0, 250.0);
            for point in line(at(150.0, 250.0), at(150.0, 150.0), 10)
                .into_iter()
                .skip(1)
            {
                laid.dispatch_pointer_move_after(point.dx, point.dy, SAMPLE);
            }
            laid.dispatch_pointer_move_after(150.0, 150.0, Duration::from_millis(300));
            laid.dispatch_pointer_up(150.0, 150.0);
        }
        settle(&mut laid.borrow_mut());
        let dragged = controller.pixels();
        let Some(baseline) = baseline else {
            baseline = Some(dragged);
            *probe.pre.borrow_mut() = None;
            continue;
        };
        assert!(sent.get(), "the participant sent the action");
        assert_eq!(dragged, baseline, "only the drag moved the list");
        assert_eq!(
            probe.started().len(),
            1,
            "no sequence of its own entered the chain: {:?}",
            probe.seen()
        );
        *probe.pre.borrow_mut() = None;
        scroll_down(&laid.borrow());
        settle(&mut laid.borrow_mut());
        assert!(
            controller.pixels() > dragged,
            "outside a participant the action scrolls"
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
            controller.jump_to(0.0).expect("no build is running");
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
        a_panic_in_post_scroll_consumes_the_remainder(device);
        caught::a_panic_above_a_caught_ancestor_keeps_its_settle(device);
    }
}

/// A contact that ends without a release ends its sequence with the reason
/// it did (ADR-0209 §1): a cancelled drag ends `Cancelled`, and a press that
/// stopped a moving list whose contact the pager's drag then took ends
/// `ArenaLost`. Either way the participant hears the end once, never
/// `released`, and the next drag is a sequence like any other.
pub(crate) fn a_contact_that_ends_without_release_names_why() {
    // Cancelled: touch only, a mouse press cannot be cancelled.
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(&controller)),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    let mut script = TouchScript::new("drag, then cancel").down(1, at(150.0, 250.0));
    for point in line(at(150.0, 250.0), at(150.0, 150.0), 8)
        .into_iter()
        .skip(1)
    {
        script = script.wait(SAMPLE).move_to(1, point);
    }
    let script = script
        .wait(SAMPLE)
        .cancel(1)
        .build()
        .expect("one cancelled contact");
    laid.replay(&script);
    settle(&mut laid);
    let sequence = probe.started();
    assert_eq!(sequence.len(), 1, "the drag is one sequence");
    assert_eq!(
        probe.ended(),
        vec![(sequence[0], ScrollEndReason::Cancelled)],
        "a cancelled drag ends its sequence cancelled"
    );
    assert!(
        !probe
            .seen()
            .iter()
            .any(|seen| matches!(seen, Seen::Released(_))),
        "and is never released: {:?}",
        probe.seen()
    );

    // ArenaLost: a probe between the list and the pager hears the list's
    // sequences only.
    for device in DEVICES {
        let (pager, list_controller, top, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Probe::new(),
            Probe::new(),
            Vsync::new(),
        );
        let pages = Row::new(vec![
            SizedBox::new(300.0, 300.0)
                .child(NestedScroll::new(
                    inner.connection(),
                    list(&list_controller),
                ))
                .boxed(),
            SizedBox::new(300.0, 300.0).boxed(),
            SizedBox::new(300.0, 300.0).boxed(),
        ]);
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(
                top.connection(),
                Scrollable::new()
                    .controller(pager.clone())
                    .scroll_direction(Axis::Horizontal)
                    .child(pages),
            ),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        laid.replay(&fling(device, at(150.0, 250.0), at(150.0, 100.0)));
        laid.pump_for(SAMPLE);
        laid.pump_for(SAMPLE);
        let moving = list_controller.pixels();
        laid.pump_for(SAMPLE);
        assert!(
            list_controller.pixels() > moving,
            "{device:?}: the list still flings"
        );
        inner.take();
        laid.replay(&slow(device, at(200.0, 150.0), at(40.0, 150.0)));
        settle(&mut laid);
        assert!(
            pager.pixels() > 100.0,
            "{device:?}: the pager's drag took the contact, at {}",
            pager.pixels()
        );
        let held = inner.started();
        assert_eq!(
            held.len(),
            1,
            "{device:?}: the press began a sequence on the moving list: {:?}",
            inner.seen()
        );
        let (held_ends, fling_ends): (Vec<_>, Vec<_>) = inner
            .ended()
            .into_iter()
            .partition(|(id, _)| *id == held[0]);
        assert_eq!(
            held_ends,
            vec![(held[0], ScrollEndReason::ArenaLost)],
            "{device:?}: which ends arena-lost once the pager takes the contact"
        );
        assert!(
            fling_ends
                .iter()
                .all(|(_, reason)| *reason == ScrollEndReason::Preempted),
            "{device:?}: the press stopped the fling: {fling_ends:?}"
        );
        assert!(
            !inner
                .seen()
                .iter()
                .any(|seen| matches!(seen, Seen::Released(_))),
            "{device:?}: and is never released: {:?}",
            inner.seen()
        );

        inner.take();
        pager.jump_to(0.0).expect("no build is running");
        laid.tick();
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 150.0)));
        settle(&mut laid);
        let next = inner.started();
        assert_eq!(next.len(), 1, "{device:?}: the next drag is a sequence");
        assert_eq!(
            inner.ended(),
            vec![(next[0], ScrollEndReason::Released)],
            "{device:?}: released as usual"
        );
    }
}

/// A release that reentrant code takes over ends its sequence at once: a
/// press lifted on a moving pager starts its settle, the settle raises the
/// scroll activity, and an activity listener jumps the pager, so no settle
/// runs. The press's sequence still ends `Released` there, before any next
/// contact, and the jump is a sequence of its own.
pub(crate) fn a_release_taken_over_by_reentrant_code_still_ends_its_sequence() {
    for device in DEVICES {
        let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
        let tree = VsyncScope::new(
            vsync.clone(),
            NestedScroll::new(
                probe.connection(),
                Scrollable::new()
                    .controller(controller.clone())
                    .snap_policy(Arc::new(PageSnap::new(ViewportFraction::FULL)))
                    .child(SizedBox::new(300.0, 3000.0)),
            ),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        laid.replay(&fling(device, at(150.0, 250.0), at(150.0, 100.0)));
        laid.pump_for(SAMPLE);
        laid.pump_for(SAMPLE);
        let moving = controller.pixels();
        laid.pump_for(SAMPLE);
        assert!(
            controller.pixels() > moving && controller.pixels() % 300.0 != 0.0,
            "{device:?}: the pager settles between pages, at {}",
            controller.pixels()
        );
        let armed = Rc::new(Cell::new(true));
        let (jumping, position) = (controller.clone(), controller.position());
        let arm = Rc::clone(&armed);
        let listener = position.add_activity_listener(Rc::new(move || {
            if jumping.position().is_scrolling() && arm.replace(false) {
                jumping.jump_to(900.0).expect("no build is running");
            }
        }));
        probe.take();
        laid.replay(&press(device, at(150.0, 150.0)));
        settle(&mut laid);
        position.remove_activity_listener(listener);
        assert!(!armed.get(), "{device:?}: the settle raised the activity");
        assert_eq!(controller.pixels(), 900.0, "{device:?}: the jump took over");
        let started = probe.started();
        assert_eq!(
            started.len(),
            2,
            "{device:?}: the press and the jump: {:?}",
            probe.seen()
        );
        let ended: Vec<_> = probe
            .ended()
            .into_iter()
            .filter(|(id, _)| started.contains(id))
            .collect();
        assert_eq!(
            ended,
            vec![
                (started[1], ScrollEndReason::Released),
                (started[0], ScrollEndReason::Released),
            ],
            "{device:?}: the jump ends, then the press it took over: {:?}",
            probe.seen()
        );
    }
}

/// A participant that panics in `post_scroll` consumes the whole remainder:
/// nothing reaches the participant above it, it leaves the sequence with
/// `Cancelled` while the others finish it, and it takes part again in the
/// next sequence.
fn a_panic_in_post_scroll_consumes_the_remainder(device: Device) {
    let (controller, near, far, vsync) = (
        ScrollController::new(),
        Probe::new(),
        Probe::new(),
        Vsync::new(),
    );
    let mut laid = mounted(panic_tree(&controller, &near, &far, &vsync), vsync);
    let armed = Rc::new(Cell::new(true));
    let trigger = Rc::clone(&armed);
    near.set_post(move |_| {
        assert!(!trigger.replace(false), "post_scroll failure");
        Vec2::ZERO
    });
    let toward_the_top = || slow(device, at(150.0, 100.0), at(150.0, 250.0));
    let failures = replay_contained(&mut laid, &toward_the_top());
    settle(&mut laid);
    assert_eq!(
        failures,
        vec!["post_scroll failure".to_owned()],
        "{device:?}: the failure resumes, once"
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
    assert_eq!(
        near.posts().len(),
        1,
        "{device:?}: a closed link is offered no further remainder"
    );
    assert_eq!(
        far.posts(),
        Vec::new(),
        "{device:?}: the failed participant consumed the remainder: {:?}",
        far.seen()
    );

    near.take();
    far.take();
    assert!(
        replay_contained(&mut laid, &toward_the_top()).is_empty(),
        "{device:?}: the next sequence runs clean"
    );
    settle(&mut laid);
    assert!(
        near.posts().len() > 1,
        "{device:?}: the participant is offered remainders again"
    );
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
        controller.jump_to(10_000.0).expect("no build is running");
        for _ in 0..4 {
            laid.tick();
        }
        probe.take();

        // Controls: the comparison below tells a move through the channel
        // from one outside it. A direct position write is a bypass: the offset
        // moves and the probe hears nothing. A jump is a programmatic step:
        // the probe hears exactly what moved.
        let resting = controller.pixels();
        controller.set_pixels(resting - 30.0);
        laid.tick();
        assert_eq!(
            probe.steps(),
            vec![],
            "{device:?}: control: a direct write bypasses the channel"
        );
        controller.jump_to(resting).expect("no build is running");
        let jump: Vec<_> = probe.steps();
        assert_eq!(
            jump.iter().map(|step| step.available.y).sum::<f64>(),
            30.0,
            "{device:?}: control: a jump offers what it moved: {jump:?}"
        );
        assert!(
            jump.iter()
                .all(|step| step.cause == ScrollCause::Programmatic),
            "{device:?}: control: and as a programmatic step"
        );
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
/// nearest page or anchor from rest, the next one past half a page or after
/// a fling, an extent for a target that is not finite or lies past it; a
/// target a half device pixel off still counts as on it (ADR-0209 §9).
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
        let pages = || Arc::new(PageSnap::new(ViewportFraction::FULL)) as Arc<dyn SnapPolicy>;
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
        let stops = || {
            Arc::new(AnchorSnap::try_new([500.0, 0.0, 120.0]).expect("finite anchors"))
                as Arc<dyn SnapPolicy>
        };
        let anchored = rests(stops(), slow(device, at(250.0, 150.0), at(150.0, 150.0)));
        assert!(
            near(anchored, 120.0),
            "{device:?}: a drag rests on the nearest anchor, at {anchored}"
        );
        let anchor_fling = rests(stops(), fling(device, at(250.0, 150.0), at(190.0, 150.0)));
        assert!(
            near(anchor_fling, 120.0),
            "{device:?}: a fling takes the next anchor, at {anchor_fling}"
        );
        let past_extent = rests(
            Arc::new(AnchorSnap::try_new([0.0, 900.0]).expect("finite anchors")),
            fling(device, at(250.0, 150.0), at(190.0, 150.0)),
        );
        assert!(
            near(past_extent, 600.0),
            "{device:?}: an anchor past the extent rests on it, at {past_extent}"
        );
    }

    // The targets themselves, at device pixel ratio 2 (a quarter-pixel slop).
    let metrics =
        |pixels: f64| ScrollMetrics::new(pixels, 0.0, 900.0, 300.0).with_device_pixel_ratio(2.0);
    let pages = PageSnap::new(ViewportFraction::FULL);
    let stops = AnchorSnap::try_new([400.0, 0.0, 120.0, 120.0]).expect("finite anchors");
    assert_eq!(
        stops.anchors(),
        [0.0, 120.0, 400.0],
        "sorted, one per offset"
    );
    let rows: [(&str, &dyn SnapPolicy, f64, f64, f64); 14] = [
        ("page: nearest at rest", &pages, 130.0, 0.0, 0.0),
        ("page: halfway rounds forward", &pages, 150.0, 0.0, 300.0),
        ("page: a slow release is no fling", &pages, 130.0, 19.0, 0.0),
        (
            "page: a fling commits from anywhere",
            &pages,
            10.0,
            500.0,
            300.0,
        ),
        (
            "page: a fling leaves an exact page",
            &pages,
            300.0,
            500.0,
            600.0,
        ),
        ("page: and a page a hair off", &pages, 299.8, 500.0, 600.0),
        ("page: back from a hair past", &pages, 300.2, -500.0, 0.0),
        (
            "page: but not from farther off",
            &pages,
            299.0,
            500.0,
            300.0,
        ),
        ("anchor: nearest at rest", &stops, 50.0, 0.0, 0.0),
        (
            "anchor: halfway takes the farther",
            &stops,
            60.0,
            0.0,
            120.0,
        ),
        (
            "anchor: a fling leaves an anchor a hair off",
            &stops,
            119.8,
            500.0,
            400.0,
        ),
        ("anchor: back from one", &stops, 120.0, -500.0, 0.0),
        ("anchor: stays on the last", &stops, 400.0, 500.0, 400.0),
        ("anchor: and on the first", &stops, 0.0, -500.0, 0.0),
    ];
    for (case, policy, pixels, velocity, target) in rows {
        assert_eq!(policy.target(&metrics(pixels), velocity), target, "{case}");
    }
    // A page shorter than a pixel (a viewport collapsing to nothing) names
    // no page to rest on: the release stays where it is, at rest or flung.
    let collapsed = ScrollMetrics::new(100.0, 0.0, 900.0, 0.5);
    for velocity in [0.0, 500.0, -500.0] {
        assert_eq!(
            pages.target(&collapsed, velocity),
            100.0,
            "page: a sub-pixel page length at {velocity} px/s"
        );
    }
    assert_eq!(AnchorSnap::try_new([]), Err(AnchorSnapError::Empty));
    assert_eq!(
        AnchorSnap::try_new([0.0, f64::NAN]),
        Err(AnchorSnapError::NonFinite { index: 1 })
    );
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
    send_sync::<AnchorSnap>();
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
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
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

/// An outer bouncing list scrolled past its top with a list on top of its
/// content.
fn list_at_the_top_of_a_bouncing_list(
    outer: &ScrollController,
    inner: &ScrollController,
    vsync: &Vsync,
) -> BoxedView {
    let content = Column::new(vec![
        SizedBox::new(300.0, 200.0).child(list(inner)).boxed(),
        SizedBox::new(300.0, 4800.0).boxed(),
    ]);
    VsyncScope::new(
        vsync.clone(),
        Scrollable::new()
            .controller(outer.clone())
            .physics(Rc::new(flui_widgets::BouncingScrollPhysics::new()))
            .child(content),
    )
    .boxed()
}

/// A remainder that reaches an ancestor scrolled past its edge moves it by
/// the remainder toward its range, never against the remainder and never
/// past the remainder's length, and never farther out.
pub(crate) fn a_remainder_moves_an_overscrolled_ancestor_only_toward_its_range() {
    for device in DEVICES {
        // The inner list at its end: a finger moving up leaves its whole
        // move to the ancestor, which follows it back toward its top.
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_at_the_top_of_a_bouncing_list(&outer, &inner, &vsync),
            vsync,
        );
        inner.jump_to(800.0).expect("no build is running");
        laid.tick();
        outer.set_pixels(-100.0);
        laid.tick();
        let mut track = Vec::new();
        laid.replay_with(&slow(device, at(150.0, 280.0), at(150.0, 230.0)), |_, _| {
            track.push(outer.pixels());
        });
        assert!(
            track.iter().all(|pixels| *pixels < -50.0),
            "{device:?}: a 50 px move moves the ancestor less than 50 px: {track:?}"
        );
        assert!(
            outer.pixels() > -100.0,
            "{device:?}: toward its range: {track:?}"
        );

        // The inner list at its top: a finger moving down asks the ancestor
        // to go farther past its top, which a remainder never does.
        let (outer, inner, vsync) = (
            ScrollController::new(),
            ScrollController::new(),
            Vsync::new(),
        );
        let mut laid = mounted(
            list_at_the_top_of_a_bouncing_list(&outer, &inner, &vsync),
            vsync,
        );
        laid.tick();
        outer.set_pixels(-100.0);
        laid.tick();
        let mut track = Vec::new();
        laid.replay_with(&slow(device, at(150.0, 180.0), at(150.0, 230.0)), |_, _| {
            track.push(outer.pixels());
        });
        assert!(
            track.iter().all(|pixels| *pixels == -100.0),
            "{device:?}: an outward remainder moves nothing: {track:?}"
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

/// A reveal in a list inside a list moves each scrollable by its own
/// geometry, nearest first, as a programmatic sequence of its own through
/// that scrollable's chain: the connection between the two hears only the
/// inner list's move, the one above both hears each move once, and neither
/// is offered the other's remainder (ADR-0209 §3, C8).
pub(crate) fn a_nested_reveal_moves_each_ancestor_in_a_sequence_of_its_own() {
    use flui_testing::a11y::{Action, ActionRequest, TreeId};
    let (outer, inner, top, middle, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        Probe::new(),
        Probe::new(),
        Vsync::new(),
    );
    let target = flui_widgets::Semantics::new()
        .container(true)
        .role(flui_widgets::SemanticsRole::Button)
        .label("nested target")
        .child(SizedBox::new(300.0, 40.0));
    let inner_list = Scrollable::new()
        .controller(inner.clone())
        .child(Column::new(vec![
            SizedBox::new(300.0, 400.0).boxed(),
            target.boxed(),
            SizedBox::new(300.0, 600.0).boxed(),
        ]));
    let content = Column::new(vec![
        SizedBox::new(300.0, 600.0).boxed(),
        SizedBox::new(300.0, 200.0)
            .child(NestedScroll::new(middle.connection(), inner_list))
            .boxed(),
        SizedBox::new(300.0, 600.0).boxed(),
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
    laid.enable_semantics();
    laid.tick();
    top.take();
    middle.take();
    let target = laid
        .a11y_tree()
        .expect("published semantics")
        .find_by_label("nested target")
        .expect("the offscreen target is addressable")
        .id();
    laid.invoke_semantics_action(ActionRequest {
        action: Action::ScrollIntoView,
        target_tree: TreeId::ROOT,
        target_node: target,
        data: None,
    })
    .expect("the request resolves");
    settle(&mut laid);
    assert_eq!(inner.pixels(), 240.0, "the inner list shows the target");
    assert_eq!(outer.pixels(), 500.0, "the outer list shows the inner list");

    let programmatic = |probe: &Probe| {
        probe.steps().iter().all(|step| {
            step.cause == ScrollCause::Programmatic && step.device == ScrollDevice::Programmatic
        })
    };
    let between = middle.started();
    assert_eq!(
        between.len(),
        1,
        "the connection between hears the inner move alone: {:?}",
        middle.seen()
    );
    assert!(programmatic(&middle), "{:?}", middle.seen());
    assert_eq!(
        middle
            .steps()
            .iter()
            .map(|step| step.available.y)
            .sum::<f64>(),
        240.0,
        "it is offered the inner list's own distance"
    );
    let above = top.started();
    assert_eq!(
        above.len(),
        2,
        "the connection above hears each move as its own sequence: {:?}",
        top.seen()
    );
    assert_eq!(above[0], between[0], "the nearest scrollable moves first");
    assert!(programmatic(&top), "{:?}", top.seen());
    assert_eq!(
        top.ended(),
        vec![
            (above[0], ScrollEndReason::Released),
            (above[1], ScrollEndReason::Released),
        ],
        "each ends once"
    );
    assert!(
        top.seen()
            .iter()
            .chain(middle.seen().iter())
            .all(|seen| !matches!(seen, Seen::Post(_))),
        "a reveal hands no remainder up"
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
    assert!(
        !silent.accepts_programmatic(),
        "a connection takes part in programmatic sequences only by opting in"
    );
    let devices = [
        ScrollDevice::Pointer {
            kind: flui_platform::pointer::PointerKind::Touch,
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

/// A participant that takes part of a fling's steps leaves the list short
/// of its edge; the residual the run still carries there is offered to the
/// chain all the same (ADR-0209 §4).
pub(crate) fn fling_residual_survives_a_participant_taking_part_of_it() {
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
        middle.set_pre(|step| {
            if step.cause == ScrollCause::SideEffect {
                Vec2::new(0.0, step.available.y * 0.5)
            } else {
                Vec2::ZERO
            }
        });
        outer.jump_to(600.0).expect("no build is running");
        inner.jump_to(650.0).expect("no build is running");
        laid.tick();
        top.take();
        middle.take();
        laid.replay(&fling(device, at(150.0, 150.0), at(150.0, 60.0)));
        for _ in 0..400 {
            laid.pump_for(SAMPLE);
        }
        let taken: f64 = middle
            .steps()
            .iter()
            .filter(|step| step.cause == ScrollCause::SideEffect)
            .map(|step| step.available.y)
            .sum();
        assert!(
            taken > 0.0 && inner.pixels() < 800.0,
            "{device:?}: the participant took part of the run, so the list stops short of \
             its end, at {}",
            inner.pixels()
        );
        assert!(
            middle.flings() > 0,
            "{device:?}: the residual is still offered: {:?}",
            middle.seen()
        );
        assert!(
            outer.pixels() > 600.0,
            "{device:?}: and the enclosing list runs it, at {}",
            outer.pixels()
        );
        let started = top.started();
        assert_eq!(started.len(), 1, "{device:?}: one sequence");
        assert_eq!(
            top.ended(),
            vec![(started[0], ScrollEndReason::Released)],
            "{device:?}: ended once, after the hand-off"
        );
    }
}

/// A connection above two scrollables a contact reaches (a press holding a
/// moving list and the list moving it) hears the contact's sequence start
/// once and end once (ADR-0209 §1).
pub(crate) fn shared_connection_hears_a_sequence_once() {
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
        outer.jump_to(600.0).expect("no build is running");
        laid.tick();
        for controller in [&outer, &inner] {
            controller
                .animate_to(
                    700.0,
                    Duration::from_secs(2),
                    flui_animation::SharedCurve::new(flui_animation::curves::LINEAR),
                )
                .expect("no build is running");
        }
        laid.pump_for(Duration::from_millis(100));
        laid.pump_for(Duration::from_millis(100));
        laid.replay(&press(device, at(150.0, 100.0)));
        settle(&mut laid);
        let unique = |ids: &[InputSequenceId]| {
            let mut seen = std::collections::HashSet::new();
            ids.iter().all(|id| seen.insert(*id))
        };
        let started = top.started();
        let ended: Vec<_> = top.ended().into_iter().map(|(id, _)| id).collect();
        assert!(
            unique(&started),
            "{device:?}: each sequence starts once for the connection above both: {started:?}"
        );
        assert!(unique(&ended), "{device:?}: and ends once: {ended:?}");
        assert!(
            started.len() >= 3,
            "{device:?}: two animations and the press: {started:?}"
        );
        assert_eq!(
            ended.len(),
            started.len(),
            "{device:?}: every sequence it heard start it heard end"
        );
    }
}

// ---------------------------------------------------------------------------
// Keyboard
// ---------------------------------------------------------------------------

use flui_interaction::events::{Code, Key, KeyState, Modifiers, NamedKey};

/// Key events into the focus tree of a mounted tree.
pub(crate) struct Keys {
    manager: Rc<flui_interaction::FocusManager>,
}

impl Keys {
    pub(crate) fn of(laid: &LaidOut) -> Self {
        Self {
            manager: laid.focus_manager(),
        }
    }

    fn send(&self, code: Code, key: Key, state: KeyState, repeat: bool, mods: Modifiers) -> bool {
        let event = flui_interaction::testing::input::KeyEventBuilder::new(code)
            .with_state(state)
            .with_key(key)
            .with_repeat(repeat)
            .with_modifiers(mods)
            .build();
        self.manager.dispatch_key_event(&event).is_handled()
    }

    pub(crate) fn down(&self, code: Code, key: Key) -> bool {
        self.send(code, key, KeyState::Down, false, Modifiers::NONE)
    }

    pub(crate) fn repeat(&self, code: Code, key: Key) -> bool {
        self.send(code, key, KeyState::Down, true, Modifiers::NONE)
    }

    pub(crate) fn up(&self, code: Code, key: Key) {
        self.send(code, key, KeyState::Up, false, Modifiers::NONE);
    }
}

/// The scroll keys move a focused scrollable through the channel: each press
/// is a sequence from the keyboard device, a held key's repeats continue it,
/// the release ends it, and a scrollable at the end of its range lets the key
/// bubble to the enclosing one (ADR-0209 C6, C8).
pub(crate) fn keyboard_scrolls_through_the_channel_as_its_own_sequence() {
    let down = Key::Named(NamedKey::ArrowDown);
    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(
            probe.connection(),
            Scrollable::new()
                .controller(controller.clone())
                .autofocus(true)
                .child(SizedBox::new(300.0, 1000.0)),
        ),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    laid.tick();
    let keys = Keys::of(&laid);
    let line = 53.0;

    assert!(keys.down(Code::ArrowDown, down.clone()), "handled");
    assert_eq!(controller.pixels(), line, "an arrow moves a line");
    let first = probe.started();
    assert_eq!(first.len(), 1, "the press began a sequence");
    assert_eq!(
        probe.ended(),
        vec![],
        "and a held key keeps it open: {:?}",
        probe.seen()
    );
    assert!(keys.repeat(Code::ArrowDown, down.clone()));
    assert!(keys.repeat(Code::ArrowDown, down.clone()));
    assert_eq!(controller.pixels(), 3.0 * line, "repeats keep moving");
    assert_eq!(probe.started(), first, "within the same sequence");
    keys.up(Code::ArrowDown, down.clone());
    settle(&mut laid);
    assert_eq!(
        probe.ended(),
        vec![(first[0], ScrollEndReason::Released)],
        "the release ends it once"
    );
    let steps = probe.steps();
    assert!(
        steps.len() == 3
            && steps.iter().all(|step| step.cause == ScrollCause::UserInput
                && step.device == ScrollDevice::Keyboard
                && step.sequence == first[0]
                && step.available == Vec2::new(0.0, line)),
        "its steps are user input from the keyboard: {steps:?}"
    );

    assert!(keys.down(Code::ArrowDown, down.clone()));
    keys.up(Code::ArrowDown, down.clone());
    let started = probe.started();
    assert_eq!(started.len(), 2, "the next press is a sequence of its own");
    assert_ne!(started[0], started[1]);

    controller.jump_to(0.0).expect("no build is running");
    laid.tick();
    for (code, key, expected) in [
        (Code::PageDown, Key::Named(NamedKey::PageDown), 240.0),
        (Code::PageUp, Key::Named(NamedKey::PageUp), 0.0),
        (Code::Space, Key::character(" "), 240.0),
        (Code::End, Key::Named(NamedKey::End), 700.0),
        (Code::Home, Key::Named(NamedKey::Home), 0.0),
    ] {
        assert!(keys.down(code, key.clone()), "{code:?} is handled");
        keys.up(code, key);
        settle(&mut laid);
        assert_eq!(controller.pixels(), expected, "{code:?}");
    }
    controller.jump_to(240.0).expect("no build is running");
    laid.tick();
    assert!(keys.send(
        Code::Space,
        Key::character(" "),
        KeyState::Down,
        false,
        Modifiers::SHIFT,
    ));
    settle(&mut laid);
    assert_eq!(controller.pixels(), 0.0, "Shift+Space goes a page back");

    assert!(
        !keys.send(
            Code::ArrowDown,
            down.clone(),
            KeyState::Down,
            false,
            Modifiers::CONTROL,
        ),
        "a command chord is not a scroll key"
    );
    assert!(
        !keys.down(Code::ArrowRight, Key::Named(NamedKey::ArrowRight)),
        "an arrow across the axis is not either"
    );
    assert!(
        !keys.down(Code::ArrowUp, Key::Named(NamedKey::ArrowUp)),
        "at the start, up moves nothing"
    );
    controller.jump_to(700.0).expect("no build is running");
    laid.tick();
    probe.take();
    assert!(
        !keys.down(Code::ArrowDown, down),
        "at the end, down moves nothing and bubbles"
    );
    assert_eq!(probe.started(), vec![], "and begins no sequence");
}

/// A held scroll key whose release cannot reach the scrollable still ends its
/// sequence: focus moved to another control before the release, or the
/// window lost activation while the key was down.
pub(crate) fn a_held_scroll_key_ends_its_sequence_when_its_release_goes_elsewhere() {
    let down = Key::Named(NamedKey::ArrowDown);
    let (probe, vsync) = (Probe::new(), Vsync::new());
    let elsewhere = flui_interaction::focus::FocusNode::with_debug_label("elsewhere");
    let tree = VsyncScope::new(
        vsync.clone(),
        Column::new(vec![
            SizedBox::new(300.0, 200.0)
                .child(NestedScroll::new(
                    probe.connection(),
                    Scrollable::new()
                        .autofocus(true)
                        .child(SizedBox::new(300.0, 1000.0)),
                ))
                .boxed(),
            flui_widgets::interaction::Focus::new(SizedBox::new(40.0, 40.0))
                .focus_node(Rc::clone(&elsewhere))
                .boxed(),
        ]),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    laid.tick();
    let keys = Keys::of(&laid);
    let manager = laid.focus_manager();
    let scrollable = manager.primary_focus().expect("the scrollable has focus");

    assert!(keys.down(Code::ArrowDown, down.clone()), "handled");
    let held = probe.started()[0];
    let _ = elsewhere.request_focus();
    assert!(elsewhere.has_primary_focus());
    keys.up(Code::ArrowDown, down.clone());
    settle(&mut laid);
    assert_eq!(
        probe.ended(),
        vec![(held, ScrollEndReason::Released)],
        "a release after focus moved ends the held key's sequence once"
    );

    let _ = scrollable.request_focus();
    assert!(keys.down(Code::ArrowDown, down.clone()));
    let held = probe.started()[1];
    manager.forget_pressed_keys();
    settle(&mut laid);
    assert!(
        probe.ended().contains(&(held, ScrollEndReason::Released)),
        "so does the window losing activation: {:?}",
        probe.seen()
    );
    keys.up(Code::ArrowDown, down);
    assert_eq!(
        probe.ended().len(),
        2,
        "and the orphan release ends nothing more"
    );
}

/// A key a nested scrollable cannot use bubbles to the enclosing one, whose
/// own sequence the key begins; arrows follow a horizontal axis and its
/// reading direction (ADR-0209 C6).
pub(crate) fn keyboard_bubbles_to_the_enclosing_scrollable_and_follows_the_axis() {
    use flui_painting::typography::TextDirection;
    let down = Key::Named(NamedKey::ArrowDown);
    let (outer, inner, top, vsync) = (
        ScrollController::new(),
        ScrollController::new(),
        Probe::new(),
        Vsync::new(),
    );
    let content = Column::new(vec![
        SizedBox::new(300.0, 600.0).boxed(),
        SizedBox::new(300.0, 200.0)
            .child(list(&inner).autofocus(true))
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
    outer.jump_to(600.0).expect("no build is running");
    inner.jump_to(800.0).expect("no build is running");
    laid.tick();
    top.take();
    let keys = Keys::of(&laid);
    assert!(keys.down(Code::ArrowDown, down.clone()));
    keys.up(Code::ArrowDown, down);
    settle(&mut laid);
    assert_eq!(inner.pixels(), 800.0, "the focused list is at its end");
    assert_eq!(outer.pixels(), 653.0, "so the enclosing list took the line");
    let started = top.started();
    assert_eq!(started.len(), 1, "as a keyboard sequence of its own");
    assert!(
        top.steps()
            .iter()
            .all(|step| step.device == ScrollDevice::Keyboard && step.sequence == started[0]),
        "{:?}",
        top.steps()
    );

    for (direction, right) in [(TextDirection::Ltr, 1.0), (TextDirection::Rtl, -1.0)] {
        let (controller, vsync) = (ScrollController::new(), Vsync::new());
        let tree = VsyncScope::new(
            vsync.clone(),
            flui_widgets::Directionality::new(
                direction,
                Scrollable::new()
                    .controller(controller.clone())
                    .scroll_direction(Axis::Horizontal)
                    .autofocus(true)
                    .child(SizedBox::new(1000.0, 300.0)),
            ),
        )
        .boxed();
        let mut laid = mounted(tree, vsync);
        controller.jump_to(300.0).expect("no build is running");
        laid.tick();
        let keys = Keys::of(&laid);
        let (right_key, left_key) = (
            Key::Named(NamedKey::ArrowRight),
            Key::Named(NamedKey::ArrowLeft),
        );
        assert!(keys.down(Code::ArrowRight, right_key.clone()));
        keys.up(Code::ArrowRight, right_key);
        assert_eq!(
            controller.pixels(),
            300.0 + right * 16.0,
            "{direction:?}: right"
        );
        assert!(keys.down(Code::ArrowLeft, left_key.clone()));
        keys.up(Code::ArrowLeft, left_key);
        assert_eq!(controller.pixels(), 300.0, "{direction:?}: left undoes it");
        assert!(
            !keys.down(Code::ArrowDown, Key::Named(NamedKey::ArrowDown)),
            "{direction:?}: across the axis"
        );
    }
}

/// A drag on a scrollable gives it the keys, by touch or by mouse
/// (ADR-0209 C6).
pub(crate) fn dragging_a_scrollable_gives_it_the_scroll_keys() {
    let down = Key::Named(NamedKey::ArrowDown);
    for device in DEVICES {
        let (controller, vsync) = (ScrollController::new(), Vsync::new());
        let tree = VsyncScope::new(vsync.clone(), list(&controller)).boxed();
        let mut laid = mounted(tree, vsync);
        assert!(
            !Keys::of(&laid).down(Code::ArrowDown, down.clone()),
            "{device:?}: nothing has the focus yet"
        );
        laid.replay(&slow(device, at(150.0, 250.0), at(150.0, 200.0)));
        settle(&mut laid);
        let dragged = controller.pixels();
        assert!(
            Keys::of(&laid).down(Code::ArrowDown, down.clone()),
            "{device:?}: the drag focused the list"
        );
        assert_eq!(controller.pixels(), dragged + 53.0, "{device:?}");
    }
}

/// A platform drag of plain text over a mounted tree, delivered the way the
/// binding delivers one, at `x = 150`.
struct HoveringDrag {
    owner: flui_interaction::drag::DragOwner,
    id: flui_foundation::DataTransferId,
    window: flui_foundation::PresentationId,
}

impl HoveringDrag {
    /// The drag enters at height `y`.
    fn enter(laid: &LaidOut, y: f64) -> Self {
        use flui_foundation::geometry::Point;
        use flui_foundation::{PresentationId, UiRuntimeId};
        use flui_interaction::drag::{DragOwner, ProcessNonce};
        use flui_platform::DragDropEvent;
        use flui_platform::data_transfer::{
            DataTransferOffer, RepresentationDescriptor, TransferActions, TransferFormat,
        };

        let owner = DragOwner::new(
            ProcessNonce::from_raw(std::num::NonZeroU64::new(7).expect("nonzero")),
            UiRuntimeId::new(7),
            Arc::new(flui_platform::NullDataTransferSource),
            Arc::new(|| {}),
        );
        let representations: Arc<[RepresentationDescriptor]> =
            Arc::from(vec![RepresentationDescriptor {
                format: TransferFormat::Text,
                declared_len: None,
            }]);
        // The id a source minted for the offer.
        let id = flui_foundation::DataTransferId::new_gen(0, std::num::NonZeroU32::MIN);
        let offer = DataTransferOffer::new(id, representations);
        let drag = Self {
            owner,
            id,
            window: PresentationId::new(1),
        };
        let path = Self::path_at(laid, y);
        laid.enter_owner_scope(|| {
            drag.owner.deliver(
                drag.window,
                DragDropEvent::Entered {
                    offer,
                    allowed: TransferActions::COPY,
                    position: Some(Point::new(150.0, y)),
                },
                &mut |_, _| Some(path.clone()),
            );
        });
        drag
    }

    fn path_at(laid: &LaidOut, y: f64) -> Vec<flui_interaction::HitTestEntry> {
        laid.hit_test_pointer(at(150.0, y), flui_platform::pointer::PointerKind::Mouse)
            .path()
            .to_vec()
    }

    /// The drag moves to height `y`.
    fn move_to(&self, laid: &LaidOut, y: f64) {
        use flui_foundation::geometry::Point;
        use flui_platform::DragDropEvent;
        use flui_platform::data_transfer::TransferActions;

        let path = Self::path_at(laid, y);
        laid.enter_owner_scope(|| {
            self.owner.deliver(
                self.window,
                DragDropEvent::Moved {
                    id: self.id,
                    allowed: TransferActions::COPY,
                    position: Point::new(150.0, y),
                },
                &mut |_, _| Some(path.clone()),
            );
        });
    }

    /// A frame of the drag machinery at `ms` milliseconds.
    fn tick(&self, laid: &LaidOut, ms: u64) {
        laid.enter_owner_scope(|| {
            self.owner
                .tick(EventTime::from_nanos(1_000_000_000 + ms * 1_000_000));
        });
    }

    fn exit(&self, laid: &LaidOut) {
        let id = self.id;
        laid.enter_owner_scope(|| {
            self.owner.deliver(
                self.window,
                flui_platform::DragDropEvent::Exited { id },
                &mut |_, _| None,
            );
        });
    }
}

/// A drag hovering near a scrollable's edge scrolls it toward that edge, each
/// frame a `SideEffect` step of the drag's own sequence through the channel
/// (ADR-0211 §8): steady while the drag holds still, ended when it moves out
/// of the band, the other way at the other edge, and nothing at an extent.
pub(crate) fn drag_autoscroll_dispatches_side_effect_deltas() {
    use flui_platform::pointer::PointerKind;

    let (controller, probe, vsync) = (ScrollController::new(), Probe::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        NestedScroll::new(probe.connection(), list(&controller)),
    )
    .boxed();
    let mut laid = mounted(tree, vsync);
    laid.tick();
    let side_effects = |probe: &Probe| {
        probe
            .steps()
            .into_iter()
            .filter(|step| step.cause == ScrollCause::SideEffect)
            .collect::<Vec<_>>()
    };

    // Hovering in the band along the bottom edge.
    let drag = HoveringDrag::enter(&laid, 290.0);
    drag.tick(&laid, 0);
    drag.tick(&laid, 100);
    let first = controller.pixels();
    assert!(first > 0.0, "the drag scrolls toward the bottom edge");
    drag.tick(&laid, 200);
    let second = controller.pixels();
    assert!(
        (second - 2.0 * first).abs() < 1e-9,
        "a still drag scrolls at a steady speed: {first} then {second}"
    );
    let steps = side_effects(&probe);
    assert_eq!(steps.len(), 2, "one step per timed frame: {steps:?}");
    assert!(
        steps.iter().all(|step| step.available.y > 0.0
            && step.device
                == ScrollDevice::Pointer {
                    kind: PointerKind::Unknown,
                    device: None
                }),
        "{steps:?}"
    );
    let sequence = steps[0].sequence;
    assert!(steps.iter().all(|step| step.sequence == sequence));
    assert_eq!(probe.started(), [sequence], "the drag's own sequence");
    assert!(
        drag.owner.next_deadline().is_some(),
        "a scrolling frame asks for the next"
    );

    // Out of the band: the sequence ends and the list holds still.
    drag.move_to(&laid, 150.0);
    drag.tick(&laid, 300);
    drag.tick(&laid, 400);
    assert_eq!(controller.pixels(), second);
    assert_eq!(probe.ended(), [(sequence, ScrollEndReason::Released)]);
    assert_eq!(
        drag.owner.next_deadline(),
        None,
        "nobody scrolls: no more frames"
    );

    // The band along the top edge scrolls back, under a new sequence.
    drag.move_to(&laid, 10.0);
    drag.tick(&laid, 500);
    drag.tick(&laid, 600);
    assert!(controller.pixels() < second, "the top edge scrolls up");
    let back = side_effects(&probe);
    assert!(
        back[2..]
            .iter()
            .all(|step| step.available.y < 0.0 && step.sequence != sequence)
    );

    // At the top extent nothing moves, and frames stop.
    controller.jump_to(0.0).expect("no build is running");
    let before = side_effects(&probe).len();
    drag.tick(&laid, 700);
    drag.tick(&laid, 800);
    assert_eq!(controller.pixels(), 0.0);
    assert_eq!(side_effects(&probe).len(), before);
    assert_eq!(drag.owner.next_deadline(), None);
    drag.exit(&laid);
}

/// A drop autoscroll over a bouncing list scrolled past its top moves it by
/// each frame's step toward its range, not onto its edge at once, and never
/// farther out.
pub(crate) fn drag_autoscroll_from_past_an_edge_moves_by_its_step() {
    let (controller, vsync) = (ScrollController::new(), Vsync::new());
    let tree = VsyncScope::new(
        vsync.clone(),
        list(&controller).physics(Rc::new(flui_widgets::BouncingScrollPhysics::new())),
    )
    .boxed();
    let laid = mounted(tree, vsync);
    controller.set_pixels(-100.0);

    // Near the bottom edge: toward the content, by the frame's step.
    let drag = HoveringDrag::enter(&laid, 290.0);
    drag.tick(&laid, 0);
    drag.tick(&laid, 16);
    let stepped = controller.pixels();
    assert!(
        stepped > -100.0 && stepped < -50.0,
        "one frame moves the list by its step, at {stepped}"
    );

    // Near the top edge: the list is past it already and stays.
    drag.move_to(&laid, 10.0);
    drag.tick(&laid, 32);
    drag.tick(&laid, 48);
    assert_eq!(controller.pixels(), stepped, "never farther past its top");
    drag.exit(&laid);
}
