//! Shared "owner lane" machinery for the macOS backend.
//!
//! The owner lane is the single serialization point every thread-affine AppKit
//! call must travel through: the AppKit main thread's dispatch queue for
//! production instances, and one process-wide serial dispatch queue for test
//! instances (test processes never pump the AppKit main thread, so the main
//! queue there would never drain and `exec_sync` would block forever). Routing
//! onto the same lane the rest of the process already serializes on puts FLUI
//! on exactly the serialization AppKit's main-thread convention implies.
//!
//! # Hazards
//!
//! A cross-thread call **blocks** the caller on the owner lane until the
//! operation completes. Two residual hazards follow.
//!
//! - **Un-drained lane stall** — a worker calling onto a main-lane owner
//!   before the AppKit main run loop has started servicing the main queue
//!   (`[NSApp run]`) blocks until it does; `exec_sync` gives no timeout. Do
//!   not call through a main-lane owner from a background thread before
//!   `Platform::run`.
//! - **Cross-lane reentrancy deadlock** — a block executing ON a lane that
//!   dispatches to a DIFFERENT lane and back deadlocks (the second lane's
//!   worker waits on the first, which waits on the second). The reentrancy
//!   probe rescues only same-lane nesting, and the `is_main` shortcut rescues
//!   only **main-lane** instances — never nest across lanes.
//!
//! The reentrancy probe's exactness depends on the `OnceLock` caching in
//! [`owner_queue`] / [`test_owner_queue`]: the marker is the lane queue's
//! ADDRESS, so a per-call re-created `Queue::main()` / `Queue::create` would
//! break the identity comparison. Keep the statics.

use objc::{class, msg_send, sel, sel_impl};

/// The production owner lane: the dispatch queue bound to the AppKit main
/// thread. Calls from other threads are dispatched here synchronously.
pub(super) fn owner_queue() -> &'static dispatch::Queue {
    static Q: std::sync::OnceLock<dispatch::Queue> = std::sync::OnceLock::new();
    Q.get_or_init(dispatch::Queue::main)
}

/// The test-instance owner lane: one process-wide serial queue shared by every
/// test instance. `cargo test` never pumps the AppKit main thread, so
/// [`owner_queue`] there would deadlock; a serial queue serializes owner-lane
/// traffic exactly as the main lane does for production. Every test instance
/// routes through this same lane so test traffic never straddles lanes.
#[cfg(test)]
pub(super) fn test_owner_queue() -> &'static dispatch::Queue {
    static Q: std::sync::OnceLock<dispatch::Queue> = std::sync::OnceLock::new();
    Q.get_or_init(|| {
        dispatch::Queue::create("flui.owner-lane.test", dispatch::QueueAttribute::Serial)
    })
}

thread_local! {
    /// The owner lane the current thread is executing a block on, if any —
    /// recorded by [`OnOwnerQueueGuard`]. GCD pools worker threads across
    /// queues, so a sticky flag would route a later call on a reused thread
    /// off-lane; the guard's `Drop` clears the marker when the block returns.
    static ON_OWNER_QUEUE: std::cell::Cell<Option<*const dispatch::Queue>> =
        const { std::cell::Cell::new(None) };
}

/// Reentrancy probe: is the current thread executing a block ON `owner`'s
/// lane? The marker is the lane identity itself — the owner queue's address —
/// so this can tell "on THIS instance's lane" from "on some other lane a
/// reused worker thread last ran". All production instances share [`owner_queue`]
/// and all test instances share [`test_owner_queue`], so the probe is exact for
/// every instance of either kind.
pub(super) fn on_owner_queue(owner: &'static dispatch::Queue) -> bool {
    ON_OWNER_QUEUE.with(std::cell::Cell::get) == Some(std::ptr::from_ref(owner))
}

/// RAII reentrancy marker: marks the current thread as executing a block on
/// the owner lane. The `Drop` clears the marker only if it is still this
/// guard's own owner — a nested block on a different lane overwrites it, and
/// it is that block's own guard, which runs after this one, that clears it.
///
/// That mechanism is a rescue for SAME-lane nesting only, and the limitation is
/// deliberate: a block on lane B entered inline inside a block on lane A
/// overwrites the marker with B; B's guard clears it to `None`, and A's guard —
/// seeing `None`, not A — cannot restore it, so a later nested
/// `exec_on_owner(A)` on that thread misses the probe and dispatches, a
/// self-deadlock. Never nest across lanes; the `is_main` shortcut rescues only
/// main-lane instances.
struct OnOwnerQueueGuard {
    owner: *const dispatch::Queue,
}

