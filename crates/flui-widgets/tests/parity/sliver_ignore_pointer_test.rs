//! ## Test parity notes
//!
//! Flutter source:
//! - Widget: `packages/flutter/lib/src/widgets/sliver.dart`
//!   `SliverIgnorePointer` (line 1420)
//! - Tests: `packages/flutter/test/widgets/slivers_test.dart` (tag `3.44.0`)
//!   — `group('SliverIgnorePointer - ')` (5 cases) plus the neighboring
//!   `group('SliverEnsureSemantics - ')` (1 case, a different widget,
//!   accounted here for locality — see the closeout map below).
//!
//! Widget → render-object mapping:
//! - `SliverIgnorePointer` → `RenderSliverIgnorePointer` (sliver child of
//!   `RenderViewport`)
//! - The sliver child → `RenderConstrainedBox` (box child via `SizedBox::shrink`)
//!
//! Divergence:
//! - Flutter's `SliverIgnorePointer` has an optional `ignoringSemantics`
//!   field. FLUI's `SliverIgnorePointer` (`crates/flui-widgets/src/scroll/
//!   sliver_ignore_pointer.rs`) carries only the pointer-blocking
//!   `ignoring: bool` — see the Cross.H note below for the full gap writeup
//!   and why it is low-priority.
//! - Flutter verifies pointer routing via `tester.hitTest`/`tester.tap`; FLUI
//!   dispatches synthetic pointer events directly and asserts on tap-counter
//!   side effects (this file's own pre-existing render-node-count tests plus
//!   the new tap-target tests below).
//!
//! Key Flutter parity behavior tested here:
//! - `SliverIgnorePointer` is a pure layout/paint passthrough; it only
//!   short-circuits `hit_test`. The render tree is identical whether
//!   `ignoring = true` or `false` (pre-existing tests below).
//! - `ignoring = true` blocks a wrapped `GestureDetector` from ever
//!   receiving a tap; `ignoring = false` is fully transparent to pointer
//!   routing (new tap-target tests below).
//!
//! Geometry oracle (one child, 800 × 600 surface):
//!   RenderSliverIgnorePointer delegates layout and paint to its single child;
//!   render nodes: 1 RenderViewport + 1 RenderSliverIgnorePointer + 1 child = 3.
//!
//! ## Cross.H — the missing `ignoringSemantics` knob
//!
//! Checked before filing (grepped `docs/ROADMAP.md` for `IgnorePointer`/
//! `ignoringSemantics` — no existing entry) and checked against the actual
//! Flutter 3.44.0 source (`packages/flutter/lib/src/widgets/sliver.dart`,
//! `packages/flutter/lib/src/widgets/basic.dart`), not assumed from a
//! changelog: `SliverIgnorePointer.ignoringSemantics` IS present at this tag
//! but is itself `@Deprecated('Create a custom sliver ignore pointer widget
//! instead. This feature was deprecated after v3.8.0-12.0.pre.')` —
//! deprecated-but-present, not removed, so the oracle still exercises it
//! (the three semantics-only/semantics-plus-pointer cases below). A future
//! FLUI port has no obligation to match a parameter upstream itself steers
//! users away from; the more durable target would be a semantics-tree-scoped
//! mechanism (Flutter's own suggested replacement direction —
//! `SliverSemantics`/`ExcludeSemantics`), neither of which FLUI has ported.
//! Practically subsumed by the standing gap every semantics-adjacent port in
//! this directory already cites (no semantics-tree assembly step in the
//! headless harness to assert against at all) — filed as its own line only
//! so the field-level absence is recorded distinctly, in case a semantics
//! pipeline lands before a `SliverSemantics` port would obsolete the need.
//!
//! ## 38-row closeout map — `slivers_test.dart` @ 3.44.0, fully accounted
//!
//! Every one of the file's 38 `testWidgets` cases, and where it is
//! accounted. Recount: `grep -c "testWidgets(" slivers_test.dart` at this
//! tag = 38; the table below has 38 data rows.
//!
//! | # | Oracle case | Accounted in | Disposition |
//! |---|---|---|---|
//! | 1 | `'Viewport basic test'` | `viewport_test.rs` | ported, real green |
//! | 2 | `'Viewport anchor test'` | `viewport_test.rs` | OOS — no `anchor`/`center` equivalent |
//! | 3 | `'Multiple grids and lists'` | `viewport_test.rs` | ported, real green |
//! | 4 | `'Sliver grid can replace intermediate items'` | `sliver_grid_test.rs` (case 1) | OOS — arbitrary-geometry + per-item-key gaps |
//! | 5 | `'SliverFixedExtentList correctly clears garbage'` | `sliver_fixed_extent_list_test.rs` (case 1) | OOS — needs `.builder` |
//! | 6 | `'SliverFixedExtentList handles underflow…'` | `sliver_fixed_extent_list_test.rs` (case 2) | OOS, `#[ignore]`d pin (verified failing) |
//! | 7 | `'SliverGrid Correctly layout children after rearranging'` | `sliver_grid_test.rs` (case 2) | ported, real green |
//! | 8 | `'SliverGrid negative usableCrossAxisExtent'` | `sliver_grid_test.rs` (case 3) | ported, real green |
//! | 9 | `'SliverList can handle inaccurate scroll offset…'` | `sliver_list_correction_test.rs` (case 1) | ported partial green + `#[ignore]`d pin |
//! | 10 | `'SliverFixedExtentList Correctly layout children after rearranging'` | `sliver_fixed_extent_list_test.rs` (case 3) | ported, real green |
//! | 11 | `'Can override ErrorWidget.build'` | `error_widget_test.rs` | accounted; contract ported, exact mechanism pinned as a gap |
//! | 12 | `'…auto-correct scroll offset - super fast'` | `sliver_fixed_extent_list_test.rs` (case 4) | OOS — needs `.builder` |
//! | 13 | `'…auto-correct scroll offset - reasonable'` | `sliver_fixed_extent_list_test.rs` (case 5) | OOS — needs `.builder` |
//! | 14 | `'offstage true'` | `sliver_offstage_test.rs` (cross-ref) | covered in part (mounted + geometry halves); paint/finder-exclusion halves unported |
//! | 15 | `'offstage false'` | `sliver_offstage_test.rs` (cross-ref) | covered in part (mounted + geometry halves); paint/finder-exclusion halves unported |
//! | 16 | `'painting & semantics'` (`SliverOpacity`) | `sliver_opacity_test.rs` | ported, real green (paint half); semantics half OOS |
//! | 17 | `'ignores pointer events'` | `sliver_ignore_pointer_test.rs` (this file) | ported, real green (pointer half, combined with #20); semantics half OOS |
//! | 18 | `'ignores semantics'` | `sliver_ignore_pointer_test.rs` (this file) | OOS — semantics half unportable; pointer half subsumed by #21 |
//! | 19 | `'ignoring only block semantics actions'` | `sliver_ignore_pointer_test.rs` (this file) | OOS — entirely semantics |
//! | 20 | `'ignores pointer events & semantics'` | `sliver_ignore_pointer_test.rs` (this file) | ported, real green (pointer half, combined with #17); semantics half OOS |
//! | 21 | `'ignores nothing'` | `sliver_ignore_pointer_test.rs` (this file) | ported, real green (pointer half); semantics half OOS |
//! | 22 | `'ensure semantics'` (`SliverEnsureSemantics`) | `sliver_ignore_pointer_test.rs` (this file) | OOS — entirely semantics; different widget, accounted here for locality |
//! | 23 | `'SliverList handles 0 scrollOffsetCorrection'` | `sliver_list_correction_test.rs` (case 2) | ported, real green |
//! | 24 | `'SliverGrid children can be arbitrarily placed'` | `sliver_grid_test.rs` (case 4) | OOS — arbitrary-geometry gap |
//! | 25 | `'SliverFixedExtentList.builder should respect semanticIndexOffset'` | `sliver_fixed_extent_list_test.rs` (case 6) | OOS — needs `.builder` + semantics |
//! | 26 | `'SliverList.builder can build children'` (1st occurrence) | `sliver_list_constructors_test.rs` (case 1) | ported, real green |
//! | 27 | `'SliverList.builder can build children'` (2nd, verbatim duplicate) | `sliver_list_constructors_test.rs` (case 1) | same port as #26, both accounted |
//! | 28 | `'SliverList.separated can build children'` | `sliver_list_constructors_test.rs` (case 2) | ported, real green |
//! | 29 | `'SliverList.separated has correct number of children'` | `sliver_list_constructors_test.rs` (case 3) | ported, real green |
//! | 30 | `'SliverList.list can build children'` (1st, real `SliverList.list`) | `sliver_list_constructors_test.rs` (case 4) | ported, real green |
//! | 31 | `'SliverFixedExtentList.builder can build children'` | `sliver_fixed_extent_list_test.rs` (case 7) | OOS — needs `.builder` |
//! | 32 | `'SliverList.list can build children'` (2nd — misleadingly named; body constructs `SliverFixedExtentList.list`) | `sliver_fixed_extent_list_test.rs` (case 9) | ported, real green |
//! | 33 | `'SliverGrid.builder can build children'` | `sliver_grid_test.rs` (case 5) | ported, real green |
//! | 34 | `'SliverGrid.list can display children'` | `sliver_grid_test.rs` (case 6) | ported, real green |
//! | 35 | `'SliverGrid.list with empty children list'` | `sliver_grid_test.rs` (case 7) | already covered by a pre-existing test |
//! | 36 | `'SliverGrid.builder respects semanticIndexOffset'` | `sliver_grid_test.rs` (case 8) | OOS — semantics |
//! | 37 | `'SliverGridRegularTileLayout.computeMaxScrollOffset handles 0 children'` | `sliver_grid_test.rs` (case 9) | ported, real green |
//! | 38 | `'RenderSliverFixedExtentBoxAdaptor.layoutDimensions reflects the current constraints'` | `sliver_fixed_extent_list_test.rs` (case 8) | OOS — no such getter on FLUI's render object |
//!
//! **Total: 38 = 38 accounted for above.** No case with no home. Rows
//! 14/15 are covered by scenarios that predate their explicit
//! cross-reference (verified by reading both files); every other row's
//! accounting lives where the table says.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_types::Color;
use flui_widgets::{
    Container, CustomScrollView, GestureDetector, SizedBox, SliverIgnorePointer,
    SliverToBoxAdapter, Text,
};

