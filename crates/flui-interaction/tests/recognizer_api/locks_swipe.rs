//! Axis and transform locks and the swipe recognizer (ADR-0208 §4).
use std::{cell::RefCell, rc::Rc};

use flui_foundation::geometry::Offset;
use flui_interaction::events::{
    PointerEvent, PointerKind, make_down_event_for_id, make_move_event_for_id, make_up_event_for_id,
};
use flui_interaction::gesture::GestureArena;
use flui_interaction::pointer::PointerDispatch;
use flui_interaction::{
    AxisLock, DragAxis, DragGestureRecognizer, GestureRecognizer, PointerId,
    ScaleGestureRecognizer, TransformLock, gesture::ScaleStartMode,
};
use flui_platform_api::EventTime;

fn pointer(raw: u64) -> PointerId {
    PointerId::new(std::num::NonZeroU64::new(raw).expect("nonzero pointer"))
}

/// `event` stamped `ms` milliseconds into the timeline.
fn timed(mut event: PointerEvent, ms: u64) -> PointerEvent {
    let time = EventTime::from_nanos(ms * 1_000_000);
    match &mut event {
        PointerEvent::Down(down) => down.sample.time = time,
        PointerEvent::Up(up) => up.sample.time = time,
        PointerEvent::Move(movement) => {
            let mut sample = *movement.current();
            sample.time = time;
            *movement = flui_interaction::events::PointerMove::new(
                movement.pointer,
                movement.buttons,
                sample,
            );
        }
        _ => {}
    }
    event
}

fn down(id: u64, at: Offset<f64>, kind: PointerKind, ms: u64) -> PointerEvent {
    timed(
        make_down_event_for_id(pointer(id), at, kind).expect("finite down"),
        ms,
    )
}

fn moved(id: u64, at: Offset<f64>, kind: PointerKind, ms: u64) -> PointerEvent {
    timed(
        make_move_event_for_id(pointer(id), at, kind).expect("finite move"),
        ms,
    )
}

fn up(id: u64, at: Offset<f64>, kind: PointerKind, ms: u64) -> PointerEvent {
    timed(
        make_up_event_for_id(pointer(id), at, kind).expect("finite up"),
        ms,
    )
}

/// A lone recognizer's contacts: each Down admitted and won by default.
fn admit(arena: &GestureArena, recognizer: &dyn GestureRecognizer, downs: &[PointerEvent]) {
    for event in downs {
        recognizer.add_pointer(PointerDispatch::at_root(event));
    }
    for event in downs {
        if let PointerEvent::Down(down) = event {
            arena.close(down.pointer.id);
        }
    }
    arena.drain_deferred_resolutions();
}

fn send(recognizer: &dyn GestureRecognizer, event: &PointerEvent) {
    recognizer.handle_event(PointerDispatch::at_root(event));
}

/// 60 px at `degrees` off the horizontal, in 6 samples 16 ms apart.
fn path(degrees: f64) -> impl Iterator<Item = (Offset<f64>, u64)> {
    let radians = degrees.to_radians();
    (1..=6_u32).map(move |step| {
        let distance = 10.0 * f64::from(step);
        (
            Offset::new(distance * radians.cos(), distance * radians.sin()),
            u64::from(step) * 16,
        )
    })
}

/// A pan with `lock` at `degrees` by `kind`: the vertical delta it
/// published after its start, and its release velocity's vertical part.
fn pan(kind: PointerKind, lock: AxisLock, degrees: f64) -> (f64, f64) {
    let arena = GestureArena::new();
    let (deltas, released) = (
        Rc::new(RefCell::new(0.0_f64)),
        Rc::new(RefCell::new(None::<f64>)),
    );
    let (d, r) = (Rc::clone(&deltas), Rc::clone(&released));
    let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Free)
        .axis_lock(lock)
        .on_update(move |update| *d.borrow_mut() += update.delta.dy.abs())
        .on_end(move |end| *r.borrow_mut() = Some(end.velocity.pixels_per_second.dy))
        .build();
    admit(&arena, &*drag, &[down(1, Offset::ZERO, kind, 0)]);
    let mut last = Offset::ZERO;
    for (at, ms) in path(degrees) {
        send(&*drag, &moved(1, at, kind, ms));
        last = at;
    }
    send(&*drag, &up(1, last, kind, 100));
    let vertical = *deltas.borrow();
    let released = released.borrow().expect("the pan ended");
    (vertical, released)
}

