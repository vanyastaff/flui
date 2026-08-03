//! The single `WindowId -> UiAddress` mapping authority.
//!
//! ADR-0037 §2 names one authority for the native-window-to-presentation
//! map; no second one may live in `AppBinding`, `UiRealm`, an input
//! registry, or a platform callback. This module is that authority's home.
//! `WindowId` (the platform-internal native-handle key) is confined to this
//! file within `flui-app` — every other module addresses a presentation
//! through [`UiAddress`] only, minted here. The mechanical
//! `forbidden_pattern` scan in `docs/runtime-contract.toml` confines the
//! `WindowId` token itself to this one file within `crates/flui-app/src`.
//!
//! # Derived-cache invariant
//!
//! `RealmHost.address: Option<UiAddress>` (`runner.rs`) is a **derived
//! cache** of this registry, not a second source of truth. Both are written
//! only by `install_platform_realm`/`teardown_platform_realm`, inside the
//! same TLS borrow, in that order: on install, the registry is written
//! first, then the cache; on teardown, the registry entry is removed first
//! — so map removal stops new routing before the queued old-generation
//! events still sitting in the host's queue are dropped (ADR-0037 §2).

use std::sync::Arc;

use flui_foundation::{PresentationId, RealmId};
use flui_platform::traits::{PlatformWindow, WindowId};

/// One presentation's routable address: which realm incarnation owns it,
/// and which presentation incarnation within that realm.
///
/// Field names match the `RealmDispatcher.realm_id` precedent
/// (`runner.rs`) and ADR-0037 §7's literal `SemanticsCommand` fields —
/// `realm_id`/`presentation_id`, not a bare tuple or abbreviated names.
///
/// Deliberately `flui-app`-private: it graduates to `flui-foundation` only
/// when a second crate needs it (rule of three).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct UiAddress {
    pub(crate) realm_id: RealmId,
    pub(crate) presentation_id: PresentationId,
}

/// Errors from [`WindowRegistry::try_register`].
// The strict path has no production call site yet (today's single-window
// install always uses the replace semantics of `register_window`); it is
// reserved for a future multi-window install path that chooses
// strict-vs-replace per call site, and is exercised by this module's own
// tests in the meantime.
#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "reserved for a future multi-window install path; exercised by this module's tests"
    )
)]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RegistryError {
    /// The window already has a mapped address; `try_register` never
    /// replaces (use [`WindowRegistry::register_window`] for replace
    /// semantics).
    #[error("window is already mapped to {existing:?}")]
    WindowAlreadyMapped {
        /// The address the window was already mapped to.
        existing: UiAddress,
    },
}

/// The sole `WindowId -> UiAddress` mint/lookup authority.
///
/// API designed for N windows, instantiated for exactly one today: storage
/// is a plain linear-scan `Vec` with no TLS assumption inside the type
/// itself — a future multi-window `AppRuntime` lifts this struct unchanged.
/// `WindowId` never crosses this module's boundary except through the
/// methods below, which take an already-known window/id and hand back an
/// address; callers outside this file never construct or hold a `WindowId`.
#[derive(Debug, Default)]
pub(crate) struct WindowRegistry {
    entries: Vec<(WindowId, UiAddress)>,
}

impl WindowRegistry {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Registers `window` at `address`, **replacing** any existing mapping
    /// for the same window and returning the displaced address.
    ///
    /// Replacement (not a hard error) keeps install recoverable after a
    /// mid-`on_ready` panic: `OwnerHostClearGuard` only clears
    /// `OWNER_PLATFORM_HOST`, not the realm host this registry lives
    /// inside, and the web host never tears down at all — a hard error here
    /// would brick reinstall on either path. A replacement is traced at
    /// `warn` with both addresses so a genuine double-install bug is still
    /// visible; [`Self::try_register`] is the strict alternative for a
    /// caller that wants a hard error instead.
    ///
    /// Calls `window.id()` internally so callers never need to name
    /// [`WindowId`] themselves. Performs the install-time self-check read
    /// immediately after inserting: the very next [`Self::resolve`] must
    /// see exactly what was just written.
    pub(crate) fn register_window(
        &mut self,
        window: &Arc<dyn PlatformWindow>,
        address: UiAddress,
    ) -> Option<UiAddress> {
        let id = window.id();
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
        if let Some(displaced) = displaced {
            tracing::warn!(
                ?id,
                new_address = ?address,
                displaced_address = ?displaced,
                "window_registry: replacing an existing window mapping"
            );
        }
        debug_assert_eq!(
            self.resolve(id),
            Some(address),
            "BUG: window_registry install-time self-check failed immediately after insert"
        );
        displaced
    }

