//! The normative ownership table (ADR-0208 §1) as a conformance check.
//!
//! [`assert_ownership_table`] runs one row per cell of the table, the 44°,
//! 45° and 46° rows with first-sample noise, the [`PanClaim`] rows and an
//! `allowed_devices` row (a pen draws, a finger scrolls), each
//! for a touch and a mouse contact. Every row scripts its contact with a
//! [`PointerScript`] and dispatches it through a [`HeadlessBinding`] on a hit
//! route of nested targets, the member under test innermost, so the arena sees
//! the members in the order a real route admits them.
//!
//! The widget rows mount the widgets themselves: a `PageView` over a
//! vertical drag and a tap, an `InteractiveViewer` in a `PageView`, and a
//! press or a vertical drag on a pager still settling, which the hold on Down
//! (ADR-0208 rule 4) stops without the press becoming a tap. The anchored
//! leaf rows mount a `SwipeActions` row in a vertical list, on a pager's page
//! and alone.

use std::{
    any::Any,
    cell::RefCell,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
    time::Duration,
};

use flui_foundation::geometry::{Axis, EdgeInsets, Offset};
use flui_interaction::{
    DragAxis, DragGestureRecognizer, GestureRecognizer, HitTestEntry, HitTestResult,
    LongPressGestureRecognizer, MultiDragAxis, MultiDragEndDetails, MultiDragGestureRecognizer,
    MultiDragHandle, MultiDragUpdateDetails, PointerId, PointerKind, PointerKindSet, RecognizerSet,
    RenderId, ScaleGestureRecognizer, TapAndDragGestureRecognizer,
    gesture::{GestureArena, GestureSettings, PanClaim, ScaleStartMode, SequencedBranch},
};
use flui_painting::styling::Color;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;
use flui_view::{BoxedView, LifecycleContext, ViewState};
use flui_widgets::{
    Alternative, ColoredBox, Column, GestureAlternatives, GestureArenaScope, GestureCommand,
    GestureDetector, InteractiveViewer, Listener, MenuSpec, PageController, PageIntent, PageView,
    ScrollController, Scrollable, SizedBox, SwipeAction, SwipeActions, SwipeAlternative,
    ZoomIntent,
};

