//! `Card` widget-level integration coverage — mounts a real `Card` through
//! the full render pipeline (`tests/common/mod.rs`, the same harness
//! `tests/material.rs`/`tests/elevated_button.rs` use) and probes the
//! composed [`Material`](flui_material::Material) (`RenderPhysicalShape`)
//! and [`Padding`](flui_widgets::Padding) (`RenderPadding`) render objects it
//! produces, proving `_CardDefaultsM3` actually reaches paint configuration
//! rather than just being computed in isolation.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, tight};
use flui_material::{Card, MaterialShape, Theme, ThemeData};
use flui_types::Color;
use flui_types::geometry::{Radius, px};
use flui_types::styling::BorderRadius;
use flui_widgets::{ColoredBox, GestureDetector};

/// `_CardDefaultsM3`'s formatted `Debug` string for a resolved
/// [`Color`](flui_types::Color) — what `RenderPhysicalShape`'s
/// `Diagnosticable::debug_fill_properties` writes into its `"color"`
/// property, mirroring `tests/elevated_button.rs`'s identical helper.
fn color_property(color: Color) -> String {
    format!("{color:?}")
}

#[test]
fn default_material_matches_card_defaults_m3() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let laid = lay_out(
        Theme::new(theme, Card::new(ColoredBox::new(Color::rgb(1, 2, 3)))),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material (RenderPhysicalShape) surface");

    let color = laid
        .render_property(material, "color")
        .expect("RenderPhysicalShape reports a \"color\" diagnostics property");
    assert_eq!(
        color,
        color_property(colors.surface_container_low),
        "_CardDefaultsM3.color is ColorScheme.surfaceContainerLow"
    );

    let elevation = laid
        .render_property(material, "elevation")
        .expect("RenderPhysicalShape reports an \"elevation\" diagnostics property");
    assert_eq!(
        elevation.parse::<f32>(),
        Ok(1.0),
        "_CardDefaultsM3 constructs with elevation: 1.0"
    );

    let clip_behavior = laid
        .render_property(material, "clip_behavior")
        .expect("RenderPhysicalShape reports a \"clip_behavior\" diagnostics property");
    assert_eq!(
        clip_behavior, "None",
        "_CardDefaultsM3 constructs with clipBehavior: Clip.none"
    );
}

/// `_CardDefaultsM3`'s 12dp corner radius (`card.dart`, oracle tag `3.44.0`),
/// proven against the REAL mounted `RenderPhysicalShape` clip via
/// hit-testing — not `MaterialShape::to_rrect` computed in isolation.
///
/// The probe point `(10, 10)` is chosen empirically (measured directly
/// against `MaterialShape::to_path`/`Path::contains`, not derived from an
/// idealized inscribed-circle formula — the corner's actual point-in-path
/// test lands on the `x + y >= radius` side of a straight diagonal, not a
/// true arc, at the tolerance these probes sit at): included at the 12dp
/// default (`10 + 10 = 20 >= 12`), excluded once the radius grows past `20`
/// (companion test
/// `an_overridden_99dp_corner_radius_excludes_the_same_probe_point` below,
/// `10 + 10 = 20 < 99`) — i.e. this probe is chosen specifically to flip if
/// the default drifts from 12.0 to 99.0, the exact drift a prior review
/// caught this test missing.
#[test]
fn default_corner_radius_reaches_the_mounted_material() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(
                GestureDetector::new()
                    .on_tap(move || {
                        counted.fetch_add(1, Ordering::SeqCst);
                    })
                    .child(ColoredBox::new(Color::rgb(5, 5, 5))),
            ),
        ),
        tight(400.0, 400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");
    let origin = laid.absolute_offset(material);

    laid.dispatch_pointer_down(origin.dx.get() + 10.0, origin.dy.get() + 10.0);
    laid.dispatch_pointer_up(origin.dx.get() + 10.0, origin.dy.get() + 10.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "_CardDefaultsM3's 12dp corner radius must INCLUDE a point 10px from the corner along \
         the diagonal — the mounted Material's actual registered clip, not merely \
         MaterialShape's geometry computed in isolation"
    );
}

/// Companion to `default_corner_radius_reaches_the_mounted_material`: the
/// SAME probe point, against an explicit 99dp override, must be EXCLUDED —
/// proving the mounted hit-test is actually sensitive to the corner radius
/// (a 12→99 drift in the default would flip the test above to this result).
#[test]
fn an_overridden_99dp_corner_radius_excludes_the_same_probe_point() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(
                GestureDetector::new()
                    .on_tap(move || {
                        counted.fetch_add(1, Ordering::SeqCst);
                    })
                    .child(ColoredBox::new(Color::rgb(5, 5, 5))),
            )
            .shape(MaterialShape::RoundedRect(BorderRadius::all(
                Radius::circular(px(99.0)),
            ))),
        ),
        tight(400.0, 400.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");
    let origin = laid.absolute_offset(material);

    laid.dispatch_pointer_down(origin.dx.get() + 10.0, origin.dy.get() + 10.0);
    laid.dispatch_pointer_up(origin.dx.get() + 10.0, origin.dy.get() + 10.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "a 99dp corner radius must EXCLUDE the same probe point the 12dp default includes"
    );
}

/// `_CardDefaultsM3`'s margin (`EdgeInsets.all(4.0)`) reaches the composed
/// `Padding`: under a fixed-size root the `Material` must sit inset by
/// exactly 4 logical pixels on every side, positioned at `(4, 4)`.
#[test]
fn default_margin_insets_the_material_by_four_pixels() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(ColoredBox::new(Color::rgb(1, 2, 3))),
        ),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");

    assert_eq!(
        laid.offset(material),
        common::offset(4.0, 4.0),
        "_CardDefaultsM3's margin (EdgeInsets.all(4.0)) must inset the Material by 4px"
    );
}

/// An explicit `.margin(...)` override reaches the same `Padding`, replacing
/// the `_CardDefaultsM3` default rather than composing with it.
#[test]
fn margin_override_replaces_the_default_inset() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(ColoredBox::new(Color::rgb(1, 2, 3)))
                .margin(flui_types::EdgeInsets::all(flui_types::geometry::px(10.0))),
        ),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");

    assert_eq!(
        laid.offset(material),
        common::offset(10.0, 10.0),
        "an overridden margin must replace _CardDefaultsM3's 4px default"
    );
}

/// The child actually renders inside the `Material` surface: a `ColoredBox`
/// child mounts its `RenderDecoratedBox` as the `Material`'s only child.
#[test]
fn child_mounts_inside_the_material_surface() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            Card::new(ColoredBox::new(Color::rgb(9, 9, 9))),
        ),
        tight(200.0, 200.0),
    );

    let material = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("Card must compose a Material surface");
    let child = laid.only_child(material);

    let decoration = laid
        .render_property(child, "decoration")
        .expect("RenderDecoratedBox reports a \"decoration\" diagnostics property");
    assert!(
        decoration.contains(&color_property(Color::rgb(9, 9, 9))),
        "the ColoredBox child must mount as the Material's child with its own color intact, \
         not be dropped or repainted over — got decoration {decoration:?}"
    );
    assert_eq!(
        laid.size(child),
        laid.size(material),
        "a childless-margin ColoredBox must fill the Material's whole content area"
    );
}
