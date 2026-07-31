//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/directionality_test.dart`
//! (tag `3.44.0`), all four cases.
//! - Widget: `packages/flutter/lib/src/widgets/basic.dart` `Directionality`
//!   (an `InheritedWidget`).
//! - FLUI port: [`Directionality`] (`crates/flui-widgets/src/localization/
//!   directionality.rs`), itself a direct wrap of `flui_view::InheritedView`.
//!
//! ## Case ledger
//!
//! 1. `'Directionality'` — ported, real green (see the falsification note on
//!    [`directionality_change_notifies_the_dependent_without_a_widget_tree_rebuild`]):
//!    same test. See "The identical-instance mechanism, translated" below for
//!    why this needed [`Memo`] rather than a bare probe.
//! 2. `'Directionality default'` — ported directly, no substitution: the
//!    oracle itself reads `Directionality.maybeOf` from inside a `Builder`'s
//!    own `build`, exactly FLUI's `BuildContext` seam:
//!    [`directionality_maybe_of_is_none_with_no_ancestor`].
//! 3. `'Directionality.maybeOf'` — **SUBSTITUTED**:
//!    [`directionality_maybe_of_finds_the_nearest_ancestor_and_is_none_above_it`].
//!    See "Why cases 3 and 4 are substituted" below.
//! 4. `'Directionality.of'` — **SUBSTITUTED** for the success half, plus a
//!    **divergence pin** for the failure half, in one function (mirroring the
//!    oracle's single `testWidgets` body):
//!    [`directionality_of_finds_the_nearest_ancestor_and_recovers_from_a_missing_one`].
//!    See "Why cases 3 and 4 are substituted" below.
//!
//! ## The identical-instance mechanism, translated (case 1)
//!
//! The oracle constructs ONE `Builder` (`final Widget inner = Builder(...)`)
//! and reuses that SAME Dart object across all three `pumpWidget` calls.
//! Flutter's `Element.updateChild` has a fast path — `if (child.widget ==
//! newWidget) return child;` — that is an *identity* check (the default
//! `Widget.==` is `identical()`), so `inner`'s own element is never visited
//! by the normal top-down rebuild on any of the three pumps. The log only
//! grows when `Directionality`'s `updateShouldNotify` returns `true`:
//! `InheritedElement.notifyClients` then walks its `_dependents` map
//! directly — a side channel, independent of widget-tree traversal — and
//! marks each dependent element dirty for the next build.
//!
//! FLUI has no implicit identity-based skip. `crates/flui-view/src/element/
//! dispatch.rs`'s `dispatch_view_update` calls `View::should_skip_rebuild`,
//! whose documented DEFAULT is `false` — "always rebuild" — precisely to
//! avoid what `crates/flui-view/src/view/memo.rs`'s module doc calls "the
//! Druid trap" (a blanket `PartialEq` bound poisoning every view type that
//! carries a closure). The opt-in equivalent is [`Memo<V>`]
//! (`crates/flui-view/src/view/memo.rs`), which skips a rebuild when the
//! wrapped view compares `PartialEq`-equal to the previously-stored one.
//! [`DirectionalityProbe`] below implements `PartialEq` by `Arc::ptr_eq` on
//! its shared log, not by the log's contents — content equality would
//! self-deadlock (`should_skip_rebuild` compares the incoming and
//! previously-stored probes, and both wrap clones of the SAME `Arc`, so
//! locking both sides would double-lock one `std::sync::Mutex` on one
//! thread) and, more to the point, pointer identity is the more faithful
//! translation of "the same Dart object reused verbatim": both mechanisms
//! ultimately answer "is this dependent the SAME conceptual widget as
//! before", not "does its data still look the same".
//!
//! Separately, FLUI's `InheritedBehavior::on_view_updated`
//! (`crates/flui-view/src/element/behavior.rs`) confirms the exact side
//! channel the oracle relies on: on `update_should_notify() == true` it
//! calls `owner.schedule_build_for(dep_id, ...)` for every entry in its own
//! `dependents` map — the direct FLUI analogue of `notifyClients` walking
//! `_dependents`, independent of the `Memo` skip decision made when the
//! normal top-down reconcile separately visits the same child slot.
//!
//! ## Why cases 3 and 4 are substituted
//!
//! Both oracle cases capture a `BuildContext` via `GlobalKey.currentContext`
//! **after** `pumpWidget` has fully settled, then call
//! `Directionality.maybeOf`/`Directionality.of` from ordinary test code, not
//! from inside any widget's `build`. `flui-widgets` has no test-harness
//! capability for that shape: `LaidOut` (`crates/flui-widgets/tests/
//! common/mod.rs`) exposes only `RenderId`-keyed navigation (`child`,
//! `only_child`, `find_by_render_type`, …), never an `ElementId` or a
//! `BuildContext` for an arbitrary mounted node. `flui_view::context::
//! ElementBuildContext::for_element(element_id, tree, owner)` DOES exist and
//! is `pub`, but constructing one needs the `Arc<RwLock<ElementTree>>` /
//! `Arc<RwLock<BuildOwner>>` pair that `LaidOut` never surfaces. This is the
//! same category of gap `table_test.rs` already logs as out-of-scope for
//! `'Table widget can be detached and re-attached'` ("no generic
//! GlobalKey-wrapping combinator for an arbitrary `View` was found exposed
//! to tests within this slice's budget").
//!
//! Both cases below substitute a `BuildContext` read taken DURING `build()`,
//! at the same tree position the oracle's `GlobalKey` would have captured —
//! same ancestor-scoping claim, same expected values. What this substitution
//! cannot catch: whether `Directionality::maybe_of`/`Directionality::of`
//! remain valid to call from a `BuildContext` that was captured earlier and
//! is queried again after the frame has already settled (a capability
//! question, not a scoping-algorithm question) — the oracle's `GlobalKey`
//! round-trip specifically exercises that, and nothing in this file does.

