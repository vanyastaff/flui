//! Hot-path regression tests for owner-routed pointer delivery.

#![expect(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Barrier};

use flui_foundation::geometry::Point;
use flui_interaction::events::{PointerEventExt, PointerKind, make_move_event};
use flui_interaction::{HitTestEntry, InteractionLane, Offset, PointerTarget, RenderId};
use flui_platform::pointer::{
    DeviceId, PointerEvent, PointerMove, PointerPosition, PointerRole, PointerSample, Pressure,
};
use flui_platform::{EventTime, Modifiers};

thread_local! {
    /// The allocations made on this thread. The count is per thread because
    /// the measured code runs on the test's own thread while the harness's
    /// main thread keeps allocating (its bookkeeping for the test it just
    /// spawned), and a process-wide count would charge those to the
    /// measurement. Constant-initialised with no destructor, so reading it
    /// from the allocator never allocates.
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// Start counting this thread's allocations from zero.
fn reset_allocations() {
    ALLOCATIONS.set(0);
}

/// The allocations this thread made since [`reset_allocations`].
fn allocations() -> usize {
    ALLOCATIONS.get()
}

fn count_allocation() {
    // `try_with`: an allocation during the thread's own teardown is not
    // counted rather than a panic inside the allocator.
    let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
}

struct CountingAllocator;

// SAFETY: This test allocator forwards every allocation operation to
// `std::alloc::System` with the exact layout/pointer contract it received. The
// only added behavior is a thread-local counter increment before `alloc`.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: Forwarding the caller-provided layout unchanged to the
        // wrapped system allocator preserves `GlobalAlloc::alloc`'s contract.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: Forwarding the original pointer/layout pair unchanged to the
        // wrapped system allocator preserves `GlobalAlloc::dealloc`'s contract.
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count_allocation();
        // SAFETY: Forwarding the original pointer/layout plus requested size
        // unchanged to the wrapped system allocator preserves the contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count_allocation();
        // SAFETY: Forwarding the caller-provided layout unchanged to the
        // wrapped system allocator preserves `GlobalAlloc::alloc_zeroed`.
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

/// What another thread allocates while a measurement runs (the harness's main
/// thread, a sibling test) is not charged to it.
#[test]
fn allocation_count_is_the_measuring_threads_own() {
    let start = Arc::new(Barrier::new(2));
    let done = Arc::new(Barrier::new(2));
    let other = std::thread::spawn({
        let (start, done) = (Arc::clone(&start), Arc::clone(&done));
        move || {
            start.wait();
            drop(std::hint::black_box(vec![0_u8; 64]));
            done.wait();
        }
    });
    reset_allocations();
    start.wait();
    done.wait();
    assert_eq!(
        allocations(),
        0,
        "another thread's allocation is not this thread's"
    );
    other.join().expect("the allocating thread");
}

fn hit_entry(index: usize, target: PointerTarget) -> HitTestEntry {
    HitTestEntry::new(RenderId::new(index + 1)).pointer_target(target)
}

#[test]
fn resolved_route_move_invocation_allocates_no_heap_after_setup() {
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let deliveries = Rc::new(Cell::new(0));
    let event = make_move_event(Offset::ZERO, PointerKind::Mouse).expect("valid fixture sample");

    lane.enter(|| {
        let targets: Vec<_> = (0..4)
            .map(|_| {
                let deliveries = Rc::clone(&deliveries);
                handle
                    .register_pointer(move |_| deliveries.set(deliveries.get() + 1))
                    .expect("register target")
            })
            .collect();
        let path: Vec<_> = targets
            .iter()
            .enumerate()
            .map(|(index, target)| hit_entry(index, *target))
            .collect();
        let route = handle
            .resolve_pointer_route(&path)
            .expect("resolve route")
            .token();

        // Warm TLS/borrow machinery before counting the actual common Move
        // invocation. Setup allocations above are expected; the cached route
        // invoke itself should only clone Rc handles and walk stack values.
        assert!(
            handle
                .invoke_pointer_route(route, &event)
                .expect("warm invocation")
                .is_none()
        );

        reset_allocations();
        assert!(
            handle
                .invoke_pointer_route(route, &event)
                .expect("measured invocation")
                .is_none()
        );
        assert_eq!(
            allocations(),
            0,
            "cached Move route invocation must not allocate after setup"
        );
        assert_eq!(deliveries.get(), 8);

        handle.release_route(route).expect("release route");
    });

    for target_count in [1_usize, 4, 16] {
        for shape in [
            RouteShape::Global,
            RouteShape::Identity,
            RouteShape::Translated,
            RouteShape::NearIdentity,
        ] {
            for history in [false, true] {
                measure_route_shape(target_count, shape, history);
            }
        }
    }
    for stop in [false, true] {
        measure_resampler_delivery(stop);
    }
    resampler_raised_timestamp_keeps_checked_history_policy();
    saturated_resampler_admission_reuses_bounded_history_storage();
    binding_frame_coalescing_moves_bounded_history();
    steady_hover_allocates_nothing();
    empty_hit_test_result_allocates_nothing();
}

/// A contact's moves coalesced into one frame move their history instead of
/// copying it, and keep a bounded tail however many arrive before the frame.
fn binding_frame_coalescing_moves_bounded_history() {
    use flui_interaction::{GestureBinding, HitTestResult, InputSurface};
    use flui_platform::pointer::{PointerButton, PointerButtons, PointerInfo, PointerPress};
    let binding = GestureBinding::new();
    let pointer = PointerInfo::new(
        flui_platform::pointer::PointerId::new(core::num::NonZeroU64::MIN),
        PointerKind::Touch,
    );
    let buttons = PointerButtons::only(PointerButton::PRIMARY);
    let longest = Rc::new(Cell::new(0));
    let observed = Rc::clone(&longest);
    binding
        .pointer_router()
        .add_global_handler(Rc::new(move |event| {
            if let PointerEvent::Move(movement) = event {
                observed.set(observed.get().max(movement.coalesced().len() + 1));
            }
        }));
    let down = PointerEvent::Down(PointerPress::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        route_sample(0, 0.0, 0.0),
    ));
    let moves: Vec<_> = (1..=300_u64)
        .map(|millis| {
            PointerEvent::Move(PointerMove::new(
                pointer,
                buttons,
                route_sample(millis * 1_000_000, millis as f64, 0.0),
            ))
        })
        .collect();
    binding.handle_pointer_event(&down, |_| HitTestResult::new());
    // Warm the coalescer's slot before counting.
    binding.handle_pointer_event(&moves[0], |_| HitTestResult::new());
    binding.flush_pending_moves();
    reset_allocations();
    for movement in &moves[1..] {
        let _ = binding.dispatch_pointer_on(InputSurface::Window, movement, |_| {
            panic!("a contact keeps its route")
        });
    }
    let allocations = allocations();
    binding.flush_pending_moves();
    assert!(
        allocations <= 16,
        "folding a stalled frame's moves grows one history, never copies it: {allocations}"
    );
    assert_eq!(
        longest.get(),
        PointerMove::COALESCED_HISTORY_LIMIT + 1,
        "the frame's move keeps the latest bounded history and its own reading"
    );
    binding.handle_lifecycle_pause();
}

