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
use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory};

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
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(output) => return output,
            Poll::Pending => {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                assert!(
                    !remaining.is_zero(),
                    "bridged load failed to complete within ten seconds"
                );
                std::thread::park_timeout(remaining);
            }
        }
    }
}

/// With no ambient tokio runtime anywhere in this process, `load_image_bridged`
/// must still start its own owned single-worker runtime and decode the
/// fixture. This is the test that fails if the owned-runtime fallback is ever
/// removed or broken — a plain `#[test]` has no tokio context to fall back on.
fn load_image_bridged_starts_an_owned_runtime_and_decodes_the_fixture() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(flui_assets::CacheCapacity::Entries(
            std::num::NonZeroU64::new(102).expect("nonzero test capacity"),
        ))
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
fn load_image_bridged_reports_a_missing_file_as_an_error_not_a_hang() {
    let registry = AssetRegistryBuilder::new()
        .with_capacity(flui_assets::CacheCapacity::Entries(
            std::num::NonZeroU64::new(102).expect("nonzero test capacity"),
        ))
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
    let recovered = block_on(registry.load_image_bridged(fixture_path()))
        .expect("a failed file load must not prevent the next load");
    assert_eq!((recovered.width(), recovered.height()), (4, 2));
}

fn entered_current_thread_runtime_does_not_trap_bridged_loads() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("the test runtime starts");
    let _entered = runtime.enter();
    // No Runtime::block_on: the ambient runtime never drives its queue.
    load_image_bridged_reports_a_missing_file_as_an_error_not_a_hang();
}

/// Copies of the 4x2 fixture (32 decoded bytes each) under distinct paths, in
/// a directory removed on drop.
struct FixtureCopies(std::path::PathBuf);

/// Decoded bytes of one fixture image.
const FIXTURE_BYTES: usize = 4 * 2 * 4;

impl FixtureCopies {
    fn new(name: &str) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "flui-assets-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("test clock is after the epoch")
                .as_nanos()
        ));
        std::fs::create_dir(&directory).expect("test directory is created");
        Self(directory)
    }

    fn copy(&self, name: &str) -> String {
        let path = self.0.join(name);
        std::fs::copy(fixture_path(), &path).expect("fixture is copied");
        path.to_string_lossy().into_owned()
    }
}

impl Drop for FixtureCopies {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn budgeted(bytes: usize) -> flui_assets::AssetRegistry {
    AssetRegistryBuilder::new()
        .with_default_capacity()
        .with_decoded_image_budget(bytes)
        .build()
}

fn is_kept(registry: &flui_assets::AssetRegistry, path: &str) -> bool {
    registry
        .cached_image(&flui_assets::ImageSource::Asset(path.into()))
        .is_some()
}

fn budget_evicts_the_least_recently_used_image() {
    let files = FixtureCopies::new("budget");
    let registry = budgeted(2 * FIXTURE_BYTES);
    let [a, b, c] = ["a.png", "b.png", "c.png"].map(|name| files.copy(name));
    for path in [&a, &b, &c] {
        block_on(registry.load_image_bridged(path.as_str())).expect("fixture decodes");
    }
    assert!(
        !is_kept(&registry, &a),
        "the oldest image goes past the budget"
    );
    assert!(is_kept(&registry, &b) && is_kept(&registry, &c));
}

fn an_image_over_the_budget_is_returned_but_not_kept() {
    let files = FixtureCopies::new("over-budget");
    let registry = budgeted(FIXTURE_BYTES - 1);
    let path = files.copy("large.png");
    let image = block_on(registry.load_image_bridged(path.as_str())).expect("fixture decodes");
    assert_eq!((image.width(), image.height()), (4, 2));
    assert!(!is_kept(&registry, &path));
}

fn trims_by_pressure_level(level: MemoryPressureLevel, older_kept: bool, newer_kept: bool) {
    let files = FixtureCopies::new("pressure");
    let registry = budgeted(2 * FIXTURE_BYTES);
    let [older, newer] = ["older.png", "newer.png"].map(|name| files.copy(name));
    for path in [&older, &newer] {
        block_on(registry.load_image_bridged(path.as_str())).expect("fixture decodes");
    }

    registry.reclaim(level);

    assert_eq!(is_kept(&registry, &older), older_kept, "{level:?}");
    assert_eq!(is_kept(&registry, &newer), newer_kept, "{level:?}");
    let reloaded = block_on(registry.load_image_bridged(older.as_str())).expect("reloads");
    assert_eq!((reloaded.width(), reloaded.height()), (4, 2));
}

fn moderate_pressure_trims_to_half_the_budget() {
    trims_by_pressure_level(MemoryPressureLevel::Moderate, false, true);
}

fn critical_pressure_drops_every_image() {
    trims_by_pressure_level(MemoryPressureLevel::Critical, false, false);
}

/// A registry keeps decoded images within its byte budget, least recently used
/// first, and gives them back under memory pressure by level.
#[test]
fn decoded_images_stay_within_their_byte_budget_and_give_back_under_pressure() {
    crate::cases::run_cases(&[
        (
            "budget evicts the least recent",
            budget_evicts_the_least_recently_used_image,
        ),
        (
            "an image over the budget",
            an_image_over_the_budget_is_returned_but_not_kept,
        ),
        (
            "moderate pressure",
            moderate_pressure_trims_to_half_the_budget,
        ),
        ("critical pressure", critical_pressure_drops_every_image),
    ]);
}

/// The bridge end to end with no ambient runtime: the success path decodes and
/// the failure path resolves to an error instead of hanging.
#[test]
fn load_image_bridged_completes_both_the_success_and_the_failure_path() {
    crate::cases::run_cases(&[
        (
            "owned runtime decode",
            load_image_bridged_starts_an_owned_runtime_and_decodes_the_fixture,
        ),
        (
            "missing file recovery",
            load_image_bridged_reports_a_missing_file_as_an_error_not_a_hang,
        ),
        (
            "undriven ambient runtime",
            entered_current_thread_runtime_does_not_trap_bridged_loads,
        ),
    ]);
}
