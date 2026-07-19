//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/fractionally_sized_box_test.dart`
//! (tag `3.44.0`, 4 `testWidgets` cases).
//!
//! `FractionallySizedBox` (`crates/flui-widgets/src/layout/fractionally_sized_box.rs`)
//! → `RenderFractionallySizedBox` (`crates/flui-objects/src/layout/fractionally_sized_box.rs`)
//! implements the real Flutter algorithm — a per-axis factor tightens the
//! child's constraint to `factor * incoming.max` (falling back to
//! `incoming.min` on an unbounded axis, a deliberate documented divergence
//! from Flutter's own degenerate-infinite-child behavior; see the module's
//! own doc comment), an unset factor passes the incoming constraint through
//! unchanged, the box itself sizes to `incoming.constrain(child_size)`, and
//! the child is aligned within that box (able to overflow when a factor
//! exceeds `1.0`) — already unit-tested in the module's own `#[cfg(test)]`
//! block (`child_constraints`/`align_child` cases). This file adds the layer
//! those don't cover: the widget wired through `View` → render object via a
//! real widget tree (`pump_widget`), reproducing the Dart oracle's own tree
//! shapes and constraint chains.
//!
//! Ported cases (4 of 4):
//! - `'FractionallySizedBox'` — [`fractionally_sized_box_control_test`]. An
//!   `OverflowBox` (overrides `0..100` on both axes, `Alignment.topLeft`)
//!   around a `Center` around a `FractionallySizedBox(widthFactor: 0.5,
//!   heightFactor: 0.25)` around a leaf child — the child ends up tight at
//!   `(50, 25)` (half/quarter of the `OverflowBox`'s 100×100 override, not
//!   the 800×600 screen) and its global offset is `(25, 37.5)` (`Center`
//!   centers the 50×25 box within its own 100×100 claim; `OverflowBox`'s
//!   `topLeft` puts that at the screen origin).
//! - `'FractionallySizedBox alignment'` —
//!   [`fractionally_sized_box_alignment_places_the_child_by_physical_alignment`].
//!   `FractionallySizedBox(widthFactor: 0.5, heightFactor: 0.5,
//!   alignment: Alignment.topRight)` mounted directly under the 800×600
//!   screen, wrapped in `Directionality(rtl)` — proving `Alignment.topRight`
//!   is a physical (non-directional) alignment unaffected by the ambient
//!   text direction. The child tightens to `(400, 300)` (half of the
//!   screen); `topRight` places its center at `(600, 150)`.
//! - `'FractionallySizedBox alignment (direction-sensitive)'` —
//!   [`fractionally_sized_box_alignment_resolves_directional_alignment`].
//!   Same tree, but `alignment: AlignmentDirectional.topEnd` under RTL.
//!   FLUI's `FractionallySizedBox::alignment` (like every shifted-box-family
//!   widget in FLUI — `Align`, `Center`, `OverflowBox`, `SizedOverflowBox`)
//!   takes an already-resolved `Alignment`, not an
//!   `AlignmentGeometry`/`AlignmentDirectional` plus ambient
//!   `Directionality` — there is no code path in this widget that reads a
//!   `TextDirection`. This test resolves `AlignmentDirectional::TOP_END`
//!   with `resolve(false)` (RTL) at the call site — the same resolution the
//!   Dart oracle's build phase performs internally — and asserts the
//!   identical resulting position. It proves `AlignmentDirectional::resolve`'s
//!   RTL math and `RenderFractionallySizedBox::align_child`'s placement
//!   arithmetic agree with the oracle; it does **not** exercise automatic
//!   ambient-`Directionality` resolution inside the widget tree, because
//!   that capability does not exist yet for this widget family — a
//!   pre-existing, systemic gap across the whole shifted-box family, not
//!   something introduced or discovered as `FractionallySizedBox`-specific,
//!   and out of scope for a parity-test port to newly build. Filed to
//!   `docs/ROADMAP.md` Cross.H ("the shifted-box family ... takes an
//!   already-resolved `Alignment` ..." / "Extending this same entry: the
//!   `FractionallySizedBox` parity port ..."); this header is a pointer to
//!   that record, not the record itself.
//! - `'OverflowBox alignment with FractionallySizedBox'` —
//!   [`overflow_box_alignment_with_fractionally_sized_box_resolves_directional_alignment`].
//!   The same tree as the control test above, but the OUTER `OverflowBox`
//!   carries `alignment: AlignmentDirectional.topEnd` under RTL (the inner
//!   `FractionallySizedBox` has no explicit alignment of its own — moot
//!   anyway since its box always sizes exactly to its tight child in this
//!   tree). Same gap as the previous case, on `OverflowBox` this time
//!   (already the OverflowBox/SizedOverflowBox port's own finding). Resolves
//!   `AlignmentDirectional::TOP_END.resolve(false)` at the call site, which
//!   equals `Alignment::TOP_LEFT` — the identical physical alignment as the
//!   control test, so this asserts the identical `(50, 25)` size / `(25,
//!   37.5)` offset, confirming the resolved alignment reaches `OverflowBox`
//!   correctly with `FractionallySizedBox` nested inside it.
//!
//! Denominator: 4 upstream `testWidgets` cases, 4 ported (2 direct, 2 via the
//! call-site directional-resolve workaround described above), 0 out of
//! scope.
//!
//! Scaffolding substitution: the Dart oracle's leaf children are
//! `Container(key: inner)` (case 1 and 4) and `Placeholder(key: inner)`
//! (cases 2 and 3). Both leaves are read under TIGHT constraints in every
//! case here (`FractionallySizedBox`'s factors always produce a tight child
//! constraint when set), so the leaf's own sizing preference cannot affect
//! the asserted geometry — any leaf widget is forced to the same size
//! regardless of what it would prefer standing alone. FLUI has no
//! `Placeholder` widget; every case below uses [`SizedBox::shrink`] (a
//! `0×0`-preferred leaf) to make that point explicit: the box that arrives
//! at the assertion is the tight constraint, not the leaf's own preference.

