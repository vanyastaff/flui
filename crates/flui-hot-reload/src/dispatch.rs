//! Worker ↔ host dispatch helpers (Phase B).
//!
//! Worker crates register reloadable `build()` bodies via `flui_worker_init`.
//! The host retains a [`RebuildHookRegistration`] so gesture handlers inside
//! the worker can schedule a frame without linking `flui-app`.

use std::sync::{
    Arc, LazyLock,
    atomic::{AtomicU64, Ordering},
};

use flui_view::BuildContext;
use parking_lot::Mutex;

type RebuildHook = Arc<dyn Fn() + Send + Sync + 'static>;

#[derive(Default)]
struct RebuildHookSlot {
    generation: u64,
    hook: Option<RebuildHook>,
}

static NEXT_HOOK_GENERATION: AtomicU64 = AtomicU64::new(1);
static REQUEST_REBUILD: LazyLock<Mutex<RebuildHookSlot>> =
    LazyLock::new(|| Mutex::new(RebuildHookSlot::default()));

fn next_hook_generation(counter: &AtomicU64) -> u64 {
    counter
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            (current != 0).then(|| current.checked_add(1).unwrap_or(0))
        })
        .expect("BUG: rebuild hook generation capacity exhausted")
}

/// Incoming captures remain in custody if generation admission fails.
struct PendingRebuildHook(Option<RebuildHook>);

impl Drop for PendingRebuildHook {
    fn drop(&mut self) {
        if std::thread::panicking() {
            std::mem::forget(self.0.take());
        }
    }
}

/// RAII ownership of the currently installed host rebuild hook.
///
/// Dropping a registration removes its hook only when it is still current.
/// Replacing registration A with B therefore makes a late `drop(A)` harmless.
/// A request that already cloned A may still run; the host-side stamped realm
/// dispatcher is responsible for rejecting that stale incarnation.
#[derive(Debug)]
#[must_use = "retain this registration for as long as rebuild requests should be routed"]
pub struct RebuildHookRegistration {
    generation: u64,
}

impl Drop for RebuildHookRegistration {
    fn drop(&mut self) {
        let detached = {
            let mut slot = REQUEST_REBUILD.lock();
            (slot.generation == self.generation)
                .then(|| slot.hook.take())
                .flatten()
        };
        // A captured closure may own arbitrary user state whose destructor
        // re-enters this registry. Never run that destructor under the slot
        // mutex.
        drop(detached);
    }
}

/// Install or replace the host hook that schedules a widget rebuild.
///
/// The returned guard must be retained for exactly the lifetime of the host
/// realm and dropped before that realm is torn down. Replacement is atomic
/// with respect to [`request_rebuild`]: callers observe either the old or the
/// new owned closure, never a partially-updated registration.
///
/// # Panics
///
/// Panics after the final nonzero generation has been admitted. Exhaustion is
/// permanent, including after a caught panic or removal of a registration.
/// The rejected hook is retained before this capacity panic so its captures'
/// destructors cannot replace the failure; accepted hooks keep their ordinary
/// lifetime policy.
pub fn register_request_rebuild(
    hook: impl Fn() + Send + Sync + 'static,
) -> RebuildHookRegistration {
    register_request_rebuild_with_counter(hook, &NEXT_HOOK_GENERATION)
}

fn register_request_rebuild_with_counter(
    hook: impl Fn() + Send + Sync + 'static,
    counter: &AtomicU64,
) -> RebuildHookRegistration {
    let mut hook = PendingRebuildHook(Some(Arc::new(hook)));
    let generation = next_hook_generation(counter);
    let replaced = {
        let mut slot = REQUEST_REBUILD.lock();
        slot.generation = generation;
        slot.hook
            .replace(hook.0.take().expect("BUG: pending rebuild hook is owned"))
    };
    // As in `Drop`, replacement must not destroy captured user state while
    // the registry lock is held.
    drop(replaced);
    RebuildHookRegistration { generation }
}

/// Ask the host to rebuild dirty elements on the next frame.
///
/// No-op with a warning when the host has not registered a hook yet.
pub fn request_rebuild() {
    let hook = REQUEST_REBUILD.lock().hook.clone();
    if let Some(hook) = hook {
        hook();
    } else {
        tracing::warn!("flui_hot_reload::request_rebuild called before host registered a hook");
    }
}

/// Context passed from the host-owned `ViewState::build` into worker code.
pub struct WorkerBuildEnv<'a> {
    ctx: &'a dyn BuildContext,
}

