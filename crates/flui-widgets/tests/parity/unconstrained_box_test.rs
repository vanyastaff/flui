//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/basic_test.dart` (tag
//! `3.44.0`), the `'UnconstrainedBox'` group (3 cases: a plain `test` for
//! `toString`, plus 2 `testWidgets` cases) — plus one appendable case,
//! `'toString'`, from the adjacent `'ConstraintsTransformBox'` group in the
//! same file.
//!
//! Widget → render-object mapping: `UnconstrainedBox`
//! (`crates/flui-widgets/src/layout/unconstrained_box.rs`) →
//! [`flui_objects::RenderConstraintsTransformBox`]
//! (`crates/flui-objects/src/layout/constraints_transform_box.rs`) — a NEW
//! render object this port introduces (FLUI had no
//! `RenderConstraintsTransformBox`/`ConstraintsTransformBox` of any kind
//! before this unit; the pre-existing `RenderConstrainedOverflowBox`
//! (`OverflowBox`'s render object) is a different per-axis-override
//! abstraction, not a fit for `UnconstrainedBox`'s `Axis`-narrowed transform
//! or its `clip_behavior`/overflow contract — see that render object's own
//! module doc for the full mapping and the narrowed `constrained_axis`
//! design).
//!
//! ## Ledger (3 upstream cases + 1 appendable from the adjacent group)
//! 1. `'UnconstrainedBox toString'` — **ported at contract level** (not Dart
//!    string formatting — see the doc comment on each test): both `test`
//!    sub-assertions are ported as one case each —
//!    [`unconstrained_box_diagnostics_default_alignment_vertical_axis`],
//!    [`unconstrained_box_diagnostics_custom_alignment_horizontal_axis`].
//!    `textDirection` is dropped from the second sub-case's input — no
//!    widget in FLUI's shifted-box family resolves a `TextDirection` yet
//!    (pre-existing, systemic; see `docs/ROADMAP.md` Cross.H, "the
//!    shifted-box family ... takes an already-resolved `Alignment` ...";
//!    this port extends that same entry to include `UnconstrainedBox`).
//! 2. `'UnconstrainedBox can set and update clipBehavior'` — **ported**:
//!    [`unconstrained_box_can_set_and_update_clip_behavior`].
//!    `RenderConstraintsTransformBox` carries a real `clip_behavior` field
//!    end to end (unlike, e.g., `Wrap`'s still-missing knob), so this ports
//!    with no narrowing.
//! 3. `'UnconstrainedBox warns only when clipBehavior is Clip.none'` —
//!    **out of scope**, missing machinery. FLUI has no equivalent anywhere
//!    in `flui-rendering`/`flui-objects` to Flutter's
//!    `DebugOverflowIndicatorMixin` (the debug-mode yellow-and-black striped
//!    overflow warning plus its companion `FlutterError` diagnostic) — a
//!    grep across the workspace for the pattern
//!    (`debug_paint_overflow`/`DebugPaintOverflow`/`paintOverflowIndicator`/
//!    `overflow indicator`) found no prior art anywhere, confirming this is
//!    a first-class, repo-wide gap, not specific to this widget. Filed to
//!    `docs/ROADMAP.md` Cross.H (search `DebugOverflowIndicatorMixin`). The
//!    oracle's `clipBehavior` defaulting/threading half of this case is not
//!    a distinct fragment to port on top of case 2 above — it is the exact
//!    same observable case 2 already covers — so this case is accounted OOS
//!    as a whole rather than padded with a redundant partial port.
//!    `RenderConstraintsTransformBox::has_visual_overflow()` exposes the
//!    same overflow fact as a plain queryable flag instead, matching the
//!    precedent `RenderFittedBox::has_visual_overflow` already established
//!    — see [`unconstrained_box_reports_overflow_as_a_queryable_flag`] for
//!    that half, ported as a stand-in.
//!
//! Appendable from the adjacent group (1 of 1):
//! `'ConstraintsTransformBox' > 'toString'`
//! — **out of scope**, no equivalent widget. FLUI carries no
//! `ConstraintsTransformBox` widget at all (see the render-object mapping
//! above); `UnconstrainedBox` targets `RenderConstraintsTransformBox`
//! directly rather than through an intermediate general-transform widget, so
//! there is no `ConstraintsTransformBox::toString()`-equivalent surface to
//! port this onto.
//!
//! **Total: 3 upstream cases + 1 appendable = 1 ported in full (2
//! sub-assertions), 1 ported at contract level (2 sub-assertions), 1 out of
//! scope (missing debug-overflow machinery, half stood in as a queryable-flag
//! check), 1 appendable out of scope (no equivalent widget) — no narrowing:
//! every oracle assertion is either ported, stood in for, or named as a
//! divergence.**

