//! `Hero` / `HeroController` parity — cases anchored to named upstream
//! `testWidgets` descriptions.
//!
//! Flutter source: `packages/flutter/test/widgets/heroes_test.dart` (tag `3.44.0`).
//! Citations below name the upstream test description only, never a line
//! number — line numbers drift as the oracle file changes and the
//! description is what actually identifies the case.
//!
//! FLUI already carries deep `Hero` coverage: `tests/hero_public.rs`
//! (public-API, render-tree observation) and the crate-internal
//! `src/navigator/hero_{tests,flight_tests,controller_tests,gesture_tests,
//! seam_tests}.rs`. All of it is anchored to `heroes.dart` — the
//! *implementation* source — not to `heroes_test.dart`'s own named cases.
//! This file's distinct value, the way `navigator_test.rs` does for
//! `Navigator`/routes, is anchoring each case to a **named upstream
//! `testWidgets` description** and asserting the invariant that oracle
//! itself asserts — not re-deriving behavior already pinned elsewhere.
//!
//! ## Ported cases
//! - `'Heroes animate'` — the load-bearing push-flight case: a shuttle
//!   exists mid-flight and its rect genuinely interpolates between the
//!   source and destination, rather than jumping.
//!   **Adapted:** the oracle observes this through `isOnstage`/`isInCard`
//!   key matchers (`Card`/`Offstage` widget-tree lookups this port's public
//!   render-tree API has no equivalent for); ported onto the shuttle's own
//!   committed box size across frames instead —
//!   [`heroes_animate_the_shuttles_size_interpolates_between_source_and_destination`].
//! - `'Hero push transition interrupted by a pop'` — a push flight
//!   interrupted by a pop reverses without a forward jump, continuing from
//!   wherever the push left it. **Adapted** the same way (no `Card`/`Offstage`
//!   matcher; ported onto the shuttle's committed height, the same signal the
//!   oracle itself samples via `getSize(...).height`). `'Pop interrupts push,
//!   reverses flight'` generalizes the same invariant to *position* under
//!   `MaterialRectArcTween`, which this port carries no equivalent for (no
//!   `MaterialApp`/arc tween) — the height-based, linear-tween version is the
//!   load-bearing subset both oracles agree on —
//!   [`hero_push_transition_interrupted_by_a_pop_retraces_its_path_without_a_jump`].
//! - `'One route, two heroes, same tag, throws'` — **adapted, a documented
//!   divergence** (`src/navigator/hero.rs`'s "Duplicate tags" module doc):
//!   Flutter throws a `FlutterError` inside a debug `assert`
//!   (`heroes.dart:287-305`); FLUI has no debug/release split and reserves
//!   panics for framework invariants, not caller mistakes
//!   (`docs/PANIC-POLICY.md`), so it logs and keeps the *first* registered
//!   hero. This port pins the FLUI-side contract instead of Flutter's
//!   diagnostics tree: no panic, and specifically the *first* declared hero
//!   (not a race, not the second) is the one that flies —
//!   [`two_heroes_sharing_one_tag_the_first_registered_flies_and_no_panic`].
//! - `'Heroes can transition on gesture in one frame'` and `'Heroes do not
//!   transition on back gestures by default'` — a pair flies during a
//!   gesture-driven transition iff **both** ends opt into
//!   `Hero.transitionOnUserGestures`. **Adapted:** the oracles drive a real
//!   finger drag through `TestGesture`; FLUI's public surface has no
//!   drag-driven back-gesture entry point yet (`BackGestureController` is
//!   crate-private — see `src/navigator/hero_gesture_tests.rs` for the
//!   drag-level coverage). These two cases drive the same public hook a real
//!   edge swipe ultimately calls — `NavigatorHandle::did_start_user_gesture`
//!   — directly, and pin the load-bearing claim both oracles share: opting
//!   in on both ends starts exactly one flight within a frame or two (the
//!   opt-in oracle itself checks after a single `tester.pump()`, not before
//!   any); opting into neither starts none across the same window —
//!   [`a_user_gesture_flies_a_hero_pair_when_both_ends_opt_in`],
//!   [`a_user_gesture_flies_nothing_when_the_heroes_did_not_opt_in`].
//! - `'Heroes fly on pushReplacement'` — a replaced top route is still a
//!   top-route *change*, so `HistoryState::flush_inner` fires the same
//!   `Notification::TopChanged` an ordinary push does, and the hero flies
//!   unmodified — no dedicated `push_replacement` wiring exists, and none
//!   was needed —
//!   [`heroes_fly_on_push_replacement`].
//!
//! ## Already covered elsewhere (redundant; citation only, not re-ported)
//! - `'Can push/pop on outer Navigator if nested Navigator contains Heroes'`
//!   and `'Can hero from route in root Navigator to route in nested
//!   Navigator'` — `tests/hero_public.rs`'s nested-navigator block
//!   (`a_hero_flies_from_a_nested_navigators_current_route_to_the_outer_navigator`,
//!   `a_covered_nested_route_does_not_contribute_its_heroes_to_an_outer_flight`,
//!   `a_hero_flies_from_two_levels_of_nested_navigators_to_the_outermost`,
//!   `a_nested_navigator_does_not_fly_heroes_by_default`) already pins the
//!   underlying invariant these two oracles exercise — a hero inside a
//!   nested `Navigator`'s current route is visible to an outer flight, a
//!   push/pop on the outer navigator around it never panics, and a covered
//!   nested route contributes nothing. Re-porting them here would assert the
//!   identical fact through the identical public calls.
//! - Same-tag divert (push during an active flight redirects) — half of
//!   "flight interruption" — is already covered by `tests/hero_public.rs`'s
//!   `a_same_tag_divert_does_not_stack_shuttles` and
//!   `a_divert_rebuilds_the_shuttle_through_the_hook`. This file ports the
//!   other half — a pop interrupting a push — which those two do not cover.
//! - `keepPlaceholder` on a completed flight (`'Heroes animate should hide
//!   original hero'`) is pinned at the unit level by
//!   `src/navigator/hero_tests.rs::end_flight_keep_placeholder_keeps_placeholder`,
//!   which reads `HeroHandle::placeholder_size()` directly. Porting that
//!   oracle's own assertion (`Offstage.offstage == true` on the covered
//!   hero) would need an "is this render node currently painted/offstage"
//!   primitive the shared parity harness (`tests/common/mod.rs`) does not
//!   have — only render-type/size/offset queries exist, which cannot
//!   distinguish a mounted-but-offstage node from an onstage one. Not ported
//!   for that reason; the unit test above is the load-bearing evidence.
//!
//! ## Not ported
//! - `'Heroes still animate after hero controller is swapped.'` — not
//!   because of the declarative-route-table scaffolding the oracle happens
//!   to use (that part is incidental and would be portable through this
//!   file's existing imperative `seed_initial`/`push` pattern instead of
//!   `onGenerateRoute`); the real reason is structural. `HeroControllerScope`'s
//!   own module doc records that a `Navigator` resolves the ambient scope
//!   **once**, in `init_state` (`hero_controller_scope.rs`: "A Navigator
//!   reads the controller once... a changed controller is not picked up
//!   mid-life — a deliberate read-once simplification"). A later scope swap
//!   is therefore a structural no-op for an already-mounted `Navigator`, and
//!   an in-flight flight — an independently ticking overlay entry, not owned
//!   by "whichever controller happens to be ambient" — is never at risk from
//!   it. Probed directly (not just inferred from the doc comment): a flight
//!   sampled `[23.08, 36.87]` mid-push, survived a `pump_widget` root-swap to
//!   a fresh `HeroControllerScope`/`HeroController` pair, and continued
//!   `[49.96, 56.18, 59.01, 59.97, 60.0]` to a clean landing with no jump, no
//!   restart, and no leaked shuttle. There is no code path a swap could
//!   reach that would make a port of this oracle fail today, and the one
//!   internal signal that would
//!   distinguish "still tracked by the old controller" from "silently
//!   dropped" — `HeroController::flights()` — is `pub(crate)`, unreachable
//!   from this file. A test with no way to go red is not a test; not ported
//!   for that reason.
//! - `'Heroes should unhide if no animation'` — exercises the declarative
//!   `Navigator(pages:, onDidRemovePage:)` surface plus `showDialog`
//!   (Material-specific); FLUI's `Navigator` is imperative-only (documented
//!   gap, `navigator_test.rs`'s "Not ported").
//! - `'Destination hero is rebuilt midflight'` — the oracle body contains no
//!   `expect(...)` calls at all; it is a bare "does not crash when the
//!   destination route calls `setState` mid-flight" smoke test. The general
//!   robustness it checks is already the implicit contract every test in
//!   this file and `hero_public.rs` depends on to pass at all, so porting it
//!   would add a case with nothing of its own to assert.
//! - `'Hero within a Hero, throws'` and its three subtree variants
//!   (`'Hero within a Hero subtree, throws'`, `'Hero within a Hero subtree
//!   with Builder, throws'`, `'Hero within a Hero subtree with
//!   LayoutBuilder, throws'`) — Flutter's `_HeroState.build` asserts no
//!   `Hero` ancestor exists via an element-tree walk
//!   (`context.findAncestorWidgetOfExactType<Hero>()`). FLUI's `Hero` never
//!   walks ancestors (see `hero.rs`'s module doc: registration replaces the
//!   element walk by design) and carries no such detection at all — porting
//!   this would mean building new production detection machinery, not
//!   translating existing behavior, which is out of this task's boundary.
//! - `'Can push/pop on outer Navigator if nested Navigators contains same
//!   Heroes'` — built on `CupertinoTabScaffold`/`CupertinoTabView`
//!   (tab-based nested navigation), which FLUI's widget catalog does not
//!   have.
//! - `'Default Hero animation is fastOutSlowIn'`, `'Heroes animation curve is
//!   customizable with curve=$curve'` (the parameterised curve suite),
//!   `'Default popped hero uses fastOutSlowIn curve'` and its variants
//!   (`'popped hero curve is customizable with reverseCurve=$reverseCurve'`,
//!   `'popped hero uses flipped curve=$curve when reverseCurve is not
//!   specified'`) — flight easing curves are already pinned
//!   crate-internally against `heroes.dart` line references in
//!   `src/navigator/hero_flight_tests.rs`'s "Flight easing" block; no
//!   `TransitionDurationObserver` equivalent exists on the public surface to
//!   port the oracle's own timing-observation mechanism.
//! - `'On an iOS back swipe and snap, only a single flight should take
//!   place'`, `"From hero's state should be preserved..."`, `"Hero works
//!   with images..."`, `'Check if previous page is laid out on backswipe
//!   gesture before flight'` — all drive a real finger drag through
//!   `TestGesture`/`CupertinoPageRoute`'s edge-swipe detector; FLUI's public
//!   surface has no drag entry point (see the `did_start_user_gesture`
//!   adaptation above) and no `CupertinoPageRoute`. The single-flight-per-
//!   gesture invariant the first of these pins is exercised at the drag
//!   level by `src/navigator/hero_gesture_tests.rs`.
//! - `'Can add two page with heroes simultaneously using page API.'` and
//!   `'Can still trigger hero even if page underneath changes'` — both are
//!   the declarative Page API this crate does not have. (`'Heroes fly on
//!   pushReplacement'`, the other case originally grouped here, turned out
//!   to need no port-time adaptation at all — see "Ported cases" above.)
//! - `'kept alive Hero does not throw when the transition begins'`, `'toHero
//!   becomes unpaintable after the transition begins'`, `'diverting to a
//!   keepalive but unpaintable hero'` — all three depend on
//!   `AutomaticKeepAliveClientMixin`/`Visibility(maintainState: true,
//!   visible: false)` inside a `PageView`, machinery this port does not
//!   have.
//! - `'smooth transition between different incoming data'` and `'Hero does
//!   not crash at zero area'` — both are `Image` specific (`RenderImage`,
//!   custom `ImageProvider`), which this catalog slice does not exercise
//!   through `Hero`.
//! - `'Heroes are not interactive'` — tap-suppression during flight. Every
//!   existing shuttle test already relies on the shuttle being wrapped in
//!   `IgnorePointer` (`heroes.dart:594`, the very render type
//!   `find_all_by_render_type("RenderIgnorePointer")` scans for across this
//!   whole corpus); asserting it via an actual pointer dispatch mid-flight is
//!   deferred rather than faked here.
//! - `'Popping on first frame does not cause hero observer to crash'`,
//!   `'Overlapping starting and ending a hero transition works ok'`,
//!   `'Handles transitions when a non-default initial route is set'`,
//!   `'Remove user gesture driven flights when the gesture is invalid'`,
//!   `'In a pop transition, when fromHero is null, the to hero should
//!   eventually become visible'` — narrow regression tests for specific
//!   historical Flutter issues with no corresponding FLUI history; the
//!   general robustness they check (no panic across an interruption/timing
//!   edge) is already the implicit contract every test in this file and
//!   `hero_public.rs` depends on to pass at all.
//!
//! Widget → type mapping: `Hero` is FLUI's own type of the same name; a
//! flight's shuttle is observed the only way public API allows — scanning
//! the render tree for the shuttle's `RenderIgnorePointer` (`heroes.dart:594`),
//! exactly as `tests/hero_public.rs` does.

