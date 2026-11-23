//! `Material` widget-level coverage — proves the shape actually reaches
//! paint/hit-test through the real owner-lane path-clip registration, not
//! just that `MaterialShape::to_path` computes the right geometry in
//! isolation.
//!
//! `RenderPhysicalModelBase::hit_test` always tests the resolved clip shape
//! before testing the child (`crates/flui-objects/src/proxy/physical_model.rs`
//! — a deliberate FLUI-wide divergence from the oracle, which gates this on
//! `_clipper != null`). For the `PathClip` variant `Material` uses,
//! `compute_clip` calls `flui_interaction::pointer::resolve_path_clip_target`
//! against the **actually registered** `PathClipTarget` — the same
//! `sync_path_clip_target` wiring `create_render_object`/`update_render_object`
//! install during a real mount. So a corner-point hit-test through a fully
//! mounted `Material` exercises the live registration end to end: if
//! `sync_path_clip_target` ever stopped forwarding the configured `shape`
//! (e.g. closed over a stale/default value instead), these tests would flip.

use crate::common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{LaidOut, lay_out, rgba, tight};
use flui_material::{Material, MaterialShape, Theme, ThemeData};
use flui_sdk::foundation::inspect::Rgba;
use flui_sdk::painting::styling::Color;
use flui_sdk::view::EventContext;
use flui_sdk::widgets::{ColoredBox, GestureDetector, SizedBox};

/// A point near the top-left corner of a 120x40 rect: outside the Stadium's
/// inscribed corner circle (radius = shortest_side/2 = 20, centered at
/// (20, 20); distance from (2, 2) is ≈25.5 > 20) but inside the plain
/// bounding rectangle a sharp-cornered shape would fill.
const CORNER_PROBE: (f64, f64) = (2.0, 2.0);

fn tap_counter() -> (Arc<AtomicUsize>, impl Fn(&mut EventContext<'_>) + 'static) {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    (taps, move |_ctx: &mut EventContext<'_>| {
        counted.fetch_add(1, Ordering::SeqCst);
    })
}

pub fn stadium_shape_excludes_a_corner_a_sharp_rectangle_would_include() {
    let (taps, on_tap) = tap_counter();
    let laid = lay_out(
        Material::new()
            .color(Color::WHITE)
            .shape(MaterialShape::Stadium)
            .child(
                GestureDetector::new()
                    .on_tap(on_tap)
                    .child(ColoredBox::new(Color::rgb(200, 10, 10))),
            ),
        tight(120.0, 40.0),
    );

    laid.dispatch_pointer_down(CORNER_PROBE.0, CORNER_PROBE.1);
    laid.dispatch_pointer_up(CORNER_PROBE.0, CORNER_PROBE.1);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        0,
        "Material's registered Stadium clip must exclude this corner point from hit-testing, \
         so the wrapped GestureDetector never sees the tap"
    );
}

/// The fill color the one mounted `Material` paints.
fn painted_color(laid: &LaidOut) -> Rgba {
    let surface = laid
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("one Material surface is mounted");
    laid.render_node(surface)
        .get::<Rgba>("color")
        .expect("the surface reports its fill color")
}

/// A `Material` given no color paints the theme's `color_scheme.surface`
/// and repaints with the new surface when the theme changes; one given a
/// color keeps it under any theme.
pub fn a_material_without_a_color_paints_the_theme_surface() {
    let recolored = {
        let mut theme = ThemeData::light();
        theme.color_scheme.surface = Color::rgb(12, 34, 56);
        theme
    };
    let surface = || Material::new().child(SizedBox::new(10.0, 10.0));

    let mut laid = lay_out(Theme::new(ThemeData::light(), surface()), tight(40.0, 40.0));
    assert_eq!(
        painted_color(&laid),
        rgba(ThemeData::light().color_scheme.surface)
    );

    laid.pump_widget(Theme::new(recolored.clone(), surface()));
    assert_eq!(painted_color(&laid), rgba(Color::rgb(12, 34, 56)));

    laid.pump_widget(Theme::new(ThemeData::dark(), surface()));
    assert_eq!(
        painted_color(&laid),
        rgba(ThemeData::dark().color_scheme.surface)
    );

    let own = Color::rgb(200, 10, 10);
    let mut laid = lay_out(
        Theme::new(ThemeData::light(), surface().color(own)),
        tight(40.0, 40.0),
    );
    assert_eq!(painted_color(&laid), rgba(own));
    laid.pump_widget(Theme::new(recolored, surface().color(own)));
    assert_eq!(painted_color(&laid), rgba(own));

    // No `Theme` above: the light baseline, as Flutter's `ThemeData.fallback()`.
    let laid = lay_out(surface(), tight(40.0, 40.0));
    assert_eq!(
        painted_color(&laid),
        rgba(ThemeData::light().color_scheme.surface)
    );
}
