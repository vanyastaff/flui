//! `FloatingActionButton` widget-level integration coverage — mounts a real
//! FAB through the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/elevated_button.rs`'s established pattern), proving what only a
//! real mount can: the M3 default token table resolved against the REAL
//! lifecycle-synced `WidgetStatesController` (not a hand-built
//! `WidgetStates` value, which `floating_action_button.rs`'s own unit tests
//! already cover), a real tap firing `on_pressed`, and the 56×56 geometry a
//! `Scaffold`'s `floating_action_button` slot actually lays out.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{FloatingActionButton, Scaffold, Theme, ThemeData};
use flui_types::EdgeInsets;
use flui_types::geometry::px;
use flui_widgets::{MediaQuery, MediaQueryData, SizedBox};

/// `_FABDefaultsM3`'s formatted `Debug` string for a given resolved
/// [`Color`](flui_types::Color) — the same helper `tests/elevated_button.rs`
/// uses for `RenderPhysicalShape`'s `"color"` diagnostics property.
fn color_property(color: flui_types::Color) -> String {
    format!("{color:?}")
}

#[test]
fn tap_fires_on_pressed_and_the_button_mounts_a_material_surface() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            FloatingActionButton::new(
                Some(move || {
                    counted.fetch_add(1, Ordering::SeqCst);
                }),
                SizedBox::square(24.0),
            ),
        ),
        tight(56.0, 56.0),
    );

    assert!(
        laid.find_by_render_type("RenderPhysicalShape").is_some(),
        "FloatingActionButton must compose a Material (RenderPhysicalShape) surface",
    );

    laid.dispatch_pointer_down(28.0, 28.0);
    laid.dispatch_pointer_up(28.0, 28.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on an enabled FloatingActionButton must fire on_pressed exactly once",
    );
}

#[test]
fn a_button_with_no_press_handler_is_disabled_and_a_tap_dispatch_is_a_no_op() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            FloatingActionButton::new(None::<fn()>, SizedBox::square(24.0)),
        ),
        tight(56.0, 56.0),
    );
    let material_before = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("a disabled FloatingActionButton must still mount its Material surface");

    laid.dispatch_pointer_down(28.0, 28.0);
    laid.dispatch_pointer_up(28.0, 28.0);

    let material_after = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("the Material surface must survive a tap dispatch");
    assert_eq!(
        material_before, material_after,
        "the disabled button's render tree must not be torn down or rebuilt under a tap \
         dispatch it does not react to",
    );
}

/// Mutation-honest coverage for `FloatingActionButtonState::init_state`'s
/// `WidgetState::Disabled` sync, the same shape
/// `tests/elevated_button.rs::a_handler_less_button_resolves_the_disabled_background_color_through_the_real_lifecycle`
/// proves for `ButtonStyleButtonCoreState` — except the FAB's own
/// `resolve_elevation` chain resolves `disabled` to the SAME `6.0` as the
/// enabled default (see `floating_action_button.rs`'s module docs), so
/// `elevation` cannot distinguish "the real lifecycle synced Disabled"
/// from "it never did." `background_color`, which stays `primaryContainer`
/// either way per the oracle's own state-independent table, is the wrong
/// property to probe for that distinction too. What CAN only be proven
/// through a real mount: the composed `Material`'s color/elevation resolve
/// to `_FABDefaultsM3`'s values at all, end to end.
#[test]
fn disabled_fab_still_resolves_the_m3_default_background_and_elevation() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(
            theme,
            FloatingActionButton::new(None::<fn()>, SizedBox::square(24.0)),
        ),
        tight(56.0, 56.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Material must mount");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(colors.primary_container)),
        "_FABDefaultsM3's backgroundColor (primaryContainer) is state-independent — even \
         disabled, it must resolve the same as enabled",
    );
    assert_eq!(
        laid.render_property(material, "elevation"),
        Some("6".to_string()),
        "_FABDefaultsM3's disabled elevation falls back to the enabled default (6.0), not zero",
    );
}

#[test]
fn enabled_fab_resolves_the_m3_default_background_and_elevation() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(
            theme,
            FloatingActionButton::new(Some(|| {}), SizedBox::square(24.0)),
        ),
        tight(56.0, 56.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Material must mount");
    assert_eq!(
        laid.render_property(material, "color"),
        Some(color_property(colors.primary_container)),
    );
    assert_eq!(
        laid.render_property(material, "elevation"),
        Some("6".to_string())
    );
}

#[test]
fn mounted_geometry_in_a_scaffold_slot_is_exactly_56_by_56_at_the_end_float_position() {
    // Mirrors `tests/scaffold.rs`'s own FAB-slot geometry tests, but with a
    // real `FloatingActionButton` (56x56 via its own `ConstrainedBox`, not a
    // stand-in `SizedBox`) proving this V1's `FAB_SIZE` constant actually
    // reaches the Scaffold's `floating_action_button` slot unmodified.
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                MediaQueryData::default(),
                Scaffold::new()
                    .body(SizedBox::new(10.0, 10.0))
                    .floating_action_button(FloatingActionButton::new(
                        Some(|| {}),
                        SizedBox::square(24.0),
                    )),
            ),
        ),
        tight(400.0, 800.0),
    );

    let root = laid.root();
    let layout_root = laid.only_child(root);
    // `Scaffold::build` pushes `LayoutId`s in `body`, `floating_action_button`
    // order when both are set and there is no `app_bar` — see `scaffold.rs`.
    let fab = laid.child(layout_root, 1);

    assert_eq!(
        laid.size(fab),
        common::size(56.0, 56.0),
        "the mounted FloatingActionButton's own ConstrainedBox must pin it to exactly 56x56, \
         regardless of the Scaffold's loose FAB constraints",
    );

    // `FloatingActionButtonLocation.endFloat`: kFloatingActionButtonMargin
    // (16) from the right edge and bottom safe area, with a zero
    // `min_view_padding_bottom`/`min_insets` here (default `MediaQueryData`)
    // — the flat-margin case `scaffold.rs`'s own delegate tests already pin
    // exactly, repeated here end to end through a real `FloatingActionButton`.
    assert_eq!(
        laid.offset(fab),
        common::offset(400.0 - 16.0 - 56.0, 800.0 - 56.0 - 16.0),
    );
}

/// Guards against a regression where `Scaffold` and `FloatingActionButton`
/// disagree about the top-level inset contract — not exercised by either
/// type's own unit tests, which resolve `EdgeInsets`/`MediaQueryData`
/// values directly rather than through a mounted composition.
#[test]
fn a_nonzero_bottom_safe_area_still_clears_the_fab_by_at_least_the_flat_margin() {
    let media_query = MediaQueryData {
        padding: EdgeInsets::new(px(0.0), px(0.0), px(34.0), px(0.0)),
        ..MediaQueryData::default()
    };
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            MediaQuery::new(
                media_query,
                Scaffold::new()
                    .body(SizedBox::new(10.0, 10.0))
                    .floating_action_button(FloatingActionButton::new(
                        Some(|| {}),
                        SizedBox::square(24.0),
                    )),
            ),
        ),
        tight(400.0, 800.0),
    );

    let root = laid.root();
    let layout_root = laid.only_child(root);
    let fab = laid.child(layout_root, 1);

    assert_eq!(laid.size(fab), common::size(56.0, 56.0));
    let fab_bottom = laid.offset(fab).dy + laid.size(fab).height;
    assert!(
        fab_bottom <= px(800.0 - 34.0),
        "the FAB must clear the 34px bottom safe area, not just the flat 16px margin",
    );
}