use std::time::Duration;

use crate::common::{LaidOut, lay_out_animated, tight};
use flui_animation::Vsync;
use flui_widgets::VsyncScope;
use flui_widgets::prelude::*;

const TRANSITION: Duration = Duration::from_millis(100);
const FRAME: Duration = Duration::from_millis(16);
/// Enough 16 ms frames to run a 100 ms transition to completion, twice over
/// — matching `tests/hero_public.rs`'s `SETTLE`.
const SETTLE: usize = 16;

/// A `Navigator` whose route transitions tick against `vsync`, with zero
/// boilerplate hero wiring — the Navigator creates its own default
/// `HeroController`, exactly as `tests/hero_public.rs`'s `app` helper does.
fn app(vsync: &Vsync, navigator: &NavigatorHandle) -> impl View {
    VsyncScope::new(vsync.clone(), Navigator::new(navigator.clone()))
}

/// A `PageRoute` whose page centres one `Hero` tagged `"shared"`, sized
/// `w`x`h`. Distinct sizes across two pushed pages give a flight a
/// non-degenerate begin/end rect to interpolate between.
fn sized_hero_page(w: f32, h: f32) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Center::new()
            .child(Hero::new(ValueKey::new("shared"), SizedBox::new(w, h)))
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

/// As [`sized_hero_page`], but the hero opts into (or out of) a
/// gesture-driven transition.
fn gesture_hero_page(opt_in: bool, w: f32, h: f32) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Center::new()
            .child(
                Hero::new(ValueKey::new("shared"), SizedBox::new(w, h))
                    .transition_on_user_gestures(opt_in),
            )
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