/// A mouse resting over the same regions updates its hover without
/// allocating.
fn steady_hover_allocates_nothing() {
    use flui_interaction::events::PointerButtons;
    use flui_interaction::pointer::{
        MouseRegionCallbacks, MouseTracker, MouseTrackerAnnotation, PointerMotionKind,
    };
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let tracker = MouseTracker::new();
    let hover = |millis: u64| {
        let PointerEvent::Move(base) =
            make_move_event(Offset::new(5.0, 5.0), PointerKind::Mouse).expect("hover fixture")
        else {
            panic!("move fixture")
        };
        PointerEvent::Move(PointerMove::new(
            base.pointer
                .with_device(DeviceId::try_from(7_u64).expect("device")),
            PointerButtons::NONE,
            route_sample(millis * 1_000_000, 5.0, 5.0),
        ))
    };
    let moves: Vec<_> = (1..=8).map(hover).collect();
    lane.enter(|| {
        let mut path = flui_interaction::HitTestResult::new();
        for index in 1..=3 {
            let region = handle
                .register_mouse_region(MouseRegionCallbacks {
                    on_enter: Some(Rc::new(|_, _| {})),
                    on_exit: Some(Rc::new(|_, _| {})),
                    ..MouseRegionCallbacks::default()
                })
                .expect("region");
            let id = RenderId::new(index);
            path.add(
                HitTestEntry::new(id).mouse_annotation(MouseTrackerAnnotation::new(id, region)),
            );
        }
        tracker.update_with_motion(&moves[0], PointerMotionKind::Hover, &path);
        tracker.update_with_motion(&moves[1], PointerMotionKind::Hover, &path);
        reset_allocations();
        for movement in &moves[2..] {
            tracker.update_with_motion(movement, PointerMotionKind::Hover, &path);
        }
        assert_eq!(
            allocations(),
            0,
            "an unchanged hover keeps its regions and collections"
        );
    });
}

