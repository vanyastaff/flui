//! Whether the current thread is inside an AppKit display pass of a view.
//!
//! AppKit discards a `setNeedsDisplay:` that is issued *while the view is being
//! displayed*: the flag is cleared when the pass unwinds, so the request that
//! was supposed to schedule the next frame disappears with it. Measured on
//! macOS 15.7 (Darwin 24.6) with an isolated probe, on a window that is fully
//! visible (`occlusionState` has the visible bit) in every arm:
//!
//! | re-arm issued from inside `drawRect:` | draws over 4 s |
//! |---|---|
//! | `[view setNeedsDisplay:YES]` | 1 |
//! | nothing at all (control) | 1 |
//! | `[view.layer setNeedsDisplay]` | 1 |
//! | `setNeedsDisplay:` deferred to the next main-queue turn | 396 |
//!
//! So a view's `drawRect:` cannot re-arm itself in place, and neither can its
//! layer; deferring the same call by one turn of the lane that is already
//! serialized is what actually re-arms it. `[view needsDisplay]` reads `NO`
//! immediately after the in-pass call and `[view.layer needsDisplay]` reads
//! `YES` while still never being serviced — the flag that survives is not the
//! flag AppKit acts on, so neither is a usable signal to test against.
//!
//! This module is the condition that decision is made on, kept apart from both
//! ends so it can be tested without a window: [`DisplayPassGuard`] is entered
//! by the view's `drawRect:` for the duration of the frame it dispatches, and
//! consulted by the window's `request_redraw` to choose between setting the
//! flag in place and deferring it.
//!
//! It is the AppKit realisation of ADR-0039 §4(b)/(c): the frame transaction
//! is a region the drain gate is closed for, and a wake arriving while it is
//! closed **defers** — the lane stays queued and the wake re-arms at the next
//! top-level anchor — so the frame transaction stays uninterruptible. The
//! `DrainGate` that same section specifies for the lane is this marker's
//! sibling: a wake delivered during a nested modal run loop must defer for the
//! same reason, and when the AppKit `CFRunLoopSource` relay lands it must
//! consult a gate that agrees with this one. Two owner-thread re-entrancy
//! markers that never reference each other is how they drift apart, so the
//! relation is written down here rather than implied.
//!
//! The marker is thread-local because "displaying" is a property of the thread
//! AppKit is displaying on, not of the process: every other thread is free to
//! request a redraw at that moment, and its request is queued behind the pass
//! rather than swallowed by it. The grain is therefore the *thread*, not "this
//! window's display pass" — a request for a different window issued on this
//! thread while any view here is displaying defers as well. That is
//! conservative in the safe direction (a deferred `setNeedsDisplay:` is always
//! honoured; it costs one lane turn at worst), and it is the reason
//! `request_redraw` does not claim to defer exactly the discarded call.

use std::cell::Cell;

thread_local! {
    /// True while this thread is inside a `drawRect:`-driven display pass.
    static IN_DISPLAY_PASS: Cell<bool> = const { Cell::new(false) };
}

/// Is the current thread inside an AppKit display pass of a view?
pub(super) fn in_display_pass() -> bool {
    IN_DISPLAY_PASS.with(Cell::get)
}

/// Marks the current thread as inside a display pass until dropped.
///
/// The guard restores the flag's previous value rather than clearing it, so
/// nesting (a display pass that somehow triggers another) unwinds correctly
/// instead of leaving the outer pass's marker cleared early.
pub(super) struct DisplayPassGuard {
    /// The flag's value before this guard set it.
    previous: bool,
}

impl DisplayPassGuard {
    /// Mark the current thread as inside a display pass.
    pub(super) fn enter() -> Self {
        let previous = IN_DISPLAY_PASS.with(|flag| flag.replace(true));
        Self { previous }
    }
}

impl Drop for DisplayPassGuard {
    fn drop(&mut self) {
        let previous = self.previous;
        IN_DISPLAY_PASS.with(|flag| flag.set(previous));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_is_clear_outside_a_pass() {
        assert!(!in_display_pass());
    }

    #[test]
    fn flag_is_set_inside_a_pass_and_restored_on_drop() {
        {
            let _guard = DisplayPassGuard::enter();
            assert!(in_display_pass());
        }
        assert!(!in_display_pass());
    }

    #[test]
    fn nested_guards_restore_at_each_level() {
        let outer = DisplayPassGuard::enter();
        assert!(in_display_pass());
        {
            let _inner = DisplayPassGuard::enter();
            assert!(in_display_pass());
        }
        // The inner guard must not clear the outer pass's marker.
        assert!(in_display_pass());
        drop(outer);
        assert!(!in_display_pass());
    }

    #[test]
    fn another_thread_does_not_observe_this_threads_pass() {
        let _guard = DisplayPassGuard::enter();
        let observed = std::thread::spawn(in_display_pass)
            .join()
            .expect("the probe thread must not panic");
        // The displaying thread is inside a pass and the probing thread is not
        // — a process-wide flag would report the same value to both.
        assert!(in_display_pass());
        assert!(!observed);
    }
}