impl std::fmt::Debug for WorkerBuildEnv<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerBuildEnv")
            .field("element_id", &self.ctx.element_id())
            .finish_non_exhaustive()
    }
}

impl<'a> WorkerBuildEnv<'a> {
    /// Wrap the framework `BuildContext` for a worker build call.
    pub fn new(ctx: &'a dyn BuildContext) -> Self {
        Self { ctx }
    }

    /// Underlying framework build context.
    pub fn framework_ctx(&self) -> &'a dyn BuildContext {
        self.ctx
    }

    /// Schedule a host rebuild (see [`request_rebuild`]).
    pub fn request_rebuild(&self) {
        request_rebuild();
    }
}

/// Serializes this crate's unit tests that install a rebuild hook:
/// [`REQUEST_REBUILD`] is process-global, and `cargo test` runs a binary's
/// tests on parallel threads.
#[cfg(test)]
pub(crate) static REBUILD_HOOK_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    #[test]
    fn hook_generation_exhaustion_preserves_current_registration() {
        struct DropBomb(Arc<AtomicUsize>);
        impl Drop for DropBomb {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::Relaxed);
                panic!("rejected capture destruction");
            }
        }
        let _registry = REBUILD_HOOK_TEST_LOCK.lock();
        let counter = AtomicU64::new(1);
        let calls = Arc::new(AtomicUsize::new(0));
        let first = register_request_rebuild_with_counter(|| {}, &counter);
        counter.store(u64::MAX, Ordering::Relaxed);
        let hook_calls = Arc::clone(&calls);
        let last = register_request_rebuild_with_counter(
            move || {
                hook_calls.fetch_add(1, Ordering::Relaxed);
            },
            &counter,
        );
        for _ in 0..8 {
            let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                register_request_rebuild_with_counter(|| panic!("rejected hook ran"), &counter)
            }))
            .expect_err("exhausted allocator must permanently refuse");
            flui_foundation::panic::retain_opaque_payload(failure);
        }
        drop(first);
        request_rebuild();
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "stale guard cannot remove terminal hook"
        );

        let drops = Arc::new(AtomicUsize::new(0));
        let captures = (DropBomb(Arc::clone(&drops)), DropBomb(Arc::clone(&drops)));
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            register_request_rebuild_with_counter(
                move || {
                    std::hint::black_box(&captures);
                },
                &counter,
            )
        }))
        .expect_err("hostile rejected hook is refused");
        assert!(
            flui_foundation::panic::payload_text(failure.as_ref()).is_some_and(|message| {
                message.starts_with("BUG: rebuild hook generation capacity exhausted")
            })
        );
        flui_foundation::panic::retain_opaque_payload(failure);
        assert_eq!(
            drops.load(Ordering::Relaxed),
            0,
            "rejected captures retained before Drop"
        );
        request_rebuild();
        assert_eq!(calls.load(Ordering::Relaxed), 2);
        drop(last);

        let fresh_counter = AtomicU64::new(2);
        let hook_calls = Arc::clone(&calls);
        let fresh = register_request_rebuild_with_counter(
            move || {
                hook_calls.fetch_add(1, Ordering::Relaxed);
            },
            &fresh_counter,
        );
        request_rebuild();
        assert_eq!(
            calls.load(Ordering::Relaxed),
            3,
            "independent admission still delivers"
        );
        drop(fresh);
    }

    #[test]
    fn replacing_registration_is_generation_safe_for_racing_old_clone() {
        let _registry = REBUILD_HOOK_TEST_LOCK.lock();
        let a_calls = Arc::new(AtomicUsize::new(0));
        let a_in_hook = Arc::clone(&a_calls);
        let registration_a = register_request_rebuild(move || {
            a_in_hook.fetch_add(1, Ordering::Relaxed);
        });
        let captured_a = REQUEST_REBUILD
            .lock()
            .hook
            .clone()
            .expect("A hook registered");

        let b_calls = Arc::new(AtomicUsize::new(0));
        let b_in_hook = Arc::clone(&b_calls);
        let registration_b = register_request_rebuild(move || {
            b_in_hook.fetch_add(1, Ordering::Relaxed);
        });
        drop(registration_a);

        captured_a();
        assert_eq!(a_calls.load(Ordering::Relaxed), 1);
        assert_eq!(b_calls.load(Ordering::Relaxed), 0);

        request_rebuild();
        assert_eq!(b_calls.load(Ordering::Relaxed), 1);
        drop(registration_b);
    }
}