fn rails_keep_a_near_axial_touch_pan_on_its_axis() {
    let (moved, released) = pan(PointerKind::Touch, AxisLock::Rails, 10.0);
    assert!(moved.abs() < f64::EPSILON, "vertical motion {moved}");
    assert!(released.abs() < f64::EPSILON, "vertical release {released}");
}

fn rails_leave_a_diagonal_pan_free() {
    let (moved, _) = pan(PointerKind::Touch, AxisLock::Rails, 40.0);
    assert!(moved > 10.0, "vertical motion {moved}");
}

fn rails_leave_a_mouse_pan_free() {
    let (moved, released) = pan(PointerKind::Mouse, AxisLock::Rails, 10.0);
    assert!(moved > 1.0, "vertical motion {moved}");
    assert!(released.abs() > 1.0, "vertical release {released}");
}

fn a_free_pan_follows_the_contact() {
    let (moved, _) = pan(PointerKind::Touch, AxisLock::Free, 10.0);
    assert!(moved > 1.0, "vertical motion {moved}");
}

fn rails_keep_a_one_finger_scale_pan_on_its_axis() {
    for (lock, railed) in [(AxisLock::Rails, true), (AxisLock::Free, false)] {
        let arena = GestureArena::new();
        let vertical = Rc::new(RefCell::new(0.0_f64));
        let v = Rc::clone(&vertical);
        let scale = ScaleGestureRecognizer::builder(arena.clone())
            .start_mode(ScaleStartMode::PanOrScale)
            .axis_lock(lock)
            .on_update(move |update| *v.borrow_mut() += update.focal_point_delta.dy.abs())
            .build();
        admit(
            &arena,
            &*scale,
            &[down(1, Offset::ZERO, PointerKind::Touch, 0)],
        );
        for (at, ms) in path(10.0) {
            send(&*scale, &moved(1, at, PointerKind::Touch, ms));
        }
        let moved = *vertical.borrow();
        assert_eq!(moved.abs() < f64::EPSILON, railed, "{lock:?}: {moved}");
    }
}

/// A trackpad pan rails like a touch: the focal point a native session
/// reports moves at 10° and publishes horizontal deltas only.
fn rails_keep_a_near_axial_trackpad_pan_on_its_axis() {
    use flui_foundation::geometry::Point;
    use flui_interaction::pointer::PanZoomDispatch;
    use flui_platform_api::pointer::{
        PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerInfo, PointerPosition,
    };
    for (lock, railed) in [(AxisLock::Rails, true), (AxisLock::Free, false)] {
        let vertical = Rc::new(RefCell::new(0.0_f64));
        let v = Rc::clone(&vertical);
        let scale = ScaleGestureRecognizer::builder(GestureArena::new())
            .axis_lock(lock)
            .on_update(move |update| *v.borrow_mut() += update.focal_point_delta.dy.abs())
            .build();
        let info = PointerInfo::new(pointer(30), PointerKind::Trackpad);
        let deliver = |ms: u64, phase, at: Offset<f64>| {
            let event = PanZoomEvent::new(
                info,
                EventTime::from_nanos(ms * 1_000_000),
                PointerPosition::try_new(Point::new(at.dx, at.dy)).expect("finite focal"),
                phase,
            );
            let _ = scale.handle_pan_zoom(PanZoomDispatch::at_root(&event));
        };
        deliver(0, PanZoomPhase::Start, Offset::ZERO);
        for (at, ms) in path(10.0) {
            deliver(ms, PanZoomPhase::Update(PanZoomTransform::IDENTITY), at);
        }
        deliver(120, PanZoomPhase::End, Offset::ZERO);
        let moved = *vertical.borrow();
        assert_eq!(moved.abs() < f64::EPSILON, railed, "{lock:?}: {moved}");
    }
}

