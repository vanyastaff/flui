//! Cross-module serialization for tests that touch flui-view's
//! process-global error-view builder (`set_error_view_builder` /
//! `clear_error_view_builder`, a `RwLock<Option<ErrorViewBuilder>>`).
//!
//! Since the single-binary consolidation, every former test file shares one
//! process, so libtest's default parallelism can interleave a test that
//! installs a counting builder (error_view_recovery) with a panic-recovery
//! test that trips the error-view path without installing one
//! (inherited_dependency). Every test that can REACH the global builder —
//! writer or not — takes this guard.

use std::sync::Mutex;

static GLOBAL_BUILDER_GUARD: Mutex<()> = Mutex::new(());

/// Acquires the process-global builder guard.
///
/// Poison-tolerant: a failing neighbour's panic must not cascade — we
/// extract the inner guard either way.
pub fn acquire_builder_guard() -> std::sync::MutexGuard<'static, ()> {
    match GLOBAL_BUILDER_GUARD.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