/// The airborne shuttle's committed height, or `None` when no flight is in
/// progress — the public signal for "where is the shuttle's rect right now"
/// (`heroes.dart:594`'s `IgnorePointer`, the same render type
/// `tests/hero_public.rs`'s shuttle-presence checks count).
///
/// # Panics
///
/// Panics if more than one shuttle is airborne at once — the same "never
/// two, never stacked" invariant `tests/hero_public.rs`'s
/// `a_same_tag_divert_does_not_stack_shuttles` pins, enforced here as a
/// hard assertion so any test using this helper gets it for free.
fn shuttle_height(laid: &LaidOut) -> Option<f32> {
    match laid
        .find_all_by_render_type("RenderIgnorePointer")
        .as_slice()
    {
        [] => None,
        [id] => Some(laid.size(*id).height.0),
        many => panic!("more than one shuttle airborne at once: {many:?}"),
    }
}

/// Pump `frames` and collect the shuttle's height on every frame a flight is
/// airborne (frames with no shuttle yet are skipped, not recorded as gaps).
///
/// **Load-bearing timing note:** every caller that asserts "no flight ever
/// started" from this function's result relies on it actually pumping —
/// an event that merely *schedules* a deferred flight (e.g.
/// `did_start_user_gesture`) produces no shuttle until a frame runs. Always
/// call this (or an explicit `pump_for`) between such an event and any
/// assertion about its effect; asserting immediately after the event with
/// no pump in between cannot distinguish "correctly did nothing" from
/// "correctly scheduled something that just hasn't run yet."
fn track_heights(laid: &mut LaidOut, frames: usize) -> Vec<f32> {
    let mut heights = Vec::new();
    for _ in 0..frames {
        laid.pump_for(FRAME);
        if let Some(height) = shuttle_height(laid) {
            heights.push(height);
        }
    }
    heights
}

