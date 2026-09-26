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
//! - **probe traits** add methods to public types without widening their
//!   inherent API;
//! - **re-exports** name private types raised to `pub` at their definition,
//!   inside private modules, so nothing else reaches them;
//! - **test types** ([`ZeroDurationRoute`]) stand in for a capability that
//!   must not be reachable on its own.
//!
//! Not every entry is a read. Some drive state the navigator normally drives
//! itself — `NavigatorProbe::pop_paced`, `ModalHandle::set_offstage` and
//! `set_maintain_state`, `HeroHandle::start_flight` and `end_flight`,
//! `LocalHistoryHandle::add` — and a re-exported type brings all of its `pub`
//! methods along. None of that is public API: a crate that imports this
//! module is outside its contract.
//!
//! An entry leaves when its tests assert through public API instead — for the
//! navigator internals that is the Router conformance suite (ADR-0093). The
//! module is deleted when it is empty. `tests::lists_exactly_the_temporary_entries`
//! pins every name and probe method, and refuses a glob, a module re-export or
//! any other kind of `pub` item, so adding to the surface is a reviewed edit.

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::Curve;
use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::{LocalPostFrameHandle, TickerFuture};

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
    HeroController, NavigatorHandle, NavigatorRoute, PageRoute, PopupRoute, PushCompletion, Route,
    RouteBindingSlot, RouteContentBuilder, RouteId, RouteSettings, SimpleRoute,
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

/// A zero-duration transition route: it parks in `Pushing`, completes its
/// entrance with an already-resolved future, and finalizes itself from
/// `did_pop` — inside the flush that popped it.
///
/// It exists so the route-animation seam can be driven end to end without
/// making the capability behind a [`RouteBindingSlot`] reachable: the route
/// finalizes only itself, and only when the navigator pops it. `RouteBinding`
/// stays unnameable and a slot still has no accessor outside the crate.
pub struct ZeroDurationRoute {
    settings: RouteSettings,
    builder: RouteContentBuilder,
    binding: RouteBindingSlot,
}

impl ZeroDurationRoute {
    /// A route named by `settings` whose content `builder` builds.
    #[must_use]
    pub fn new(settings: RouteSettings, builder: RouteContentBuilder) -> Self {
        Self {
            settings,
            builder,
            binding: RouteBindingSlot::new(),
        }
    }
}

impl std::fmt::Debug for ZeroDurationRoute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZeroDurationRoute")
            .field("settings", &self.settings)
            .field("binding", &self.binding)
            .finish_non_exhaustive()
    }
}

impl Route for ZeroDurationRoute {
    type Output = i32;

    fn settings(&self) -> &RouteSettings {
        &self.settings
    }

    /// `TransitionRoute.finishedWhenPopped => controller.isDismissed` — false
    /// while the exit transition runs, so disposal defers to `finalize()`.
    fn finished_when_popped(&self) -> bool {
        false
    }

    fn did_push(&mut self) -> PushCompletion {
        // The future is already resolved by the time it is handed out — the
        // zero-duration shape. `NavigatorShared::apply` registers a
        // continuation on the future once the push's own flush releases the
        // history lock, and that continuation raises `PushCompleted`
        // (ADR-0064) — a route has no seam to raise it directly.
        PushCompletion::Animating(TickerFuture::complete())
    }

    fn did_pop(&mut self) -> bool {
        if let Some(binding) = self.binding.get() {
            binding.finalize();
        }
        true
    }
}

impl NavigatorRoute for ZeroDurationRoute {
    fn content_builder(&self) -> RouteContentBuilder {
        Rc::clone(&self.builder)
    }

    fn binding_slot(&self) -> Option<&RouteBindingSlot> {
        Some(&self.binding)
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
        (
            "ZeroDurationRoute",
            "drives the route-animation seam without exposing RouteBinding",
        ),
    ];

