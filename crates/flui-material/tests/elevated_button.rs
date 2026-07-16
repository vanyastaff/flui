//! `ElevatedButton` widget-level integration coverage — mounts a real button
//! through the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/ink_well.rs`/`tests/material.rs`'s established pattern) and drives
//! real pointer dispatch. Hit-testing runs inside `enter_owner_scope` (see
//! `common::LaidOut::route_event`'s doc comment) since `Material`'s clip
//! resolves through the owner-lane path-clipper registry — mounting without
//! it would silently degrade to the whole-box fallback clip instead of
//! erroring, which is exactly the trap that module's doc comment warns
//! about.
//!
//! `ElevatedButton` stands in for the whole `ButtonStyleButtonCore`
//! composition here; `FilledButton`/`OutlinedButton`/`TextButton` share the
//! identical composition path (only their `default_style` tables differ,
//! covered by each file's own unit tests), so one button's worth of
//! integration coverage is enough to prove the wiring, not four.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{ElevatedButton, Theme, ThemeData};
use flui_widgets::Text;

#[test]
fn tap_fires_on_pressed_and_the_button_mounts_a_material_surface() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            ElevatedButton::new(Text::new("Save")).on_pressed(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        ),
        tight(120.0, 48.0),
    );

    assert!(
        laid.find_by_render_type("RenderPhysicalShape").is_some(),
        "ElevatedButton must compose a Material (RenderPhysicalShape) surface",
    );

    laid.dispatch_pointer_down(60.0, 24.0);
    laid.dispatch_pointer_up(60.0, 24.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on an enabled ElevatedButton must fire on_pressed exactly once",
    );
}

#[test]
fn a_button_with_no_press_handler_is_disabled_and_a_tap_dispatch_is_a_no_op() {
    // No `.on_pressed(..)`: `ButtonStyleButtonCore::is_interactive` is
    // false, so the inner `InkWell` never gets an `on_tap` closure at all
    // (unit-tested directly at the construction level by
    // `elevated_button::tests::is_disabled_when_no_press_handler_is_set`).
    // What only an end-to-end mount can prove: a real pointer down+up
    // dispatched at a disabled button's composed
    // ConstrainedBox/Material/InkWell/Padding stack does not panic and
    // leaves the composition mounted — a regression guard against any of
    // those four layers assuming an `on_tap` closure is always present.
    let laid = lay_out(
        Theme::new(ThemeData::light(), ElevatedButton::new(Text::new("Save"))),
        tight(120.0, 48.0),
    );
    let material_before = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("a disabled ElevatedButton must still mount its Material surface");

    laid.dispatch_pointer_down(60.0, 24.0);
    laid.dispatch_pointer_up(60.0, 24.0);

    let material_after = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("the Material surface must survive a tap dispatch");
    assert_eq!(
        material_before, material_after,
        "the disabled button's render tree must not be torn down or rebuilt \
         under a tap dispatch it does not react to",
    );
}
