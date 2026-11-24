//! The window a perf scenario counts: [`measure`] runs the scenario's steps
//! and returns what they cost, which a [`Baselines`](super::Baselines) hold
//! takes (ADR-0396 §1).
//!
//! Each window a thread opens takes the next number of that thread's
//! sequence, so a test run again from its start opens the same windows in the
//! same order. That is how a window's instructions are counted (ADR-0396 §3):
//! the hold runs its test again under callgrind with the window's number, and
//! that run collects instructions only while the window with that number runs
//! ([`super::instructions`]).

use std::num::NonZeroU32;

use flui_foundation::IdMint;
use flui_foundation::testing::heap::HeapCounts;

use super::instructions;

thread_local! {
    /// The numbers of the windows this thread opens, from 1. libtest runs
    /// each test on a thread of its own, so a test's first window is 1.
    static WINDOWS: IdMint<NonZeroU32> = const { IdMint::new() };
}

/// What a scenario's measured steps cost, as [`measure`] counted it: what
/// they asked of the heap on this thread, and which of the thread's windows
/// they ran in, which a hold that counts instructions runs again.
#[derive(Clone, Copy, Debug)]
pub struct Cost {
    heap: HeapCounts,
    window: NonZeroU32,
}

impl Cost {
    /// What the steps asked of the heap on this thread: allocations,
    /// reallocations and the bytes they asked for. `live_bytes` is zero:
    /// what a window leaves live is a mount's figure, which no frame's
    /// window is held to.
    #[must_use]
    pub fn heap(&self) -> HeapCounts {
        self.heap
    }

    /// The number of the window the steps ran in.
    pub(super) fn window(&self) -> NonZeroU32 {
        self.window
    }
}

/// Runs a scenario's measured steps and returns their result with what they
/// cost.
///
/// The heap counts need the target's `#[global_allocator]` to be the kit's
/// counting allocator (`flui_testing::heap`); without it they are zero. Under
/// a hold that counts instructions, this is the window the run under
/// callgrind collects.
///
/// # Panics
///
/// When this thread has opened `u32::MAX` windows, or the run under callgrind
/// names its window in a form it cannot read.
pub fn measure<R>(steps: impl FnOnce() -> R) -> (R, Cost) {
    let window = WINDOWS
        .with(IdMint::next)
        .expect("BUG: a thread opens fewer than u32::MAX perf windows");
    // Read before the heap is, since reading the environment allocates.
    let collected = instructions::collected_window() == Some(window);
    let before = HeapCounts::now();
    let result = if collected {
        flui_perf_collected_window(steps)
    } else {
        steps()
    };
    let heap = HeapCounts {
        live_bytes: 0,
        ..HeapCounts::now().since(before)
    };
    (result, Cost { heap, window })
}

/// The steps of the one window a run under callgrind collects: its
/// `--toggle-collect` names this function ([`instructions::WINDOW_SYMBOL`]),
/// so it keeps its name and stays out of line. `black_box` keeps the call of
/// `steps` from becoming a tail jump, which callgrind would not see return
/// through this frame.
#[inline(never)]
fn flui_perf_collected_window<R>(steps: impl FnOnce() -> R) -> R {
    std::hint::black_box(steps())
}