use flui_foundation::DiagnosticsNode;
use flui_rendering::testing::inspect::render_diagnostics;
use flui_types::Alignment;
use flui_types::layout::Axis;
use flui_types::painting::Clip;
use flui_widgets::{SizedBox, UnconstrainedBox};

use crate::common::{loose, size};
use crate::harness;

/// The mounted `UnconstrainedBox`'s own `RenderConstraintsTransformBox` node.
fn render_node(laid: &crate::common::LaidOut) -> DiagnosticsNode {
    let owner = laid.pipeline_owner();
    let owner = owner.read();
    render_diagnostics(&owner)
        .find_descendant_unique("RenderConstraintsTransformBox")
        .expect("UnconstrainedBox mounts exactly one RenderConstraintsTransformBox")
        .clone()
}

/// `UnconstrainedBox`'s diagnostics carry `alignment` (default `CENTER`) and
/// `constrained_axis` (`Vertical`) through to the mounted render object.
///
/// Flutter parity: `basic_test.dart` `'UnconstrainedBox toString'` (3.44.0),
/// first sub-assertion — `UnconstrainedBox(constrainedAxis: Axis.vertical)
/// .toString()` equals `'UnconstrainedBox(alignment: Alignment.center,
/// constrainedAxis: vertical)'`. FLUI's `View` has no widget-level
/// diagnostics of its own — only render objects implement `Diagnosticable`
/// (the same standing gap `colored_box_test.rs`'s own `toString` port
/// documents) — so this pumps the widget live and reads its mounted
/// `RenderConstraintsTransformBox`'s diagnostics instead, checking the same
/// two-part contract the oracle checks (correct alignment, correct
/// constrained axis), not Dart's exact string formatting (`Alignment.center`
/// vs. FLUI's own `(0, 0)`; `vertical` vs. FLUI's `Vertical`).
#[test]
fn unconstrained_box_diagnostics_default_alignment_vertical_axis() {
    let laid = harness::pump_widget(
        UnconstrainedBox::new().constrained_axis(Axis::Vertical),
        harness::screen(),
    );

    let node = render_node(&laid);
    let alignment = node
        .properties()
        .iter()
        .find(|p| p.name() == "alignment")
        .expect("RenderConstraintsTransformBox emits an alignment property");
    assert_eq!(
        alignment.value(),
        format!("({}, {})", Alignment::CENTER.x, Alignment::CENTER.y),
        "the default alignment must be Alignment::CENTER"
    );

    let constrained_axis = node
        .properties()
        .iter()
        .find(|p| p.name() == "constrained_axis")
        .expect("RenderConstraintsTransformBox emits a constrained_axis property");
    assert_eq!(
        constrained_axis.value(),
        format!("{:?}", Axis::Vertical),
        "constrained_axis must carry the configured Vertical axis"
    );
}

