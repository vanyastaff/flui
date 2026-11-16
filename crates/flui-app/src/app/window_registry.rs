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

use std::collections::VecDeque;
use std::rc::Rc;

use flui_foundation::PresentationAddress;
use flui_platform::{OpenTarget, PlatformWindow, WindowId};

/// A native identity sampled before entering a registry publication interval.
/// The registration stores only identity; the installation separately retains
/// its native window through initialization.
#[derive(Debug)]
pub(crate) struct PreparedWindowRegistration {
    id: WindowId,
}

impl PreparedWindowRegistration {
    pub(crate) fn new(window: &Rc<dyn PlatformWindow>) -> Self {
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
    /// The windows whose presentation was removed, the most recent last and
    /// at most [`RETIRED_WINDOWS_KEPT`]: a URL the platform addressed to one
    /// while it closed is refused, not held for a window that never returns.
    /// A host never issues an id twice (`WindowIdMint`), so an id here names
    /// no later window.
    retired: VecDeque<WindowId>,
}

/// How many closed windows the registry remembers. A report addressed to a
/// window is made while the window is open; it reaches the runner after the
/// close only from the platform's queue of opened reports waiting for the
/// handler (`AppHandler::opened`), which holds fewer than this. An older id
/// forgotten here names no installation either, so a URL for it is still
/// refused, as a window that is not open.
const RETIRED_WINDOWS_KEPT: usize = flui_platform::MAX_OPENED_ITEMS;

/// Where what the operating system opened for an [`OpenTarget`] goes now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OpenDestination {
    /// The presentation that takes it.
    Presentation(PresentationAddress),
    /// No presentation yet: the window the target names is being installed,
    /// or the application has no window.
    Pending,
    /// The window the target names has closed: nothing opens in it again.
    Closed,
    /// The window the target names has no presentation and no installation
    /// under way: nobody announced it, or its installation failed. Decided
    /// by the installed host, which knows the installations
    /// (`InstalledHost::open_target`).
    NotOpen,
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
    /// Remove every mapping, each window remembered as closed.
    pub(crate) fn clear(&mut self) -> usize {
        let count = self.entries.len();
        for (id, _) in std::mem::take(&mut self.entries) {
            self.retire(id);
        }
        count
    }

    /// Remember `id` as closed, forgetting the oldest past
    /// [`RETIRED_WINDOWS_KEPT`].
    fn retire(&mut self, id: WindowId) {
        if self.retired.len() == RETIRED_WINDOWS_KEPT {
            self.retired.pop_front();
        }
        self.retired.push_back(id);
    }

    /// Whether `target` names a window and `window` is it, read before the
    /// window is registered: the native identity stays in this file.
    pub(crate) fn names_window(target: OpenTarget, window: &dyn PlatformWindow) -> bool {
        matches!(target, OpenTarget::Window(_)) && Self::opens_in(target, window)
    }

    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            retired: VecDeque::new(),
        }
    }

    /// Looks up the address currently mapped to `id`, if any.
    pub(crate) fn resolve(&self, id: WindowId) -> Option<PresentationAddress> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, address)| *address)
    }

    /// Where what the operating system opened goes (`flui_platform::Opened`):
    /// the window's own presentation for [`OpenTarget::Window`], once it has
    /// one, and [`OpenDestination::Closed`] once it closed; for the
    /// application as a whole, `active` (the window focused last) while it
    /// is still mapped, else the window opened first, and
    /// [`OpenDestination::Pending`] while it has none.
    pub(crate) fn open_target(
        &self,
        target: OpenTarget,
        active: Option<PresentationAddress>,
    ) -> OpenDestination {
        let address = match target {
            OpenTarget::Window(id) if self.retired.contains(&id) => {
                return OpenDestination::Closed;
            }
            OpenTarget::Window(id) => self.resolve(id),
            OpenTarget::Application | _ => active
                .filter(|address| self.contains_address(*address))
                .or_else(|| self.entries.first().map(|(_, address)| *address)),
        };
        address.map_or(OpenDestination::Pending, OpenDestination::Presentation)
    }

    /// Whether what the operating system opened for `target` goes to
    /// `window`'s presentation before the window is registered: the
    /// window's own, or any for the application as a whole.
    pub(crate) fn opens_in(target: OpenTarget, window: &dyn PlatformWindow) -> bool {
        match target {
            OpenTarget::Window(id) => window.id() == id,
            OpenTarget::Application | _ => true,
        }
    }

    /// Whether `address` names a window mapping currently held by this
    /// registry — the addressed-dispatch validity check
    /// `dispatch_platform_ui_runtime` (`runner/owner_dispatch.rs`) uses in place of comparing
    /// against a single cached "current" address (the
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
        for (id, _) in &removed {
            self.retire(*id);
        }
        removed
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    static_assertions::assert_impl_all!(PresentationAddress: Send, Sync, Copy);
}