fn empty_hit_test_result_allocates_nothing() {
    reset_allocations();
    let result = flui_interaction::HitTestResult::new();
    assert!(result.is_empty());
    drop(result);
    assert_eq!(allocations(), 0, "an empty hit test result owns no storage");
}

fn resampler_packets(down_time: u64) -> [PointerEvent; 3] {
    use flui_platform::pointer::{
        PointerButton, PointerButtons, PointerInfo, PointerPress, PointerRelease,
    };
    let pointer = PointerInfo::new(
        flui_platform::pointer::PointerId::new(core::num::NonZeroU64::MIN),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(7_u64).expect("source device"))
    .with_role(PointerRole::Additional);
    let buttons = PointerButtons::only(PointerButton::PRIMARY);
    [
        PointerEvent::Down(PointerPress::new(
            pointer,
            PointerButton::PRIMARY,
            buttons,
            route_sample(down_time, 0.0, 0.0),
        )),
        PointerEvent::Move(
            PointerMove::new(pointer, buttons, route_sample(3_000_000, 30.0, 50.0))
                .with_modifiers(Modifiers::SHIFT)
                .with_coalesced(vec![
                    route_sample(1_000_000, 10.0, 20.0),
                    route_sample(2_000_000, 20.0, 30.0),
                ])
                .with_predicted(vec![route_sample(4_000_000, 40.0, 60.0)]),
        ),
        PointerEvent::Up(PointerRelease::new(
            pointer,
            PointerButton::PRIMARY,
            PointerButtons::NONE,
            route_sample(down_time.max(5_000_000), 30.0, 50.0),
        )),
    ]
}

fn measure_resampler_delivery(stop: bool) {
    use flui_interaction::pointer::PointerEventResampler;
    use web_time::{Duration, Instant};
    let packets = resampler_packets(0);
    let resampler = PointerEventResampler::new(packets[0].pointer_id().expect("contact identity"));
    let base = Instant::now();
    for (packet, millis) in packets.iter().zip([0, 3, 5]) {
        resampler.add_event_at(packet.clone(), base + Duration::from_millis(millis));
    }
    let mut deliveries = 0;
    let mut consume = |packet| {
        assert_eq!(
            packet, packets[deliveries],
            "owned delivery preserves every source field"
        );
        deliveries += 1;
    };
    reset_allocations();
    if stop {
        resampler.stop(&mut consume);
    } else {
        resampler.sample(
            base + Duration::from_millis(3),
            base + Duration::from_millis(4),
            &mut consume,
        );
    }
    let allocations = allocations();
    assert_eq!(
        allocations, 0,
        "unchanged-time measured history delivery reuses owned storage: stop={stop}"
    );
    if !stop {
        resampler.stop(&mut consume);
    }
    assert_eq!(
        deliveries, 3,
        "the complete accepted sequence remains deliverable"
    );
    assert!(!resampler.has_pending_events());
}