use flui_types::Alignment;
use flui_types::geometry::px;
use flui_types::layout::AlignmentDirectional;
use flui_types::typography::TextDirection;
use flui_widgets::{Center, Directionality, FractionallySizedBox, OverflowBox, SizedBox};

use crate::common::{offset, size};
use crate::harness;

/// `OverflowBox` (override `0..100` both axes, `Alignment.topLeft`) around
/// `Center` around `FractionallySizedBox(widthFactor: 0.5, heightFactor: 0.25)`
/// around a leaf — the child tightens to half/quarter of the `OverflowBox`'s
/// 100×100 override (NOT the 800×600 screen the tree is mounted under), and
/// its global offset comes from `Center`'s own centering within its 100×100
/// claim plus `OverflowBox`'s `topLeft` placement of that claim at the
/// screen origin.
///
/// Flutter parity: `'FractionallySizedBox'` (`fractionally_sized_box_test.dart`,
/// tag `3.44.0`).
#[test]
fn fractionally_sized_box_control_test() {
    let laid = harness::pump_widget(
        OverflowBox::new()
            .with_alignment(Alignment::TOP_LEFT)
            .with_min_width(px(0.0))
            .with_max_width(px(100.0))
            .with_min_height(px(0.0))
            .with_max_height(px(100.0))
            .child(
                Center::new().child(
                    FractionallySizedBox::new()
                        .width_factor(0.5)
                        .height_factor(0.25)
                        .child(SizedBox::shrink()),
                ),
            ),
        harness::screen(),
    );

    let root = laid.root();
    let center = laid.only_child(root);
    let fractionally_sized_box = laid.only_child(center);
    let leaf = laid.only_child(fractionally_sized_box);

    assert_eq!(
        laid.size(leaf),
        size(50.0, 25.0),
        "the child must tighten to half the OverflowBox's 100 width and a \
         quarter of its 100 height, regardless of the leaf's own preference",
    );
    assert_eq!(
        laid.absolute_offset(leaf),
        offset(25.0, 37.5),
        "Center puts the 50x25 box at (25, 37.5) within its own 100x100 \
         claim, and OverflowBox's topLeft places that claim at the screen \
         origin",
    );
}

/// `FractionallySizedBox(widthFactor: 0.5, heightFactor: 0.5,
/// alignment: Alignment.topRight)` mounted directly under the 800×600
/// screen, wrapped in `Directionality(rtl)` to prove the ambient direction
/// has no bearing on a physical `Alignment` (only `AlignmentDirectional`
/// would consult it, and this widget cannot accept one — see the module
/// doc's case-3 note).
///
/// Flutter parity: `'FractionallySizedBox alignment'`
/// (`fractionally_sized_box_test.dart`, tag `3.44.0`).
#[test]
fn fractionally_sized_box_alignment_places_the_child_by_physical_alignment() {
    let laid = harness::pump_widget(
        Directionality::new(
            TextDirection::Rtl,
            FractionallySizedBox::new()
                .width_factor(0.5)
                .height_factor(0.5)
                .alignment(Alignment::TOP_RIGHT)
                .child(SizedBox::shrink()),
        ),
        harness::screen(),
    );

    // `Directionality` is an `InheritedView` — it contributes no render
    // object of its own, so the render-tree root is `FractionallySizedBox`
    // directly.
    let root = laid.root();
    let leaf = laid.only_child(root);

    assert_eq!(
        laid.size(leaf),
        size(400.0, 300.0),
        "the child must tighten to half the 800x600 screen on each axis",
    );

    let leaf_offset = laid.absolute_offset(leaf);
    let leaf_size = laid.size(leaf);
    let center_x = leaf_offset.dx.get() + leaf_size.width.get() / 2.0;
    let center_y = leaf_offset.dy.get() + leaf_size.height.get() / 2.0;
    assert!(
        (center_x - 600.0).abs() < 1e-3 && (center_y - 150.0).abs() < 1e-3,
        "the child's global center must be (600, 150) -- topRight puts the \
         400x300 child flush with the right edge of the 800x600 box \
         (regardless of the RTL ambient direction); got ({center_x}, {center_y})",
    );
}

