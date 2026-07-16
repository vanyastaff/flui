//! Background tokio runtime backing [`AssetRegistry::load_image_bridged`](super::AssetRegistry::load_image_bridged).
//!
//! FLUI's view layer polls futures cooperatively on its own local scheduler
//! (`flui-scheduler`'s `AsyncDriver`), not tokio — but the decode path this
//! module bridges to (`ImageAsset::load`) is a genuine `tokio::fs` read plus a
//! CPU-bound decode. Rather than require every host application to already
//! run a tokio runtime, [`AssetRegistry`](super::AssetRegistry) owns one: a
//! single-worker, named background runtime, started lazily on the first
//! bridged load. A host that already runs tokio can inject its [`Handle`]
//! instead via `AssetRegistryBuilder::with_runtime_handle` — misconfiguration
//! is impossible either way, because [`resolve`] always falls back to
//! starting an owned runtime when nothing else is available.

use std::sync::OnceLock;

use tokio::runtime::{Builder, Handle, Runtime};

/// Thread name of the lazily-started owned runtime's single worker.
const WORKER_THREAD_NAME: &str = "flui-assets-bridge";

/// Either a runtime this registry started and owns, or a handle borrowed from
/// one a host application (or an ambient `#[tokio::test]`/`#[tokio::main]`
/// context) already runs.
pub(crate) enum BridgeRuntime {
    /// Started on first use; kept alive for the registry's lifetime and never
    /// shut down early — dropping the registry drops (and gracefully shuts
    /// down) the runtime.
    Owned(Runtime),
    /// Borrowed from a host-supplied or ambient runtime; this registry never
    /// shuts it down.
    Injected(Handle),
}

impl BridgeRuntime {
    fn handle(&self) -> Handle {
        match self {
            Self::Owned(runtime) => runtime.handle().clone(),
            Self::Injected(handle) => handle.clone(),
        }
    }
}

/// Resolves the handle a bridged load should spawn onto, memoizing the choice
/// in `cell` on first call.
///
/// Resolution order: `injected` (set at registry construction) wins
/// unconditionally; otherwise an ambient tokio runtime already running on the
/// calling thread (`Handle::try_current`) is reused; otherwise a dedicated
/// single-worker runtime is started and kept alive for the registry's
/// lifetime.
pub(crate) fn resolve(cell: &OnceLock<BridgeRuntime>, injected: Option<&Handle>) -> Handle {
    cell.get_or_init(|| choose(injected)).handle()
}

/// The choice `resolve` memoizes: injected, then ambient, then owned.
fn choose(injected: Option<&Handle>) -> BridgeRuntime {
    if let Some(handle) = injected {
        return BridgeRuntime::Injected(handle.clone());
    }
    if let Ok(handle) = Handle::try_current() {
        return BridgeRuntime::Injected(handle);
    }
    let runtime = Builder::new_multi_thread()
        .worker_threads(1)
        .thread_name(WORKER_THREAD_NAME)
        .enable_all()
        .build()
        .expect(
            "BUG: building a single-worker tokio runtime must succeed \
             on any platform this crate targets",
        );
    BridgeRuntime::Owned(runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A handle passed explicitly must win over both the ambient-runtime and
    /// owned-runtime fallbacks — checked by matching the memoized variant
    /// directly (white-box), not by inferring it from timing.
    #[test]
    fn resolve_prefers_an_injected_handle_over_starting_an_owned_runtime() {
        let source = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("building a current-thread runtime must succeed");
        let handle = source.handle().clone();

        let cell = OnceLock::new();
        resolve(&cell, Some(&handle));

        assert!(
            matches!(cell.get(), Some(BridgeRuntime::Injected(_))),
            "an injected handle must resolve to BridgeRuntime::Injected, not Owned",
        );
    }

    /// With no injected handle and no ambient tokio context, `resolve` must
    /// fall back to starting its own owned runtime.
    #[test]
    fn resolve_starts_an_owned_runtime_with_no_injection_and_no_ambient_context() {
        let cell = OnceLock::new();
        resolve(&cell, None);

        assert!(
            matches!(cell.get(), Some(BridgeRuntime::Owned(_))),
            "no injection and no ambient runtime must resolve to BridgeRuntime::Owned",
        );
    }

    /// With no injected handle but an ambient tokio runtime already running on
    /// the calling thread, `resolve` must reuse it (`Handle::try_current`)
    /// rather than start a redundant owned runtime.
    #[tokio::test]
    async fn resolve_reuses_an_ambient_runtime_with_no_injection() {
        let cell = OnceLock::new();
        resolve(&cell, None);

        assert!(
            matches!(cell.get(), Some(BridgeRuntime::Injected(_))),
            "an ambient tokio context with no injection must resolve to \
             BridgeRuntime::Injected via Handle::try_current, not Owned",
        );
    }

    /// The choice is memoized: a second call with a DIFFERENT (and in this
    /// case invalid/dropped) handle must not change what the first call
    /// already committed to.
    #[test]
    fn resolve_memoizes_the_first_choice() {
        let first_source = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("building a current-thread runtime must succeed");
        let first_handle = first_source.handle().clone();

        let cell = OnceLock::new();
        let resolved_first = resolve(&cell, Some(&first_handle));

        let second_source = Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("building a second current-thread runtime must succeed");
        let second_handle = second_source.handle().clone();
        let resolved_second = resolve(&cell, Some(&second_handle));

        assert_eq!(
            resolved_first.id(),
            resolved_second.id(),
            "the second call must return the SAME memoized handle as the first, \
             ignoring the differing `injected` argument",
        );
    }
}
