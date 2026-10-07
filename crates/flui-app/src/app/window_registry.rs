//! The single `WindowId -> PresentationAddress` mapping authority.
//!
//! ADR-0037 §2 names one authority for the native-window-to-presentation
//! map; no second one may live in `AppRuntime`, `UiRuntime`, an input
//! registry, or a platform callback. This module is that authority's home.
//! `WindowId` (the platform-internal native-handle key) is confined to this
//! file within `flui-app` — every other module addresses a presentation
//! through [`PresentationAddress`] only. [`PreparedWindowRegistration`] reads
//! the native identity before the caller borrows either registry. Publication
//! consumes that opaque value and invokes no window method or diagnostic hook.
//! No caller outside this file names or passes a bare `WindowId`: no routing API elsewhere in
//! `flui-app` accepts or returns one. The exceptions are structural — the
//! test-only `PlatformWindow` mock restating `fn id(&self) -> WindowId`, and
//! `AppRuntime::release_redraw_window_for`, which compares a window's own id.
//!
//! # Derived-cache invariant
//!
//! Each hosted UI runtime's own `RuntimeSlot.address: PresentationAddress`
//! (`app/runtime.rs`'s `RuntimeRegistry`) is a **derived cache** of this
//! registry, not a second source of truth. Both are written together, in
//! the same TLS borrow: `install_platform_ui_runtime`/`teardown_platform_ui_runtime`
//! for the legacy single-primary-UI runtime path, and `AppRuntime::apply_install`/
//! `apply_uninstall` for the multi-UI runtime registry/uninstall path (issue
//! #555) — in each case the registry write and the `RuntimeSlot` write happen
//! inside the same borrow, in the order ADR-0037 §2 requires: on install,
//! the registry is written first (which also removes every mapping of a
//! UI runtime displaced by a panic-recovery reinstall — never just the new
//! window), then the UI runtime entry; on uninstall/teardown, the registry
//! entries are removed first — so map removal stops new routing before the
//! queued old-generation events still sitting in the host's queue are
//! dropped.

use std::sync::Arc;

use flui_foundation::{PresentationAddress, UiRuntimeId};
use flui_platform::traits::{PlatformWindow, WindowId};

/// A native identity sampled before entering a registry publication interval.
/// The window itself is not retained by a deferred logical/native install.
#[derive(Debug)]
pub(crate) struct PreparedWindowRegistration {
    id: WindowId,
}

impl PreparedWindowRegistration {
    pub(crate) fn new(window: &Arc<dyn PlatformWindow>) -> Self {
        Self { id: window.id() }
    }
}

/// Errors from [`WindowRegistry::try_register`].
///
/// Reached from `AppRuntime::apply_install`
/// (`crates/flui-app/src/app/runtime.rs`): the strict, refuse-on-collision
/// path `install_ui_runtime_alongside`'s non-displacing install uses, so a
/// second UI runtime's window id colliding with an already-registered one is
/// refused rather than silently re-routed onto the sibling's mapping.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RegistryError {
    /// The window already has a mapped address; `try_register` never
    /// replaces (use `WindowRegistry::register` — the Android/web
    /// replace-semantics install, not linked because it is target-gated).
    #[error("window is already mapped to {existing:?}")]
    WindowAlreadyMapped {
        /// The address the window was already mapped to.
        existing: PresentationAddress,
    },
}

/// The sole `WindowId -> PresentationAddress` mint/lookup authority.
///
/// API designed for N windows per UI runtime, instantiated for exactly one
/// window today: storage is a plain linear-scan `Vec` with no TLS
/// assumption inside the type itself — a future multi-window `AppRuntime`
/// lifts this struct unchanged. `WindowId` never crosses this module's
/// boundary except through the methods below, which take an already-known
/// window/id and hand back an address; callers outside this file never
/// construct or hold a `WindowId`.
#[derive(Debug, Default)]
pub(crate) struct WindowRegistry {
    entries: Vec<(WindowId, PresentationAddress)>,
}

