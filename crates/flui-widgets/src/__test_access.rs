//! Temporary access to private items for this crate's integration tests
//! (ADR-0083 §4). **No semver promise; only `crates/flui-widgets/tests` may
//! import it.**
//!
//! The tests that drive the headless harness live in `tests/`, where the
//! library links once; the harness module is not compiled into the unit-test
//! build (`#[cfg(all(feature = "testing", not(test)))]` in `lib.rs`). Some of
//! them still assert on private state — the navigator's route stack
//! (ADR-0019), the overlay's entry list (ADR-0076), the hero registry, the
//! transition and modal routes. This module is the one place those items are
//! nameable from outside the crate:
//!
//! - **probe traits** add read-only (or test-driving) methods to public types
//!   without widening their inherent API;
//! - **re-exports** name private types raised to `pub` at their definition,
//!   inside private modules, so nothing else reaches them.
//!
//! An entry leaves when its tests assert through public API instead — for the
//! navigator internals that is the Router conformance suite (ADR-0093). The
//! module is deleted when it is empty. `tests::lists_exactly_the_temporary_entries`
//! pins the list, so adding an entry is a reviewed edit.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::Curve;
use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::LocalPostFrameHandle;

pub use crate::navigator::back_gesture::{BackGestureController, BackGestureRuntime};
pub use crate::navigator::binding::{TransitionGroup, TransitionPeer};
pub use crate::navigator::hero::{HeroHandle, HeroRegistry, HeroScope, HeroTag};
pub use crate::navigator::hero_controller::{HeroFlightManifest, Measurement};
pub use crate::navigator::hero_flight::{FlightManager, HeroFlight};
pub use crate::navigator::lifecycle::RouteLifecycle;
pub use crate::navigator::local_history::{
    LocalHistoryEntry, LocalHistoryEntryHandle, LocalHistoryHandle,
};
pub use crate::navigator::modal_route::{ModalHandle, ModalRoute};
pub use crate::navigator::subtree::RouteSubtree;
pub use crate::navigator::transition_route::{TransitionHandle, TransitionRoute};

use crate::navigator::{
    HeroController, NavigatorHandle, PageRoute, PopupRoute, RouteBindingSlot, RouteId, SimpleRoute,
};
use crate::overlay::{InsertPosition, OverlayEntry, OverlayEntryId, OverlayHandle};
use crate::text::TextEditingController;

/// Reads of a [`NavigatorHandle`]'s private route stack and registries.
pub trait NavigatorProbe {
    /// How many attached observers drive hero flights.
    fn hero_observer_count(&self) -> usize;
    /// The lifecycle state of `id`'s entry, or `None` once disposed.
    fn route_state(&self, id: RouteId) -> Option<RouteLifecycle>;
    /// The overlay entry `id`'s route presents.
    fn entry_of(&self, id: RouteId) -> Option<OverlayEntry>;
    /// How many `RouteId -> OverlayEntry` pairs the navigator holds.
    fn tracked_entry_count(&self) -> usize;
    /// How many `RouteId -> RouteSubtreeCell` pairs the navigator holds.
    fn tracked_subtree_count(&self) -> usize;
    /// `id`'s subtree cell, element half and render half, unjoined.
    fn route_subtree_parts(&self, id: RouteId) -> Option<(Option<ElementId>, Option<RenderId>)>;
    /// Flutter's `Route.isCurrent`.
    fn is_current(&self, id: RouteId) -> bool;
    /// The overlay the navigator presents its routes in.
    fn overlay(&self) -> &OverlayHandle;
    /// `id`'s modal handle, or `None` for a non-modal or disposed route.
    fn route_modal(&self, id: RouteId) -> Option<ModalHandle>;
    /// Where `id`'s page subtree lives, once mounted and attached.
    fn route_subtree(&self, id: RouteId) -> Option<RouteSubtree>;
    /// What `id` publishes about its transition.
    fn route_peer(&self, id: RouteId) -> Option<TransitionPeer>;
    /// The owner-local post-frame handle captured at mount.
    fn local_post_frame_handle(&self) -> Option<LocalPostFrameHandle>;
    /// The render tree the navigator is mounted in.
    fn render_tree(&self) -> Option<PipelineCell>;
    /// Whether `route` may start an edge-swipe-back gesture right now.
    fn pop_gesture_enabled(&self, route: RouteId) -> bool;
    /// Pop `route`, pacing its reverse transition with `duration` and `curve`.
    fn pop_paced(
        &self,
        route: RouteId,
        duration: Duration,
        curve: Arc<dyn Curve + Send + Sync>,
    ) -> bool;
}

