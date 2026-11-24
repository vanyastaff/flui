//! Dev-only fixture for `flui-hot-reload`'s reload-lifecycle integration test
//! (`tests/reload_lifecycle.rs`).
//!
//! Never shipped. The repository test tasks build this plugin's library before
//! running its integration tests. Cargo publishes the directly built cdylib to
//! its artifact directory; no test starts a nested Cargo build.
//! See `reload_lifecycle.rs`'s module doc for why this one test earns the
//! "build + dlopen a real plugin" cost that the rest of `flui-hot-reload`'s
//! tests avoid.

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
    fn create_element(self: std::rc::Rc<Self>) -> flui_view::element::ElementKind {
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
#[expect(
    unsafe_code,
    reason = "`#[unsafe(no_mangle)]` exports the symbol the lifecycle test resolves with `dlsym`"
)]
#[unsafe(no_mangle)]
pub extern "C" fn fixture_tick() -> u32 {
    TICK.fetch_add(1, Ordering::SeqCst) + 1
}

/// The worker family beside the app family, so the lifecycle test can load
/// this image as a worker too. It registers no build function: the test
/// reads only what `flui_worker_init` binds.
fn fixture_worker_init(_register: flui_hot_reload::RegisterWorkerBuildFn) {}

flui_hot_reload::hot_reload_worker!(fixture_worker_init);

/// Calls this image's own copy of `flui_hot_reload::request_rebuild`, as a
/// worker's gesture handler does.
#[expect(
    unsafe_code,
    reason = "`#[unsafe(no_mangle)]` exports the symbol the lifecycle test resolves with `dlsym`"
)]
#[unsafe(no_mangle)]
pub extern "C-unwind" fn fixture_request_rebuild() {
    flui_hot_reload::request_rebuild();
}