/// The same tree as
/// [`fractionally_sized_box_alignment_places_the_child_by_physical_alignment`],
/// but with `AlignmentDirectional::TOP_END` resolved under RTL — which
/// resolves to `Alignment::TOP_LEFT` (`resolve` flips only the horizontal
/// sign for RTL, leaving `y` untouched), moving the child from the top-right
/// corner to the top-left corner of the screen.
///
/// Flutter parity: `'FractionallySizedBox alignment (direction-sensitive)'`
/// (`fractionally_sized_box_test.dart`, tag `3.44.0`) — identical tree under
/// `Directionality(rtl)` with `alignment: AlignmentDirectional.topEnd`.
/// FLUI's `FractionallySizedBox` (like the rest of the shifted-box family)
/// takes an already-resolved `Alignment` — see the module doc's case-3 note
/// for why this test resolves at the call site instead of threading a
/// `Directionality` ancestor through the tree.
#[test]
fn fractionally_sized_box_alignment_resolves_directional_alignment() {
    let resolved = AlignmentDirectional::TOP_END.resolve(false); // RTL
    assert_eq!(
        resolved,
        Alignment::TOP_LEFT,
        "AlignmentDirectional::TOP_END under RTL must resolve to TOP_LEFT \
         (only the horizontal sign flips)",
    );

    let laid = harness::pump_widget(
        FractionallySizedBox::new()
            .width_factor(0.5)
            .height_factor(0.5)
            .alignment(resolved)
            .child(SizedBox::shrink()),
        harness::screen(),
    );

    let root = laid.root();
    let leaf = laid.only_child(root);

    assert_eq!(laid.size(leaf), size(400.0, 300.0));

    let leaf_offset = laid.absolute_offset(leaf);
    let leaf_size = laid.size(leaf);
    let center_x = leaf_offset.dx.get() + leaf_size.width.get() / 2.0;
    let center_y = leaf_offset.dy.get() + leaf_size.height.get() / 2.0;
    assert!(
        (center_x - 200.0).abs() < 1e-3 && (center_y - 150.0).abs() < 1e-3,
        "the child's global center must be (200, 150) -- same y as the \
         topRight case, but left-aligned instead of right-aligned; got \
         ({center_x}, {center_y})",
    );
}

/// The same tree as [`fractionally_sized_box_control_test`], but the OUTER
/// `OverflowBox` carries `alignment: AlignmentDirectional.topEnd` under RTL
/// instead of a literal `Alignment.topLeft`. `AlignmentDirectional::TOP_END`
/// resolved under RTL equals `Alignment::TOP_LEFT` — the identical physical
/// alignment the control test uses — so this asserts the identical
/// `(50, 25)` size / `(25, 37.5)` offset, confirming the resolved alignment
/// reaches `OverflowBox` correctly with `FractionallySizedBox` nested inside
/// it (the inner box has no explicit alignment here, which is moot anyway
/// since its own box always sizes exactly to its tight child in this tree).
///
/// Flutter parity: `'OverflowBox alignment with FractionallySizedBox'`
/// (`fractionally_sized_box_test.dart`, tag `3.44.0`). Same directional-
/// resolution gap and workaround as
/// [`fractionally_sized_box_alignment_resolves_directional_alignment`], this
/// time on `OverflowBox::with_alignment` rather than
/// `FractionallySizedBox::alignment`.
#[test]
fn overflow_box_alignment_with_fractionally_sized_box_resolves_directional_alignment() {
    let resolved = AlignmentDirectional::TOP_END.resolve(false); // RTL
    assert_eq!(resolved, Alignment::TOP_LEFT);

    let laid = harness::pump_widget(
        OverflowBox::new()
            .with_alignment(resolved)
            .with_min_width(px(0.0))
            .with_max_width(px(100.0))
            .with_min_height(px(0.0))
            .with_max_height(px(100.0))
            .child(
                Center::new().child(
                    FractionallySizedBox::new()
                        .width_factor(0.5)
                        .height_factor(0.25)
                        .child(SizedBox::shrink()),
                ),
            ),
        harness::screen(),
    );

    let root = laid.root();
    let center = laid.only_child(root);
    let fractionally_sized_box = laid.only_child(center);
    let leaf = laid.only_child(fractionally_sized_box);

    assert_eq!(laid.size(leaf), size(50.0, 25.0));
    assert_eq!(laid.absolute_offset(leaf), offset(25.0, 37.5));
}