impl NavigatorProbe for NavigatorHandle {
    fn hero_observer_count(&self) -> usize {
        NavigatorHandle::hero_observer_count(self)
    }
    fn route_state(&self, id: RouteId) -> Option<RouteLifecycle> {
        NavigatorHandle::route_state(self, id)
    }
    fn entry_of(&self, id: RouteId) -> Option<OverlayEntry> {
        NavigatorHandle::entry_of(self, id)
    }
    fn tracked_entry_count(&self) -> usize {
        NavigatorHandle::tracked_entry_count(self)
    }
    fn tracked_subtree_count(&self) -> usize {
        NavigatorHandle::tracked_subtree_count(self)
    }
    fn route_subtree_parts(&self, id: RouteId) -> Option<(Option<ElementId>, Option<RenderId>)> {
        NavigatorHandle::route_subtree_parts(self, id)
    }
    fn is_current(&self, id: RouteId) -> bool {
        NavigatorHandle::is_current(self, id)
    }
    fn overlay(&self) -> &OverlayHandle {
        NavigatorHandle::overlay(self)
    }
    fn route_modal(&self, id: RouteId) -> Option<ModalHandle> {
        NavigatorHandle::route_modal(self, id)
    }
    fn route_subtree(&self, id: RouteId) -> Option<RouteSubtree> {
        NavigatorHandle::route_subtree(self, id)
    }
    fn route_peer(&self, id: RouteId) -> Option<TransitionPeer> {
        NavigatorHandle::route_peer(self, id)
    }
    fn local_post_frame_handle(&self) -> Option<LocalPostFrameHandle> {
        NavigatorHandle::local_post_frame_handle(self)
    }
    fn render_tree(&self) -> Option<PipelineCell> {
        NavigatorHandle::render_tree(self)
    }
    fn pop_gesture_enabled(&self, route: RouteId) -> bool {
        NavigatorHandle::pop_gesture_enabled(self, route)
    }
    fn pop_paced(
        &self,
        route: RouteId,
        duration: Duration,
        curve: Arc<dyn Curve + Send + Sync>,
    ) -> bool {
        NavigatorHandle::pop_paced(self, route, duration, curve)
    }
}

/// The navigator capability behind a public [`RouteBindingSlot`], for a
/// hand-written test route. `RouteBinding` itself stays unnameable.
pub trait RouteBindingSlotProbe {
    /// Raise `finalize()` on the bound route; a no-op on an unbound slot.
    fn finalize(&self);
}

impl RouteBindingSlotProbe for RouteBindingSlot {
    fn finalize(&self) {
        if let Some(binding) = self.get() {
            binding.finalize();
        }
    }
}

/// The transition behind a public route, for driving its animation by hand.
pub trait RouteProbe {
    /// The route's transition handle.
    fn transition_handle(&self) -> TransitionHandle;
}

impl<T: Send + Clone + 'static> RouteProbe for PageRoute<T> {
    fn transition_handle(&self) -> TransitionHandle {
        PageRoute::transition_handle(self)
    }
}

impl<T: Send + Clone + 'static> RouteProbe for PopupRoute<T> {
    fn transition_handle(&self) -> TransitionHandle {
        PopupRoute::transition_handle(self)
    }
}

/// The modal behind a [`PageRoute`].
pub trait PageRouteProbe {
    /// The route's modal handle, whose `set_offstage` the hero measurement drives.
    fn modal_handle(&self) -> ModalHandle;
}

impl<T: Send + Clone + 'static> PageRouteProbe for PageRoute<T> {
    fn modal_handle(&self) -> ModalHandle {
        PageRoute::modal_handle(self)
    }
}

/// Pop-refusal builders on [`SimpleRoute`], modelling the deferred
/// `LocalHistoryRoute`.
pub trait SimpleRouteProbe: Sized {
    /// Make `did_pop` refuse.
    #[must_use]
    fn refusing_pop(self) -> Self;
    /// Make `will_handle_pop_internally` true.
    #[must_use]
    fn handling_pop_internally(self) -> Self;
}

impl<T> SimpleRouteProbe for SimpleRoute<T> {
    fn refusing_pop(self) -> Self {
        SimpleRoute::refusing_pop(self)
    }
    fn handling_pop_internally(self) -> Self {
        SimpleRoute::handling_pop_internally(self)
    }
}