/// Two fingers 200 px apart: `first` moves them, then `second` does; the
/// rotation the scale reports at the end.
fn turn(lock: TransformLock, pan_first: bool) -> f64 {
    let arena = GestureArena::new();
    let rotation = Rc::new(RefCell::new(0.0_f64));
    let r = Rc::clone(&rotation);
    let scale = ScaleGestureRecognizer::builder(arena.clone())
        .transform_lock(lock)
        .on_update(move |update| *r.borrow_mut() = update.rotation)
        .build();
    let kind = PointerKind::Touch;
    let (mut a, mut b) = (Offset::new(0.0, 0.0), Offset::new(200.0, 0.0));
    admit(&arena, &*scale, &[down(1, a, kind, 0), down(2, b, kind, 0)]);
    let mut ms = 0;
    let mut step =
        |a_by: Offset<f64>, b_by: Offset<f64>, a: &mut Offset<f64>, b: &mut Offset<f64>| {
            ms += 8;
            *a += a_by;
            send(&*scale, &moved(1, *a, kind, ms));
            ms += 8;
            *b += b_by;
            send(&*scale, &moved(2, *b, kind, ms));
        };
    let pan = Offset::new(4.0, 0.0);
    let twist = Offset::new(0.0, 5.0);
    for round in 0..20 {
        if (round < 10) == pan_first {
            step(pan, pan, &mut a, &mut b);
        } else {
            step(-twist, twist, &mut a, &mut b);
        }
    }
    *rotation.borrow()
}

fn a_scale_begun_as_a_pan_keeps_its_rotation_locked() {
    let locked = turn(TransformLock::Rotation, true);
    assert!(locked.abs() < f64::EPSILON, "rotation {locked}");
    let free = turn(TransformLock::None, true);
    assert!(free.abs() > 0.3, "an unlocked scale rotates: {free}");
}

fn a_scale_begun_as_a_turn_rotates_under_the_lock() {
    let turned = turn(TransformLock::Rotation, false);
    assert!(turned.abs() > 0.3, "rotation {turned}");
}

#[test]
fn axis_and_transform_locks() {
    let rows: &[(&str, fn())] = &[
        (
            "rails_keep_a_near_axial_touch_pan_on_its_axis",
            rails_keep_a_near_axial_touch_pan_on_its_axis,
        ),
        (
            "rails_leave_a_diagonal_pan_free",
            rails_leave_a_diagonal_pan_free,
        ),
        ("rails_leave_a_mouse_pan_free", rails_leave_a_mouse_pan_free),
        (
            "a_free_pan_follows_the_contact",
            a_free_pan_follows_the_contact,
        ),
        (
            "rails_keep_a_one_finger_scale_pan_on_its_axis",
            rails_keep_a_one_finger_scale_pan_on_its_axis,
        ),
        (
            "rails_keep_a_near_axial_trackpad_pan_on_its_axis",
            rails_keep_a_near_axial_trackpad_pan_on_its_axis,
        ),
        (
            "a_scale_begun_as_a_pan_keeps_its_rotation_locked",
            a_scale_begun_as_a_pan_keeps_its_rotation_locked,
        ),
        (
            "a_scale_begun_as_a_turn_rotates_under_the_lock",
            a_scale_begun_as_a_turn_rotates_under_the_lock,
        ),
    ];
    run_rows(rows);
}

