//! The normative ownership table (ADR-0208 §1) as a conformance check.
//!
//! [`assert_ownership_table`] runs one row per cell of the table, the 44°,
//! 45° and 46° rows with first-sample noise, and the [`PanClaim`] rows, each
//! for a touch and a mouse contact. Every row scripts its contact with a
//! [`PointerScript`] and dispatches it through a [`HeadlessBinding`] on a hit
//! route of nested targets, the member under test innermost, so the arena sees
//! the members in the order a real route admits them.
//!
//! Widget rows (pager, `InteractiveViewer`, `SwipeActions`, a vertical drag
//! over a settling pager) land with each widget in `flui-widgets`; an anchored
//! drag row is played here by the axial drag it is in the arena.

use std::{
    any::Any,
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    time::Duration,
};

use flui_foundation::geometry::{Axis, Offset};
use flui_interaction::{
    DragAxis, DragGestureRecognizer, GestureRecognizer, HitTestEntry, HitTestResult,
    LongPressGestureRecognizer, MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer,
    MultiDragHandle, MultiDragUpdateDetails, PointerId, PointerKind, RecognizerSet, RenderId,
    ScaleGestureRecognizer, TapAndDragGestureRecognizer,
    gesture::{GestureArena, GestureSettings, PanClaim, ScaleStartMode, SequencedBranch},
};

use crate::{
    HeadlessBinding,
    replay::{PointerPhase, PointerScript, ScriptedPointer},
};

