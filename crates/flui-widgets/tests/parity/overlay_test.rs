//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/overlay_test.dart` (tag
//! `3.44.0`) — 30 `testWidgets` cases, plus 4 plain `test()`s.
//!
//! ## Why this file is thin
//!
//! Almost the entire oracle file exercises `Overlay`'s *mutation* surface —
//! `insert`/`insertAll`/`rearrange`, `OverlayEntry.opaque`/`maintainState`,
//! `markNeedsBuild` — which stays `pub(crate)` in FLUI (ADR-0036 §1: only the
//! four *types* `Overlay`/`OverlayEntry`/`OverlayEntryId`/`OverlayHandle`, and
//! `Overlay::of`/`maybe_of`, are `pub`; there is no public `Overlay::new`,
//! `Overlay::wrap`, or `OverlayEntry::new` at all — see ADR-0036's Deferred
//! list). None of that surface is reachable from a `tests/` integration
//! binary, which is exactly why `crates/flui-widgets/src/overlay/tests.rs`
//! exists as an in-crate suite (its own module docs explain the same
//! constraint). That file ports every mutation-surface and
//! opaque/`maintainState` case in scope; this file does not repeat them.
//!
//! What's left, reachable only through the public API, is `Overlay::of`/
//! `maybe_of` **resolving**. The only way to get a mounted `Overlay` within
//! reach of a `tests/` binary at all is through a mounted [`Navigator`],
//! which mounts one per ADR-0036's Context section (`navigator.rs:1610`,
//! `overlay_route.rs`'s `SimpleRoute` bridge) — there is no other public
//! constructor.
//!
//! ## Ported
//! - The "an Overlay does exist" half of `'OverlayState.maybeOf() works when
//!   an Overlay does and doesn't exist'` —
//!   [`overlay_of_and_maybe_of_resolve_inside_a_mounted_navigators_route`].
//!   `flui_widgets::navigator::navigator_tests::overlay_of_from_route_content_resolves_the_navigators_own_overlay`
//!   already pins the identical fact in-crate; this file's version is not a
//!   duplicate of its *assertion* but of its *reach*: it proves the same fact
//!   holds through the crate's public API alone (`Navigator::new`,
//!   `NavigatorHandle::new`/`seed_initial`, `SimpleRoute::new`,
//!   `Overlay::of`/`maybe_of`) — nothing `pub(crate)` — which the in-crate
//!   version, reached through `mount()` and `OverlayHandle::overlay()`,
//!   cannot demonstrate by construction.
//!
//! ## Not ported, and why
//! - The "no ancestor" halves of `'OverlayState.of() throws when called if an
//!   Overlay does not exist'` and `'...maybeOf() works when an Overlay does
//!   and doesn't exist'` — already pinned in-crate by
//!   `overlay_of_panics_with_a_helpful_message_without_an_overlay_ancestor`
//!   and `overlay_maybe_of_is_none_without_an_overlay_ancestor`
//!   (`overlay/tests.rs`). Neither needs a mounted `Overlay` at all, only a
//!   bare `BuildContext`, so re-porting here would assert the identical fact
//!   through a heavier fixture.
//! - Every `insert*`/`rearrange`/`opaque`/`maintainState`/`markNeedsBuild`
//!   case (`'insert top'`, `'insert below'`, `'insert above'`, `'insertAll
//!   top/below/above'`, `'rearrange'`, `'OverlayEntry.opaque can be changed
//!   when OverlayEntry is not part of an Overlay (yet)'`, `'OverlayEntries do
//!   not rebuild when opaqueness changes'`, `'... when opaque entry is
//!   added'`, and the rest of that family) — `pub(crate)`, ported in
//!   `overlay/tests.rs`.
//! - `'OverflowEntries context contains Overlay'`, `'Offstage overlay'` —
//!   assert on `RenderObject.toStringDeep()`; FLUI's render-object debug-tree
//!   printing is not part of this parity slice.
//! - `'debugVerifyInsertPosition'` — asserts Flutter's runtime validation of
//!   the mutually-exclusive `above`/`below` named arguments. FLUI's
//!   `InsertPosition` enum (`overlay/mod.rs`) makes that combination
//!   unrepresentable at the type level instead of a runtime assert — there is
//!   nothing left to port.
//! - `'rearrange above'`, `'rearrange below'` — `OverlayHandle::rearrange`'s
//!   `above:`/`below:` placement of the unmentioned trailing group is an
//!   explicitly named deferral (`overlay/mod.rs`'s `rearrange` doc); no
//!   caller, including `Navigator`, passes either.
//! - `'entries below opaque entries are ignored for hit testing'` — the
//!   enforcing mechanism is real and unit-tested directly:
//!   `RenderTheater::hit_test` bounds its walk to `first_onstage(..)`
//!   (`crates/flui-objects/src/layout/theater.rs`). A pointer-dispatch
//!   integration test *through* a mounted `Overlay` would need the same kind
//!   of bespoke gesture-dispatch harness `gesture_detector_test.rs` builds for
//!   itself — infrastructure this suite does not have and this task does not
//!   add.
//! - `'Semantics of entries below opaque entries are ignored'` — parity is
//!   **not** claimed; a real, documented divergence in
//!   `crates/flui-objects/src/layout/theater.rs`'s module docs ("Semantics
//!   are not skipped" — FLUI's `RenderTheater` has no per-child semantics
//!   visitor, only the whole-subtree `excludes_semantics_subtree`).
//! - The `OverlayEntry listenable` group (`'mounted state can be listened'`,
//!   `'throw if disposed before removal'`, `'dispose works'`, `'delayed
//!   dispose'`, `'asserts when remove is called twice'`) — FLUI's
//!   `OverlayEntry` is not a `Listenable` and has no separate `dispose()` to
//!   port against (documented divergence, `overlay/entry.rs`). The one live
//!   behavior that group exercises which FLUI *does* implement — a second
//!   `remove()` is inert rather than panicking — is pinned by
//!   `removed_entry_cannot_reinsert_or_rebuild_silently` (`overlay/tests.rs`).
//! - The `LookupBoundary` group (3 cases) — `LookupBoundary` itself is listed
//!   as not-yet-started in ADR-0036's Deferred section; nothing in this
//!   codebase has an equivalent lookup-boundary concept yet.
//! - `'Overlay.wrap'`, `'Overlay.wrap is sized by child ...'`, `'Overlay is
//!   sized by child in an unconstrained environment'`, the three `'Overlay
//!   throws if unconstrained ...'` cases, the two `'... alwaysSizeToContent
//!   ...'` cases, `'Overlay is not visible from sub-views'`, `'Overlay does
//!   not crash at zero area'` — every one needs a public `Overlay::new`/
//!   `Overlay::wrap` constructor, `canSizeOverlay`, `alwaysSizeToContent`, or
//!   `ViewAnchor` sub-view support, none of which exist. Each is named either
//!   in ADR-0036's Deferred list (`Overlay.wrap`, a public constructor) or in
//!   `crates/flui-objects/src/layout/theater.rs`'s divergences list
//!   (`canSizeOverlay`/`alwaysSizeToContent`).
//! - `'Overlay can set and update clipBehavior'`, `'Overlay always applies
//!   clip'` — FLUI's `RenderTheater` has no `clipBehavior` property; its
//!   divergences list records that it never has positioned children, so
//!   nothing can overflow the clip Flutter's version guards against.
//! - `'OverlayEntry dispatches memory events'` (leak-tracker `test()`, not
//!   `testWidgets`) and `'of method calls
//!   getElementForInheritedWidgetOfExactType'` (a hand-rolled
//!   `FakeBuildContext` probing exactly which `BuildContext` method
//!   `Overlay.maybeOf` calls through) — both assert Dart-specific mechanisms
//!   (leak-tracker create/dispose events; a fake-context call-through probe)
//!   with no Rust analogue. The second one's *effect* — `maybe_of` returning
//!   `None` with no ancestor — is what
//!   `overlay_maybe_of_is_none_without_an_overlay_ancestor` asserts directly.
//! - `'Can use Positioned within OverlayEntry'` — already ported, with a
//!   wider from-first-principles assertion of *why* it must hold, by
//!   `positioned_inside_an_overlay_entry_is_laid_out_by_an_inner_stack`
//!   (`overlay/tests.rs`).
//!
//! Denominator: 34 oracle cases (30 `testWidgets` + 4 `test()`), all
//! accounted for above — 1 ported here, ~19 ported in `overlay/tests.rs`
//! (see that file's own header), the remainder named out of scope with a
//! reason.