fn resampler_raised_timestamp_keeps_checked_history_policy() {
    use flui_interaction::pointer::PointerEventResampler;
    use web_time::{Duration, Instant};
    let mut packets = resampler_packets(10_000_000);
    let resampler = PointerEventResampler::new(packets[0].pointer_id().expect("contact identity"));
    let base = Instant::now();
    for (packet, millis) in packets.iter().zip([0, 1, 2]) {
        resampler.add_event_at(packet.clone(), base + Duration::from_millis(millis));
    }
    let PointerEvent::Move(movement) = &packets[1] else {
        panic!("Move fixture")
    };
    let mut current = *movement.current();
    current.time = EventTime::from_nanos(10_000_000);
    packets[1] = PointerEvent::Move(
        PointerMove::new(movement.pointer, movement.buttons, current)
            .with_modifiers(movement.modifiers)
            .with_coalesced(movement.coalesced().to_vec()),
    );
    let mut deliveries = 0;
    resampler.stop(|packet| {
        assert_eq!(
            packet, packets[deliveries],
            "raised time revalidates predictions while preserving measured history"
        );
        deliveries += 1;
    });
    assert_eq!(deliveries, 3);
}

fn saturated_resampler_admission_reuses_bounded_history_storage() {
    use flui_interaction::pointer::PointerEventResampler;
    use flui_platform::pointer::{PointerButton, PointerButtons, PointerRelease};
    use web_time::{Duration, Instant};
    let [down, PointerEvent::Move(base_move), _] = resampler_packets(0) else {
        panic!("measured Move fixture")
    };
    let pointer = base_move.pointer;
    let buttons = base_move.buttons;
    let movement = |millis: u64| {
        PointerEvent::Move(
            PointerMove::new(
                pointer,
                buttons,
                route_sample(millis * 1_000_000, millis as f64, 50.0),
            )
            .with_modifiers(Modifiers::SHIFT)
            .with_predicted(vec![route_sample(
                (millis + 1) * 1_000_000,
                (millis + 1) as f64,
                50.0,
            )]),
        )
    };
    let resampler = PointerEventResampler::new(pointer.id);
    let base = Instant::now();
    resampler.add_event_at(down.clone(), base);
    for millis in 1..=300 {
        resampler.add_event_at(movement(millis), base + Duration::from_millis(millis));
    }
    let last = movement(301);
    reset_allocations();
    resampler.add_event_at(last, base + Duration::from_millis(301));
    let allocations = allocations();
    // The queue retires the older packet, so its checked history storage can
    // transfer to the newer packet. At most one growth allocation is needed.
    assert!(
        allocations <= 1,
        "saturated admission transfers existing history storage: {allocations}"
    );
    let up = PointerEvent::Up(PointerRelease::new(
        pointer,
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        route_sample(302_000_000, 301.0, 50.0),
    ));
    resampler.add_event_at(up.clone(), base + Duration::from_millis(302));
    let (mut downs, mut moves, mut readings, mut ups) = (0, 0, 0, 0);
    let mut previous = 0;
    resampler.stop(|event| match event {
        PointerEvent::Down(_) => {
            assert_eq!(event, down);
            downs += 1;
        }
        PointerEvent::Move(movement) => {
            assert_eq!(movement.pointer, pointer);
            assert_eq!(movement.buttons, buttons);
            assert_eq!(movement.modifiers, Modifiers::SHIFT);
            for sample in movement
                .coalesced()
                .iter()
                .chain(std::iter::once(movement.current()))
            {
                let millis = sample.time.as_nanos() / 1_000_000;
                assert!(
                    millis > previous,
                    "retained measured readings stay chronological"
                );
                assert_eq!(
                    *sample,
                    route_sample(millis * 1_000_000, millis as f64, 50.0)
                );
                previous = millis;
                readings += 1;
            }
            let millis = movement.current().time.as_nanos() / 1_000_000;
            assert_eq!(
                movement.predicted(),
                &[route_sample(
                    (millis + 1) * 1_000_000,
                    (millis + 1) as f64,
                    50.0
                )]
            );
            moves += 1;
        }
        PointerEvent::Up(_) => {
            assert_eq!(event, up);
            ups += 1;
        }
        _ => panic!("unexpected accepted sequence event"),
    });
    assert_eq!((downs, moves, readings, ups), (1, 99, 199, 1));
    assert_eq!(previous, 301, "latest accepted reading remains deliverable");
}