/// What a [`HeroController`] scheduled, measured and launched.
pub trait HeroControllerProbe {
    /// The navigator this controller observes, or `None` when detached.
    fn navigator(&self) -> Option<NavigatorHandle>;
    /// How many post-frame measurements have been scheduled.
    fn scheduled_count(&self) -> usize;
    /// Everything the post-frame callbacks resolved, in order.
    fn measurements(&self) -> Vec<Measurement>;
    /// The flights that started, one per shared tag.
    fn manifests(&self) -> Vec<HeroFlightManifest>;
    /// The flights currently in the air.
    fn flights(&self) -> &Arc<FlightManager>;
}

impl HeroControllerProbe for HeroController {
    fn navigator(&self) -> Option<NavigatorHandle> {
        HeroController::navigator(self)
    }
    fn scheduled_count(&self) -> usize {
        HeroController::scheduled_count(self)
    }
    fn measurements(&self) -> Vec<Measurement> {
        HeroController::measurements(self)
    }
    fn manifests(&self) -> Vec<HeroFlightManifest> {
        HeroController::manifests(self)
    }
    fn flights(&self) -> &Arc<FlightManager> {
        HeroController::flights(self)
    }
}

/// How many change listeners a [`TextEditingController`] holds.
pub trait TextEditingControllerProbe {
    /// The number of registered change listeners.
    fn listener_count(&self) -> usize;
}

impl TextEditingControllerProbe for TextEditingController {
    fn listener_count(&self) -> usize {
        TextEditingController::listener_count(self)
    }
}

/// Inspection of an [`OverlayHandle`]'s entry list, and the crate-private
/// group insert.
///
/// The overlay's public API (ADR-0076) mutates the list but never exposes it:
/// nothing outside a test needs to read the stacking order back.
#[expect(
    clippy::len_without_is_empty,
    reason = "a test-facing count; the tests compare it with exact values"
)]
pub trait OverlayProbe {
    /// The ids of the entries in the list, bottom → top (the last one paints
    /// on top), whether or not the overlay is mounted.
    fn entry_ids(&self) -> Vec<OverlayEntryId>;
    /// How many entries the list holds.
    fn len(&self) -> usize;
    /// Whether both handles name the same overlay.
    fn is_same(&self, other: &Self) -> bool;
    /// Insert `entries` as one contiguous group at `position`.
    fn insert_all(&self, entries: &[OverlayEntry], position: &InsertPosition);
}

impl OverlayProbe for OverlayHandle {
    fn entry_ids(&self) -> Vec<OverlayEntryId> {
        self.ids_bottom_to_top()
    }
    fn len(&self) -> usize {
        OverlayHandle::len(self)
    }
    fn is_same(&self, other: &Self) -> bool {
        OverlayHandle::is_same(self, other)
    }
    fn insert_all(&self, entries: &[OverlayEntry], position: &InsertPosition) {
        OverlayHandle::insert_all(self, entries, position);
    }
}

/// Crate-private builders and reads on an [`OverlayEntry`].
pub trait OverlayEntryProbe: Sized {
    /// The entry with `opaque` set before it is inserted.
    #[must_use]
    fn with_opaque(self, opaque: bool) -> Self;
    /// The entry with `maintain_state` set before it is inserted.
    #[must_use]
    fn with_maintain_state(self, maintain_state: bool) -> Self;
    /// Whether the entry is opaque.
    fn opaque(&self) -> bool;
    /// Whether the entry keeps its subtree built while covered.
    fn maintain_state(&self) -> bool;
    /// The entry's stable identity.
    fn id(&self) -> OverlayEntryId;
    /// Whether the entry's element is mounted.
    fn is_mounted(&self) -> bool;
    /// The entry's element, while mounted.
    fn element_id(&self) -> Option<ElementId>;
}

impl OverlayEntryProbe for OverlayEntry {
    fn with_opaque(self, opaque: bool) -> Self {
        OverlayEntry::with_opaque(self, opaque)
    }
    fn with_maintain_state(self, maintain_state: bool) -> Self {
        OverlayEntry::with_maintain_state(self, maintain_state)
    }
    fn opaque(&self) -> bool {
        OverlayEntry::opaque(self)
    }
    fn maintain_state(&self) -> bool {
        OverlayEntry::maintain_state(self)
    }
    fn id(&self) -> OverlayEntryId {
        OverlayEntry::id(self)
    }
    fn is_mounted(&self) -> bool {
        OverlayEntry::is_mounted(self)
    }
    fn element_id(&self) -> Option<ElementId> {
        OverlayEntry::element_id(self)
    }
}