    /// The methods the probe traits and test types add, as `Owner::method`.
    /// The re-exported types carry their own `pub` methods, which are pinned
    /// at their definitions, not here.
    const METHODS: [&str; 37] = [
        "HeroControllerProbe::flights",
        "HeroControllerProbe::manifests",
        "HeroControllerProbe::measurements",
        "HeroControllerProbe::navigator",
        "HeroControllerProbe::scheduled_count",
        "NavigatorProbe::entry_of",
        "NavigatorProbe::hero_observer_count",
        "NavigatorProbe::is_current",
        "NavigatorProbe::local_post_frame_handle",
        "NavigatorProbe::overlay",
        "NavigatorProbe::pop_gesture_enabled",
        "NavigatorProbe::pop_paced",
        "NavigatorProbe::render_tree",
        "NavigatorProbe::route_modal",
        "NavigatorProbe::route_peer",
        "NavigatorProbe::route_state",
        "NavigatorProbe::route_subtree",
        "NavigatorProbe::route_subtree_parts",
        "NavigatorProbe::tracked_entry_count",
        "NavigatorProbe::tracked_subtree_count",
        "OverlayEntryProbe::element_id",
        "OverlayEntryProbe::id",
        "OverlayEntryProbe::is_mounted",
        "OverlayEntryProbe::maintain_state",
        "OverlayEntryProbe::opaque",
        "OverlayEntryProbe::with_maintain_state",
        "OverlayEntryProbe::with_opaque",
        "OverlayProbe::entry_ids",
        "OverlayProbe::insert_all",
        "OverlayProbe::is_same",
        "OverlayProbe::len",
        "PageRouteProbe::modal_handle",
        "RouteProbe::transition_handle",
        "SimpleRouteProbe::handling_pop_internally",
        "SimpleRouteProbe::refusing_pop",
        "TextEditingControllerProbe::listener_count",
        "ZeroDurationRoute::new",
    ];

    /// What kind of column-0 block a line sits in.
    enum Block {
        /// A `pub trait`: every `fn` is surface.
        Trait,
        /// An inherent `impl`: every `pub fn` is surface, any other `pub`
        /// item is refused.
        Inherent,
        /// A `pub struct` body: a `pub` field is refused.
        Struct,
        /// A trait `impl` or anything else: its methods belong to the trait.
        Other,
    }