/// **A push flight's shuttle interpolates between the source and
/// destination sizes mid-flight** — not merely present, but strictly
/// between the two extremes at some point, and never regressing back toward
/// the source once it has moved.
///
/// Oracle: `'Heroes animate'` — see the module doc for the
/// `isOnstage`/`isInCard` → shuttle-size adaptation.
///
/// Red-check: in `FlightInner::current_rect`, return the *end* rect
/// unconditionally instead of `endpoints.transform(t)` — every sampled
/// height would already equal the destination height and the "some sample
/// strictly between the two" assertion would fail.
#[test]
fn heroes_animate_the_shuttles_size_interpolates_between_source_and_destination() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(sized_hero_page(30.0, 20.0));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);

    let _push = laid.enter_owner_scope(|| navigator.push(sized_hero_page(90.0, 60.0)));
    let heights = track_heights(&mut laid, SETTLE);

    assert!(
        heights.len() >= 3,
        "need several in-flight samples to observe interpolation, got {heights:?}"
    );
    assert!(
        heights.iter().all(|&h| (19.9..=60.1).contains(&h)),
        "every sample must stay within [source, destination]: {heights:?}"
    );
    assert!(
        heights.iter().any(|&h| h > 20.5 && h < 59.5),
        "at least one sample must be strictly between source and destination \
         — proof of interpolation, not a jump straight to the end: {heights:?}"
    );
    for pair in heights.windows(2) {
        assert!(
            pair[1] + 0.05 >= pair[0],
            "height must not regress mid-flight (monotonically closing the \
             gap toward the destination): {pair:?} in {heights:?}"
        );
    }

    assert!(shuttle_height(&laid).is_none(), "the flight landed");
    assert_eq!(navigator.route_ids().len(), 2, "the push completed");
}

