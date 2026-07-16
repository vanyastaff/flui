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
//! `IconButton::build` computes `icon_color` directly from
//! `self.on_pressed.is_some()`, not from `ButtonStyleButtonCore`'s own
//! `WidgetStatesController`) that the disabled/enabled icon color actually
//! reaches the `IconTheme` ancestor the icon child reads.

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, loose, tight};
use flui_material::{IconButton, Theme, ThemeData};
use flui_view::prelude::*;
use flui_widgets::{IconTheme, IconThemeData, SizedBox};

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

#[test]
fn tap_fires_on_pressed_and_the_button_mounts_a_material_surface() {
    let taps = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&taps);
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            IconButton::new(SizedBox::square(24.0)).on_pressed(move || {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        ),
        tight(40.0, 40.0),
    );

    assert!(
        laid.find_by_render_type("RenderPhysicalShape").is_some(),
        "IconButton must compose a Material (RenderPhysicalShape) surface",
    );

    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_up(20.0, 20.0);

    assert_eq!(
        taps.load(Ordering::SeqCst),
        1,
        "a down+up on an enabled IconButton must fire on_pressed exactly once",
    );
}

#[test]
fn a_button_with_no_press_handler_is_disabled_and_a_tap_dispatch_is_a_no_op() {
    let laid = lay_out(
        Theme::new(ThemeData::light(), IconButton::new(SizedBox::square(24.0))),
        tight(40.0, 40.0),
    );
    let material_before = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("a disabled IconButton must still mount its Material surface");

    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_up(20.0, 20.0);

    let material_after = laid
        .find_by_render_type("RenderPhysicalShape")
        .expect("the Material surface must survive a tap dispatch");
    assert_eq!(
        material_before, material_after,
        "the disabled button's render tree must not be torn down or rebuilt under a tap \
         dispatch it does not react to",
    );
}

#[test]
fn an_unconstrained_icon_button_collapses_to_the_40_by_40_m3_minimum_size() {
    let laid = lay_out(
        Theme::new(
            ThemeData::light(),
            IconButton::new(SizedBox::square(10.0)).on_pressed(|| {}),
        ),
        loose(200.0),
    );

    assert_eq!(
        laid.size(laid.root()),
        common::size(40.0, 40.0),
        "_IconButtonDefaultsM3.minimumSize (40x40) must be the mounted button's actual size \
         when nothing forces it larger",
    );
}

/// The "through the mount" pattern the button-family review requires: an
/// `IconButton` with no press handler must resolve the disabled foreground
/// color (`onSurface@38%`) all the way down to the `IconTheme` its icon
/// child actually reads — not merely inside `default_style`'s own
/// `WidgetStateProperty`, which `icon_button.rs`'s unit tests already
/// exercise directly.
#[test]
fn disabled_icon_button_resolves_the_disabled_color_through_the_real_mount() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let captured = Rc::new(RefCell::new(None));
    let probe = IconThemeProbe {
        captured: Rc::clone(&captured),
    };

    let _laid = lay_out(Theme::new(theme, IconButton::new(probe)), tight(40.0, 40.0));

    let resolved = captured
        .borrow()
        .clone()
        .expect("IconThemeProbe must have built at least once");
    assert_eq!(
        resolved.color,
        Some(colors.on_surface.with_opacity(0.38)),
        "a disabled IconButton must publish _IconButtonDefaultsM3's disabled foreground color \
         (onSurface@38%) to its icon child's IconTheme",
    );
    assert_eq!(resolved.size, Some(24.0));
}

#[test]
fn enabled_icon_button_resolves_the_on_surface_variant_color_through_the_real_mount() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let captured = Rc::new(RefCell::new(None));
    let probe = IconThemeProbe {
        captured: Rc::clone(&captured),
    };

    let _laid = lay_out(
        Theme::new(theme, IconButton::new(probe).on_pressed(|| {})),
        tight(40.0, 40.0),
    );

    let resolved = captured
        .borrow()
        .clone()
        .expect("IconThemeProbe must have built at least once");
    assert_eq!(
        resolved.color,
        Some(colors.on_surface_variant),
        "an enabled IconButton must publish _IconButtonDefaultsM3's onSurfaceVariant foreground \
         color to its icon child's IconTheme",
    );
}
