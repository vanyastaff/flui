//! The single `WindowId -> PresentationAddress` mapping authority.
//!
//! ADR-0037 §2 names one authority for the native-window-to-presentation
//! map; no second one may live in `AppRuntime`, `UiRealm`, an input
//! registry, or a platform callback. This module is that authority's home.
//! `WindowId` (the platform-internal native-handle key) is confined to this
//! file within `flui-app` — every other module addresses a presentation
//! through [`PresentationAddress`] only, minted here. The mechanical
//! `forbidden_pattern` scan in `docs/runtime-contract.toml` confines the
//! `WindowId` token itself to this one file within `crates/flui-app/src`.
//!
//! # Derived-cache invariant
//!
//! `AppRuntime.address: Option<PresentationAddress>` (`app/runtime.rs`) is a
//! **derived cache** of this registry, not a second source of truth. Both
//! are written only by `install_platform_realm`/`teardown_platform_realm`,
//! inside the same TLS borrow, in that order: on install, the registry is
//! written first (which also removes every mapping of a realm displaced by
//! a panic-recovery reinstall — never just the new window), then the
//! cache; on teardown, the registry entries are removed first — so map
//! removal stops new routing before the queued old-generation events still
//! sitting in the host's queue are dropped (ADR-0037 §2).

use std::sync::Arc;

use flui_foundation::{PresentationAddress, RealmId};
use flui_platform::traits::{PlatformWindow, WindowId};

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
        existing: PresentationAddress,
    },
}

/// The sole `WindowId -> PresentationAddress` mint/lookup authority.
///
/// API designed for N windows per realm, instantiated for exactly one
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

    /// Registers `window` at `address`, **replacing** any existing mapping
    /// for the same window and returning the displaced address.
    ///
    /// Replacement (not a hard error) keeps install recoverable after a
    /// mid-`on_ready` panic: `OwnerHostClearGuard` only clears
    /// `AppRuntime.owner_platform`, not the realm-facing fields this
    /// registry lives alongside, and the web host never tears down at all — a hard error here
    /// would brick reinstall on either path. A replacement is traced at
    /// `warn` with both addresses so a genuine double-install bug is still
    /// visible; [`Self::try_register`] is the strict alternative for a
    /// caller that wants a hard error instead.
    ///
    /// This only replaces the mapping for the exact same `WindowId` — it
    /// does **not** remove any *other* window mapped to a realm this
    /// address's realm is displacing. A caller reinstalling an entire realm
    /// under a fresh window must call [`Self::remove_realm`] for the
    /// displaced realm first (see `install_platform_realm`'s use of both).
    ///
    /// Calls `window.id()` internally so callers never need to name
    /// [`WindowId`] themselves. Performs the install-time self-check read
    /// immediately after inserting: the very next [`Self::resolve`] must
    /// see exactly what was just written.
    pub(crate) fn register_window(
        &mut self,
        window: &Arc<dyn PlatformWindow>,
        address: PresentationAddress,
    ) -> Option<PresentationAddress> {
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
        address: PresentationAddress,
    ) -> Result<(), RegistryError> {
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

    /// Removes and returns **every** entry addressed to `realm_id`.
    ///
    /// The target model is one realm owning any number of windows, so a
    /// realm's teardown (or its displacement by a panic-recovery reinstall)
    /// must not leave a second, third, ... window's mapping behind just
    /// because only the first one happened to be removed. This is the
    /// teardown real read: the caller asserts the returned entries against
    /// the address(es) it installed, proving the registry tracked the same
    /// window/address pairs for this realm's whole lifetime.
    // `teardown_platform_realm` and `install_platform_realm` (runner.rs) are
    // the only production callers, and `teardown_platform_realm` does not
    // exist on wasm32 — the web host never tears down (see its own module
    // doc) — so the wasm lib check would see this as dead if
    // `install_platform_realm`'s reinstall-cleanup call did not also reach
    // it; kept unconditional since that second call site is not wasm-gated.
    pub(crate) fn remove_realm(
        &mut self,
        realm_id: RealmId,
    ) -> Vec<(WindowId, PresentationAddress)> {
        let mut removed = Vec::new();
        self.entries.retain(|(id, address)| {
            if address.realm_id == realm_id {
                removed.push((*id, *address));
                false
            } else {
                true
            }
        });
        removed
    }

    /// The number of window mappings currently held. Test/introspection
    /// only — production code never needs to enumerate the registry, only
    /// resolve or remove by identity.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// Companion to [`Self::len`] (clippy's `len_without_is_empty`); also
    /// exercised directly by this module's own tests.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU32;

    use super::*;

    static_assertions::assert_impl_all!(PresentationAddress: Send, Sync, Copy);

    #[test]
    fn addresses_compare_by_both_fields() {
        let one = PresentationAddress {
            realm_id: RealmId::new_gen(0, NonZeroU32::MIN),
            presentation_id: flui_foundation::PresentationId::new_gen(0, NonZeroU32::MIN),
        };
        let same_realm_different_presentation = PresentationAddress {
            realm_id: one.realm_id,
            presentation_id: flui_foundation::PresentationId::new_gen(
                0,
                NonZeroU32::new(2).unwrap(),
            ),
        };
        assert_ne!(one, same_realm_different_presentation);
    }

    fn address(slot: u32) -> PresentationAddress {
        PresentationAddress {
            realm_id: RealmId::new_gen(slot, NonZeroU32::MIN),
            presentation_id: flui_foundation::PresentationId::new_gen(slot, NonZeroU32::MIN),
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
            vec![(window.id(), installed)],
            "teardown must return exactly the entries that were installed"
        );
        assert_eq!(
            registry.resolve(window.id()),
            None,
            "the entry must be gone after removal"
        );
        assert!(registry.is_empty());
    }

    /// The target model is one realm owning any number of windows:
    /// `remove_realm` must remove every mapping addressed to that realm,
    /// not just the first one found.
    ///
    /// If reverted: use `position` + single `remove` instead of `retain` and
    /// this fails — only the first-registered window's mapping is removed,
    /// the second survives as a dead entry.
    #[test]
    fn remove_realm_removes_every_mapping_for_that_realm_not_just_the_first() {
        let mut registry = WindowRegistry::new();
        let realm_address = address(0);
        let same_realm_second_window = PresentationAddress {
            realm_id: realm_address.realm_id,
            presentation_id: flui_foundation::PresentationId::new_gen(1, NonZeroU32::MIN),
        };
        let other_realm_address = address(1);

        let window_a = stub_window(100);
        let window_b = stub_window(101);
        let window_c = stub_window(102);
        registry.register_window(&window_a, realm_address);
        registry.register_window(&window_b, same_realm_second_window);
        registry.register_window(&window_c, other_realm_address);

        let removed = registry.remove_realm(realm_address.realm_id);
        assert_eq!(removed.len(), 2, "both same-realm windows must be removed");
        assert!(removed.contains(&(window_a.id(), realm_address)));
        assert!(removed.contains(&(window_b.id(), same_realm_second_window)));

        assert_eq!(registry.resolve(window_a.id()), None);
        assert_eq!(registry.resolve(window_b.id()), None);
        assert_eq!(
            registry.resolve(window_c.id()),
            Some(other_realm_address),
            "an unrelated realm's window mapping must survive"
        );
        assert_eq!(registry.len(), 1);
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