    /// The strict, design-for-N alternative to [`Self::register_window`]:
    /// refuses instead of replacing when `id` is already mapped.
    ///
    /// Not dead code despite zero production call sites today — exercised
    /// by this module's own tests, and reserved for a future multi-window
    /// install path that chooses strict-vs-replace per call site.
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "reserved for a future multi-window install path; exercised by this module's tests"
        )
    )]
    pub(crate) fn try_register(
        &mut self,
        id: WindowId,
        address: UiAddress,
    ) -> Result<(), RegistryError> {
        if let Some(existing) = self.resolve(id) {
            return Err(RegistryError::WindowAlreadyMapped { existing });
        }
        self.entries.push((id, address));
        Ok(())
    }

    /// Looks up the address currently mapped to `id`, if any.
    pub(crate) fn resolve(&self, id: WindowId) -> Option<UiAddress> {
        self.entries
            .iter()
            .find(|(existing, _)| *existing == id)
            .map(|(_, address)| *address)
    }

    /// Removes and returns the sole entry for `realm_id`, if one exists.
    ///
    /// This is the teardown real read: the caller asserts the returned
    /// entry against the address it installed, proving the registry tracked
    /// the same window/address pair for this realm's whole lifetime.
    // `teardown_platform_realm` (runner.rs) is the only production caller,
    // and it does not exist on wasm32 — the web host never tears down (see
    // its own module doc) — so the wasm lib check sees this as dead.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "consumed only by teardown_platform_realm, which does not exist on wasm32, and by this module's tests"
        )
    )]
    pub(crate) fn remove_realm(&mut self, realm_id: RealmId) -> Option<(WindowId, UiAddress)> {
        let position = self
            .entries
            .iter()
            .position(|(_, address)| address.realm_id == realm_id)?;
        Some(self.entries.remove(position))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    static_assertions::assert_impl_all!(UiAddress: Send, Sync, Copy);

    #[test]
    fn addresses_compare_by_both_fields() {
        let one = UiAddress {
            realm_id: RealmId::new_gen(0, NonZeroU32::MIN),
            presentation_id: PresentationId::new_gen(0, NonZeroU32::MIN),
        };
        let same_realm_different_presentation = UiAddress {
            realm_id: one.realm_id,
            presentation_id: PresentationId::new_gen(0, NonZeroU32::new(2).unwrap()),
        };
        assert_ne!(one, same_realm_different_presentation);
    }

    fn address(slot: u32) -> UiAddress {
        UiAddress {
            realm_id: RealmId::new_gen(slot, NonZeroU32::MIN),
            presentation_id: PresentationId::new_gen(slot, NonZeroU32::MIN),
        }
    }

    struct StubWindow(WindowId);

    impl PlatformWindow for StubWindow {
        fn id(&self) -> WindowId {
            self.0
        }

        fn physical_size(&self) -> flui_types::geometry::Size<flui_types::geometry::DevicePixels> {
            flui_types::geometry::Size::default()
        }

        fn logical_size(&self) -> flui_types::geometry::Size<flui_types::Pixels> {
            flui_types::geometry::Size::default()
        }

        fn scale_factor(&self) -> f64 {
            1.0
        }

        fn request_redraw(&self) {}

        fn is_focused(&self) -> bool {
            false
        }

        fn is_visible(&self) -> bool {
            true
        }

        fn set_cursor(
            &self,
            _cursor: flui_platform::CursorIcon,
        ) -> Result<(), flui_platform::CursorError> {
            Ok(())
        }
    }

    fn stub_window(id: u64) -> Arc<dyn PlatformWindow> {
        Arc::new(StubWindow(WindowId(id)))
    }

    #[test]
    fn register_window_replaces_same_window_with_trace() {
        let mut registry = WindowRegistry::new();
        let window = stub_window(1);
        let first = address(0);
        let second = address(1);

        assert_eq!(registry.register_window(&window, first), None);
        let displaced = registry.register_window(&window, second);
        assert_eq!(
            displaced,
            Some(first),
            "re-registering the same window must return the displaced address"
        );
        assert_eq!(registry.resolve(window.id()), Some(second));
    }

    #[test]
    fn try_register_rejects_duplicate_window() {
        let mut registry = WindowRegistry::new();
        let id = WindowId(7);
        let first = address(0);

        registry
            .try_register(id, first)
            .expect("first registration succeeds");
        let error = registry
            .try_register(id, address(1))
            .expect_err("duplicate window must be refused, not replaced");
        assert_eq!(
            error,
            RegistryError::WindowAlreadyMapped { existing: first }
        );
        assert_eq!(
            registry.resolve(id),
            Some(first),
            "a refused try_register must not change the existing mapping"
        );
    }

    #[test]
    fn remove_realm_returns_the_installed_entry() {
        let mut registry = WindowRegistry::new();
        let window = stub_window(3);
        let installed = address(0);
        registry.register_window(&window, installed);

        let removed = registry.remove_realm(installed.realm_id);
        assert_eq!(
            removed,
            Some((window.id(), installed)),
            "teardown must return exactly the entry that was installed"
        );
        assert_eq!(
            registry.resolve(window.id()),
            None,
            "the entry must be gone after removal"
        );
    }

    #[test]
    fn two_windows_get_two_distinct_addresses() {
        let mut registry = WindowRegistry::new();
        let window_a = stub_window(10);
        let window_b = stub_window(20);
        let address_a = address(0);
        let address_b = address(1);

        registry.register_window(&window_a, address_a);
        registry.register_window(&window_b, address_b);

        assert_eq!(registry.resolve(window_a.id()), Some(address_a));
        assert_eq!(registry.resolve(window_b.id()), Some(address_b));
        assert_ne!(address_a, address_b);
    }
}