use std::sync::{Arc, Mutex};

use flui_types::Size;
use flui_types::geometry::px;
use flui_types::typography::TextDirection;
use flui_view::element::ElementKind;
use flui_view::{FlutterError, clear_error_view_builder, set_error_view_builder};
use flui_widgets::prelude::*;
use flui_widgets::{Directionality, SizedBox};

use crate::common;
use crate::error_widget_test::acquire_error_builder_guard;
use crate::harness;

/// Test-only lock helper: recovers from poisoning instead of the bare
/// `.lock().unwrap()` `clippy::unwrap_used` forbids even in test code (this
/// crate's panic policy gates it workspace-wide, not just production paths).
/// A poisoned `Mutex` here only ever means an assertion or panic fired while
/// a PRIOR guard on the same log was held; recovering the data instead of
/// re-panicking keeps a failure's diagnostics attributable to whichever
/// assertion actually failed first, rather than a second, confusing
/// poison-propagation panic.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ============================================================================
// Case 1 — 'Directionality'
// ============================================================================

/// Flutter's reused `Builder` instance analogue: on every `build()` it reads
/// the ambient [`Directionality`] and appends it to a shared log, mirroring
/// the oracle's `log.add(Directionality.of(context))`.
///
/// See the module doc's "identical-instance mechanism" section for why
/// [`PartialEq`] compares by the log's pointer identity rather than its
/// content.
#[derive(Clone)]
struct DirectionalityProbe {
    log: Arc<Mutex<Vec<TextDirection>>>,
}

impl DirectionalityProbe {
    fn new(log: &Arc<Mutex<Vec<TextDirection>>>) -> Self {
        Self {
            log: Arc::clone(log),
        }
    }
}

impl PartialEq for DirectionalityProbe {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.log, &other.log)
    }
}

impl StatelessView for DirectionalityProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let direction = Directionality::maybe_of(ctx).expect(
            "test wiring: DirectionalityProbe must always be built under a \
             Directionality ancestor",
        );
        lock(&self.log).push(direction);
        SizedBox::shrink()
    }
}

