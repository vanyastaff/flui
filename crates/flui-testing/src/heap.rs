//! What a test thread asks of the heap: the foundation layer's counting allocator, the
//! allocation sites a profile sums, and the plateau a mounted and dropped application
//! reaches.
//!
//! A test target that counts allocations installs [`CountingAllocator`] as its own global
//! allocator; the kit cannot install it, because a `#[global_allocator]` belongs to the
//! binary:
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOCATOR: flui_testing::heap::CountingAllocator =
//!     flui_testing::heap::CountingAllocator;
//! ```
//!
//! [`CountingAllocator`] and [`HeapCounts`] are `flui_foundation::testing::heap`'s, so a crate
//! below this kit counts with the same allocator through `flui-foundation`'s `testing`
//! feature.

pub mod sites;

pub use flui_foundation::testing::heap::{
    CountingAllocator, HeapCounts, largest_allocation_during,
};

/// Rounds [`assert_rounds_plateau`] runs before it starts comparing: the first ones fill what a
/// process keeps for good (a font system, shaped-text and type tables, thread-local pools).
const PLATEAU_WARM_UP: usize = 3;
/// Rounds [`assert_rounds_plateau`] compares.
const PLATEAU_ROUNDS: usize = 8;
// The comparison is between the last warm-up round and the last compared one.
const _: () = assert!(PLATEAU_WARM_UP >= 1 && PLATEAU_ROUNDS >= 1);

/// Runs `round` again and again on this thread and panics when its live heap keeps growing:
/// the last round has to leave no more bytes live than the last warm-up round did.
///
/// A round mounts an application, drives it and drops it. Everything an owner thread holds is
/// `Rc` (ADR-0323 §1), so a cycle between an element, its state, a listener and a signal frees
/// nothing, and no tool reports it: each round leaves one more tree behind.
///
/// The counters are per thread: a block counts for the thread that allocated it and against
/// the thread that freed it. The assertion therefore holds for a round that does its work on
/// this thread, which is where an application's tree lives. A round that frees on another
/// thread what this one allocated reads as growth and fails with nothing leaked; a round that
/// frees here what another thread allocated reads as a fall, which hides a leak of that size.
///
/// # Panics
///
/// When the rounds leave more bytes live each time, and when no round allocated anything on
/// this thread: the test target has not installed [`CountingAllocator`], or the round does
/// its work elsewhere, and a flat row of readings would say nothing.
#[track_caller]
pub fn assert_rounds_plateau(mut round: impl FnMut()) {
    // An array, so the readings are no allocation of this function's own.
    let mut live = [0_i64; PLATEAU_WARM_UP + PLATEAU_ROUNDS];
    let start = HeapCounts::now();
    for reading in &mut live {
        round();
        *reading = HeapCounts::now().since(start).live_bytes.cast_signed();
    }
    assert!(
        HeapCounts::now().since(start).allocations > 0,
        "no round allocated on this thread: the test target does not install \
         `CountingAllocator`, or the round works on another thread"
    );
    let (first, last) = (
        live[PLATEAU_WARM_UP - 1],
        live[PLATEAU_WARM_UP + PLATEAU_ROUNDS - 1],
    );
    assert!(
        last <= first,
        "the last {PLATEAU_ROUNDS} rounds left {} more bytes live; after each round, against \
         the start: {live:?}",
        last - first
    );
}
