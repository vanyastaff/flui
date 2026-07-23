//! Hot-path regression tests for owner-routed pointer delivery.

#![allow(unsafe_code)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_interaction::events::{PointerType, make_move_event};
use flui_interaction::{HitTestEntry, InteractionLane, Offset, PointerTarget, RenderId};

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
    let event = make_move_event(Offset::ZERO, PointerType::Mouse);

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
}
