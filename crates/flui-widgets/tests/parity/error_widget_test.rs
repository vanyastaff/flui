//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/slivers_test.dart` (tag
//! `3.44.0`) — `'Can override ErrorWidget.build'`, one of the file's 38
//! `testWidgets` cases (see `sliver_ignore_pointer_test.rs`'s module doc for
//! the full 38-row closeout map).
//!
//! ## The oracle case is narrower than it looks
//!
//! Read by BODY, not by title: the oracle never calls `tester.pumpWidget`
//! with a real tree. It installs a custom `ErrorWidget.builder`, builds a
//! bare `SliverChildBuilderDelegate` whose `itemBuilder` throws the string
//! `'builder'`, and calls `builderThrowsDelegate.build(_NullBuildContext(),
//! 0)` **directly** — a mechanism-level unit test of
//! `SliverChildBuilderDelegate.build`'s own internal try/catch
//! (`widgets/scroll_delegate.dart`, tag `3.44.0`), not an end-to-end pump
//! scenario. It asserts the delegate's return value wraps the custom
//! `errorText` widget and that `tester.takeException()` reports `'builder'`
//! (the error was recorded, not silently swallowed), then restores the
//! previous builder.
//!
//! ## FLUI's equivalent surface
//!
//! FLUI's analogous contract is [`flui_view::set_error_view_builder`] (the
//! global, application-configurable factory) plus
//! `build_or_recover`'s `catch_unwind`
//! (`crates/flui-view/src/element/behavior_commons.rs`) — the general
//! build-panic-recovery path every `StatelessView`/`StatefulView`/
//! `LayoutBuilderElement` build goes through (Flutter parity:
//! `ComponentElement.performRebuild`'s own try/catch,
//! `framework.dart:5810-5859`).
//!
//! That general contract — a custom builder installed, a panicking build
//! substituted with the custom error view instead of unwinding the frame,
//! the panic payload threaded through as [`flui_view::FlutterError`] — is
//! already exhaustively covered at the element-tree level by
//! `crates/flui-view/tests/error_view_recovery.rs`
//! (`stateless_build_panic_substitutes_registered_error_view` and its
//! siblings: default-builder fallback, `StatefulView` builds, nested-subtree
//! isolation, dirty-heap cleanliness, repeatability). This file does not
//! duplicate that suite. It adds exactly one thing `error_view_recovery.rs`
//! cannot reach: `error_view_recovery.rs` drives `ElementTree`/`BuildOwner`
//! directly and never lays out or produces a `RenderId`, so it cannot prove
//! the recovery survives the FULL `flui-widgets` headless frame (mount →
//! build → **layout**) — the actual object under test in this crate's
//! parity suite. [`panicking_build_survives_a_full_pump_and_lays_out_the_registered_error_view`]
//! below proves exactly that, through `harness::pump_widget`/`LaidOut`, by
//! making the custom builder return a render-observable
//! [`flui_widgets::SizedBox`] (a distinctive size) instead of the default
//! `ErrorView` — so the assertion is on real, laid-out geometry, not a
//! stubbed element-tree walk.
//!
//! ## The one piece of the oracle's contract that is NOT portable
//!
//! Which build paths route through `build_or_recover`? Exactly three call
//! sites —
//! `StatelessElement`/`StatefulElement` (`element/behavior.rs`) and
//! `LayoutBuilderElement` (`element/layout_builder.rs`). A lazy
//! `SliverList`/`SliverGrid` **item-builder closure** — the literal
//! mechanism the oracle's `SliverChildBuilderDelegate.itemBuilder` case
//! exercises — is NOT one of them: `SliverListAdaptorManager::service`
//! (`crates/flui-view/src/element/sliver_adaptor.rs`) calls
//! `(self.builder)(logical_index)` directly, and `SparseChildren`'s own
//! `refresh_resident` calls the builder the same unguarded way
//! (`crates/flui-view/src/element/sparse_children.rs`) — neither site is
//! wrapped in `catch_unwind` anywhere in the call chain (confirmed by an
//! exhaustive grep for `catch_unwind` across `flui-view`/`flui-widgets`:
//! every production hit is `build_or_recover`, the observer-callback guards
//! and the build-half restore guard in `owner/build_owner.rs`, the
//! observation-emit guard in `owner/mod.rs`, or the replay guard in
//! `binding.rs` — none of them on this path).
//!
//! Worth naming explicitly since it looks related at a glance but is not:
//! `flui-rendering`'s layout walk (`pipeline/owner/subtree_arena.rs`) DOES
//! wrap `perform_layout_raw` in its own `catch_unwind`, converting a panic
//! into `RenderError::Poisoned` and isolating it (the poisoned node reports
//! `Size::ZERO`/its sliver equivalent and stays marked for a next-frame
//! retry, without failing the frame). That net is real, but it is on a
//! DIFFERENT call stack — `ChildManager::service` runs during
//! `BuildOwner::service_child_requests`, a BUILD-phase pass, not from inside
//! any `perform_layout`. Confirmed empirically, not just by inspection: an
//! initial `harness::pump_widget` alone never invokes the item builder at
//! all (`RenderSliverList` commits its first geometry from the item-extent
//! ESTIMATE, with zero builder calls); the builder only runs once the lazy
//! children are serviced (inside the frame's layout↔build fixpoint), and
//! that service pass runs every builder call under a per-item
//! `catch_unwind` (`build_item_or_error`) — the same boundary Flutter's
//! `SliverChildBuilderDelegate.build` puts around `builder(context, index)`.
//! A panicking item builder therefore yields the registered error view at
//! exactly that index, as a render-owning `RenderErrorBox` row, and leaves
//! every other index untouched;
//! [`sliver_list_item_builder_panic_becomes_an_error_box_at_that_index`]
//! is the oracle of that recovery (it replaced the `#[should_panic]` canary
//! that pinned the earlier gap, where the builder was called bare).
//!
//! ## Disposition
//!
//! **Oracle case 'Can override ErrorWidget.build': accounted, not directly
//! ported.** The exact mechanism it exercises (a lazy-delegate item-builder
//! catch) has no FLUI equivalent at all (gap, pinned above); the broader
//! contract the case's TITLE names (an overridable `ErrorWidget.build`) is
//! fully supported and already covered by `error_view_recovery.rs`, extended
//! here with one genuinely new full-pipeline assertion.