use std::sync::Arc;

use flui_view::BuildContext;
use flui_view::element::ElementKind;
use flui_widgets::prelude::*;
use parking_lot::Mutex;

use crate::common::{lay_out, loose};

/// A stateless leaf that runs `on_build` from its **own** `build`, on its own
/// nested `BuildContext`.
///
/// Not a detail of convenience: an `OverlayEntry`'s content-builder closure
/// receives the *same* `BuildContext` its hosting `OverlayEntryViewState`
/// does, which is an ancestor of — not a descendant of — the `OverlayScope`
/// marker that same state wraps the built content in (`overlay/mod.rs`'s
/// `OverlayEntryViewState::build` docs). A lookup made with the route's own
/// top-level `ctx` can therefore never see the very overlay it is part of;
/// only a *child* view's own `build`, given a properly nested context, can.
/// `overlay/navigator_tests.rs`'s
/// `overlay_of_from_route_content_resolves_the_navigators_own_overlay` uses
/// the identical shape for the identical reason.
#[derive(Clone)]
struct Peek<F: Fn(&dyn BuildContext) + Clone + 'static>(F);

impl<F: Fn(&dyn BuildContext) + Clone + 'static> View for Peek<F> {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

impl<F: Fn(&dyn BuildContext) + Clone + 'static> StatelessView for Peek<F> {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        (self.0)(ctx);
        SizedBox::new(5.0, 5.0)
    }
}