impl View for DirectionalityProbe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// Flutter parity: `directionality_test.dart` `'Directionality'` (tag
/// `3.44.0`).
///
/// Five `pumpWidget` calls over `[Ltr, Ltr, Rtl, Rtl, Ltr]`; the dependent's
/// build log must grow ONLY on an actual direction change: `[Ltr]`, `[Ltr]`
/// (unchanged), `[Ltr, Rtl]`, `[Ltr, Rtl]` (unchanged), `[Ltr, Rtl, Ltr]`.
///
/// Falsification: temporarily changed `DirectionalityProbe`'s `PartialEq` to
/// `|_, _| false` (always different — the "no memoization at all" baseline
/// every other FLUI view gets by default) and re-ran. Captured verbatim:
/// ```text
/// thread '...' panicked at crates/flui-widgets/tests/parity/directionality_test.rs:...:
/// assertion `left == right` failed: re-pumping the SAME direction must not
/// rebuild the dependent -- Directionality's own update_should_notify sees
/// no change and never reaches the dependents side-channel
///   left: [Ltr, Ltr]
///  right: [Ltr]
/// ```
/// i.e. without `Memo`'s skip, FLUI's default "always rebuild on a new
/// parent-provided view" behavior rebuilds the dependent on every single
/// pump — the log grows to `[Ltr, Ltr]` even though the direction never
/// changed. Reverted after confirming.
#[test]
fn directionality_change_notifies_the_dependent_without_a_widget_tree_rebuild() {
    let log: Arc<Mutex<Vec<TextDirection>>> = Arc::new(Mutex::new(Vec::new()));
    let inner = || Memo::new(DirectionalityProbe::new(&log));

    let mut laid = harness::pump_widget(
        Directionality::new(TextDirection::Ltr, inner()),
        harness::screen(),
    );
    assert_eq!(
        *lock(&log),
        vec![TextDirection::Ltr],
        "initial mount must build the dependent exactly once, observing Ltr"
    );

    laid.pump_widget(Directionality::new(TextDirection::Ltr, inner()));
    assert_eq!(
        *lock(&log),
        vec![TextDirection::Ltr],
        "re-pumping the SAME direction must not rebuild the dependent -- \
         Directionality's own update_should_notify sees no change and never \
         reaches the dependents side-channel"
    );

    laid.pump_widget(Directionality::new(TextDirection::Rtl, inner()));
    assert_eq!(
        *lock(&log),
        vec![TextDirection::Ltr, TextDirection::Rtl],
        "changing direction must rebuild the dependent via the InheritedView \
         dependents side-channel"
    );

    laid.pump_widget(Directionality::new(TextDirection::Rtl, inner()));
    assert_eq!(
        *lock(&log),
        vec![TextDirection::Ltr, TextDirection::Rtl],
        "re-pumping the SAME (now Rtl) direction must not rebuild again"
    );

    laid.pump_widget(Directionality::new(TextDirection::Ltr, inner()));
    assert_eq!(
        *lock(&log),
        vec![TextDirection::Ltr, TextDirection::Rtl, TextDirection::Ltr],
        "flipping back to Ltr must rebuild the dependent once more"
    );
}

// ============================================================================
// Case 2 — 'Directionality default'
// ============================================================================

/// Records every `Directionality::maybe_of(ctx)` read (which may be `None`)
/// during `build()`. Used for case 2 directly and case 3's substitution.
#[derive(Clone)]
struct MaybeDirectionalityProbe {
    log: Arc<Mutex<Vec<Option<TextDirection>>>>,
}

impl MaybeDirectionalityProbe {
    fn new(log: &Arc<Mutex<Vec<Option<TextDirection>>>>) -> Self {
        Self {
            log: Arc::clone(log),
        }
    }
}

impl StatelessView for MaybeDirectionalityProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        lock(&self.log).push(Directionality::maybe_of(ctx));
        SizedBox::shrink()
    }
}

impl View for MaybeDirectionalityProbe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// Flutter parity: `directionality_test.dart` `'Directionality default'`
/// (tag `3.44.0`). Ported directly — the oracle already reads
/// `Directionality.maybeOf` from inside a `Builder`'s own `build`, so no
/// substitution is needed.
///
/// Falsification: temporarily changed the assertion's expected value from
/// `vec![None]` to `vec![Some(TextDirection::Ltr)]` and re-ran. Captured
/// verbatim:
/// ```text
/// thread '...' panicked at crates/flui-widgets/tests/parity/directionality_test.rs:...:
/// assertion `left == right` failed: with no Directionality ancestor at all,
/// maybe_of must return None
///   left: [None]
///  right: [Some(Ltr)]
/// ```
/// Reverted after confirming.
#[test]
fn directionality_maybe_of_is_none_with_no_ancestor() {
    let log: Arc<Mutex<Vec<Option<TextDirection>>>> = Arc::new(Mutex::new(Vec::new()));
    let _laid = harness::pump_widget(MaybeDirectionalityProbe::new(&log), harness::screen());
    assert_eq!(
        *lock(&log),
        vec![None],
        "with no Directionality ancestor at all, maybe_of must return None"
    );
}

