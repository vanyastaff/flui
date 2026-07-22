//! Background tokio runtime backing [`AssetRegistry::load_image_bridged`](super::AssetRegistry::load_image_bridged).
//!
//! FLUI's view layer polls futures cooperatively on its own local scheduler
//! (`flui-scheduler`'s `AsyncDriver`), not tokio — but the decode path this
//! module bridges to (`ImageAsset::load`) is a genuine `tokio::fs` read plus a
//! CPU-bound decode. Rather than require every host application to already
//! run a tokio runtime, [`AssetRegistry`](super::AssetRegistry) owns one: a
//! single-worker, named background runtime, started lazily on the first call
//! that needs it. A host that already runs tokio can inject its [`Handle`]
//! instead via `AssetRegistryBuilder::with_runtime_handle` — misconfiguration
//! is impossible, because [`BridgeRuntime::resolve`] always falls back to
//! starting (or reusing) an owned runtime when nothing else is available.

use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

/// Thread name of the lazily-started owned runtime's single worker.
const WORKER_THREAD_NAME: &str = "flui-assets-bridge";

/// Resolves the handle a bridged load should spawn onto, on every call.
///
/// Only the last-resort owned runtime is memoized (in `owned`, below) —
/// starting it is expensive (an OS thread), and this registry controls its
/// lifetime completely, so it is always safe to reuse once started. An
/// injected or ambient handle is **never** memoized: doing so was a real bug
/// (fixed here) — an ambient `Handle::try_current()` reflects whatever tokio
/// context happens to be active on the calling thread *right now*, and that
/// context can shut down and restart between calls. Caching a `Handle` from
/// a runtime that has since shut down would not panic (`Handle::spawn`
/// degrades silently: the task is scheduled but never polled), so every
/// later bridged load would permanently fail with a "task was dropped"
/// error, with no way to recover short of rebuilding the registry. An
/// injected handle is a deliberate, per-call-checked promise from the host
/// (`AssetRegistryBuilder::with_runtime_handle`'s contract is that the
/// injected runtime outlives the registry) — re-cloning it each call costs
/// nothing and closes the same staleness class of bug for it too.
pub(crate) struct BridgeRuntime {
    /// Started lazily on first use when neither an injected nor an ambient
    /// handle is available; reused for every subsequent call that also finds
    /// neither available.
    owned: OnceLock<Runtime>,
}

impl BridgeRuntime {
    pub(crate) fn new() -> Self {
        Self {
            owned: OnceLock::new(),
        }
    }

    /// Resolution order, freshly checked on every call: `injected` (if the
    /// registry was built with one) wins unconditionally; otherwise an
    /// ambient tokio context on the calling thread right now
    /// (`Handle::try_current`); otherwise this registry's own runtime,
    /// started on first need.
    pub(crate) fn resolve(&self, injected: Option<&Handle>) -> Handle {
        if let Some(handle) = injected {
            return handle.clone();
        }
        if let Ok(handle) = Handle::try_current() {
            return handle;
        }
        self.owned
            .get_or_init(|| {
                Builder::new_multi_thread()
                    .worker_threads(1)
                    .thread_name(WORKER_THREAD_NAME)
                    .enable_all()
                    .build()
                    .expect(
                        "BUG: building a single-worker tokio runtime must succeed \
                         on any platform this crate targets",
                    )
            })
            .handle()
            .clone()
    }
}

