//! Build phase management.
//!
//! This module provides:
//! - [`BuildOwner`] - Manages dirty elements and build scheduling
//! - [`ElementOwner`] - Split-borrow handle threaded through Element
//!   lifecycle (mount/unmount/update) so per-frame registries
//!   (GlobalKey, dirty heap, inactive queue) are updated without a
//!   blanket `&mut BuildOwner` borrow across the recursive traversal.

mod build_owner;
mod element_owner;
mod inherited_dependencies;
mod layout_builder;
mod rebuild_handle;

pub use build_owner::BuildOwner;
pub use rebuild_handle::RebuildHandle;
// Moved to flui-foundation (ADR-0040: observation events carry typed
// causes, and foundation is the only crate below every emitter); re-exported
// here so no call site changes.
pub use flui_foundation::{RebuildReason, RebuildReasons};
// Internal scheduling handle — `pub(crate)`: captured by `ElementCore` at mount,
// no public consumer. See `ExternalBuildScheduler`.
pub(crate) use build_owner::ExternalBuildScheduler;
pub use element_owner::ElementOwner;
// Build-time live-tree handle carried on `ElementOwner` during a
// `build_scope` drain (PR-K). Crate-internal.
pub(crate) use element_owner::BuildHandle;

/// Emit one tree observation through the realm's observer slot (ADR-0040).
///
/// Owns the two contract halves every emission site relies on:
/// - **Laziness**: `f` (which constructs the payload) runs only when an
///   observer is installed — the off path is one `Option` load + branch.
/// - **Panic policy**: the observer call runs under `catch_unwind`; on
///   unwind the observer is REMOVED from the slot (`detached()` is not
///   called — the observer just proved it cannot be trusted to run) and the
///   detachment is logged. The frame survives.
pub(crate) fn emit_observation(
    slot: &mut Option<std::sync::Arc<dyn flui_foundation::observe::TreeObserver>>,
    f: impl FnOnce(&dyn flui_foundation::observe::TreeObserver),
) {
    if let Some(observer) = slot.as_deref()
        && std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(observer))).is_err()
    {
        *slot = None;
        tracing::error!(
            "TreeObserver panicked during emission; observer detached (no detached() call)"
        );
    }
}
