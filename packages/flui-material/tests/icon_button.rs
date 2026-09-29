//! `IconButton` widget-level integration coverage — mounts a real button
//! through the full render pipeline (`tests/common/mod.rs`, matching
//! `tests/elevated_button.rs`'s established pattern).
//!
//! `IconButton` rides the same `ButtonStyleButtonCore` composition
//! `ElevatedButton` does (only `default_style` differs, covered by
//! `icon_button.rs`'s own unit tests), so this file's job is narrower: prove
//! the parts unique to `IconButton` — the 40×40 minimum-size constraint
//! actually reaching a mounted button, a real tap, and (the part
//! `icon_button.rs`'s unit tests structurally cannot reach, since
//! `IconButton::build` resolves `icon_color` against a `WidgetStates`
//! snapshot it builds itself, not `ButtonStyleButtonCore`'s own
//! `WidgetStatesController`) that the disabled/enabled/overridden icon color
//! actually reaches the `IconTheme` ancestor the icon child reads.

use crate::common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{lay_out, tight};
use flui_material::{
    ButtonStyle, IconButton, IconButtonThemeData, Theme, ThemeData, ThemeDataOverrides,
};
use flui_sdk::painting::Color;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{IconTheme, IconThemeData, SizedBox, WidgetStateProperty};

/// Captures the ambient [`IconThemeData`] its parent publishes at build
/// time — the same probe shape `tests/scaffold.rs`'s `MediaQueryProbe` uses
/// for `MediaQuery`, applied here to prove `IconButton` actually threads its
/// resolved `icon_color`/`icon_size` down through a real `IconTheme`
/// ancestor, not just that `default_style`'s own `foreground_color` slot
/// resolves correctly in isolation (already covered by
/// `icon_button.rs`'s unit tests).
#[derive(Clone, StatelessView)]
struct IconThemeProbe {
    captured: Rc<RefCell<Option<IconThemeData>>>,
}

impl StatelessView for IconThemeProbe {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        *self.captured.borrow_mut() = Some(IconTheme::of(ctx));
        SizedBox::new(10.0, 10.0)
    }
}

/// The middle cascade tier, proven end to end: a configured
/// `icon_button_theme.style.foreground_color` must reach the icon's
/// `IconTheme` — the same coalesce `resolve_property` performs for
/// `IconButton::build`'s widget-level override (see
/// `a_style_foreground_color_override_reaches_the_icons_icon_theme` below),
/// now with a theme-tier value and no widget-level override in the way.
pub fn icon_button_theme_slot_reaches_the_icons_icon_theme() {
    let themed_color = Color::rgb(30, 40, 50);
    let captured = Rc::new(RefCell::new(None));
    let probe = IconThemeProbe {
        captured: Rc::clone(&captured),
    };
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        icon_button_theme: Some(IconButtonThemeData {
            style: Some(ButtonStyle {
                foreground_color: Some(WidgetStateProperty::all(Some(themed_color))),
                ..Default::default()
            }),
        }),
        ..Default::default()
    });

    let _laid = lay_out(
        Theme::new(theme, IconButton::new(probe).on_pressed(|_cx| {})),
        tight(40.0, 40.0),
    );

    let resolved = captured
        .borrow()
        .clone()
        .expect("IconThemeProbe must have built at least once");
    assert_eq!(
        resolved.color,
        Some(themed_color),
        "a configured icon_button_theme.style.foreground_color must reach the icon's IconTheme",
    );
}