use crate::common;
use crate::harness;

/// `SliverIgnorePointer(ignoring = false)` with a child is a transparent
/// layout/paint proxy: 3 render nodes, child fully reachable.
///
/// Flutter parity: `sliver.dart` `SliverIgnorePointer` with `ignoring = false`
/// — layout, paint, and hit-test delegate to the child unchanged.
#[test]
fn sliver_ignore_pointer_transparent_with_child_builds_three_render_nodes() {
    let root = CustomScrollView::new((SliverIgnorePointer::new(false).child(SizedBox::shrink()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverIgnorePointer(ignoring=false, child): expected 3 render nodes \
         (1 RenderViewport + 1 RenderSliverIgnorePointer + 1 child box)"
    );
    let _sliver = laid.find_by_render_type("RenderSliverIgnorePointer");
}

/// `SliverIgnorePointer(ignoring = true)` still mounts 3 render nodes.
///
/// Flutter parity: `RenderSliverIgnorePointer` unconditionally delegates
/// layout and paint; only `hit_test` returns `false`. The child is always in
/// the render tree. This test would fail (count = 2) if FLUI incorrectly
/// removed the child when ignoring is active.
#[test]
fn sliver_ignore_pointer_ignoring_child_remains_in_render_tree() {
    let root = CustomScrollView::new((SliverIgnorePointer::new(true).child(SizedBox::shrink()),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        3,
        "SliverIgnorePointer(ignoring=true, child): child must remain in render \
         tree (layout/paint are passthroughs); expected 3 render nodes"
    );
}

/// `SliverIgnorePointer` with no child mounts 2 render nodes.
#[test]
fn sliver_ignore_pointer_no_child_builds_two_render_nodes() {
    let root = CustomScrollView::new((SliverIgnorePointer::new(true),));
    let laid = harness::pump_widget(root, harness::screen());

    assert_eq!(
        laid.render_node_count(),
        2,
        "SliverIgnorePointer(no child): expected 2 render nodes \
         (1 RenderViewport + 1 RenderSliverIgnorePointer)"
    );
}

// ============================================================================
// Pointer-half ports — cases 17/20/21 ('ignores pointer events', 'ignores
// pointer events & semantics', 'ignores nothing')
// ============================================================================

/// One tappable sliver band: `SliverIgnorePointer(ignoring)` wrapping a
/// `SliverToBoxAdapter` over a colored, tap-counting `GestureDetector` over a
/// labelled `Text` (the label is how each band's on-screen center gets
/// located below — the same `find_text` + absolute-offset idiom
/// `sliver_fixed_extent_list_test.rs`'s own hit-test case establishes).
/// Mirrors the oracle's `SliverIgnorePointer(sliver: SliverToBoxAdapter(child:
/// GestureDetector(...)))` scaffold exactly (`slivers_test.dart`, tag
/// `3.44.0`) — unlike this file's pre-existing render-node-count tests
/// (which hand a leaf `SizedBox::shrink()` straight to `.child()` and rely on
/// its trivial always-zero layout), a real box subtree here needs the
/// explicit `SliverToBoxAdapter` bridge for the child to receive proper box
/// constraints from the sliver protocol.
fn ignore_pointer_tap_band(
    ignoring: bool,
    label: &str,
    counter: Arc<AtomicUsize>,
    color: Color,
) -> SliverIgnorePointer {
    SliverIgnorePointer::new(ignoring).child(
        SliverToBoxAdapter::new().child(
            GestureDetector::new()
                .on_tap(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                })
                .child(
                    Container::new()
                        .color(color)
                        .child(Text::new(label.to_string())),
                ),
        ),
    )
}

/// Taps the center of the `Text` labelled `label`, computed from its actual
/// laid-out geometry (never hardcoded — the same discipline
/// `sliver_fixed_extent_list_test.rs`'s hit-test case follows).
fn tap_labelled_target(laid: &common::LaidOut, label: &str) {
    let id = laid
        .find_text(label)
        .unwrap_or_else(|| panic!("'{label}' must be mounted"));
    let offset = laid.absolute_offset(id);
    let extent = laid.size(id);
    let x = offset.dx.get() + extent.width.get() / 2.0;
    let y = offset.dy.get() + extent.height.get() / 2.0;
    laid.dispatch_pointer_down(x, y);
    laid.dispatch_pointer_up(x, y);
}

/// Flutter parity: `slivers_test.dart` `'ignores pointer events'` (tag
/// `3.44.0`, `SliverIgnorePointer(ignoringSemantics: false)` — `ignoring`
/// unspecified, defaulting `true`) AND `'ignores pointer events & semantics'`
/// (tag `3.44.0`, `SliverIgnorePointer(ignoringSemantics: true)` — `ignoring`
/// again unspecified/default `true`).
///
/// Both oracle cases reduce to the IDENTICAL FLUI-observable scenario: FLUI's
/// `SliverIgnorePointer` has only the pointer-blocking `ignoring: bool` (no
/// `ignoringSemantics` equivalent — see this file's Cross.H note), so both
/// configurations collapse to `ignoring = true` here. Ported once, both
/// accounted — the same "duplicate/equivalent oracle occurrence, one real
/// port" precedent `sliver_list_constructors_test.rs`'s own ledger case 1
/// established (porting each nominally-separate case would produce
/// byte-identical test bodies, no new coverage).
///
/// Two independent `ignoring = true` targets both stay silent under a tap
/// (not just "the one target the oracle happened to use"), and a third,
/// `ignoring = false` control target fires — proving taps genuinely reach a
/// `GestureDetector` in this scene (non-vacuity: a bug that broke
/// hit-testing generally, rather than the `ignoring` gate specifically,
/// would leave the control silent too and fail this test).
///
/// Mutation-checked: flipping either ignored target's `ignoring` argument
/// from `true` to `false` makes that target's counter fire, failing its
/// `assert_eq!(..., 0, ...)`.
#[test]
fn sliver_ignore_pointer_blocks_taps_on_every_ignoring_true_target() {
    let ignored_a = Arc::new(AtomicUsize::new(0));
    let ignored_b = Arc::new(AtomicUsize::new(0));
    let control = Arc::new(AtomicUsize::new(0));

    let laid = harness::pump_widget(
        CustomScrollView::new((
            ignore_pointer_tap_band(
                true,
                "ignored a",
                Arc::clone(&ignored_a),
                Color::rgb(255, 0, 0),
            ),
            ignore_pointer_tap_band(
                true,
                "ignored b",
                Arc::clone(&ignored_b),
                Color::rgb(0, 255, 0),
            ),
            ignore_pointer_tap_band(
                false,
                "control",
                Arc::clone(&control),
                Color::rgb(0, 0, 255),
            ),
        )),
        harness::screen(),
    );

    tap_labelled_target(&laid, "ignored a");
    tap_labelled_target(&laid, "ignored b");
    tap_labelled_target(&laid, "control");

    assert_eq!(
        ignored_a.load(Ordering::SeqCst),
        0,
        "ignoring=true target A must not fire on tap"
    );
    assert_eq!(
        ignored_b.load(Ordering::SeqCst),
        0,
        "ignoring=true target B must not fire on tap"
    );
    assert_eq!(
        control.load(Ordering::SeqCst),
        1,
        "the ignoring=false control must fire -- proves the harness genuinely \
         delivers taps in this scene, so targets A/B staying silent is the \
         ignoring gate at work, not a broken hit-test path"
    );
}

/// Flutter parity: `slivers_test.dart` `'ignores nothing'` (tag `3.44.0`,
/// `SliverIgnorePointer(ignoring: false, ignoringSemantics: false)`).
///
/// `ignoring = false` is a transparent pointer proxy: the wrapped
/// `GestureDetector` receives the tap normally.
#[test]
fn sliver_ignore_pointer_lets_taps_through_when_ignoring_false() {
    let fired = Arc::new(AtomicUsize::new(0));

    let laid = harness::pump_widget(
        CustomScrollView::new((ignore_pointer_tap_band(
            false,
            "target",
            Arc::clone(&fired),
            Color::rgb(255, 255, 0),
        ),)),
        harness::screen(),
    );

    tap_labelled_target(&laid, "target");

    assert_eq!(
        fired.load(Ordering::SeqCst),
        1,
        "ignoring=false must let the tap reach the wrapped GestureDetector"
    );
}

// ============================================================================
// Semantics-only cases — out of scope by standing harness gap (cases 18/19/22)
// ============================================================================
//
// - `'ignores semantics'` (:931, `ignoring: false, ignoringSemantics: true`):
//   its pointer-observable half (`ignoring = false` lets the tap through) is
//   IDENTICAL, FLUI-mechanism-wise, to
//   `sliver_ignore_pointer_lets_taps_through_when_ignoring_false` above (both
//   reduce to "ignoring=false → tap fires") — re-porting it here would add
//   no new coverage, so it is subsumed rather than duplicated. Its actual
//   subject — `ignoringSemantics: true` hiding the semantics node while
//   `ignoring: false` leaves the pointer path open — is unportable: no
//   semantics-tree assertion capability exists anywhere in `flui-widgets`'
//   headless harness (the same standing gap `sliver_grid_test.rs` case 8 and
//   `sliver_fixed_extent_list_test.rs` case 6 already cite).
// - `'ignoring only block semantics actions'` (:960, no `ignoring`/
//   `ignoringSemantics` arguments passed — both default): the ENTIRE case
//   body is a `SemanticsTester` actions-list assertion
//   (`includesNodeWith(label: 'a', actions: [])`) with no `tester.tap`/event
//   list at all — no pointer-observable half to port. OOS by the same
//   standing harness gap.
// - `'ensure semantics'` (:1038, `group('SliverEnsureSemantics - ')`): a
//   DIFFERENT widget (`SliverEnsureSemantics`, not `SliverIgnorePointer`),
//   with no FLUI port anywhere and an entirely `SemanticsTester`-based body
//   (no pointer assertions). Accounted here rather than left homeless: it
//   sits immediately after this group in the oracle file and is blocked by
//   the identical standing gap. OOS by that gap; no FLUI `SliverEnsureSemantics`
//   port exists to write a test against even if the harness gap closed.