use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_types::Size;
use flui_types::geometry::px;
use flui_view::{
    BuildContext, FlutterError, IntoView, StatelessView, View, clear_error_view_builder,
    set_error_view_builder,
};
use flui_widgets::{SizedBox, Text};

use crate::common;
use crate::harness;

/// Serializes `ERROR_VIEW_BUILDER` mutations across every file under
/// `tests/parity/` that installs a custom builder. `cargo nextest` gives
/// every `#[test]` fn its own process (no cross-test race is even possible
/// there — see this crate's `AGENTS.md` testing-quirks note), but the
/// `cargo test` fallback runs the whole `parity` binary single-process,
/// multi-threaded; a scoped guard plus an explicit restore at the end of
/// every test that installs one is a defensive no-op under nextest and a
/// real safety net under the fallback — same precedent
/// `crates/flui-view/tests/error_view_recovery.rs` already established for
/// the identical global. `pub(crate)` so sibling files (currently
/// `directionality_test.rs`) share the SAME lock instead of racing a second,
/// independent one.
static ERROR_BUILDER_GUARD: Mutex<()> = Mutex::new(());

pub(crate) fn acquire_error_builder_guard() -> std::sync::MutexGuard<'static, ()> {
    match ERROR_BUILDER_GUARD.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// A distinctive size the custom error-view builder below returns, so a
/// laid-out geometry check can tell "the registered builder's replacement
/// mounted and was laid out" apart from "the built-in default `ErrorView`
/// mounted" or "nothing recovered at all" (which would have panicked the
/// whole test instead of producing a `LaidOut` to assert against).
const CUSTOM_ERROR_VIEW_EXTENT: f32 = 42.0;

static CUSTOM_BUILDER_HITS: AtomicUsize = AtomicUsize::new(0);

/// Registered via [`set_error_view_builder`]: records that it ran and
/// returns a render-observable [`SizedBox`] instead of the default
/// `ErrorView` (which paints nothing measurable — see this file's module
/// doc for why that matters here).
fn custom_error_view_builder(_error: &FlutterError) -> Box<dyn View> {
    CUSTOM_BUILDER_HITS.fetch_add(1, Ordering::SeqCst);
    Box::new(SizedBox::new(
        CUSTOM_ERROR_VIEW_EXTENT,
        CUSTOM_ERROR_VIEW_EXTENT,
    ))
}

/// A stateless view whose `build()` always panics — mirrors
/// `error_view_recovery.rs`'s own `PanickingView` fixture (duplicated here
/// deliberately: the two files are separate crates' test targets and share
/// no test-only code today).
#[derive(Clone)]
struct PanickingView {
    message: &'static str,
}

impl StatelessView for PanickingView {
    // `!` does not implement `IntoView` — bind the panic through an
    // explicit `Box<dyn View>` so inference anchors a concrete
    // `IntoView`-satisfying type. The line still panics (the assignment
    // never completes); the bind exists purely for that anchor.
    #[expect(
        unreachable_code,
        unused_variables,
        clippy::diverging_sub_expression,
        reason = "panic body — see comment above"
    )]
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        let v: Box<dyn View> = panic!("{}", self.message);
        v
    }
}

