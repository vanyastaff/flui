//! ## Test parity notes
//!
//! Flutter source: `packages/flutter/test/widgets/flex_test.dart` line 84
//! Test name: `"Doesn't overflow because of floating point accumulated error"`
//!
//! Widget → render-object mapping:
//! - `SizedBox(height)` → `RenderConstrainedBox` (root)
//! - `Column` → `RenderFlex` (vertical)
//! - `Expanded(SizedBox())` → `RenderConstrainedBox` (leaf, inside `RenderFlex`)
//!
//! Divergence: Flutter's test uses `Center(child: SizedBox(...))` as the outer
//! wrapper; FLUI's port drops the `Center` and uses `SizedBox` directly as the
//! root since the geometry invariant (sum of flex shares == parent height) is
//! independent of outer centering.

use crate::common::{lay_out, loose, size};
use flui_view::ViewExt;
use flui_widgets::{Column, Expanded, SizedBox};

/// Six `Expanded` children in a 400 px `Column` must sum exactly to 400 px
/// with no floating-point overflow.
///
/// Flutter parity: flex_test.dart line 84 — first `pumpWidget` call (400 px
/// height). The test exists because accumulated rounding of `N × (H / N)` can
/// exceed `H` by an ULP, triggering a debug assertion inside the flex layout.
#[test]
fn six_expanded_children_in_400px_column_fit_without_overflow() {
    let laid = lay_out(
        SizedBox::new(200.0, 400.0).child(Column::new(vec![
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
        ])),
        loose(1000.0),
    );
    // Root (SizedBox) constrains to 200×400; Column (RenderFlex) fills it.
    assert_eq!(
        laid.size(laid.root()),
        size(200.0, 400.0),
        "root SizedBox must remain at 200×400"
    );
    let flex_node = laid.only_child(laid.root());
    assert_eq!(
        laid.size(flex_node),
        size(200.0, 400.0),
        "Column must fill its 400 px height with no overflow"
    );
}

/// Six `Expanded` children in a 199 px `Column` must sum exactly to 199 px —
/// a non-divisible height that historically triggered accumulated FP error.
///
/// Flutter parity: flex_test.dart line 84 — second `pumpWidget` call (199 px
/// height). This is the harder case: `⌊199 / 6⌋ × 6 = 198` with a remainder
/// that must be distributed correctly by the flex algorithm.
#[test]
fn six_expanded_children_in_199px_column_fit_without_overflow() {
    let laid = lay_out(
        SizedBox::new(200.0, 199.0).child(Column::new(vec![
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
            Expanded::new(SizedBox::shrink()).boxed(),
        ])),
        loose(1000.0),
    );
    assert_eq!(
        laid.size(laid.root()),
        size(200.0, 199.0),
        "root SizedBox must remain at 200×199"
    );
    let flex_node = laid.only_child(laid.root());
    assert_eq!(
        laid.size(flex_node),
        size(200.0, 199.0),
        "Column must fill its 199 px height with no floating-point overflow"
    );
}