/// Run every row of the ownership table for a touch and a mouse contact.
///
/// # Panics
///
/// With the name and device of every row whose contact went to an owner the
/// table does not give it to.
pub fn assert_ownership_table() {
    let mut failures = Vec::new();
    for device in [PointerKind::Touch, PointerKind::Mouse] {
        for &(name, row) in ROWS {
            if let Err(payload) = catch_unwind(AssertUnwindSafe(|| row(device))) {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
                    .unwrap_or_default();
                failures.push(format!("{name} ({device:?}): {message}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "ownership table rows failed:\n{}",
        failures.join("\n")
    );
}

type Row = fn(PointerKind);

const ROWS: &[(&str, Row)] = &[
    ("axial / orthogonal ancestor", axial_in_orthogonal_ancestor),
    (
        "axial / orthogonal tie",
        axial_orthogonal_tie_goes_innermost,
    ),
    ("axial / same-axis ancestor", axial_in_same_axis_ancestor),
    ("axial / no competitor", axial_alone_holds_until_its_axis),
    ("free / orthogonal ancestor at 44°", free_at_44_degrees),
    ("free / orthogonal ancestor at 45°", free_at_45_degrees),
    ("free / orthogonal ancestor at 46°", free_at_46_degrees),
    ("free / same-axis ancestor", free_in_same_axis_ancestor),
    ("free / PanClaim::None", free_with_pan_claim_none),
    ("free / PanClaim::PanY", free_with_pan_claim_pan_y),
    ("free / no competitor", free_alone),
    ("scale / orthogonal ancestor", scale_in_orthogonal_ancestor),
    ("scale / same-axis ancestor", scale_in_same_axis_ancestor),
    ("scale / no competitor", scale_alone),
    (
        "anchored leaf / orthogonal ancestor",
        anchored_leaf_in_orthogonal_ancestor,
    ),
    (
        "anchored leaf / same-axis ancestor",
        anchored_leaf_in_same_axis_ancestor,
    ),
    ("anchored leaf / no competitor", anchored_leaf_alone),
    (
        "anchored ancestor / orthogonal",
        anchored_ancestor_takes_no_contact,
    ),
    (
        "anchored ancestor / same-axis",
        anchored_ancestor_takes_no_contact,
    ),
    (
        "sequenced / orthogonal ancestor",
        sequenced_in_orthogonal_ancestor,
    ),
    (
        "sequenced / same-axis ancestor",
        sequenced_in_same_axis_ancestor,
    ),
    ("sequenced / no competitor", sequenced_alone),
    (
        "multi-drag free / orthogonal ancestor",
        multidrag_free_in_pager,
    ),
    (
        "multi-drag axial / orthogonal ancestor",
        multidrag_axial_in_pager,
    ),
    ("tap-and-drag / orthogonal ancestor", tap_and_drag_in_pager),
];

const ORIGIN: Offset<f64> = Offset::new(100.0, 100.0);
const POINTER: u64 = 1;
const SAMPLE: Duration = Duration::from_millis(16);
/// Past every built-in long-press timeout.
const HOLD: Duration = Duration::from_millis(700);

fn pointer() -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(POINTER).expect("nonzero pointer id"))
}

/// A first sample that stays inside every slop of `device`, pointing away
/// from the motion that follows.
fn noise(device: PointerKind) -> Offset<f64> {
    if device == PointerKind::Mouse {
        Offset::new(0.4, -0.4)
    } else {
        Offset::new(3.0, -3.0)
    }
}

/// A displacement of 40 px at `degrees` off the horizontal axis.
fn heading(degrees: f64) -> Offset<f64> {
    let radians = degrees.to_radians();
    Offset::new(40.0 * radians.cos(), 40.0 * radians.sin())
}

/// Down, a first noisy sample, then motion to `to` from Down and beyond.
fn swipe(device: PointerKind, to: Offset<f64>) -> PointerScript {
    let event = |at: u32, phase, position| {
        ScriptedPointer::new(SAMPLE * at, pointer(), phase, position).with_device(device)
    };
    PointerScript::new("ownership swipe")
        .with(event(0, PointerPhase::Down, ORIGIN))
        .with(event(1, PointerPhase::Move, ORIGIN + noise(device)))
        .with(event(2, PointerPhase::Move, ORIGIN + to))
        .with(event(3, PointerPhase::Move, ORIGIN + to * 1.5))
        .with(event(4, PointerPhase::Up, ORIGIN + to * 1.5))
}

/// Down, held still past the long-press timeout, then moved along `to`.
fn hold_then_move(device: PointerKind, to: Offset<f64>) -> PointerScript {
    let event = |at: Duration, phase, position| {
        ScriptedPointer::new(at, pointer(), phase, position).with_device(device)
    };
    PointerScript::new("ownership hold")
        .with(event(Duration::ZERO, PointerPhase::Down, ORIGIN))
        .with(event(HOLD, PointerPhase::Move, ORIGIN))
        .with(event(HOLD + SAMPLE, PointerPhase::Move, ORIGIN + to))
        .with(event(
            HOLD + SAMPLE * 2,
            PointerPhase::Move,
            ORIGIN + to * 1.5,
        ))
        .with(event(
            HOLD + SAMPLE * 3,
            PointerPhase::Up,
            ORIGIN + to * 1.5,
        ))
}

type Starts = Rc<RefCell<Vec<&'static str>>>;

/// A hit route under construction: levels innermost first, each a target
/// that feeds its recognizers.
struct Route {
    binding: HeadlessBinding,
    levels: Vec<RecognizerSet>,
    starts: Starts,
    owners: Vec<Rc<dyn Any>>,
}

impl Route {
    fn new() -> Self {
        Self {
            binding: HeadlessBinding::new(),
            levels: Vec::new(),
            starts: Starts::default(),
            owners: Vec::new(),
        }
    }

    fn arena(&self) -> GestureArena {
        self.binding.arena().clone()
    }

    /// Open the next level outwards.
    fn level(&mut self) -> &mut Self {
        self.levels.push(RecognizerSet::default());
        self
    }

    fn attach<R: GestureRecognizer + 'static>(&mut self, recognizer: Rc<R>) -> &mut Self {
        self.levels
            .last_mut()
            .expect("BUG: a row attaches after opening a level")
            .attach(&recognizer);
        self.owners.push(recognizer);
        self
    }

    fn drag(&mut self, name: &'static str, arena: GestureArena, axis: DragAxis) -> &mut Self {
        let starts = Rc::clone(&self.starts);
        let drag = DragGestureRecognizer::builder(arena, axis)
            .on_start(move |_| starts.borrow_mut().push(name))
            .build();
        self.attach(drag)
    }

    fn axial(&mut self, name: &'static str, axis: Axis) -> &mut Self {
        let arena = self.arena();
        let axis = match axis {
            Axis::Horizontal => DragAxis::Horizontal,
            Axis::Vertical => DragAxis::Vertical,
        };
        self.drag(name, arena, axis)
    }

    fn multi_drag(&mut self, name: &'static str, axis: MultiDragAxis) -> &mut Self {
        struct Client;
        impl MultiDragHandle for Client {
            fn update(&self, _: MultiDragUpdateDetails) {}
            fn end(&self, _: MultiDragEndDetails) {}
            fn cancel(&self) {}
        }
        let starts = Rc::clone(&self.starts);
        let drag = MultiDragGestureRecognizer::builder(self.arena(), axis)
            .on_start(move |_, _| {
                starts.borrow_mut().push(name);
                Some(Rc::new(Client) as Rc<dyn MultiDragHandle>)
            })
            .build();
        self.attach(drag)
    }

    fn tap_and_drag(&mut self, name: &'static str) -> &mut Self {
        let starts = Rc::clone(&self.starts);
        let drag = TapAndDragGestureRecognizer::builder(self.arena())
            .on_drag_start(move |_| starts.borrow_mut().push(name))
            .build();
        self.attach(drag)
    }

    fn pan(&mut self, name: &'static str, arena: GestureArena) -> &mut Self {
        self.drag(name, arena, DragAxis::Free)
    }

    fn scale(&mut self, name: &'static str) -> &mut Self {
        let starts = Rc::clone(&self.starts);
        // One-contact pan or pinch, as `InteractiveViewer` recognizes.
        let scale = ScaleGestureRecognizer::builder(self.arena())
            .start_mode(ScaleStartMode::PanOrScale)
            .on_start(move |_| starts.borrow_mut().push(name))
            .build();
        self.attach(scale)
    }

    /// A long-press hold whose first move hands its won contact to a pan.
    fn sequenced(&mut self) -> &mut Self {
        let branch: Rc<SequencedBranch> =
            Rc::new(self.arena().sequenced().expect("sequenced composition"));
        let (starts, handing) = (Rc::clone(&self.starts), Rc::clone(&branch));
        let hold = LongPressGestureRecognizer::builder(branch.hold())
            .on_long_press_start(move |_| starts.borrow_mut().push("hold"))
            .on_long_press_move_update(move |_| {
                handing.hand_off(pointer());
            })
            .build();
        self.attach(hold);
        self.pan("successor", branch.successor());
        self.owners.push(branch);
        self
    }

    /// Replay `script` on this route and report who started, in order.
    fn run(mut self, script: &PointerScript) -> Vec<&'static str> {
        let handle = self.binding.interaction_dispatch_handle();
        let levels = std::mem::take(&mut self.levels);
        let route = self.binding.enter_owner_scope(|| {
            let mut route = HitTestResult::new();
            for (index, set) in levels.into_iter().enumerate() {
                let target = handle
                    .register_pointer(move |dispatch| set.dispatch(dispatch))
                    .expect("register ownership level");
                route.add(HitTestEntry::new(RenderId::new(index + 1)).pointer_target(target));
            }
            route
        });
        self.binding.replay_with(script, |_, _, _| route.clone());
        self.binding.pump_frame(SAMPLE);
        self.starts.take()
    }
}