// ============================================================================
// Case 3 — 'Directionality.maybeOf' — SUBSTITUTED
// ============================================================================

/// The oracle's outer `Container(key: noDirectionality)` analogue: records
/// `maybe_of` on its OWN `BuildContext` (no `Directionality` ancestor
/// reaches here — this widget IS the tree root) and then descends into a
/// `Directionality(Rtl)` subtree wrapping [`MaybeDirectionalityProbe`], the
/// oracle's `Container(key: hasDirectionality)` analogue.
#[derive(Clone)]
struct OuterMaybeDirectionalityProbe {
    outer_log: Arc<Mutex<Vec<Option<TextDirection>>>>,
    inner_log: Arc<Mutex<Vec<Option<TextDirection>>>>,
}

impl StatelessView for OuterMaybeDirectionalityProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        lock(&self.outer_log).push(Directionality::maybe_of(ctx));
        // TWO nested providers with conflicting directions. With only one in
        // the tree, an implementation that returned *any* matching ancestor
        // would satisfy the inner assertion, so the test could not support a
        // "nearest" claim. The outer Ltr is what makes the inner Rtl result
        // evidence of nearest-wins rather than found-something.
        Directionality::new(
            TextDirection::Ltr,
            Directionality::new(
                TextDirection::Rtl,
                MaybeDirectionalityProbe::new(&self.inner_log),
            ),
        )
    }
}

impl View for OuterMaybeDirectionalityProbe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// Flutter parity: `directionality_test.dart` `'Directionality.maybeOf'`
/// (tag `3.44.0`). **SUBSTITUTED** — see the module doc's "Why cases 3 and 4
/// are substituted" section.
///
/// Oracle shape: `Container(key: noDirectionality, child: Directionality(
/// RTL, child: Container(key: hasDirectionality)))`, then
/// `Directionality.maybeOf(noDirectionality.currentContext!)` is `null` and
/// `Directionality.maybeOf(hasDirectionality.currentContext!)` is `RTL`.
/// This test reproduces the identical tree SHAPE and the identical
/// assertions, reading each `BuildContext` during its own `build()` instead
/// of via a captured `GlobalKey.currentContext`.
///
/// Falsification: temporarily swapped the outer/inner expected vectors
/// (`outer_log` expecting `Some(Rtl)`, `inner_log` expecting `None`) and
/// re-ran. Captured verbatim (first assertion to fail):
/// ```text
/// thread '...' panicked at crates/flui-widgets/tests/parity/directionality_test.rs:...:
/// assertion `left == right` failed: outside the Directionality subtree,
/// maybe_of must return None
///   left: [None]
///  right: [Some(Rtl)]
/// ```
/// Reverted after confirming.
#[test]
fn directionality_maybe_of_finds_the_nearest_ancestor_and_is_none_above_it() {
    let outer_log: Arc<Mutex<Vec<Option<TextDirection>>>> = Arc::new(Mutex::new(Vec::new()));
    let inner_log: Arc<Mutex<Vec<Option<TextDirection>>>> = Arc::new(Mutex::new(Vec::new()));
    let _laid = harness::pump_widget(
        OuterMaybeDirectionalityProbe {
            outer_log: Arc::clone(&outer_log),
            inner_log: Arc::clone(&inner_log),
        },
        harness::screen(),
    );
    assert_eq!(
        *lock(&outer_log),
        vec![None],
        "outside the Directionality subtree, maybe_of must return None"
    );
    assert_eq!(
        *lock(&inner_log),
        vec![Some(TextDirection::Rtl)],
        "the NEAREST provider wins: the inner Rtl, not the outer Ltr that also \
         encloses the probe"
    );
}

// ============================================================================
// Case 4 — 'Directionality.of' — SUBSTITUTED (success half) + divergence pin
// (failure half)
// ============================================================================

