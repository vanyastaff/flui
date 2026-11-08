//! `Divider`/`VerticalDivider` widget-level integration coverage — mounts a
//! real `Divider` through the full render pipeline (`tests/common/mod.rs`,
//! the same harness `tests/card.rs` uses) and proves the M3 geometry
//! (height/thickness/indents) and the theme cascade actually reach a mounted
//! tree, not just `resolve_style` computed in isolation.

use crate::common;

use common::{lay_out, loose};
use flui_material::{Divider, DividerThemeData, Theme, ThemeData};
use flui_sdk::painting::styling::Color;

/// A widget-level `.color(...)` override wins over a configured
/// `DividerThemeData.color` — the standard widget → theme → default cascade.
pub fn widget_color_override_wins_over_the_divider_theme() {
    let theme = ThemeData::light().with_component(DividerThemeData {
        color: Some(Color::rgb(1, 1, 1)),
        ..Default::default()
    });
    let widget_color = Color::rgb(9, 9, 9);

    let laid = lay_out(
        Theme::new(theme, Divider::new().color(widget_color)),
        loose(400.0),
    );

    let decorated = laid
        .try_find_by_render_type("RenderContainer")
        .expect("Divider must compose a decorated (filled) line");
    let decoration = laid
        .render_debug_field(decorated, "decoration")
        .expect("RenderContainer reports a \"decoration\" diagnostics property");

    assert!(
        decoration.contains(&format!("{widget_color:?}")),
        "an explicit Divider::color override must win over DividerThemeData.color — got \
         {decoration:?}"
    );
}