fn axial_in_orthogonal_ancestor(device: PointerKind) {
    for (degrees, owner) in [(80.0, "list"), (10.0, "pager")] {
        let mut route = Route::new();
        route.level().axial("list", Axis::Vertical);
        route.level().axial("pager", Axis::Horizontal);
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            [owner],
            "the larger accumulated component owns at {degrees}°"
        );
    }
}

fn axial_orthogonal_tie_goes_innermost(device: PointerKind) {
    let mut route = Route::new();
    route.level().axial("list", Axis::Vertical);
    route.level().axial("pager", Axis::Horizontal);
    assert_eq!(route.run(&swipe(device, Offset::new(40.0, 40.0))), ["list"]);
}

fn axial_in_same_axis_ancestor(device: PointerKind) {
    let mut route = Route::new();
    route.level().axial("inner", Axis::Vertical);
    route.level().axial("outer", Axis::Vertical);
    assert_eq!(route.run(&swipe(device, heading(90.0))), ["inner"]);
}

fn axial_alone_holds_until_its_axis(device: PointerKind) {
    for (degrees, owners) in [(0.0, &[][..]), (90.0, &["list"][..])] {
        let mut route = Route::new();
        route.level().axial("list", Axis::Vertical);
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            owners,
            "motion at {degrees}°"
        );
    }
}