/// **A push interrupted by a pop reverses the flight without a jump** — the
/// shuttle continues from wherever the interrupted push left it and heads
/// back toward the source, rather than snapping to a new position.
///
/// Oracle: `'Hero push transition interrupted by a pop'` — see the module
/// doc for the adaptation from `getSize(...).height` under `Card`/`Offstage`
/// matchers to the shuttle's own committed height.
///
/// Red-check: in `HeroFlight::divert`'s `(Push, Pop)` branch (`hero_flight.rs`),
/// drop the endpoint swap — leave `new_begin = rect.begin; new_end =
/// rect.end;` instead of the real `new_begin = rect.end; new_end =
/// rect.begin;` — the reversed proxy animation then reads its (still
/// continuous) progress against the *original*, un-swapped endpoint pairing,
/// so the very next sampled height jumps instead of continuing smoothly from
/// `interrupted_at`.
#[test]
fn hero_push_transition_interrupted_by_a_pop_retraces_its_path_without_a_jump() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(sized_hero_page(30.0, 20.0));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);

    let _push = laid.enter_owner_scope(|| navigator.push(sized_hero_page(90.0, 60.0)));
    let forward = track_heights(&mut laid, 4);
    assert!(
        !forward.is_empty(),
        "the push flight must be airborne before it gets interrupted"
    );
    let interrupted_at = *forward.last().expect("checked non-empty above");
    assert!(
        (20.0..=60.0).contains(&interrupted_at),
        "interrupted while still between the two endpoints: {interrupted_at}"
    );

    assert!(
        laid.enter_owner_scope(|| navigator.pop()),
        "the pop is accepted mid-flight"
    );
    laid.pump_for(FRAME);
    let just_after_pop =
        shuttle_height(&laid).expect("the reversed flight is still airborne right after the pop");
    assert!(
        just_after_pop <= interrupted_at + 0.5,
        "the reversed flight continues from where the push left off, with no \
         forward jump: interrupted_at={interrupted_at} just_after_pop={just_after_pop}"
    );

    let backward = track_heights(&mut laid, SETTLE);
    for pair in backward.windows(2) {
        assert!(
            pair[1] <= pair[0] + 0.05,
            "heading back toward the source must not regress toward the \
             destination: {pair:?} in {backward:?}"
        );
    }

    assert!(
        shuttle_height(&laid).is_none(),
        "the reversed flight landed"
    );
    assert_eq!(navigator.route_ids().len(), 1, "back on the source route");
}