/// `UnconstrainedBox`'s diagnostics carry a custom `alignment` and
/// `constrained_axis` (`Horizontal`) through to the mounted render object.
///
/// Flutter parity: `basic_test.dart` `'UnconstrainedBox toString'` (3.44.0),
/// second sub-assertion — `UnconstrainedBox(constrainedAxis:
/// Axis.horizontal, textDirection: TextDirection.rtl, alignment:
/// Alignment.topRight).toString()` equals `'UnconstrainedBox(alignment:
/// Alignment.topRight, constrainedAxis: horizontal, textDirection: rtl)'`.
/// `textDirection` is dropped from the input: no widget in FLUI's
/// shifted-box family resolves a `TextDirection` yet — see this file's
/// module doc.
#[test]
fn unconstrained_box_diagnostics_custom_alignment_horizontal_axis() {
    let laid = harness::pump_widget(
        UnconstrainedBox::new()
            .constrained_axis(Axis::Horizontal)
            .alignment(Alignment::TOP_RIGHT),
        harness::screen(),
    );

    let node = render_node(&laid);
    let alignment = node
        .properties()
        .iter()
        .find(|p| p.name() == "alignment")
        .expect("RenderConstraintsTransformBox emits an alignment property");
    assert_eq!(
        alignment.value(),
        format!("({}, {})", Alignment::TOP_RIGHT.x, Alignment::TOP_RIGHT.y),
        "alignment must carry the configured Alignment::TOP_RIGHT"
    );

    let constrained_axis = node
        .properties()
        .iter()
        .find(|p| p.name() == "constrained_axis")
        .expect("RenderConstraintsTransformBox emits a constrained_axis property");
    assert_eq!(
        constrained_axis.value(),
        format!("{:?}", Axis::Horizontal),
        "constrained_axis must carry the configured Horizontal axis"
    );
}

/// `UnconstrainedBox`'s `clip_behavior` starts at Flutter's default
/// (`Clip::None`), then follows the render object through a live
/// `update_render_object` call to `Clip::AntiAlias` — the identical
/// root-swap contract `physical_model_test.rs`'s sibling clip-behavior test
/// exercises for `PhysicalModel`.
///
/// Flutter parity: `basic_test.dart` `'UnconstrainedBox can set and update
/// clipBehavior'` (3.44.0).
#[test]
fn unconstrained_box_can_set_and_update_clip_behavior() {
    let mut laid = harness::pump_widget(UnconstrainedBox::new(), harness::screen());
    let id = laid.find_by_render_type("RenderConstraintsTransformBox");
    assert_eq!(
        laid.clip_behavior(id),
        Clip::None,
        "UnconstrainedBox's default clip_behavior must be Clip::None"
    );

    laid.pump_widget(UnconstrainedBox::new().clip_behavior(Clip::AntiAlias));
    assert_eq!(laid.clip_behavior(id), Clip::AntiAlias);
}

/// Stands in for the missing half of `'UnconstrainedBox warns only when
/// clipBehavior is Clip.none'` (see this file's module doc, case 3): FLUI
/// has no debug-overflow-indicator machinery to assert a `FlutterError`
/// against, so this instead pins the queryable-flag substitute
/// `RenderConstraintsTransformBox::has_visual_overflow()` — the same
/// precedent `RenderFittedBox::has_visual_overflow` already established —
/// reports `true` when a child overflows the box under a loose parent, and
/// `false` when it fits.
#[test]
fn unconstrained_box_reports_overflow_as_a_queryable_flag() {
    let overflowing = harness::pump_widget(
        UnconstrainedBox::new().child(SizedBox::new(200.0, 200.0)),
        loose(50.0),
    );
    let overflowing_id = overflowing.find_by_render_type("RenderConstraintsTransformBox");
    assert_eq!(
        overflowing.size(overflowing_id),
        size(50.0, 50.0),
        "under a loose 0..50 parent, a 200x200 child must clamp the box's \
         own size down to 50x50"
    );
    assert!(
        overflowing.has_visual_overflow(overflowing_id),
        "a 200x200 child inside a 50x50 box must report visual overflow"
    );

    let fitting = harness::pump_widget(
        UnconstrainedBox::new().child(SizedBox::new(20.0, 20.0)),
        loose(50.0),
    );
    let fitting_id = fitting.find_by_render_type("RenderConstraintsTransformBox");
    assert_eq!(
        fitting.size(fitting_id),
        size(20.0, 20.0),
        "under the same loose 0..50 parent, a 20x20 child fits, so the box \
         must adopt the child's own size instead of clamping to the max"
    );
    assert!(
        !fitting.has_visual_overflow(fitting_id),
        "a 20x20 child inside a 50x50 box must not report visual overflow"
    );
}
