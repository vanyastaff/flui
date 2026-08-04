//! Dev-only fixture for `flui-hot-reload`'s reload-lifecycle integration test
//! (`crates/flui-hot-reload/tests/reload_lifecycle.rs`).
//!
//! Never shipped: consumed exclusively as a `[dev-dependencies]` path entry
//! of `flui-hot-reload`, so cargo's own build graph produces this crate's
//! cdylib artifact automatically as a side effect of building
//! `flui-hot-reload`'s tests — no nested `cargo build` from inside a test.
//! See `reload_lifecycle.rs`'s module doc for why this one test earns the
//! "build + dlopen a real plugin" cost that `tests/loader.rs` documents
//! avoiding for everything else in this crate.

use std::sync::atomic::{AtomicU32, Ordering};

use flui_hot_reload::app_plugin;

/// Minimal root view — its content is irrelevant, only that mounting and
/// driving `app_plugin!`'s generated `flui_app_build` succeeds.
#[derive(Clone)]
struct FixtureRoot;

impl flui_view::StatelessView for FixtureRoot {
    fn build(&self, _ctx: &dyn flui_view::BuildContext) -> impl flui_view::IntoView {
        flui_view::ErrorView::new("hot-reload lifecycle fixture")
    }
}

impl flui_view::View for FixtureRoot {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

app_plugin!(FixtureRoot);

/// Per-image tick counter, deliberately independent of `app_plugin!`'s own
/// generated symbols.
///
/// A fresh `dlopen` of this file starts this back at zero (a fresh,
/// zero-initialized `static` in a genuinely new mapping). A runtime that
/// instead served a STALE, previously-loaded mapping for the same path — the
/// exact dlclose-deferral hazard `app_plugin!`'s `ManuallyDrop` thread-local
/// storage exists to avoid, see its "Unload semantics" docs — would keep
/// incrementing from whatever the earlier session left it at instead of
/// resetting. That divergence is what `reload_lifecycle.rs` asserts on.
static TICK: AtomicU32 = AtomicU32::new(0);

/// Increment and return this image's tick counter.
// This IS the crate's whole point (a symbol `reload_lifecycle.rs` resolves
// via `dlsym` after each dlopen) — the crate-type is `cdylib` specifically
// to export it.
#[allow(unsafe_code)]
#[unsafe(no_mangle)]
pub extern "C" fn fixture_tick() -> u32 {
    TICK.fetch_add(1, Ordering::SeqCst) + 1
}