/// **Two heroes sharing one tag within a single route: the first registered
/// flies, and FLUI does not panic.**
///
/// Oracle: `'One route, two heroes, same tag, throws'` — see the module doc
/// for why this pins FLUI's documented divergence (log + first-wins) instead
/// of reproducing Flutter's `FlutterError` diagnostics tree.
///
/// Red-check (mutation actually run): in `HeroRegistry::register`, drop the
/// `if heroes.contains_key(&tag) { return false; }` guard so a later
/// registration under the same tag *replaces* the earlier one instead of
/// being rejected. A plain `HashMap::insert` on an existing key overwrites,
/// it cannot duplicate one, so `shuttle_height`'s "more than one shuttle"
/// panic never fires either way — the map always holds exactly one hero.
/// What flips is *which* one: with the guard gone it is the **second**
/// declared hero (source height 25.0) that survives and flies, not the
/// first (20.0). Measured with the mutation applied: `first_airborne` reads
/// `27.7` (vs `23.1` on unmodified code) — comfortably on either side of the
/// `25.0` threshold below, so the assertion discriminates the two.
#[test]
fn two_heroes_sharing_one_tag_the_first_registered_flies_and_no_panic() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(PageRoute::<i32>::new(|_ctx, _p, _s| {
        Center::new()
            .child(Stack::new(vec![
                Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 20.0))
                    .into_view()
                    .boxed(),
                Hero::new(ValueKey::new("shared"), SizedBox::new(40.0, 25.0))
                    .into_view()
                    .boxed(),
            ]))
            .into_view()
            .boxed()
    }));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);

    let _push = laid.enter_owner_scope(|| navigator.push(sized_hero_page(90.0, 60.0)));
    let heights = track_heights(&mut laid, SETTLE);

    assert!(
        !heights.is_empty(),
        "exactly one hero registered under the shared tag, so one flight still runs"
    );
    let first_airborne = heights[0];
    assert!(
        first_airborne < 25.0,
        "the FIRST declared hero (source height 20.0) is the one that \
         registered and flew — not the second (source height 25.0): \
         first_airborne={first_airborne}"
    );
    assert!(shuttle_height(&laid).is_none(), "the single flight landed");
    assert_eq!(
        navigator.route_ids().len(),
        2,
        "the push completed without panicking"
    );
}

/// **A user gesture flies a hero pair only when both ends opt in** —
/// `Hero.transitionOnUserGestures`.
///
/// Oracle: `'Heroes can transition on gesture in one frame'` — see the
/// module doc for the `did_start_user_gesture` adaptation (no public drag
/// entry point yet).
///
/// Red-check: delete `HeroController`'s `NavigatorObserver::did_start_user_gesture`
/// impl (or its `self.maybe_start(Some(route), previous, true)` call) — no
/// flight is ever launched, and `shuttle_height` stays `None` even after the
/// extra pump this test allows.
#[test]
fn a_user_gesture_flies_a_hero_pair_when_both_ends_opt_in() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(gesture_hero_page(true, 30.0, 20.0));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);

    let _push = laid.enter_owner_scope(|| navigator.push(gesture_hero_page(true, 60.0, 45.0)));
    track_heights(&mut laid, SETTLE);
    assert!(
        shuttle_height(&laid).is_none(),
        "the push flight settled first"
    );

    laid.enter_owner_scope(|| navigator.did_start_user_gesture());
    laid.pump_for(FRAME);
    assert!(
        shuttle_height(&laid).is_some(),
        "both ends opted in: the gesture starts a flight within one frame"
    );

    laid.enter_owner_scope(|| navigator.did_stop_user_gesture());
    track_heights(&mut laid, SETTLE);
    assert!(
        shuttle_height(&laid).is_none(),
        "the gesture-driven flight lands cleanly"
    );
}