fn route_sample(time: u64, x: f64, y: f64) -> PointerSample {
    PointerSample::new(
        EventTime::from_nanos(time),
        PointerPosition::try_new(Point::new(x, y)).expect("finite fixture sample"),
    )
    .with_pressure(Pressure::try_new(0.65).expect("valid fixture pressure"))
}

#[derive(Clone, Copy, Debug)]
enum RouteShape {
    Global,
    Identity,
    Translated,
    NearIdentity,
}

fn measure_route_shape(target_count: usize, shape: RouteShape, history: bool) {
    let translated = matches!(shape, RouteShape::Translated | RouteShape::NearIdentity);
    let (dx, dy) = match shape {
        RouteShape::Translated => (10.0, 20.0),
        RouteShape::NearIdentity => (0.000_001, -0.000_002),
        RouteShape::Global | RouteShape::Identity => (0.0, 0.0),
    };
    let lane = InteractionLane::try_new().expect("lane");
    let handle = lane.dispatch_handle();
    let deliveries = Rc::new(Cell::new(0));
    let PointerEvent::Move(base) =
        make_move_event(Offset::new(30.0, 50.0), PointerKind::Mouse).expect("valid fixture sample")
    else {
        panic!("move fixture")
    };
    let mut movement = PointerMove::new(
        base.pointer
            .with_device(DeviceId::try_from(7_u64).expect("source device"))
            .with_role(PointerRole::Additional),
        base.buttons,
        route_sample(3_000_000, 30.0, 50.0),
    )
    .with_modifiers(Modifiers::SHIFT);
    if history {
        movement = movement
            .with_coalesced(vec![
                route_sample(1_000_000, 10.0, 20.0),
                route_sample(2_000_000, 20.0, 30.0),
            ])
            .with_predicted(vec![route_sample(4_000_000, 40.0, 60.0)]);
    }
    let event = PointerEvent::Move(movement);
    lane.enter(|| {
        let targets: Vec<_> = (0..target_count).map(|_| {
            let deliveries = Rc::clone(&deliveries);
            handle.register_pointer(move |dispatch| {
                let PointerEvent::Move(local) = dispatch.local else { panic!("local Move") };
                let PointerEvent::Move(global) = dispatch.global else { panic!("global Move") };
                assert_eq!(local.pointer, global.pointer);
                assert_eq!(local.buttons, global.buttons);
                assert_eq!(local.modifiers, global.modifiers);
                for (local, global) in std::iter::once((local.current(), global.current()))
                    .chain(local.coalesced().iter().zip(global.coalesced()))
                    .chain(local.predicted().iter().zip(global.predicted()))
                {
                    let mut original = *local;
                    original.position = global.position;
                    assert_eq!(original, *global, "localization preserves every source sample field except position");
                }
                assert_eq!(global.current().position.get(), Point::new(30.0, 50.0));
                assert_eq!(local.current().position.get(), Point::new(30.0 - dx, 50.0 - dy));
                assert_eq!(local.current().time.as_nanos(), 3_000_000);
                assert_eq!(local.coalesced().len(), if history { 2 } else { 0 });
                assert_eq!(local.predicted().len(), usize::from(history));
                if history {
                    for (sample, (time, point)) in local.coalesced().iter().zip([
                        (1_000_000, Point::new(10.0 - dx, 20.0 - dy)),
                        (2_000_000, Point::new(20.0 - dx, 30.0 - dy)),
                    ]) {
                        assert_eq!(sample.time.as_nanos(), time);
                        assert_eq!(sample.position.get(), point);
                    }
                    assert_eq!(local.predicted()[0].time.as_nanos(), 4_000_000);
                    assert_eq!(local.predicted()[0].position.get(), Point::new(40.0 - dx, 60.0 - dy));
                    assert_eq!(global.coalesced()[0].position.get(), Point::new(10.0, 20.0));
                    assert_eq!(global.coalesced()[1].position.get(), Point::new(20.0, 30.0));
                    assert_eq!(global.predicted()[0].position.get(), Point::new(40.0, 60.0));
                }
                deliveries.set(deliveries.get() + 1);
            }).expect("register target")
        }).collect();
        let mut path: Vec<_> = targets.iter().enumerate()
            .map(|(index, target)| hit_entry(index, *target)).collect();
        if !matches!(shape, RouteShape::Global) {
            let mut result = flui_interaction::HitTestResult::new();
            if translated {
                result.with_paint_offset(Offset::new(dx, dy), |result| {
                    for entry in path.drain(..) { result.add(entry); }
                }).expect("finite offset");
            } else {
                // The real hit-test producer installs the root's composed
                // identity rather than leaving these entries untransformed.
                for entry in path.drain(..) { result.add(entry); }
            }
            path = result.path().to_vec();
        }
        let route = handle.resolve_pointer_route(&path).expect("resolve route").token();
        assert!(handle.invoke_pointer_route(route, &event).expect("warm invocation").is_none());
        reset_allocations();
        let result = handle.invoke_pointer_route(route, &event).expect("measured invocation");
        let allocations = allocations();
        assert!(result.is_none());
        assert_eq!(deliveries.get(), target_count * 2);
        if !history || !translated {
            assert_eq!(allocations, 0, "scalar and identity cached delivery borrow every source history after setup: shape={shape:?}, targets={target_count}, history={history}");
        } else {
            assert!(allocations <= target_count * 2, "translated measured and predicted histories need at most one owned allocation each per target: targets={target_count}, allocations={allocations}");
        }
        // Localizing nonempty measured and predicted histories requires owned
        // storage; global-only delivery continues to borrow the source event.
        println!("cached Move: targets={target_count}, shape={shape:?}, history={history}, allocations={allocations}");
        handle.release_route(route).expect("release route");
    });
}

/// A member with no behavior: the arena's own decisions are what is measured.
struct QuietMember;

impl flui_interaction::GestureArenaMember for QuietMember {
    fn accept_gesture(&self, _: flui_interaction::GestureContest) {}
    fn reject_gesture(&self, _: flui_interaction::GestureContest) {}
}

#[test]
fn arena_decisions_allocate_nothing_for_observation_without_observers() {
    use flui_interaction::{GestureArena, GestureDisposition, PointerId};

    let arena = GestureArena::new();
    let pointer = PointerId::try_from(1_u64).expect("nonzero pointer id");
    // Four members: the most the arena holds inline, and as many joins as a
    // fresh queue would hold before the close and the verdicts outgrew it.
    let members: [Rc<QuietMember>; 4] = std::array::from_fn(|_| Rc::new(QuietMember));
    let entries = members.each_ref().map(|member| arena.add(pointer, member));
    let entry = &entries[0];

    // Close, claim, win and reject: every decision an observer would hear.
    reset_allocations();
    arena.close(pointer);
    entry.resolve(GestureDisposition::Accepted);
    assert_eq!(
        allocations(),
        0,
        "with no observer registered the arena records nothing"
    );
}
