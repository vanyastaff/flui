//! The single `WindowId -> UiAddress` mapping authority.
//!
//! ADR-0037 §2 names one authority for the native-window-to-presentation
//! map; no second one may live in `AppBinding`, `UiRealm`, an input
//! registry, or a platform callback. This module is that authority's home.
//! `WindowId` (the platform-internal native-handle key) is confined to this
//! file within `flui-app` — every other module addresses a presentation
//! through [`UiAddress`] only, minted here.

use flui_foundation::{PresentationId, RealmId};

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
}