/// **A user gesture flies nothing when the heroes have not opted in** — the
/// default.
///
/// Oracle: `'Heroes do not transition on back gestures by default'` —
/// adapted the same way as its opt-in counterpart above.
///
/// Red-check: drop `Hero._allHeroesFor`'s `inviteHero` filter —
/// `HeroController::filter_for_gesture` — so every hero (opted in or not)
/// becomes a flight candidate during a gesture. The deferred flight this
/// produces still needs a frame to become observable (see
/// [`track_heights`]'s timing note), but well within the `SETTLE` window
/// this test pumps: `track_heights` would collect a non-empty run of
/// samples instead of staying empty.
#[test]
fn a_user_gesture_flies_nothing_when_the_heroes_did_not_opt_in() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(sized_hero_page(30.0, 20.0));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);

    let _push = laid.enter_owner_scope(|| navigator.push(sized_hero_page(60.0, 45.0)));
    track_heights(&mut laid, SETTLE);
    assert!(
        shuttle_height(&laid).is_none(),
        "the push flight settled first"
    );

    laid.enter_owner_scope(|| navigator.did_start_user_gesture());
    let heights = track_heights(&mut laid, SETTLE);
    assert!(
        heights.is_empty(),
        "neither hero opted into transitionOnUserGestures, so the gesture \
         flies nothing across the whole window a real flight would need to \
         become observable: {heights:?}"
    );
    laid.enter_owner_scope(|| navigator.did_stop_user_gesture());
}

/// **A hero flies on `push_replacement`, the same as an ordinary push.**
///
/// Oracle: `'Heroes fly on pushReplacement'` — a regression test for
/// <https://github.com/flutter/flutter/issues/28041>: a replaced top route is
/// still a top-route *change*, so it still flies its heroes.
///
/// `HeroController` overrides only `did_change_top`
/// (`NavigatorObserver::did_change_top`), not `did_push`/`did_replace`
/// directly — and `HistoryState::flush_inner` (`history.rs`) computes
/// `Notification::TopChanged` purely from whether the top route's
/// *identity* changed (`self.last_topmost != Some(top)`), with no branch on
/// *how* it changed. `push_replacement` reaches the exact same check as an
/// ordinary push. There is no dedicated "fly heroes on push_replacement"
/// wiring, and none is missing — it rides the general mechanism.
///
/// Red-check (mutation actually run): comment out the
/// `notifications.push(Notification::TopChanged { .. })` line in
/// `HistoryState::flush_inner` — every flight in every test in this file
/// stops firing, including this one, because there is no
/// push_replacement-specific code path to selectively break.
#[test]
fn heroes_fly_on_push_replacement() {
    let vsync = Vsync::new();
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(sized_hero_page(30.0, 20.0));
    let mut laid = lay_out_animated(app(&vsync, &navigator), tight(400.0, 400.0), vsync);

    let _replace =
        laid.enter_owner_scope(|| navigator.push_replacement(sized_hero_page(90.0, 60.0)));
    let heights = track_heights(&mut laid, SETTLE);

    assert!(
        heights.len() >= 3,
        "need several in-flight samples to observe interpolation, got {heights:?}"
    );
    assert!(
        heights.iter().any(|&h| h > 20.5 && h < 59.5),
        "at least one sample must be strictly between source and destination \
         — proof of a real flight, not the replacement snapping straight to \
         the new size: {heights:?}"
    );
    assert!(shuttle_height(&laid).is_none(), "the flight landed");
    assert_eq!(
        navigator.route_ids().len(),
        1,
        "the replacement completed — still one route, now the new one"
    );
}