impl OnOwnerQueueGuard {
    fn new(owner: &'static dispatch::Queue) -> Self {
        let owner = std::ptr::from_ref(owner);
        ON_OWNER_QUEUE.with(|on_lane| on_lane.set(Some(owner)));
        OnOwnerQueueGuard { owner }
    }
}

impl Drop for OnOwnerQueueGuard {
    fn drop(&mut self) {
        ON_OWNER_QUEUE.with(|on_lane| {
            if on_lane.get() == Some(self.owner) {
                on_lane.set(None);
            }
        });
    }
}

/// Run `f` ON the owner lane.
///
/// When the caller is already executing on the lane (the reentrancy probe), or
/// the owner is the main queue and the caller is on the OS main thread, `f`
/// runs directly with no dispatch. From any other thread `f` is dispatched
/// synchronously to the owner queue and the caller blocks until it completes.
/// `f`'s result travels back across the thread boundary, so it must be `Send`;
/// callers must never capture a raw Objective-C `id` into `f` for exactly that
/// reason.
pub(super) fn exec_on_owner<R: Send>(
    owner: &'static dispatch::Queue,
    owner_is_main: bool,
    f: impl FnOnce() -> R + Send,
) -> R {
    if on_owner_queue(owner)
        || (owner_is_main && {
            // SAFETY: `+[NSThread isMainThread]` is a documented thread-safe
            // class method with no arguments and a BOOL return; it may be
            // called from any thread at any time.
            let is_main: bool = unsafe { msg_send![class!(NSThread), isMainThread] };
            is_main
        })
    {
        f()
    } else {
        // SAFETY: dispatch invokes the closure through an `extern "C"`
        // trampoline with no panic catch, so a Rust panic unwinding through
        // libdispatch's C frames would be undefined behaviour. The dispatched
        // body is therefore catch_unwind-wrapped and the payload is replayed
        // on the caller once `exec_sync` returns. The RAII guard and the
        // `catch_unwind` call themselves sit in front of the panicking body,
        // but both are infallible (`Cell::set` and a plain std call) while
        // GCD is running the block.
        let result = owner.exec_sync(move || {
            let _guard = OnOwnerQueueGuard::new(owner);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        });
        match result {
            Ok(value) => value,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_on_owner_runs_inline_when_already_on_the_lane() {
        let owner = test_owner_queue();
        // Entry into the lane sets the reentrancy guard; a nested call must
        // observe it and run inline on the SAME thread, with no dispatch (a
        // serial queue would deadlock if the nested call dispatched). Both
        // thread ids are captured within the same routed block, which never
        // relies on a serial queue preserving a thread across dispatches.
        let (entry_thread, nested_thread) = exec_on_owner(owner, false, || {
            let entry_thread = std::thread::current().id();
            let nested_thread = exec_on_owner(owner, false, || std::thread::current().id());
            (entry_thread, nested_thread)
        });
        assert_eq!(
            entry_thread, nested_thread,
            "exec_on_owner must run inline when already executing on the owner lane"
        );
    }

    #[test]
    fn exec_on_owner_routes_off_lane_calls_onto_the_lane() {
        let owner = test_owner_queue();
        // The routed body observes the on-lane marker within the SAME block that
        // executes it, so the witness never relies on a serial queue preserving
        // a thread across dispatches. The routing contract is LANE MEMBERSHIP,
        // not thread identity: a body running under the lane guard is serialized
        // against every other block on that lane.
        let handle =
            std::thread::spawn(move || exec_on_owner(owner, false, || on_owner_queue(owner)));
        let on_lane = handle
            .join()
            .expect("routed call must finish without panicking");
        // Deliberately NOT asserting a different thread than the spawner: modern
        // libdispatch executes an uncontended `dispatch_sync` to a serial queue
        // INLINE on the calling thread (`_dispatch_lane_barrier_sync_invoke_and_
        // complete`, observed on this machine), so thread identity is not the
        // routing contract for a serial lane. The on-lane marker is the stable
        // and sufficient proof: a non-routing `exec_on_owner` (body invoked with
        // no lane guard) leaves it unset and fails here.
        assert!(
            on_lane,
            "the routed body must observe the owner-lane marker: exec_on_owner must run its \
             body only under the lane guard, never bare on the caller"
        );
    }
}