/// A touch moved by `step` six times, `every` ms apart, then released, over a
/// swipe that `start` admits and, beside it, a vertical drag when
/// `with_drag`: the edges the swipe reported, and whether the drag started.
fn swipe(
    start: flui_interaction::SwipeStart,
    step: Offset<f64>,
    every: u64,
    with_drag: bool,
) -> (Vec<flui_interaction::PhysicalEdge>, bool) {
    use flui_interaction::SwipeGestureRecognizer;
    let arena = GestureArena::new();
    let swipes = Rc::new(RefCell::new(Vec::new()));
    let s = Rc::clone(&swipes);
    let recognizer = SwipeGestureRecognizer::builder(arena.clone(), start)
        .on_swipe(move |details| s.borrow_mut().push(details.towards))
        .build();
    let dragged = Rc::new(RefCell::new(false));
    let d = Rc::clone(&dragged);
    let drag = DragGestureRecognizer::builder(arena.clone(), DragAxis::Vertical)
        .on_start(move |_| *d.borrow_mut() = true)
        .build();
    let kind = PointerKind::Touch;
    let first = down(1, Offset::ZERO, kind, 0);
    recognizer.add_pointer(PointerDispatch::at_root(&first));
    if with_drag {
        drag.add_pointer(PointerDispatch::at_root(&first));
    }
    arena.close(pointer(1));
    arena.drain_deferred_resolutions();
    let members: [&dyn GestureRecognizer; 2] = [&*recognizer, &*drag];
    let mut at = Offset::ZERO;
    for sample in 1..=6 {
        at += step;
        let event = moved(1, at, kind, sample * every);
        for member in members {
            send(member, &event);
        }
    }
    let last = up(1, at, kind, 6 * every + 8);
    for member in members {
        send(member, &last);
    }
    arena.sweep(pointer(1));
    let reported = swipes.borrow().clone();
    let started = *dragged.borrow();
    (reported, started)
}

fn a_fast_release_along_the_axis_swipes_towards_its_edge() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::{PhysicalEdge, SwipeStart};
    let (right, _) = swipe(
        SwipeStart::Anywhere(Axis::Horizontal),
        Offset::new(20.0, 0.0),
        16,
        false,
    );
    assert_eq!(right, [PhysicalEdge::Right]);
    let (up, _) = swipe(
        SwipeStart::Anywhere(Axis::Vertical),
        Offset::new(0.0, -20.0),
        16,
        false,
    );
    assert_eq!(up, [PhysicalEdge::Top]);
}

fn a_slow_release_is_no_swipe() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::SwipeStart;
    let (swipes, _) = swipe(
        SwipeStart::Anywhere(Axis::Horizontal),
        Offset::new(5.0, 0.0),
        200,
        false,
    );
    assert!(swipes.is_empty(), "{swipes:?}");
}

fn a_swipe_from_an_edge_only_leaves_it() {
    use flui_interaction::{PhysicalEdge, SwipeStart};
    let edge = SwipeStart::FromEdge(PhysicalEdge::Left);
    let (away, _) = swipe(edge, Offset::new(20.0, 0.0), 16, false);
    assert_eq!(away, [PhysicalEdge::Right]);
    let (back, _) = swipe(edge, Offset::new(-20.0, 0.0), 16, false);
    assert!(back.is_empty(), "{back:?}");
}

/// A swipe is an axial member: an orthogonal drag takes a contact moving
/// along its own axis, and the swipe keeps one moving along the swipe's.
fn a_swipe_is_axial_in_the_arena() {
    use flui_foundation::geometry::Axis;
    use flui_interaction::{PhysicalEdge, SwipeStart};
    let start = SwipeStart::Anywhere(Axis::Horizontal);
    assert_eq!(
        swipe(start, Offset::new(20.0, 2.0), 16, true),
        (vec![PhysicalEdge::Right], false)
    );
    assert_eq!(
        swipe(start, Offset::new(2.0, 20.0), 16, true),
        (vec![], true)
    );
}

#[test]
fn swipe_recognizer() {
    let rows: &[(&str, fn())] = &[
        (
            "a_fast_release_along_the_axis_swipes_towards_its_edge",
            a_fast_release_along_the_axis_swipes_towards_its_edge,
        ),
        ("a_slow_release_is_no_swipe", a_slow_release_is_no_swipe),
        (
            "a_swipe_from_an_edge_only_leaves_it",
            a_swipe_from_an_edge_only_leaves_it,
        ),
        (
            "a_swipe_is_axial_in_the_arena",
            a_swipe_is_axial_in_the_arena,
        ),
    ];
    run_rows(rows);
}

fn run_rows(rows: &[(&str, fn())]) {
    let failures: Vec<String> = rows
        .iter()
        .filter_map(|&(name, row)| {
            std::panic::catch_unwind(row).err().map(|payload| {
                let message = payload
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| payload.downcast_ref::<&str>().map(ToString::to_string))
                    .unwrap_or_default();
                format!("{name}: {message}")
            })
        })
        .collect();
    assert!(failures.is_empty(), "failed rows:\n{}", failures.join("\n"));
}