fn free_in_pager(device: PointerKind, degrees: f64, owner: &str) {
    let mut route = Route::new();
    let arena = route.arena();
    route.level().pan("pan", arena);
    route.level().axial("pager", Axis::Horizontal);
    assert_eq!(route.run(&swipe(device, heading(degrees))), [owner]);
}

fn free_at_44_degrees(device: PointerKind) {
    free_in_pager(device, 44.0, "pager");
}

fn free_at_45_degrees(device: PointerKind) {
    let mut route = Route::new();
    let arena = route.arena();
    route.level().pan("pan", arena);
    route.level().axial("pager", Axis::Horizontal);
    // Exactly 45°: the axial member's axis takes |d| cos 45°.
    assert_eq!(
        route.run(&swipe(device, Offset::new(40.0, 40.0))),
        ["pager"]
    );
}

fn free_at_46_degrees(device: PointerKind) {
    free_in_pager(device, 46.0, "pan");
}

fn free_in_same_axis_ancestor(device: PointerKind) {
    for (degrees, owner) in [(60.0, "scroll"), (30.0, "pan")] {
        let mut route = Route::new();
        let arena = route.arena();
        route.level().pan("pan", arena);
        route.level().axial("scroll", Axis::Vertical);
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            [owner],
            "motion at {degrees}°"
        );
    }
}

fn free_with_pan_claim_none(device: PointerKind) {
    let mut route = Route::new();
    let map = route.arena().with_pan_claim(PanClaim::None);
    route.level().pan("map", map);
    route.level().axial("pager", Axis::Horizontal);
    assert_eq!(route.run(&swipe(device, heading(0.0))), ["map"]);
}

fn free_with_pan_claim_pan_y(device: PointerKind) {
    for (degrees, owner) in [(0.0, "pan"), (90.0, "scroll")] {
        let mut route = Route::new();
        let region = route.arena().with_pan_claim(PanClaim::PanY);
        route.level().pan("pan", region);
        route.level().axial("pager", Axis::Horizontal);
        route.level().axial("scroll", Axis::Vertical);
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            [owner],
            "motion at {degrees}°"
        );
    }
}

fn free_alone(device: PointerKind) {
    let mut route = Route::new();
    let arena = route.arena();
    route.level().pan("pan", arena);
    assert_eq!(route.run(&swipe(device, heading(20.0))), ["pan"]);
}

/// The ancestor claims first: its 1 px slop is crossed by a first sample
/// that stays inside the leaf's slop. A channel leaf still keeps the contact.
fn scale_in(device: PointerKind, ancestor: Option<Axis>, degrees: f64) {
    let mut route = Route::new();
    route.level().scale("scale");
    if let Some(axis) = ancestor {
        let eager = GestureSettings::default()
            .try_with_pan_slop_horizontal(1.0)
            .and_then(|settings| settings.try_with_pan_slop_vertical(1.0))
            .expect("valid slop");
        let starts = Rc::clone(&route.starts);
        let drag_axis = match axis {
            Axis::Horizontal => DragAxis::Horizontal,
            Axis::Vertical => DragAxis::Vertical,
        };
        let ancestor = DragGestureRecognizer::builder(route.arena(), drag_axis)
            .settings(eager)
            .on_start(move |_| starts.borrow_mut().push("ancestor"))
            .build();
        route.level().attach(ancestor);
    }
    let to = heading(degrees);
    let event = |at: u32, phase, position| {
        ScriptedPointer::new(SAMPLE * at, pointer(), phase, position).with_device(device)
    };
    let script = PointerScript::new("ownership leaf")
        .with(event(0, PointerPhase::Down, ORIGIN))
        .with(event(1, PointerPhase::Move, ORIGIN + to * (1.5 / 40.0)))
        .with(event(2, PointerPhase::Move, ORIGIN + to))
        .with(event(3, PointerPhase::Up, ORIGIN + to));
    assert_eq!(route.run(&script), ["scale"]);
}