#[cfg(test)]
mod tests {
    /// Every name this module makes reachable, with the reason it is still
    /// here. Each leaves when the integration tests that use it assert through
    /// public API instead: the navigator and hero entries with the Router
    /// conformance suite (ADR-0093), the overlay entries once the overlay's
    /// entry list and entry flags have a public read, the text entry once the
    /// listener count is observable some other way.
    const ENTRIES: [(&str, &str); 30] = [
        (
            "BackGestureController",
            "drives a release against a mounted navigator's route",
        ),
        (
            "BackGestureRuntime",
            "dispose and settle of a gesture on a mounted navigator",
        ),
        (
            "FlightManager",
            "the hero flights in the air, read by the flight tests",
        ),
        (
            "HeroControllerProbe",
            "measurements, manifests and flights of a HeroController",
        ),
        ("HeroFlight", "a flight's rects, opacity and direction"),
        (
            "HeroFlightManifest",
            "the manifests a measurement pass built",
        ),
        (
            "HeroHandle",
            "the placeholder and flight lifecycle of one mounted hero",
        ),
        ("HeroRegistry", "which heroes a route registered"),
        (
            "HeroScope",
            "mounts a registry above heroes without a route",
        ),
        ("HeroTag", "names a hero in the registry and flight lookups"),
        (
            "LocalHistoryEntry",
            "local-history pops through a mounted navigator",
        ),
        (
            "LocalHistoryEntryHandle",
            "returned by LocalHistoryHandle::add",
        ),
        (
            "LocalHistoryHandle",
            "the page-side local-history capability",
        ),
        (
            "Measurement",
            "what one post-frame hero measurement resolved",
        ),
        (
            "ModalHandle",
            "a modal route's offstage and maintain-state flags",
        ),
        (
            "ModalRoute",
            "the private route PageRoute and PopupRoute are built on",
        ),
        (
            "NavigatorProbe",
            "route state, entries, subtrees and peers of a navigator",
        ),
        ("OverlayEntryProbe", "an overlay entry's identity and flags"),
        (
            "OverlayProbe",
            "an overlay's stacking order (ADR-0076 keeps it unread)",
        ),
        ("PageRouteProbe", "the modal behind a PageRoute"),
        (
            "RouteBindingSlotProbe",
            "finalize from a hand-written test route",
        ),
        (
            "RouteLifecycle",
            "the route-stack state a navigator test asserts on",
        ),
        (
            "RouteProbe",
            "the transition behind a PageRoute or PopupRoute",
        ),
        ("RouteSubtree", "where a route's page subtree lives"),
        (
            "SimpleRouteProbe",
            "pop refusal, modelling the deferred LocalHistoryRoute",
        ),
        (
            "TextEditingControllerProbe",
            "listener count across EditableText rebuilds",
        ),
        (
            "TransitionGroup",
            "the transition family a route peer reports",
        ),
        ("TransitionHandle", "drives a route's transition by hand"),
        (
            "TransitionPeer",
            "what a route publishes about its transition",
        ),
        (
            "TransitionRoute",
            "the private route ModalRoute is built on",
        ),
    ];

    /// The re-exported types and the probe traits, exactly: adding an entry
    /// means adding it (with its reason) to [`ENTRIES`], which review sees.
    /// The module is also `#[doc(hidden)]`, so it never enters the rendered
    /// API.
    ///
    /// Red-check: add a `pub use` here or a new `pub trait`, or drop the
    /// `#[doc(hidden)]` above `pub mod __test_access;` in `lib.rs`.
    #[test]
    fn lists_exactly_the_temporary_entries() {
        const SOURCE: &str = include_str!("__test_access.rs");
        const LIB: &str = include_str!("lib.rs");

        let mut found: Vec<&str> = crate::navigator::export_guard::exported_identifiers(SOURCE)
            .into_iter()
            .filter(|token| token.starts_with(char::is_uppercase))
            .collect();
        found.extend(
            SOURCE
                .lines()
                .filter_map(|line| line.strip_prefix("pub trait "))
                .filter_map(|rest| {
                    rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                        .next()
                }),
        );
        found.sort_unstable();

        let mut expected: Vec<&str> = ENTRIES.iter().map(|(name, _)| *name).collect();
        expected.sort_unstable();
        assert_eq!(
            found, expected,
            "__test_access's entries changed: update ENTRIES with the reason, \
             or assert through public API instead"
        );
        assert!(
            ENTRIES.iter().all(|(_, reason)| !reason.is_empty()),
            "every entry states why it is still needed"
        );

        let lines: Vec<&str> = LIB.lines().map(str::trim).collect();
        let declared = lines
            .iter()
            .position(|line| *line == "pub mod __test_access;")
            .expect("lib.rs declares `pub mod __test_access;`");
        assert_eq!(
            lines.get(declared.wrapping_sub(1)).copied(),
            Some("#[doc(hidden)]"),
            "the module stays out of the rendered API"
        );
    }
}
