//! Hot-path regression tests for owner-routed pointer delivery.

#![expect(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_foundation::geometry::Point;
use flui_interaction::events::{PointerEventExt, PointerKind, make_move_event};
use flui_interaction::{HitTestEntry, InteractionLane, Offset, PointerTarget, RenderId};
use flui_platform_api::pointer::{
    DeviceId, PointerEvent, PointerMove, PointerPosition, PointerRole, PointerSample, Pressure,
};
use flui_platform_api::{EventTime, Modifiers};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

struct CountingAllocator;

// SAFETY: This test allocator forwards every allocation operation to
// `std::alloc::System` with the exact layout/pointer contract it received. The
// only added behavior is an atomic counter increment before `alloc`.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
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
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: Forwarding the original pointer/layout plus requested size
        // unchanged to the wrapped system allocator preserves the contract.
        unsafe { System.realloc(ptr, layout, new_size) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        // SAFETY: Forwarding the caller-provided layout unchanged to the
        // wrapped system allocator preserves `GlobalAlloc::alloc_zeroed`.
        unsafe { System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

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

        ALLOCATIONS.store(0, Ordering::Relaxed);
        assert!(
            handle
                .invoke_pointer_route(route, &event)
                .expect("measured invocation")
                .is_none()
        );
        assert_eq!(
            ALLOCATIONS.load(Ordering::Relaxed),
            0,
            "cached Move route invocation must not allocate after setup"
        );
        assert_eq!(deliveries.get(), 8);

        handle.release_route(route).expect("release route");
    });

    // Keep every counting case in this one test: a global allocator counter
    // must not race a sibling test in the same Rust test process.
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
}

fn resampler_packets(down_time: u64) -> [PointerEvent; 3] {
    use flui_platform_api::pointer::{PointerButton, PointerButtons, PointerInfo, PointerPress, PointerRelease};
    let pointer = PointerInfo::new(
        flui_platform_api::pointer::PointerId::new(core::num::NonZeroU64::MIN),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(7_u64).expect("source device"))
    .with_role(PointerRole::Additional);
    let buttons = PointerButtons::only(PointerButton::PRIMARY);
    [
        PointerEvent::Down(PointerPress::new(
            pointer, PointerButton::PRIMARY, buttons, route_sample(down_time, 0.0, 0.0),
        )),
        PointerEvent::Move(PointerMove::new(pointer, buttons, route_sample(3_000_000, 30.0, 50.0))
            .with_modifiers(Modifiers::SHIFT)
            .with_coalesced(vec![route_sample(1_000_000, 10.0, 20.0), route_sample(2_000_000, 20.0, 30.0)])
            .with_predicted(vec![route_sample(4_000_000, 40.0, 60.0)])),
        PointerEvent::Up(PointerRelease::new(
            pointer, PointerButton::PRIMARY, PointerButtons::NONE,
            route_sample(down_time.max(5_000_000), 30.0, 50.0),
        )),
    ]
}

fn measure_resampler_delivery(stop: bool) {
    use flui_interaction::processing::PointerEventResampler;
    use web_time::{Duration, Instant};
    let packets = resampler_packets(0);
    let resampler = PointerEventResampler::new(packets[0].pointer_id().expect("contact identity"));
    let base = Instant::now();
    for (packet, millis) in packets.iter().zip([0, 3, 5]) {
        resampler.add_event_at(packet.clone(), base + Duration::from_millis(millis));
    }
    let mut deliveries = 0;
    let mut consume = |packet| {
        assert_eq!(packet, packets[deliveries], "owned delivery preserves every source field");
        deliveries += 1;
    };
    ALLOCATIONS.store(0, Ordering::Relaxed);
    if stop {
        resampler.stop(&mut consume);
    } else {
        resampler.sample(base + Duration::from_millis(3), base + Duration::from_millis(4), &mut consume);
    }
    let allocations = ALLOCATIONS.load(Ordering::Relaxed);
    assert_eq!(allocations, 0, "unchanged-time measured history delivery reuses owned storage: stop={stop}");
    if !stop {
        resampler.stop(&mut consume);
    }
    assert_eq!(deliveries, 3, "the complete accepted sequence remains deliverable");
    assert!(!resampler.has_pending_events());
}

fn resampler_raised_timestamp_keeps_checked_history_policy() {
    use flui_interaction::processing::PointerEventResampler;
    use web_time::{Duration, Instant};
    let mut packets = resampler_packets(10_000_000);
    let resampler = PointerEventResampler::new(packets[0].pointer_id().expect("contact identity"));
    let base = Instant::now();
    for (packet, millis) in packets.iter().zip([0, 1, 2]) {
        resampler.add_event_at(packet.clone(), base + Duration::from_millis(millis));
    }
    let PointerEvent::Move(movement) = &packets[1] else { panic!("Move fixture") };
    let mut current = *movement.current();
    current.time = EventTime::from_nanos(10_000_000);
    packets[1] = PointerEvent::Move(PointerMove::new(movement.pointer, movement.buttons, current)
        .with_modifiers(movement.modifiers)
        .with_coalesced(movement.coalesced().to_vec()));
    let mut deliveries = 0;
    resampler.stop(|packet| {
        assert_eq!(packet, packets[deliveries], "raised time revalidates predictions while preserving measured history");
        deliveries += 1;
    });
    assert_eq!(deliveries, 3);
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
        ALLOCATIONS.store(0, Ordering::Relaxed);
        let result = handle.invoke_pointer_route(route, &event).expect("measured invocation");
        let allocations = ALLOCATIONS.load(Ordering::Relaxed);
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