fn scale_in_orthogonal_ancestor(device: PointerKind) {
    scale_in(device, Some(Axis::Horizontal), 0.0);
}

fn scale_in_same_axis_ancestor(device: PointerKind) {
    scale_in(device, Some(Axis::Vertical), 90.0);
}

fn scale_alone(device: PointerKind) {
    scale_in(device, None, 30.0);
}

fn anchored_leaf_in(device: PointerKind, ancestor: Option<Axis>) {
    let mut route = Route::new();
    route.level().axial("row", Axis::Horizontal);
    if let Some(axis) = ancestor {
        route.level().axial("ancestor", axis);
    }
    assert_eq!(route.run(&swipe(device, heading(0.0))), ["row"]);
}

fn anchored_leaf_in_orthogonal_ancestor(device: PointerKind) {
    anchored_leaf_in(device, Some(Axis::Vertical));
}

fn anchored_leaf_in_same_axis_ancestor(device: PointerKind) {
    anchored_leaf_in(device, Some(Axis::Horizontal));
}

fn anchored_leaf_alone(device: PointerKind) {
    anchored_leaf_in(device, None);
}

/// A dismissing ancestor joins the scroll channel, not the arena: the leaf
/// owns the contact whichever way it moves.
fn anchored_ancestor_takes_no_contact(device: PointerKind) {
    for degrees in [0.0, 90.0] {
        let mut route = Route::new();
        route.level().scale("photo");
        route.level();
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            ["photo"],
            "motion at {degrees}°"
        );
    }
}

fn sequenced_in(device: PointerKind, ancestor: Option<Axis>, to: Offset<f64>) {
    let mut route = Route::new();
    route.level().sequenced();
    if let Some(axis) = ancestor {
        route.level().axial("ancestor", axis);
    }
    assert_eq!(
        route.run(&hold_then_move(device, to)),
        ["hold", "successor"],
        "the hold owns the contact, then hands it to its successor"
    );
}

fn sequenced_in_orthogonal_ancestor(device: PointerKind) {
    sequenced_in(device, Some(Axis::Horizontal), heading(0.0));
}

fn sequenced_in_same_axis_ancestor(device: PointerKind) {
    sequenced_in(device, Some(Axis::Vertical), heading(90.0));
}

fn sequenced_alone(device: PointerKind) {
    sequenced_in(device, None, heading(30.0));
}

/// Each multi-drag contact is a free member: at 46° it keeps the contact, at
/// 44° the pager takes it.
fn multidrag_free_in_pager(device: PointerKind) {
    for (degrees, owner) in [(46.0, "contacts"), (44.0, "pager")] {
        let mut route = Route::new();
        route.level().multi_drag("contacts", MultiDragAxis::Free);
        route.level().axial("pager", Axis::Horizontal);
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            [owner],
            "motion at {degrees}°"
        );
    }
}

fn multidrag_axial_in_pager(device: PointerKind) {
    for (degrees, owner) in [(80.0, "contacts"), (10.0, "pager")] {
        let mut route = Route::new();
        route
            .level()
            .multi_drag("contacts", MultiDragAxis::Vertical);
        route.level().axial("pager", Axis::Horizontal);
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            [owner],
            "motion at {degrees}°"
        );
    }
}

/// A tap-and-drag drags freely: it yields to the pager at 44°, not at 46°.
fn tap_and_drag_in_pager(device: PointerKind) {
    for (degrees, owner) in [(46.0, "selection"), (44.0, "pager")] {
        let mut route = Route::new();
        route.level().tap_and_drag("selection");
        route.level().axial("pager", Axis::Horizontal);
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            [owner],
            "motion at {degrees}°"
        );
    }
}
