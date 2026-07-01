//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/safe_area_test.dart`
//!
//! Oracle tests ported:
//! - `'SafeArea - basic'` (line 13): `SafeArea(left: false)` with
//!   `MediaQueryData(padding: EdgeInsets.all(20))` on 800×600 positions the
//!   child at offset `(0, 20)` and sizes it to `780×560`.
//! - `'SafeArea - with minimums'` (line 24): `SafeArea(top: false, minimum:
//!   EdgeInsets.fromLTRB(0,10,20,30))` with `padding: EdgeInsets.all(20)` —
//!   left = max(20, 0) = 20, top = max(0, 10) = 10, right = max(20, 20) = 20,
//!   bottom = max(20, 30) = 30 → child offset `(20, 10)`, size `760×560`.
//!
//! Widget → render-object mapping:
//! - `SafeArea` → `RenderPadding` (child of the MediaQuery tree)
//! - child `SizedBox::shrink` → `RenderConstrainedBox` (child of `RenderPadding`)
//!
//! Divergence:
//! - Flutter's `SafeArea` wraps its child in `MediaQuery.removePadding` to
//!   zero the consumed edges. FLUI defers `MediaQuery.removePadding`; nested
//!   `SafeArea`s will over-pad. This is documented on [`SafeArea`].
//! - Flutter's test uses `find.byType(Placeholder)` + `getTopLeft/getBottomRight`;
//!   FLUI uses `find_by_render_type("RenderConstrainedBox")` + `offset`/`size`.
//! - `SizedBox::shrink` (0×0 additional constraints enforced against the
//!   incoming tight deflated constraints) produces a child of size `0×0`; the
//!   `RenderPadding` child's paint offset is the effective insets' `(left, top)`.

use crate::common::offset;
use crate::harness;
use flui_geometry::{EdgeInsets, px};
use flui_widgets::{MediaQuery, MediaQueryData, SafeArea, SizedBox};

/// `SafeArea(left: false)` with `MediaQueryData(padding: 20 all)` positions
/// its child at `(0, 20)` — left edge un-inset, top inset by 20 px.
///
/// Flutter parity: `safe_area_test.dart` line 13 (`'SafeArea - basic'`):
/// `getTopLeft(Placeholder) == Offset(0.0, 20.0)`.
#[test]
fn safe_area_left_false_child_offset_matches_oracle() {
    let root = MediaQuery::new(
        MediaQueryData {
            padding: EdgeInsets::all(px(20.0)),
            ..MediaQueryData::default()
        },
        SafeArea::new().left(false).child(SizedBox::shrink()),
    );
    let laid = harness::pump_widget(root, harness::screen());

    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.offset(child_id),
        offset(0.0, 20.0),
        "SafeArea(left=false) + padding=20 all: child must be at offset (0, 20) \
         (flutter: getTopLeft(Placeholder) == Offset(0.0, 20.0))"
    );
}

/// `SafeArea(top: false, minimum: fromLTRB(0,10,20,30))` with `padding=20 all`
/// computes `max(toggle ? media : 0, min)` per edge:
///   left   = max(20,  0) = 20
///   top    = max( 0, 10) = 10
///   right  = max(20, 20) = 20
///   bottom = max(20, 30) = 30
/// Child is at offset `(20, 10)`.
///
/// Flutter parity: `safe_area_test.dart` line 24 (`'SafeArea - with minimums'`):
/// `getTopLeft(Placeholder) == Offset(20.0, 10.0)`.
#[test]
fn safe_area_minimum_insets_applied_per_edge() {
    let minimum = EdgeInsets::new(
        px(10.0), // top
        px(20.0), // right
        px(30.0), // bottom
        px(0.0),  // left
    );
    let root = MediaQuery::new(
        MediaQueryData {
            padding: EdgeInsets::all(px(20.0)),
            ..MediaQueryData::default()
        },
        SafeArea::new()
            .top(false)
            .minimum(minimum)
            .child(SizedBox::shrink()),
    );
    let laid = harness::pump_widget(root, harness::screen());

    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.offset(child_id),
        offset(20.0, 10.0),
        "SafeArea(top=false, minimum=(0,10,20,30)) + padding=20 all: \
         child must be at offset (20, 10) \
         (flutter: getTopLeft(Placeholder) == Offset(20.0, 10.0))"
    );
}

/// `SafeArea` with no OS padding and no minimum emits zero insets — the child
/// is at offset `(0, 0)` (no-op).
///
/// Edge case: `MediaQueryData::default()` has `padding = EdgeInsets::ZERO`.
/// SafeArea must not introduce any inset when both media padding and minimum
/// are zero. The child paint offset must be `(0, 0)`.
///
/// Note: the child's size under tight(800, 600) constraints equals the full
/// surface (SizedBox::shrink additional constraints are clamped up to the
/// incoming tight min); what matters here is that no offset is applied.
#[test]
fn safe_area_zero_padding_child_has_zero_offset() {
    let root = MediaQuery::new(
        MediaQueryData::default(), // padding = ZERO
        SafeArea::new().child(SizedBox::shrink()),
    );
    let laid = harness::pump_widget(root, harness::screen());

    let child_id = laid.find_by_render_type("RenderConstrainedBox");
    assert_eq!(
        laid.offset(child_id),
        offset(0.0, 0.0),
        "SafeArea with zero padding and zero minimum must not inset the child \
         (paint offset must be (0, 0))"
    );
}