impl Drop for BridgeRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.owned.take() {
            // `Runtime::shutdown_background` returns immediately without
            // blocking the calling thread. `Runtime`'s own `Drop` instead
            // performs a BLOCKING shutdown (joins every worker thread) and
            // panics ("Cannot drop a runtime in a context where blocking is
            // not allowed") if the drop itself happens from inside any
            // tokio task -- exactly what happens when the last
            // `Arc<AssetRegistry>` referencing this runtime is dropped by a
            // task spawned on it (or on any other runtime). Using
            // `shutdown_background` here makes dropping the registry safe
            // from any context, at the cost of not waiting for in-flight
            // tasks to finish (they are abandoned, same as an abrupt process
            // exit would abandon them).
            runtime.shutdown_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handle passed explicitly must win over both the ambient-runtime and
    /// owned-runtime fallbacks — checked by resolving twice inside an
    /// ambient context that is NOT the injected one, proving the injected
    /// handle is preferred rather than the (also available) ambient ID.
    #[test]
    fn resolve_prefers_an_injected_handle_over_starting_an_owned_runtime() {
        let source = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("building a current-thread runtime must succeed");
        let injected_handle = source.handle().clone();

        let bridge = BridgeRuntime::new();
        let resolved = bridge.resolve(Some(&injected_handle));

        assert_eq!(
            resolved.id(),
            injected_handle.id(),
            "an injected handle must be returned as-is, never substituted",
        );
        assert!(
            bridge.owned.get().is_none(),
            "an injected handle must never cause the owned fallback runtime to start",
        );
    }

    /// With no injected handle and no ambient tokio context, `resolve` must
    /// fall back to starting its own owned runtime.
    #[test]
    fn resolve_starts_an_owned_runtime_with_no_injection_and_no_ambient_context() {
        let bridge = BridgeRuntime::new();
        bridge.resolve(None);

        assert!(
            bridge.owned.get().is_some(),
            "no injection and no ambient runtime must start the owned fallback",
        );
    }

    /// With no injected handle but an ambient tokio runtime already running on
    /// the calling thread, `resolve` must reuse it (`Handle::try_current`)
    /// rather than start a redundant owned runtime.
    #[tokio::test]
    async fn resolve_reuses_an_ambient_runtime_with_no_injection() {
        let bridge = BridgeRuntime::new();
        let resolved = bridge.resolve(None);

        assert_eq!(
            resolved.id(),
            Handle::current().id(),
            "an ambient tokio context with no injection must be reused directly",
        );
        assert!(
            bridge.owned.get().is_none(),
            "an available ambient runtime must never cause the owned fallback to start",
        );
    }

    /// The core fix: an ambient handle used on an earlier call must NEVER be
    /// reused once that runtime has shut down — `resolve` must notice (by
    /// re-checking `Handle::try_current` fresh every call, not memoizing the
    /// first result) and fall back to its own durable owned runtime instead
    /// of returning a handle that will silently drop every future spawn.
    #[test]
    fn resolve_does_not_reuse_a_since_shut_down_ambient_handle() {
        let bridge = BridgeRuntime::new();

        let ambient = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("building a current-thread runtime must succeed");
        let first = ambient.block_on(async { bridge.resolve(None) });
        drop(ambient); // the ambient runtime is now fully shut down.

        // A later call, with no ambient context anymore, must fall back to
        // (and start) the owned runtime rather than reusing the dead first
        // handle.
        let second = bridge.resolve(None);
        assert_ne!(
            first.id(),
            second.id(),
            "a stale ambient handle from a since-shut-down runtime must never be reused",
        );

        // Prove `second` is genuinely usable, not just structurally
        // different: spawn a trivial task on it and observe it complete.
        let (tx, rx) = std::sync::mpsc::channel();
        second.spawn(async move {
            let _ = tx.send(());
        });
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .expect("the fallback owned runtime must actually run spawned tasks");
    }

    /// Dropping a [`BridgeRuntime`] that started (and therefore owns) a
    /// runtime, from INSIDE a task running on that very runtime, must not
    /// panic — the scenario is the last `Arc<AssetRegistry>` going out of
    /// scope inside a spawned task. Before the `shutdown_background` fix,
    /// `Runtime`'s default blocking `Drop` panicked here.
    #[test]
    fn dropping_from_inside_its_own_task_does_not_panic() {
        let bridge = BridgeRuntime::new();
        // No ambient context in this plain #[test] fn and no injection, so
        // this starts (and owns) a runtime.
        let handle = bridge.resolve(None);

        let (done_tx, done_rx) = std::sync::mpsc::channel();
        handle.spawn(async move {
            drop(bridge);
            let _ = done_tx.send(());
        });

        done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("the drop must complete (not hang or panic) inside the async task");
    }
}
