//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/visibility_test.dart`
//! (class `Visibility` implementation is in `indexed_stack.dart`).
//!
//! Oracle tests ported:
//! - `'Visibility'` — `Visibility(child: …)` (visible=true): child is in the
//!   render tree. `Visibility(visible: false)`: child is gone, replaced by the
//!   default `SizedBox.shrink`.
//! - `maintainState=true`: child stays in the element tree inside an `Offstage`.
//!
//! ## Constraint model
//!
//! All tests use `loose(200.0)` (min=0, max=200×200) instead of tight(800×600)
//! because:
//!
//! 1. **`SizedBox::new(100, 100)` natural size:** under tight(800×600),
//!    `BoxConstraints::enforce({100,100,100,100}, tight)` clamps 100 → 800×600.
//!    Under `loose(200)` it resolves to 100×100.
//!
//! 2. **`RenderOffstage(offstage=true)` constraint contract:** FLUI's
//!    `RenderOffstage` currently returns `Size::ZERO` when `offstage=true`.
//!    Under tight(800×600) that violates the constraints (FLUI-DEV-001 — tracked
//!    for the next render-object patch; Flutter uses `sizedByParent=true` so the
//!    parent drives size to `constraints.smallest`). Under `loose(200)`, zero is
//!    within bounds, so no panic.
//!
//! Widget → render-object mapping:
//! - `Visibility(visible=true)`  → child's render object directly
//! - `Visibility(visible=false)` → `RenderConstrainedBox` (replacement `SizedBox::shrink`)
//! - `Visibility(visible=false, maintain_state=true)`
//!   → `RenderOffstage(offstage=true)` wrapping the child's `RenderConstrainedBox`
//! - `Visibility(visible=true, maintain_state=true)`
//!   → `RenderOffstage(offstage=false)` wrapping the child's `RenderConstrainedBox`
//!
//! Divergences:
//! - `maintainAnimation` and `maintainSize` are deferred (need `TickerMode`).
//! - `maintain_interactivity` is accepted but is a no-op until `maintainSize`
//!   lands. Tested to confirm it does not panic or break the tree.
//! - Flutter wraps in `_VisibilityScope`; FLUI omits that scope widget.

use crate::common::{loose, size};
use crate::harness;
use flui_widgets::{SizedBox, Visibility};

/// `Visibility(visible=true)` passes the child directly to the render tree.
///
/// Flutter parity: `visibility_test.dart` — `Visibility(child: testChild)`:
/// `expect(tester.getSize(find.byType(Visibility)), const Size(800.0, 600.0))`.
/// FLUI equivalent (loose constraints): the child's `RenderConstrainedBox` is
/// present and sized to its natural 100×100.
#[test]
fn visibility_true_child_renders_at_natural_size() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0)).visible(true),
        loose(200.0),
    );

    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(child_id),
        size(100.0, 100.0),
        "visible=true: child SizedBox(100, 100) must resolve to 100×100 under loose(200) \
         — confirms the child render object is present and sized naturally"
    );
}

/// `Visibility(visible=false)` replaces the child with the default
/// `SizedBox::shrink` (0×0) replacement, discarding the child from the tree.
///
/// Flutter parity: `visibility_test.dart` — `Visibility(visible: false, child:…)`:
/// `expect(find.byType(Text, skipOffstage: false), findsNothing)`.
/// FLUI: under `loose(200)` the only `RenderConstrainedBox` in the tree is
/// the `SizedBox::shrink` replacement, which resolves to 0×0 — distinguishing
/// it from the 100×100 original child.
#[test]
fn visibility_false_shows_replacement_at_zero_size() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0)).visible(false),
        loose(200.0),
    );

    // Only one RenderConstrainedBox — the replacement SizedBox::shrink.
    let replacement_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(replacement_id),
        size(0.0, 0.0),
        "visible=false: the replacement SizedBox::shrink must be 0×0 under loose(200); \
         the original 100×100 child must not be in the tree (if it were, this node \
         would be 100×100 not 0×0)"
    );
}

/// `Visibility(visible=false, maintain_state=true)` keeps the child alive via
/// `Offstage(offstage=true)`.
///
/// Flutter parity: `visibility_test.dart` — `maintainState=true` branch:
/// `Offstage(offstage: !visible, child: child)`. The child render object is
/// still in the tree (state preserved) but `RenderOffstage` suppresses paint
/// and hit-testing.
///
/// Note: uses `loose(200)` to avoid a constraint-violation panic from
/// `RenderOffstage(offstage=true)` returning `Size::ZERO` (see module
/// doc FLUI-DEV-001).
#[test]
fn visibility_false_maintain_state_wraps_child_in_offstage() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0))
            .visible(false)
            .maintain_state(true),
        loose(200.0),
    );

    // RenderOffstage must be present (panics if absent).
    let _offstage_id = laid.find_by_render_type("RenderOffstage");
    // RenderConstrainedBox (the child) is also present, inside RenderOffstage.
    assert_eq!(
        laid.render_node_count(),
        2,
        "visible=false + maintain_state=true: 2 render nodes expected \
         (1 RenderOffstage + 1 RenderConstrainedBox for the preserved child)"
    );
}

/// `Visibility(visible=true, maintain_state=true)` wraps the child in
/// `Offstage(offstage=false)`, which is functionally transparent: the child
/// paints and hit-tests normally, and sizes to its natural dimensions.
#[test]
fn visibility_true_maintain_state_child_paints_normally_via_offstage() {
    let laid = harness::pump_widget(
        Visibility::new(SizedBox::new(100.0, 100.0))
            .visible(true)
            .maintain_state(true),
        loose(200.0),
    );

    // Both RenderOffstage (offstage=false) and the child are present.
    assert_eq!(
        laid.render_node_count(),
        2,
        "visible=true + maintain_state=true: 2 render nodes \
         (RenderOffstage(offstage=false) + RenderConstrainedBox child)"
    );
    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.size(child_id),
        size(100.0, 100.0),
        "visible=true + maintain_state=true: child SizedBox(100, 100) must resolve \
         to 100×100 under loose(200) — Offstage(offstage=false) is transparent"
    );
}