/// Port of the "an Overlay does exist" half of `'OverlayState.maybeOf() works
/// when an Overlay does and doesn't exist'` (`overlay_test.dart`), through
/// FLUI's actual production path: a mounted [`Navigator`] is what puts an
/// `Overlay` in reach of a route's content, not a hand-built one — the "an
/// Overlay does exist" fact the in-crate suite's direct-construction tests
/// cannot exercise, since they never go through `Navigator` at all.
///
/// Both entry points are checked from the same nested `Peek`, mirroring the
/// oracle test asserting both `Overlay.of`/`Overlay.maybeOf` resolve once
/// mounted: `Overlay::of` must not panic, and `Overlay::maybe_of` must return
/// `Some` naming an overlay that reports itself mounted.
///
/// Red-check: swap `Navigator::new(handle)`'s child for a bare `SizedBox` (no
/// `Navigator`, so no `Overlay` ancestor at all) — `Overlay::of` then panics
/// instead of returning, which the harness surfaces as a build-time panic.
#[test]
fn overlay_of_and_maybe_of_resolve_inside_a_mounted_navigators_route() {
    let maybe_of_found: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let maybe_of_slot = Arc::clone(&maybe_of_found);

    let handle = NavigatorHandle::new();
    handle.seed_initial(SimpleRoute::<()>::new(move |_ctx| {
        let slot = Arc::clone(&maybe_of_slot);
        Peek(move |ctx: &dyn BuildContext| {
            let of_handle = Overlay::of(ctx); // panics if unresolved; a plain return proves it isn't
            assert!(
                Overlay::maybe_of(ctx).is_some(),
                "maybe_of must resolve the same ancestor Overlay::of just did"
            );
            *slot.lock() = Some(format!("{of_handle:?}"));
        })
        .into_view()
        .boxed()
    }));

    let _laid = lay_out(Navigator::new(handle), loose(800.0));

    let debug = maybe_of_found
        .lock()
        .clone()
        .expect("Overlay::of must have resolved and been formatted");
    assert!(
        debug.contains("mounted: true"),
        "the Navigator's overlay must report itself mounted, got: {debug:?}"
    );
}