impl WindowRegistry {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Publishes a prepared native identity at `address`, replacing any mapping
    /// for the same window and returning the displaced address.
    ///
    /// Replacement (not a hard error) keeps install recoverable after a
    /// mid-`on_ready` panic: `OwnerHostClearGuard` only clears
    /// `AppRuntime.owner_platform`, not the UI runtime-facing fields this
    /// registry lives alongside. [`Self::try_register`] is the strict alternative
    /// for a caller that must refuse a collision.
    ///
    /// This only replaces the mapping for the exact same `WindowId` — it
    /// does **not** remove any *other* window mapped to a UI runtime this
    /// address's UI runtime is displacing. A caller reinstalling an entire UI runtime
    /// under a fresh window must call [`Self::remove_ui_runtime`] for the
    /// displaced UI runtime first (see `install_platform_ui_runtime`'s use of both).
    ///
    /// Returns the displaced address for diagnostics after registry borrows end.
    #[cfg(any(test, target_os = "android", target_arch = "wasm32"))]
    pub(crate) fn register(
        &mut self,
        registration: PreparedWindowRegistration,
        address: PresentationAddress,
    ) -> Option<PresentationAddress> {
        let id = registration.id;
        let displaced = if let Some(entry) = self
            .entries
            .iter_mut()
            .find(|(existing, _)| *existing == id)
        {
            Some(std::mem::replace(&mut entry.1, address))
        } else {
            self.entries.push((id, address));
            None
        };
        debug_assert_eq!(
            self.resolve(id),
            Some(address),
            "BUG: window_registry install-time self-check failed immediately after insert"
        );
        displaced
    }

    /// The strict alternative to `Self::register` (target-gated, so
    /// not linked): refuses instead
    /// of replacing when `window`'s id is already mapped. See
    /// [`RegistryError`]'s doc for its one production caller.
    ///
    /// Preparation already sampled the native identity outside registry borrows.
    pub(crate) fn try_register(
        &mut self,
        registration: PreparedWindowRegistration,
        address: PresentationAddress,
    ) -> Result<(), RegistryError> {
        let id = registration.id;
        if let Some(existing) = self.resolve(id) {
            return Err(RegistryError::WindowAlreadyMapped { existing });
        }
        self.entries.push((id, address));
        Ok(())
    }

    /// Looks up the address currently mapped to `id`, if any.
    pub(crate) fn resolve(&self, id: WindowId) -> Option<PresentationAddress> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, address)| *address)
    }

    /// Whether `address` names a window mapping currently held by this
    /// registry — the addressed-dispatch validity check
    /// `dispatch_platform_ui_runtime` (`runner.rs`) uses in place of comparing
    /// against a single cached "current" address (issue #555's
    /// per-presentation generational `StalePresentation` extension): a
    /// UI runtime hosting more than one presentation has more than one live
    /// address at once, so membership in this one authority — not equality
    /// against any single value — is the only check general enough for N
    /// presentations.
    pub(crate) fn contains_address(&self, address: PresentationAddress) -> bool {
        self.entries
            .iter()
            .any(|(_, entry_address)| *entry_address == address)
    }

    /// Removes and returns **every** entry addressed to `ui_runtime_id`.
    ///
    /// The target model is one UI runtime owning any number of windows, so a
    /// UI runtime's teardown (or its displacement by a panic-recovery reinstall)
    /// must not leave a second, third, ... window's mapping behind just
    /// because only the first one happened to be removed. This is the
    /// teardown real read: the caller asserts the returned entries against
    /// the address(es) it installed, proving the registry tracked the same
    /// window/address pairs for this UI runtime's whole lifetime.
    // `teardown_platform_ui_runtime` and `install_platform_ui_runtime` (runner.rs) are
    // the only production callers, and `teardown_platform_ui_runtime` does not
    // exist on wasm32 — the web host never tears down (see its own module
    // doc) — so the wasm lib check would see this as dead if
    // `install_platform_ui_runtime`'s reinstall-cleanup call did not also reach
    // it; kept unconditional since that second call site is not wasm-gated.
    pub(crate) fn remove_ui_runtime(
        &mut self,
        ui_runtime_id: UiRuntimeId,
    ) -> Vec<(WindowId, PresentationAddress)> {
        let mut removed = Vec::new();
        self.entries.retain(|(id, address)| {
            if address.ui_runtime_id == ui_runtime_id {
                removed.push((*id, *address));
                false
            } else {
                true
            }
        });
        removed
    }

    /// Removes and returns every entry mapped to this EXACT
    /// `(UiRuntimeId, PresentationId)` address — never a sibling presentation
    /// within the same UI runtime, and never every window the UI runtime owns (see
    /// [`Self::remove_ui_runtime`] for that whole-UI runtime removal). This is step 1
    /// of closing a single presentation out of a UI runtime that keeps hosting
    /// others: the closed presentation's own window mapping must stop
    /// resolving to it before the presentation itself goes away, or a stale
    /// platform event delivered to that exact window would still resolve to
    /// a dead presentation id instead of being refused.
    pub(crate) fn remove_presentation(
        &mut self,
        address: PresentationAddress,
    ) -> Vec<(WindowId, PresentationAddress)> {
        let mut removed = Vec::new();
        self.entries.retain(|(id, entry_address)| {
            if *entry_address == address {
                removed.push((*id, *entry_address));
                false
            } else {
                true
            }
        });
        removed
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    static_assertions::assert_impl_all!(PresentationAddress: Send, Sync, Copy);
}