    fn ident(rest: &str) -> String {
        rest.split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    /// The leaves of one `pub use` path, or `Err` for anything the pin could
    /// not list by name: a glob, a rename, a nested group, or a lowercase
    /// leaf (a module or a function).
    fn use_leaves(path: &str) -> Result<Vec<String>, ()> {
        let path = path.trim().trim_end_matches(';').trim();
        if path.contains('*') || path.contains(" as ") {
            return Err(());
        }
        let leaves: Vec<&str> = match path.find('{') {
            Some(open) => {
                let inner = &path[open + 1..path.rfind('}').ok_or(())?];
                if inner.contains('{') {
                    return Err(());
                }
                inner
                    .split(',')
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect()
            }
            None => vec![path.rsplit("::").next().ok_or(())?],
        };
        leaves
            .into_iter()
            .map(|leaf| {
                if !leaf.contains("::") && leaf.starts_with(char::is_uppercase) {
                    Ok(leaf.to_owned())
                } else {
                    Err(())
                }
            })
            .collect()
    }

    /// Everything `source` (this module's text) makes nameable from outside
    /// the crate, sorted: each re-exported leaf, `pub trait` and `pub struct`
    /// by name, each trait method and inherent `pub fn` as `Owner::method`.
    /// `Err` names the first line that exports something else.
    fn surface(source: &str) -> Result<Vec<String>, String> {
        let mut names = Vec::new();
        let mut block: Option<(String, Block)> = None;
        let mut lines = source.lines().map(str::trim_end);
        while let Some(line) = lines.next() {
            if line == "mod tests {" {
                break;
            }
            if let Some((owner, kind)) = &block {
                if line == "}" || line == "};" {
                    block = None;
                    continue;
                }
                let item = line.trim_start();
                match kind {
                    Block::Trait => {
                        if let Some(rest) = item.strip_prefix("fn ") {
                            names.push(format!("{owner}::{}", ident(rest)));
                        }
                    }
                    Block::Inherent => {
                        if let Some(rest) = item.strip_prefix("pub fn ") {
                            names.push(format!("{owner}::{}", ident(rest)));
                        } else if item.starts_with("pub ") {
                            return Err(line.to_owned());
                        }
                    }
                    Block::Struct => {
                        if item.starts_with("pub ") {
                            return Err(line.to_owned());
                        }
                    }
                    Block::Other => {}
                }
                continue;
            }
            let opens = line.ends_with('{');
            if let Some(rest) = line.strip_prefix("pub use ") {
                let mut path = rest.to_owned();
                while !path.ends_with(';') {
                    let next = lines.next().ok_or_else(|| line.to_owned())?;
                    path.push(' ');
                    path.push_str(next.trim());
                }
                names.extend(use_leaves(&path).map_err(|()| line.to_owned())?);
            } else if let Some(rest) = line.strip_prefix("pub trait ") {
                names.push(ident(rest));
                if opens {
                    block = Some((ident(rest), Block::Trait));
                }
            } else if let Some(rest) = line.strip_prefix("pub struct ") {
                names.push(ident(rest));
                if opens {
                    block = Some((ident(rest), Block::Struct));
                }
            } else if line.starts_with("pub ") {
                return Err(line.to_owned());
            } else if opens && line.starts_with("impl") {
                let head = line.trim_start_matches("impl").trim_start();
                let head = if head.starts_with('<') {
                    head.split_once("> ").map_or(head, |(_, rest)| rest)
                } else {
                    head
                };
                block = Some(if head.contains(" for ") {
                    (String::new(), Block::Other)
                } else {
                    (ident(head), Block::Inherent)
                });
            } else if opens {
                block = Some((String::new(), Block::Other));
            }
        }
        names.sort_unstable();
        Ok(names)
    }

    /// The module's whole outside surface, exactly: adding a re-export, a
    /// probe trait, a test type or a method on either means adding it to
    /// [`ENTRIES`] (with its reason) or [`METHODS`], which review sees; a
    /// glob, a module re-export, a rename or a `pub` item of another kind is
    /// refused outright. The module is also `#[doc(hidden)]`, so it never
    /// enters the rendered API.
    ///
    /// Red-check: add a `pub use` here, a new `pub trait`, or a method on a
    /// probe trait; or drop the `#[doc(hidden)]` above `pub mod __test_access;`
    /// in `lib.rs`. `surface_refuses_what_the_pin_cannot_name` plants the
    /// shapes the pin refuses.
    #[test]
    fn lists_exactly_the_temporary_entries() {
        const SOURCE: &str = include_str!("__test_access.rs");
        const LIB: &str = include_str!("lib.rs");

        let mut expected: Vec<String> = ENTRIES
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .chain(METHODS.iter().map(|m| (*m).to_owned()))
            .collect();
        expected.sort_unstable();
        assert_eq!(
            surface(SOURCE),
            Ok(expected),
            "__test_access's surface changed: update ENTRIES (with the reason) \
             or METHODS, or assert through public API instead"
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

    /// Each shape an export can take that the pin cannot list by name is
    /// refused, and a new probe method or re-exported type shows up in the
    /// surface, so the exact comparison above fails on it.
    #[test]
    fn surface_refuses_what_the_pin_cannot_name() {
        let refused = [
            "pub use crate::navigator::hero::*;",
            "pub use crate::navigator::local_history;",
            "pub use crate::navigator::back_gesture::convert_to_logical;",
            "pub use crate::navigator::hero::HeroTag as Tag;",
            "pub use crate::navigator::{hero::HeroTag};",
            "pub fn finalize(route: RouteId) {}",
            "pub type Rh = crate::navigator::transition_route::TransitionHandle;",
            "pub const LIMIT: usize = 1;",
            "pub static LIMIT: usize = 1;",
            "pub mod inner {}",
            "pub enum Kind {}",
            "pub struct Open {\n    pub binding: RouteBindingSlot,\n}",
            "pub struct Open;\nimpl Open {\n    pub const LIMIT: usize = 1;\n}",
        ];
        for plant in refused {
            assert!(surface(plant).is_err(), "not refused: {plant}");
        }

        let listed = [
            (
                "pub use crate::navigator::hero::{\n    HeroHandle,\n    HeroTag,\n};",
                vec!["HeroHandle", "HeroTag"],
            ),
            (
                "pub trait Probe {\n    fn read(&self) -> usize;\n    fn write(\n        &self,\n    );\n}",
                vec!["Probe", "Probe::read", "Probe::write"],
            ),
            (
                "pub struct Route {\n    binding: RouteBindingSlot,\n}\n\
                 impl Route {\n    pub fn new() -> Self {}\n    fn private(&self) {}\n}\n\
                 impl<T: Send> Probe for Route {\n    fn read(&self) -> usize {}\n}",
                vec!["Route", "Route::new"],
            ),
        ];
        for (plant, names) in listed {
            assert_eq!(
                surface(plant),
                Ok(names.into_iter().map(String::from).collect())
            );
        }
    }
}
