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
//! # Joint publication
//!
//! The installed host owns this native map and the logical OwnerHost together.
//! It prepares native identity outside borrows, reserves logical publication,
//! inserts the native mapping, then commits logical membership without invoking
//! user code between those writes. Closure withdraws native routes before user
//! cleanup; active leases retain their original host across TLS replacement.

use std::sync::Arc;

use flui_foundation::PresentationAddress;
use flui_platform::traits::{PlatformWindow, WindowId};

/// A native identity sampled before entering a registry publication interval.
/// The registration stores only identity; the installation separately retains
/// its native window through initialization.
#[derive(Debug)]
pub(crate) struct PreparedWindowRegistration {
    id: WindowId,
}

impl PreparedWindowRegistration {
    pub(crate) fn new(window: &Arc<dyn PlatformWindow>) -> Self {
        Self { id: window.id() }
    }
}

/// Native identity conflicts detected before joint window publication.
/// A second runtime cannot redirect an already registered window's route.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RegistryError {
    /// The window already has a mapped address. Publication never replaces it.
    #[error("window is already mapped to {existing:?}")]
    WindowAlreadyMapped {
        /// The address the window was already mapped to.
        existing: PresentationAddress,
    },
}

/// The sole `WindowId -> PresentationAddress` mint/lookup authority.
///
/// Each installed host owns one registry for its runtimes and presentations.
/// Native identity is read before publication; the registry itself calls no
/// platform code and has no TLS dependency.
#[derive(Debug, Default)]
pub(crate) struct WindowRegistry {
    entries: Vec<(WindowId, PresentationAddress)>,
}

impl WindowRegistry {
    pub(crate) fn reserve_registration(
        &mut self,
        registration: &PreparedWindowRegistration,
    ) -> Result<(), RegistryError> {
        if let Some(existing) = self.resolve(registration.id) {
            return Err(RegistryError::WindowAlreadyMapped { existing });
        }
        self.entries.reserve(1);
        Ok(())
    }

    /// The same registry borrow reserved capacity and checked identity first.
    pub(crate) fn publish_registration(
        &mut self,
        registration: &PreparedWindowRegistration,
        address: PresentationAddress,
    ) {
        self.entries.push((registration.id, address));
    }
    pub(crate) fn clear(&mut self) -> usize {
        let count = self.entries.len();
        self.entries.clear();
        count
    }

    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
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

    /// Removes and returns every entry mapped to this EXACT
    /// `(UiRuntimeId, PresentationId)` address — never a sibling presentation
    /// within the same UI runtime. This is step 1
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