impl View for PanickingView {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

// ============================================================================
// The ported contract — custom builder wins, no unwind, survives a full pump
// ============================================================================

/// Flutter parity: `slivers_test.dart` `'Can override ErrorWidget.build'`
/// (tag `3.44.0`) — the CONTRACT the case's title names (an overridable
/// error view, installed globally, substituted for a panicking build), not
/// its narrow `SliverChildBuilderDelegate`-internal mechanism (see module
/// doc).
///
/// `PanickingView` is pumped as the tree ROOT through the real
/// `flui-widgets` headless frame. Because `PanickingView` is itself a
/// composite with no render node of its own, a passing recovery makes the
/// substituted [`SizedBox`] the render-tree root — so asserting its laid-out
/// size proves three things at once: the custom builder ran (exactly once),
/// its replacement view is what actually got reconciled into the tree (not
/// just constructed and discarded), and layout completed normally on it
/// (the frame never unwound — `harness::pump_widget` would have propagated
/// the panic and failed this test outright otherwise).
#[test]
fn panicking_build_survives_a_full_pump_and_lays_out_the_registered_error_view() {
    let _guard = acquire_error_builder_guard();
    CUSTOM_BUILDER_HITS.store(0, Ordering::SeqCst);
    set_error_view_builder(custom_error_view_builder);
    // Restore on EVERY exit path: a failing assertion below would otherwise
    // leak the custom builder into the rest of the binary under the
    // single-process cargo-test fallback this file's lock defends against.
    struct RestoreDefaultBuilder;
    impl Drop for RestoreDefaultBuilder {
        fn drop(&mut self) {
            clear_error_view_builder();
        }
    }
    let _restore = RestoreDefaultBuilder;

    let view = PanickingView {
        message: "boom during a full headless frame",
    };
    // Loose, not `harness::screen()`'s tight 800x600: a tight incoming
    // constraint would force the substituted `SizedBox` to fill the whole
    // screen (its own `constrain()` clamps a smaller request up to a tight
    // bound), masking the very distinctive-size signal this test relies on.
    let laid = harness::pump_widget(view, common::loose(800.0));

    assert_eq!(
        CUSTOM_BUILDER_HITS.load(Ordering::SeqCst),
        1,
        "the registered error-view builder must run exactly once"
    );
    assert_eq!(
        laid.size(laid.root()),
        Size::new(px(CUSTOM_ERROR_VIEW_EXTENT), px(CUSTOM_ERROR_VIEW_EXTENT)),
        "the SUBSTITUTED custom error view -- not the built-in default -- must \
         be the thing that actually got mounted and laid out"
    );
}

// ============================================================================
// The oracle's own mechanism: a panicking item builder becomes an error child
// ============================================================================

/// The mechanism `'Can override ErrorWidget.build'` pins, on the lazy path:
/// `SliverChildBuilderDelegate.build`'s own try/catch turns a throwing item
/// builder into an `ErrorWidget` at that index. FLUI's lazy child manager
/// runs every builder call under the same per-item boundary
/// (`build_item_or_error`): the panicking index gets the registered error
/// view — a render-owning `RenderErrorBox`, so it occupies a visible, finite
/// row — and every other index is untouched.
///
/// This used to be a `#[should_panic]` canary of the gap (the builder was
/// called bare and unwound the whole service pass); it is now the oracle of
/// the recovery.
#[test]
fn sliver_list_item_builder_panic_becomes_an_error_box_at_that_index() {
    use std::rc::Rc;

    use flui_objects::ERROR_BOX_FALLBACK_EXTENT;
    use flui_view::{BoxedView, ViewExt};
    use flui_widgets::{CustomScrollView, SliverList};

    const ITEM_EXTENT: f32 = 48.0;
    let builder: Rc<dyn Fn(usize) -> Option<BoxedView>> = Rc::new(|index: usize| {
        if index == 2 {
            panic!("boom in a sliver item builder");
        }
        (index < 10).then(|| Text::new(format!("item{index}")).boxed())
    });
    let root = CustomScrollView::new((SliverList::new(10, ITEM_EXTENT, builder),));
    let mut laid = harness::pump_widget(root, harness::screen());
    crate::common::settle_lazy(&mut laid);

    let error_boxes = laid.find_all_by_render_type("RenderErrorBox");
    assert_eq!(
        error_boxes.len(),
        1,
        "exactly the panicking index gets an error child"
    );
    let error_size = laid
        .try_size(error_boxes[0])
        .expect("the error child must be laid out like any other item");
    assert_eq!(
        error_size.height.get(),
        ERROR_BOX_FALLBACK_EXTENT,
        "under the list's unbounded main axis the error box takes its finite fallback row"
    );
    for text in ["item0", "item1", "item3", "item4"] {
        let id = laid
            .find_text(text)
            .unwrap_or_else(|| panic!("{text} must still be built"));
        assert!(
            laid.try_size(id).is_some_and(|s| s.height.get() > 0.0),
            "{text} must be laid out; a panic at one index does not unwind its neighbours"
        );
    }
    // The error row sits where item 2 would: after items 0 and 1.
    let item3_top = laid
        .absolute_offset(laid.find_text("item3").expect("item3"))
        .dy
        .get();
    let error_top = laid.absolute_offset(error_boxes[0]).dy.get();
    assert!(
        error_top < item3_top,
        "the error child keeps the panicking index's place in the list"
    );
}