/// Records every `Directionality::of(ctx)` read during `build()` — the
/// panicking accessor, as opposed to [`MaybeDirectionalityProbe`]'s
/// non-panicking one.
#[derive(Clone)]
struct DirectionalityOfProbe {
    log: Arc<Mutex<Vec<TextDirection>>>,
}

impl StatelessView for DirectionalityOfProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        // `Directionality::of` is evaluated to a local BEFORE the lock is
        // taken. Argument evaluation in `lock(&self.log).push(x)` acquires
        // the `MutexGuard` before evaluating `x` -- if `x` (`of`, the
        // panicking accessor) unwound while that guard was still alive, it
        // would poison `self.log`'s `Mutex` even on the "no ancestor" path
        // this probe exists to exercise. `lock()` recovers from poisoning,
        // but keeping the panicking call OUTSIDE the guard's lifetime is
        // still the honest ordering: the guard should never be alive across
        // code that is expected to panic.
        let direction = Directionality::of(ctx);
        lock(&self.log).push(direction);
        SizedBox::shrink()
    }
}

impl View for DirectionalityOfProbe {
    fn create_element(&self) -> ElementKind {
        ElementKind::stateless(self)
    }
}

/// A distinctive size the custom error-view builder below returns, so a
/// laid-out geometry check can tell "the registered builder's replacement
/// mounted and was laid out" apart from "nothing recovered at all" (which
/// would have panicked this test outright) — same idiom
/// `error_widget_test.rs`'s `CUSTOM_ERROR_VIEW_EXTENT` uses, kept as an
/// independent constant since the two files' error-view builders are
/// unrelated beyond sharing the global slot.
const DIRECTIONALITY_ERROR_VIEW_EXTENT: f32 = 43.0;

/// Captures the [`FlutterError::message`] the registered builder below was
/// called with. `ErrorViewBuilder` is a bare `fn` pointer
/// (`flui_view::view::error::ErrorViewBuilder`), not a closure trait object,
/// so it cannot capture a local `Arc` — a `static`, guarded by the same
/// `acquire_error_builder_guard()` lock `error_widget_test.rs` installs its
/// own builder under, is the idiom every builder-installing test in this
/// binary already uses for exactly this reason.
static DIRECTIONALITY_OF_ERROR_MESSAGE: Mutex<Option<String>> = Mutex::new(None);

fn record_directionality_of_error(error: &FlutterError) -> Box<dyn View> {
    *lock(&DIRECTIONALITY_OF_ERROR_MESSAGE) = Some(error.message.clone());
    Box::new(SizedBox::new(
        DIRECTIONALITY_ERROR_VIEW_EXTENT,
        DIRECTIONALITY_ERROR_VIEW_EXTENT,
    ))
}

