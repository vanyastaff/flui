//! Pointer assembly allocates nothing per event once its contacts are open.
//!
//! A counting `#[global_allocator]` is process-wide, which is why this file is
//! a test target of its own rather than a module of `tests/main.rs`. Counters
//! are per-thread, so libtest's harness thread cannot add to the measured
//! window.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::hint::black_box;

use flui_native::input::assembly::{
    ContactRegistry, GestureComponent, GestureDelta, GesturePhase, GestureReading, MoveBatch,
    NativeClock, NativeContact, NativeReading, ReadingAction, ReadingTime, decode,
};
use flui_platform::EventTime;
use flui_platform::pointer::{PointerButton, PointerKind};

thread_local! {
    #[cfg_attr(target_os = "android", allow(clippy::missing_const_for_thread_local, reason = "Const initializer; Android std macro false positive: https://github.com/rust-lang/rust-clippy/issues/13422"))]
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAllocator;

// SAFETY: `alloc`/`dealloc` forward unchanged to `System`, so every pointer
// is freed by the allocator that produced it. The default `realloc` and
// `alloc_zeroed` decompose into these and are counted through `alloc`.
#[expect(unsafe_code)]
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get().wrapping_add(1)));
        // SAFETY: `layout` is the caller's own, forwarded unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr`/`layout` are the caller's own, forwarded unchanged.
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations(run: impl FnOnce()) -> usize {
    let before = ALLOCATIONS.with(Cell::get);
    run();
    ALLOCATIONS.with(Cell::get) - before
}

fn reading(contact: u64, kind: PointerKind, action: ReadingAction, ms: u64) -> NativeReading {
    NativeReading::new(
        NativeContact(contact),
        kind,
        action,
        ReadingTime::epoch(EventTime::from_nanos(ms * 1_000_000)),
    )
    .at_device(4.0, 8.0, 2.0)
}

fn scale(phase: GesturePhase) -> ReadingAction {
    ReadingAction::Gesture(GestureReading {
        component: GestureComponent::Scale,
        phase,
        delta: GestureDelta::Scale(1.01),
    })
}

/// One round of every per-event path: hover, press, drag, chord, release,
/// two touches and a pinch.
fn round(contacts: &mut ContactRegistry, ms: u64) {
    let press = ReadingAction::Press(PointerButton::PRIMARY);
    let release = ReadingAction::Release(PointerButton::PRIMARY);
    for reading in [
        reading(0, PointerKind::Mouse, ReadingAction::Move, ms),
        reading(0, PointerKind::Mouse, press, ms + 1),
        reading(0, PointerKind::Mouse, ReadingAction::Move, ms + 2),
        reading(
            0,
            PointerKind::Mouse,
            ReadingAction::Press(PointerButton::SECONDARY),
            ms + 3,
        ),
        reading(
            0,
            PointerKind::Mouse,
            ReadingAction::Release(PointerButton::SECONDARY),
            ms + 4,
        ),
        reading(0, PointerKind::Mouse, release, ms + 5),
        reading(1, PointerKind::Touch, press, ms + 6),
        reading(2, PointerKind::Touch, press, ms + 6),
        reading(1, PointerKind::Touch, ReadingAction::Move, ms + 7),
        reading(2, PointerKind::Touch, ReadingAction::Move, ms + 7),
        reading(1, PointerKind::Touch, release, ms + 8),
        reading(2, PointerKind::Touch, release, ms + 8),
        reading(3, PointerKind::Trackpad, scale(GesturePhase::Began), ms + 9),
        reading(
            3,
            PointerKind::Trackpad,
            scale(GesturePhase::Changed),
            ms + 10,
        ),
        reading(
            3,
            PointerKind::Trackpad,
            scale(GesturePhase::Ended),
            ms + 11,
        ),
    ] {
        black_box(decode(contacts, &reading).expect("decodes"));
    }
}

#[test]
fn pointer_assembly_allocates_nothing_per_event() {
    let mut contacts = ContactRegistry::new(NativeClock::epoch_only());
    // Warm-up grows the contact storage to its working size.
    round(&mut contacts, 0);
    assert_eq!(
        allocations(|| {
            for ms in (100..10_000).step_by(100) {
                round(&mut contacts, ms);
            }
        }),
        0
    );

    // Positive control: a coalesced batch hands the move a vocabulary-owned
    // sample buffer, so the counter does see allocations.
    let mut batch = MoveBatch::new();
    let press = reading(
        5,
        PointerKind::Touch,
        ReadingAction::Press(PointerButton::PRIMARY),
        20_000,
    );
    decode(&mut contacts, &press).expect("decodes");
    let counted = allocations(|| {
        batch.push_coalesced(reading(5, PointerKind::Touch, ReadingAction::Move, 20_001));
        black_box(
            batch
                .decode(
                    &mut contacts,
                    &reading(5, PointerKind::Touch, ReadingAction::Move, 20_002)
                        .at_device(6.0, 8.0, 2.0),
                )
                .expect("decodes"),
        );
    });
    assert!(counted > 0, "the counter observes allocations");
}
