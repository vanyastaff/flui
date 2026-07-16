//! Verifies `AssetRegistry::load_image_bridged` — the async bridge between
//! flui-assets' background tokio runtime and a caller that has no ambient
//! runtime of its own (the `flui-widgets` `Image` widget's view layer polls
//! futures on its own local scheduler, not tokio).
//!
//! The "an injected handle wins over the owned-runtime fallback" mechanism
//! itself is proved deterministically (by matching the resolved
//! `BridgeRuntime` variant, white-box) in `registry::bridge`'s own unit
//! tests, not here — a black-box, timing-based version of that assertion
//! (poll once, expect `Pending`) turned out to pass even when the injection
//! path was broken, because a fast owned runtime can *also* leave a
//! just-spawned task `Pending` on its very first poll. This file sticks to
//! what an integration test can prove without timing assumptions: the public
//! API is wired correctly end to end and produces correct results.
//!
//! The "no ambient tokio" tests deliberately do **not** use `#[tokio::test]`:
//! that macro stands up an ambient runtime, which would make
//! `Handle::try_current()` always succeed and hide exactly the case the
//! owned-runtime fallback exists to serve. A hand-rolled, dependency-free
//! blocking poll loop drives the returned future instead — proof that
//! awaiting it requires no tokio context, only that *something* spawned it
//! onto a runtime once (which `load_image_bridged` itself does internally).
#![cfg(feature = "images")]

use std::future::Future;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use flui_assets::AssetRegistryBuilder;

/// Absolute path to the committed 4x2 RGBA fixture PNG (shared with
/// `image_asset_integration.rs`).
fn fixture_path() -> &'static str {
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tiny.png")
}

/// A minimal, dependency-free blocking executor: parks the calling thread
/// between polls and unparks it when the future's waker fires. Proves a
/// future can be driven to completion with no ambient async runtime at all —
/// which is exactly the property `load_image_bridged`'s returned future
/// (`rx.await` on a `tokio::sync::oneshot::Receiver`) promises: polling it
/// needs no reactor, only the one-time spawn already performed internally.
fn block_on<F: Future>(future: F) -> F::Output {
    struct ThreadWaker(std::thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let mut future = Box::pin(future);
    let waker = Waker::from(Arc::new(ThreadWaker(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => std::thread::park(),
        }
    }
}

/// With no ambient tokio runtime anywhere in this process, `load_image_bridged`
/// must still start its own owned single-worker runtime and decode the
/// fixture. This is the test that fails if the owned-runtime fallback is ever
/// removed or broken — a plain `#[test]` has no tokio context to fall back on.
#[test]
fn load_image_bridged_starts_an_owned_runtime_and_decodes_the_fixture() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(1024 * 1024)
        .build();

    let decoded = block_on(registry.load_image_bridged(fixture_path()))
        .expect("a real, well-formed PNG fixture must decode successfully");

    assert_eq!(
        (decoded.width(), decoded.height()),
        (4, 2),
        "the decoded image must keep the fixture's true 4x2 dimensions",
    );
}

/// A missing file must resolve to a typed `Err` promptly, not hang: the
/// bridge's oneshot completion must fire for the failure path exactly like
/// the success path.
#[test]
fn load_image_bridged_reports_a_missing_file_as_an_error_not_a_hang() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(1024 * 1024)
        .build();

    let missing = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/does-not-exist.png"
    );
    let result = block_on(registry.load_image_bridged(missing));

    assert!(
        result.is_err(),
        "a nonexistent path must surface a typed error, got {result:?}",
    );
}

/// `AssetRegistryBuilder::with_runtime_handle` must actually thread the given
/// handle through to `load_image_bridged` end to end: injecting a handle to a
/// currently-running ambient runtime and loading through it must succeed and
/// decode the fixture, exactly like the owned-runtime fallback does.
///
/// (That the INJECTED handle specifically — not a silently-substituted owned
/// runtime — is the one actually used is proved deterministically by
/// `registry::bridge::tests::resolve_prefers_an_injected_handle_over_starting_an_owned_runtime`,
/// which has white-box access to the resolved `BridgeRuntime` variant.)
#[tokio::test]
async fn load_image_bridged_works_with_an_explicitly_injected_handle() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(1024 * 1024)
        .with_runtime_handle(tokio::runtime::Handle::current())
        .build();

    let decoded = registry
        .load_image_bridged(fixture_path())
        .await
        .expect("the fixture must decode through the injected handle");

    assert_eq!((decoded.width(), decoded.height()), (4, 2));
}