/// Flutter parity: `directionality_test.dart` `'Directionality.of'` (tag
/// `3.44.0`). Success half **SUBSTITUTED** (see the module doc); failure
/// half a **divergence pin, asserting FLUI's actual recovery behavior**, not
/// the oracle's synchronous throw.
///
/// Oracle: `Directionality.of(hasDirectionality.currentContext!)` returns
/// `TextDirection.rtl` when an ancestor exists; `Directionality.of(
/// noDirectionality.currentContext!)` throws a synchronous `AssertionError`
/// containing `'No Directionality widget found.'` when it does not, caught
/// directly by `expect(() => ..., throwsA(...))` — the call happens in plain
/// test code, outside any widget's `build`.
///
/// FLUI's `Directionality::of` panics via `.expect(...)` on the identical
/// "no ancestor" condition (`crates/flui-widgets/src/localization/
/// directionality.rs`), but calling it FROM plain test code is exactly the
/// missing capability the module doc's "Why cases 3 and 4 are substituted"
/// section describes — there is no `BuildContext` obtainable outside
/// `build()` in this harness at all. Calling it DURING build (the only
/// place it CAN be called here) does not reproduce the oracle's synchronous
/// throw either: every `StatelessView::build` in FLUI runs under
/// `build_or_recover`'s `catch_unwind` (`crates/flui-view/src/element/
/// behavior_commons.rs`), which substitutes the registered error view
/// instead of unwinding the frame — the same mechanism
/// `error_widget_test.rs` exercises for a different panicking build. So the
/// failure half below pins FLUI's REAL, current behavior (recovery via
/// `ErrorView`, carrying `Directionality::of`'s own panic message) rather
/// than asserting the oracle's unreachable synchronous-throw shape.
///
/// Falsification (failure half): temporarily wrapped the "no ancestor" probe
/// in `Directionality::new(TextDirection::Ltr, ..)` (giving it an ancestor
/// after all, so `Directionality::of` cannot panic) and re-ran. Captured
/// verbatim (the FIRST assertion to trip, not the geometry one below it --
/// with a real ancestor `of` succeeds and pushes `Ltr`, so the log is no
/// longer empty):
/// ```text
/// thread '...' panicked at crates/flui-widgets/tests/parity/directionality_test.rs:...:
/// Directionality::of must have panicked before pushing anything -- the
/// probe never gets a value to log
/// ```
/// confirming the probe's `build()` really did run to completion (and the
/// error-view machinery below it was never exercised) once the panic
/// condition was removed. Reverted after confirming.
#[test]
fn directionality_of_finds_the_nearest_ancestor_and_recovers_from_a_missing_one() {
    // Success half: an ancestor exists -- `of` returns it directly, matching
    // the oracle's `Directionality.of(hasDirectionality.currentContext!) ==
    // TextDirection.rtl` assertion.
    let ok_log: Arc<Mutex<Vec<TextDirection>>> = Arc::new(Mutex::new(Vec::new()));
    // Nested providers with conflicting directions: with a single provider the
    // assertion would hold for any implementation that returned some matching
    // ancestor, which is not what "nearest" claims.
    let _laid_ok = harness::pump_widget(
        Directionality::new(
            TextDirection::Ltr,
            Directionality::new(
                TextDirection::Rtl,
                DirectionalityOfProbe {
                    log: Arc::clone(&ok_log),
                },
            ),
        ),
        harness::screen(),
    );
    assert_eq!(
        *lock(&ok_log),
        vec![TextDirection::Rtl],
        "Directionality::of must return the NEAREST ancestor's direction — the \
         inner Rtl, not the outer Ltr"
    );

    // Failure half: no ancestor. See this test's doc comment for why this
    // asserts FLUI's build-panic-recovery substitution rather than the
    // oracle's synchronous throw.
    let _guard = acquire_error_builder_guard();
    *lock(&DIRECTIONALITY_OF_ERROR_MESSAGE) = None;
    set_error_view_builder(record_directionality_of_error);
    struct RestoreDefaultBuilder;
    impl Drop for RestoreDefaultBuilder {
        fn drop(&mut self) {
            clear_error_view_builder();
        }
    }
    let _restore = RestoreDefaultBuilder;

    let no_ancestor_log: Arc<Mutex<Vec<TextDirection>>> = Arc::new(Mutex::new(Vec::new()));
    // Loose, not `harness::screen()`'s tight 800x600: a tight incoming
    // constraint would force the substituted `SizedBox` to fill the whole
    // screen, masking the distinctive-size signal this assertion relies on
    // (same reasoning as `error_widget_test.rs`'s identical `common::loose`
    // choice).
    let laid_missing = harness::pump_widget(
        DirectionalityOfProbe {
            log: Arc::clone(&no_ancestor_log),
        },
        common::loose(800.0),
    );

    assert!(
        lock(&no_ancestor_log).is_empty(),
        "Directionality::of must have panicked before pushing anything -- \
         the probe never gets a value to log"
    );
    assert_eq!(
        laid_missing.size(laid_missing.root()),
        Size::new(
            px(DIRECTIONALITY_ERROR_VIEW_EXTENT),
            px(DIRECTIONALITY_ERROR_VIEW_EXTENT)
        ),
        "the panicking build must be caught and substituted with the \
         registered error view, not unwind the whole frame"
    );
    let message = lock(&DIRECTIONALITY_OF_ERROR_MESSAGE).clone();
    assert!(
        message
            .as_deref()
            .is_some_and(|m| m.contains("no Directionality ancestor")),
        "the captured FlutterError must carry Directionality::of's own panic \
         message, not some other failure -- got: {message:?}"
    );
}