use crate::{
    HeadlessBinding,
    replay::{MouseScript, PenScript, PointerScript, TouchScript},
    widgets::{LaidOut, lay_out, tight},
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
    ("axial / channel-leaf ancestor", axial_in_channel_leaf),
    ("free / orthogonal ancestor at 44°", free_at_44_degrees),
    ("free / orthogonal ancestor at 45°", free_at_45_degrees),
    ("free / orthogonal ancestor at 46°", free_at_46_degrees),
    ("free / same-axis ancestor", free_in_same_axis_ancestor),
    ("free / PanClaim::None", free_with_pan_claim_none),
    ("free / PanClaim::PanY", free_with_pan_claim_pan_y),
    ("free / PanClaim::PanX", free_with_pan_claim_pan_x),
    (
        "axial inside a PanClaim::None region",
        axial_inside_a_pan_claim_none_region,
    ),
    ("free / allowed_devices: pen", free_admitting_only_a_pen),
    ("free / no competitor", free_alone),
    ("free / channel-leaf ancestor", free_in_channel_leaf),
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
    ("widget: pager / vertical drag and tap", pager_widget),
    (
        "widget: InteractiveViewer / pager",
        interactive_viewer_in_pager_widget,
    ),
    (
        "widget: press on a settling pager",
        press_on_settling_pager_widget,
    ),
    (
        "widget: vertical drag over a settling pager",
        vertical_drag_over_settling_pager_widget,
    ),
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

/// One contact of `device`: down at [`ORIGIN`], then each `(wait, position)`
/// move, then up a sample later where it is. Touch and mouse scripts both
/// name their contact [`POINTER`].
fn contact(device: PointerKind, path: &[(Duration, Offset<f64>)]) -> PointerScript {
    let script = if device == PointerKind::Mouse {
        let mut mouse = MouseScript::new("ownership contact").press(ORIGIN);
        for &(wait, position) in path {
            mouse = mouse.wait(wait).move_to(position);
        }
        mouse.wait(SAMPLE).release().build()
    } else {
        let mut touch = TouchScript::new("ownership contact").down(POINTER, ORIGIN);
        for &(wait, position) in path {
            touch = touch.wait(wait).move_to(POINTER, position);
        }
        touch.wait(SAMPLE).up(POINTER).build()
    };
    script.expect("BUG: an ownership script ends every contact it starts")
}

/// Down, a first noisy sample, then motion to `to` from Down and beyond.
fn swipe(device: PointerKind, to: Offset<f64>) -> PointerScript {
    contact(
        device,
        &[
            (SAMPLE, ORIGIN + noise(device)),
            (SAMPLE, ORIGIN + to),
            (SAMPLE, ORIGIN + to * 1.5),
        ],
    )
}

/// Down, held still past the long-press timeout, then moved along `to`.
fn hold_then_move(device: PointerKind, to: Offset<f64>) -> PointerScript {
    contact(
        device,
        &[
            (HOLD, ORIGIN),
            (SAMPLE, ORIGIN + to),
            (SAMPLE, ORIGIN + to * 1.5),
        ],
    )
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

/// A list inside a viewer: the innermost claimant owns its axis, and the
/// viewer, which yields to no ancestor, takes what the list's axis does not
/// (rule 2, the viewer standing as the free member).
fn axial_in_channel_leaf(device: PointerKind) {
    for (degrees, owner) in [
        (90.0, "list"),
        (46.0, "list"),
        (44.0, "viewer"),
        (0.0, "viewer"),
    ] {
        let mut route = Route::new();
        route.level().axial("list", Axis::Vertical);
        route.level().scale("viewer");
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            [owner],
            "motion at {degrees}°"
        );
    }
}

/// A pan inside a viewer is the innermost claimant whichever way it moves.
fn free_in_channel_leaf(device: PointerKind) {
    for degrees in [0.0, 90.0] {
        let mut route = Route::new();
        let arena = route.arena();
        route.level().pan("pan", arena);
        route.level().scale("viewer");
        assert_eq!(
            route.run(&swipe(device, heading(degrees))),
            ["pan"],
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

/// A carousel inside a map's `PanClaim::None` region takes its axis from the
/// map's pan whether or not it declares a region of its own: the region
/// encloses both members, so it declares nothing between them.
fn axial_inside_a_pan_claim_none_region(device: PointerKind) {
    for own in [None, Some(PanClaim::Auto), Some(PanClaim::PanY)] {
        let mut route = Route::new();
        let map = route.arena().with_pan_claim(PanClaim::None);
        let carousel = own.map_or_else(|| map.clone(), |claim| map.with_pan_claim(claim));
        route
            .level()
            .drag("carousel", carousel, DragAxis::Horizontal);
        route.level().pan("map", map);
        assert_eq!(
            route.run(&swipe(device, heading(0.0))),
            ["carousel"],
            "carousel region {own:?}"
        );
    }
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

fn free_with_pan_claim_pan_x(device: PointerKind) {
    for (degrees, owner) in [(90.0, "pan"), (0.0, "pager")] {
        let mut route = Route::new();
        let region = route.arena().with_pan_claim(PanClaim::PanX);
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

/// A pen-only pan in a vertical list, moved at 60° off the list's axis: the
/// pen draws, where the table gives a free member such a contact, and the
/// other device scrolls, because admission refused it to the pan before any
/// rule of the table ran.
fn free_admitting_only_a_pen(device: PointerKind) {
    const DEGREES: f64 = 30.0;
    let pen_swipe = || {
        let to = heading(DEGREES);
        PenScript::new("pen contact")
            .down(ORIGIN, 0.5)
            .wait(SAMPLE)
            .move_to(ORIGIN + noise(PointerKind::Touch), 0.5)
            .wait(SAMPLE)
            .move_to(ORIGIN + to, 0.5)
            .wait(SAMPLE)
            .move_to(ORIGIN + to * 1.5, 0.5)
            .wait(SAMPLE)
            .up()
            .build()
            .expect("BUG: the pen lifts")
    };
    for (pen, owner) in [(false, "scroll"), (true, "pen")] {
        let mut route = Route::new();
        let starts = Rc::clone(&route.starts);
        let drawing = DragGestureRecognizer::builder(route.arena(), DragAxis::Free)
            .allowed_devices(PointerKindSet::PEN)
            .on_start(move |_| starts.borrow_mut().push("pen"))
            .build();
        route.level().attach(drawing);
        route.level().axial("scroll", Axis::Vertical);
        let script = if pen {
            pen_swipe()
        } else {
            swipe(device, heading(DEGREES))
        };
        assert_eq!(route.run(&script), [owner], "a pen contact: {pen}");
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
    let script = contact(
        device,
        &[(SAMPLE, ORIGIN + to * (1.5 / 40.0)), (SAMPLE, ORIGIN + to)],
    );
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

/// The swipe row's content.
const ROW: Color = Color::rgb(30, 90, 150);

/// A command a swipe row's action performs; nothing answers it.
struct Act;
impl flui_widgets::Intent for Act {}

/// A 300 × 60 swipe row, an action at each edge, its top 70 px into its
/// parent so [`ORIGIN`] lands on it.
fn swipe_row() -> BoxedView {
    let action = |label: &'static str| {
        SwipeAction::new(GestureCommand::new(label, Act), SizedBox::new(10.0, 10.0))
    };
    let row = SwipeActions::try_new(
        ColoredBox::new(ROW).child(SizedBox::new(300.0, 60.0)),
        [action("Pin")],
        [action("Delete")],
        SwipeAlternative::PresentedMenu(Rc::new(|_cx, _request| {})),
    )
    .expect("BUG: two labelled actions");
    Column::new(vec![SizedBox::new(300.0, 70.0).boxed(), row.boxed()])
        .main_axis_size(flui_widgets::MainAxisSize::Min)
        .boxed()
}

/// How far the row's content is painted from its rest, through every layer
/// above its picture.
fn row_moved(laid: &LaidOut) -> bool {
    use flui_rendering::layer::Layer;
    let tree = laid.layer_tree().expect("BUG: a laid-out tree has a scene");
    tree.iter()
        .filter_map(|(_, node)| match node.layer() {
            Layer::Picture(picture) => Some((node, picture)),
            _ => None,
        })
        .find_map(|(node, picture)| {
            let command = picture.picture().iter().find(|command| {
                matches!(&command.op, flui_painting::display_list::DrawOp::Rect { paint, .. }
                    if paint.color == ROW)
            })?;
            let flui_painting::display_list::DrawOp::Rect { rect, .. } = &command.op else {
                return None;
            };
            let (x, y) = command.transform.transform_point(rect.left(), rect.top());
            let mut point = flui_foundation::geometry::Point::new(x, y);
            let mut parent = node.parent();
            while let Some(id) = parent {
                let ancestor = tree.get(id)?;
                match ancestor.layer() {
                    Layer::Transform(layer) => point = layer.transform_point(point),
                    Layer::Offset(layer) => point += layer.offset(),
                    _ => {}
                }
                parent = ancestor.parent();
            }
            Some(point.x.abs() > 0.5)
        })
        .expect("BUG: the swipe row paints its content")
}

/// Who took a swipe at `degrees`: the row, the ancestor whose offset
/// `ancestor` reads, or both.
fn swipe_owners(
    laid: &mut LaidOut,
    device: PointerKind,
    degrees: f64,
    ancestor: impl Fn() -> f64,
) -> Vec<&'static str> {
    laid.replay(&swipe(device, heading(degrees)));
    laid.pump_for(SAMPLE);
    let mut owners = Vec::new();
    if row_moved(laid) {
        owners.push("row");
    }
    if ancestor() != 0.0 {
        owners.push("ancestor");
    }
    owners
}

/// A swipe row in a vertical list: the larger accumulated component owns
/// the contact, the row's axis at 0°, the list's upward at 270°.
fn anchored_leaf_in_orthogonal_ancestor(device: PointerKind) {
    for (degrees, owner) in [(0.0, "row"), (270.0, "ancestor")] {
        let list = ScrollController::new();
        let content = Column::new(vec![swipe_row(), SizedBox::new(300.0, 1000.0).boxed()]);
        let mut laid = lay_out(
            Scrollable::new().controller(list.clone()).child(content),
            tight(PAGE, PAGE),
        );
        assert_eq!(
            swipe_owners(&mut laid, device, degrees, || list.pixels()),
            [owner],
            "motion at {degrees}°"
        );
    }
}

/// A swipe row on a pager's page: the inner row owns the horizontal
/// contact, and the pager stays on its page.
fn anchored_leaf_in_same_axis_ancestor(device: PointerKind) {
    let controller = PageController::new();
    let mut laid = pager(&controller, swipe_row);
    assert_eq!(
        swipe_owners(&mut laid, device, 0.0, || page_of(&controller)),
        ["row"]
    );
}

/// A swipe row alone takes its axis.
fn anchored_leaf_alone(device: PointerKind) {
    let mut laid = lay_out(swipe_row(), tight(PAGE, PAGE));
    assert_eq!(swipe_owners(&mut laid, device, 0.0, || 0.0), ["row"]);
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

/// The page content of the widget rows: a vertical drag around a tap, each
/// logging its start.
#[derive(Clone, StatefulView)]
struct VerticalDragArea {
    starts: Starts,
}

impl std::fmt::Debug for VerticalDragArea {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerticalDragArea").finish_non_exhaustive()
    }
}

#[derive(Debug, Default)]
struct VerticalDragAreaState {
    drag: Option<Rc<DragGestureRecognizer>>,
    /// The current view's log, which the recognizer's callback writes.
    log: Rc<RefCell<Starts>>,
}

impl StatefulView for VerticalDragArea {
    type State = VerticalDragAreaState;

    fn create_state(&self) -> Self::State {
        VerticalDragAreaState::default()
    }
}

impl ViewState<VerticalDragArea> for VerticalDragAreaState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let log = Rc::clone(&self.log);
        self.drag = Some(
            DragGestureRecognizer::builder(GestureArenaScope::of(ctx), DragAxis::Vertical)
                .on_start(move |_| log.borrow().borrow_mut().push("vertical"))
                .build(),
        );
    }

    fn build(&self, view: &VerticalDragArea, _: &dyn BuildContext) -> impl IntoView {
        let previous = self.log.replace(Rc::clone(&view.starts));
        drop(previous);
        let tapped = Rc::clone(&view.starts);
        let mut listener = Listener::new().behavior(HitTestBehavior::Opaque);
        if let Some(drag) = &self.drag {
            listener = listener.recognizer(drag);
        }
        listener.child(
            GestureDetector::new()
                .on_tap(move |_| tapped.borrow_mut().push("tap"))
                .child(ColoredBox::new(Color::rgb(20, 40, 60))),
        )
    }
}

const PAGE: f64 = 300.0;

/// A horizontal pager of three pages built by `page`, mounted.
fn pager(controller: &PageController, page: impl Fn() -> BoxedView) -> LaidOut {
    let pages: Vec<BoxedView> = (0..3).map(|_| page()).collect();
    lay_out(
        PageView::new(pages, pager_alternatives()).controller(controller.clone()),
        tight(PAGE, PAGE),
    )
}

fn page_of(controller: &PageController) -> f64 {
    controller.page().expect("BUG: a laid-out pager has a page")
}

fn drag_pages(starts: &Starts) -> impl Fn() -> BoxedView {
    let starts = Rc::clone(starts);
    move || {
        VerticalDragArea {
            starts: Rc::clone(&starts),
        }
        .into_view()
        .boxed()
    }
}

/// A press held still for `hold`, then lifted where it went down.
fn press(device: PointerKind, hold: Duration) -> PointerScript {
    contact(device, &[(hold, ORIGIN)])
}

/// A pager over a vertical drag and a tap: the larger accumulated component
/// owns a moving contact, and a still press is a tap.
fn pager_widget(device: PointerKind) {
    for (degrees, owners, paged) in [(190.0, &[][..], true), (100.0, &["vertical"][..], false)] {
        let starts = Starts::default();
        let controller = PageController::new();
        let mut laid = pager(&controller, drag_pages(&starts));
        laid.replay(&swipe(device, heading(degrees)));
        assert_eq!(*starts.borrow(), owners, "motion at {degrees}°");
        assert_eq!(
            page_of(&controller) > 0.0,
            paged,
            "the pager moved at {degrees}°"
        );
    }
    let starts = Starts::default();
    let controller = PageController::new();
    let mut laid = pager(&controller, drag_pages(&starts));
    laid.replay(&press(device, SAMPLE));
    laid.pump_for(SAMPLE);
    assert_eq!(*starts.borrow(), ["tap"], "a still press on an idle pager");
}

/// A viewer is a channel leaf: it keeps the contact its pager would take.
fn interactive_viewer_in_pager_widget(device: PointerKind) {
    let starts = Starts::default();
    let controller = PageController::new();
    let viewer = {
        let starts = Rc::clone(&starts);
        move || {
            let starts = Rc::clone(&starts);
            InteractiveViewer::new(viewer_alternatives())
                .boundary_margin(EdgeInsets::all(PAGE))
                .on_interaction_start(move |_, _| starts.borrow_mut().push("viewer"))
                .child(ColoredBox::new(Color::rgb(20, 40, 60)))
                .into_view()
                .boxed()
        }
    };
    let mut laid = pager(&controller, viewer);
    laid.replay(&swipe(device, heading(180.0)));
    assert_eq!(*starts.borrow(), ["viewer"]);
    assert!(
        page_of(&controller).abs() < f64::EPSILON,
        "the pager stayed on its page"
    );
}

/// A viewer's zoom commands in a context menu nobody opens, so the viewer
/// keeps the whole page.
fn viewer_alternatives() -> GestureAlternatives {
    GestureAlternatives::try_new(Alternative::ContextMenu(
        MenuSpec::new(
            GestureCommand::new("Zoom in", ZoomIntent::In),
            |_cx, _request| {},
        )
        .item(GestureCommand::new("Zoom out", ZoomIntent::Out)),
    ))
    .expect("BUG: two distinct labels")
}

/// A pager's page commands in a context menu nobody opens, so the pager
/// keeps the whole row.
fn pager_alternatives() -> GestureAlternatives {
    GestureAlternatives::try_new(Alternative::ContextMenu(
        MenuSpec::new(
            GestureCommand::new("Previous page", PageIntent::Previous),
            |_cx, _request| {},
        )
        .item(GestureCommand::new("Next page", PageIntent::Next)),
    ))
    .expect("BUG: two distinct labels")
}

/// Two seconds of frames, for a settling pager to come to rest.
fn settle(laid: &mut LaidOut) {
    for _ in 0..125 {
        laid.pump_for(SAMPLE);
    }
}

/// Swipe a pager part of the way towards its next page and release, so it is
/// settling when the row presses it.
fn settling_pager(device: PointerKind, starts: &Starts) -> (LaidOut, PageController) {
    let controller = PageController::new();
    let mut laid = pager(&controller, drag_pages(starts));
    laid.replay(&swipe(device, heading(180.0)));
    laid.pump_for(SAMPLE);
    let page = page_of(&controller);
    assert!(
        page.fract() != 0.0,
        "the released pager is settling, at page {page}"
    );
    (laid, controller)
}

/// A press on a settling pager stops it and is not a tap; lifted, the pager
/// still comes to rest on a page.
fn press_on_settling_pager_widget(device: PointerKind) {
    let starts = Starts::default();
    let (mut laid, controller) = settling_pager(device, &starts);
    let mut pressed = None;
    laid.replay_with(&press(device, SAMPLE * 3), |_, step| {
        let page = page_of(&controller);
        match step {
            0 => pressed = Some(page),
            _ => assert_eq!(Some(page), pressed, "the press held the pager"),
        }
    });
    settle(&mut laid);
    assert!(starts.borrow().is_empty(), "the press was not a tap");
    let page = page_of(&controller);
    assert!(page.fract() == 0.0, "the pager rests on a page, not {page}");
}

/// A vertical drag pressed on a settling pager stops it, then owns the
/// contact; the pager comes to rest on a page.
fn vertical_drag_over_settling_pager_widget(device: PointerKind) {
    let starts = Starts::default();
    let (mut laid, controller) = settling_pager(device, &starts);
    let vertical = contact(
        device,
        &[
            (SAMPLE, ORIGIN + Offset::new(0.0, 0.5)),
            (SAMPLE, ORIGIN + Offset::new(0.0, 40.0)),
            (SAMPLE, ORIGIN + Offset::new(0.0, 60.0)),
        ],
    );
    let mut pressed = None;
    laid.replay_with(&vertical, |_, step| {
        let page = page_of(&controller);
        match step {
            0 => pressed = Some(page),
            1 => assert_eq!(Some(page), pressed, "the press held the pager"),
            _ => {}
        }
    });
    settle(&mut laid);
    assert_eq!(*starts.borrow(), ["vertical"]);
    let page = page_of(&controller);
    assert!(page.fract() == 0.0, "the pager rests on a page, not {page}");
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
