//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/physical_model_test.dart`
//! (tag `3.44.0`, 3 `testWidgets` cases).
//!
//! Widget → render-object mapping:
//! - `PhysicalModel` → [`flui_objects::RenderPhysicalModel`]
//!   (`RenderPhysicalModelBase<RectangleClip>`)
//! - `PhysicalShape` → [`flui_objects::RenderPhysicalShape`]
//!   (`RenderPhysicalModelBase<PathClip>`)
//!   both in `crates/flui-objects/src/proxy/physical_model.rs`. The widget
//!   wrappers live in `crates/flui-widgets/src/physical_model/`.
//!
//! Ported cases (2 of 3):
//! - `'PhysicalModel updates clipBehavior in updateRenderObject'` —
//!   [`physical_model_update_render_object_transitions_clip_behavior_to_anti_alias`].
//! - `'PhysicalShape updates clipBehavior in updateRenderObject'` —
//!   [`physical_shape_update_render_object_transitions_clip_behavior_to_anti_alias`].
//!
//! Both oracle cases assert exactly one observable — `clipBehavior` — across
//! a `Clip.none -> Clip.antiAlias` `updateRenderObject` transition on the
//! same live render object; the ports below assert that single observable
//! faithfully, no more and no less (nothing else is asserted upstream to
//! port). The remaining VALUE fields each widget's `updateRenderObject`
//! also pushes (`shape`/`borderRadius`/`elevation`/`color`/`shadowColor`
//! for `PhysicalModel`; `elevation`/`color`/`shadowColor` for
//! `PhysicalShape`) are covered by the widget's own unit tests
//! (`crates/flui-widgets/src/physical_model/physical_model.rs::tests::update_render_object_pushes_every_field`
//! and the sibling test in `physical_shape.rs`). `PhysicalShape`'s
//! `clipper` push is the one write those unit tests can NOT assert: they
//! run against a detached `RenderObjectContext`, where
//! `sync_path_clip_target` short-circuits (no owner lane), and the parity
//! pumps exercise the registration live but assert only `clip_behavior` —
//! the path-clip RESOLUTION itself is pinned at render level by the
//! `harness_physical_shape_*` tests — not just the one this oracle file
//! happens to exercise end-to-end.
//!
//! Out of scope (1 of 3):
//! - `'PhysicalModel - clips when overflows and elevation is 0'` — two
//!   independent blockers, either one alone would already exclude it:
//!   1. The case's actual subject is a `RenderFlex` overflow: four
//!      `PhysicalModel`-wrapped `Text` children in a `Row` with no `Expanded`
//!      force an overflow, and the assertion is on the resulting
//!      `FlutterError`'s diagnostics (`'A RenderFlex overflowed by '`) — a
//!      `RenderErrorBox`-style overflow-diagnostics machinery FLUI's
//!      `RenderFlex` (`crates/flui-objects/src/layout/flex.rs`) does not
//!      implement at all (no overflow detection, no error diagnostics, no
//!      analog of `RenderErrorBox`). `PhysicalModel` itself is incidental to
//!      what's actually being tested here.
//!   2. Even with that machinery, the case's closing assertion is
//!      `matchesGoldenFile('physical_model_overflow.png')` — FLUI has no
//!      golden-file harness (standing gap, cited throughout this directory,
//!      e.g. `clip_test.rs`'s module doc).
//!
//! **Total: 3 oracle cases = 2 ported (both real green, full-fidelity: the
//! sole assertion each makes) + 1 out of scope (named, both blockers cited)
//! — no narrowing.**

use flui_types::Color;
use flui_types::Point;
use flui_types::Rect;
use flui_types::Size;
use flui_types::painting::{Clip, Path};
use flui_widgets::{PhysicalModel, PhysicalShape};

use crate::harness::{pump_widget, screen};

/// A whole-box rectangle clipper — `PhysicalShape::clipper`'s shape is
/// incidental to this test (only `clipBehavior` is observed), so the
/// simplest faithful `Fn(Size) -> Path` stands in for the oracle's
/// `ShapeBorderClipper(shape: CircleBorder())`.
fn whole_box_clipper(size: Size) -> Path {
    let mut path = Path::new();
    path.add_rect(Rect::from_origin_size(Point::ZERO, size));
    path
}

/// `PhysicalModel`'s `clip_behavior` starts at Flutter's default
/// (`Clip::None`), then follows the render object through a live
/// `update_render_object` call to `Clip::AntiAlias`.
///
/// Flutter parity: `physical_model_test.dart` `'PhysicalModel updates
/// clipBehavior in updateRenderObject'` (3.44.0).
#[test]
fn physical_model_update_render_object_transitions_clip_behavior_to_anti_alias() {
    let mut laid = pump_widget(PhysicalModel::new(Color::BLACK), screen());
    let id = laid.find_by_render_type("RenderPhysicalModel");
    assert_eq!(
        laid.clip_behavior(id),
        Clip::None,
        "PhysicalModel's default clip_behavior must be Clip::None"
    );

    laid.pump_widget(PhysicalModel::new(Color::BLACK).clip_behavior(Clip::AntiAlias));
    assert_eq!(laid.clip_behavior(id), Clip::AntiAlias);
}

/// `PhysicalShape`'s `clip_behavior` starts at Flutter's default
/// (`Clip::None`), then follows the render object through a live
/// `update_render_object` call to `Clip::AntiAlias` — the `PhysicalShape`
/// sibling of
/// [`physical_model_update_render_object_transitions_clip_behavior_to_anti_alias`].
///
/// Flutter parity: `physical_model_test.dart` `'PhysicalShape updates
/// clipBehavior in updateRenderObject'` (3.44.0).
#[test]
fn physical_shape_update_render_object_transitions_clip_behavior_to_anti_alias() {
    let mut laid = pump_widget(
        PhysicalShape::new(whole_box_clipper, Color::BLACK),
        screen(),
    );
    let id = laid.find_by_render_type("RenderPhysicalShape");
    assert_eq!(
        laid.clip_behavior(id),
        Clip::None,
        "PhysicalShape's default clip_behavior must be Clip::None"
    );

    laid.pump_widget(
        PhysicalShape::new(whole_box_clipper, Color::BLACK).clip_behavior(Clip::AntiAlias),
    );
    assert_eq!(laid.clip_behavior(id), Clip::AntiAlias);
}
