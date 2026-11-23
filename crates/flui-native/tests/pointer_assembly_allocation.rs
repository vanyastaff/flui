//! Pointer assembly allocates nothing per event once its contacts are open.
//!
//! The counting `#[global_allocator]` is process-wide, which is why this file is
//! a test target of its own rather than a module of `tests/main.rs`. Counters
//! are per-thread, so libtest's harness thread cannot add to the measured
//! window.

use std::hint::black_box;

use flui_foundation::geometry::DevicePixelRatio;
use flui_foundation::testing::heap::{CountingAllocator, HeapCounts};
use flui_native::input::assembly::{
    ContactRegistry, GestureComponent, GestureDelta, GesturePhase, GestureReading, MoveBatch,
    NativeClock, NativeContact, NativeReading, ReadingAction, ReadingTime, decode,
};
use flui_platform::EventTime;
use flui_platform::input::pointer::{PointerButton, PointerKind};

/// Two device pixels per logical pixel.
const TWO: DevicePixelRatio = match DevicePixelRatio::try_new(2.0) {
    Some(ratio) => ratio,
    None => panic!("BUG: 2 is a ratio a screen can have"),
};

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

fn allocations(run: impl FnOnce()) -> u64 {
    HeapCounts::during(run).0.allocations
}

fn reading(contact: u64, kind: PointerKind, action: ReadingAction, ms: u64) -> NativeReading {
    NativeReading::new(
        NativeContact(contact),
        kind,
        action,
        ReadingTime::epoch(EventTime::from_nanos(ms * 1_000_000)),
    )
    .at_device(4.0, 8.0, TWO)
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
                        .at_device(6.0, 8.0, TWO),
                )
                .expect("decodes"),
        );
    });
    assert!(counted > 0, "the counter observes allocations");
}
