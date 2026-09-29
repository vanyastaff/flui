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
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{lay_out, loose, tight};
use flui_material::{
    ButtonStyle, IconButton, IconButtonThemeData, Theme, ThemeData, ThemeDataOverrides,
};
use flui_sdk::painting::Color;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{IconTheme, IconThemeData, SizedBox, WidgetState, WidgetStateProperty};

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
            IconButton::new(SizedBox::square(24.0)).on_pressed(move |_cx| {
                counted.fetch_add(1, Ordering::SeqCst);
            }),
        ),
        tight(40.0, 40.0),
    );

    assert!(
        laid.try_find_by_render_type("RenderPhysicalShape")
            .is_some(),
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
        .try_find_by_render_type("RenderPhysicalShape")
        .expect("a disabled IconButton must still mount its Material surface");

    laid.dispatch_pointer_down(20.0, 20.0);
    laid.dispatch_pointer_up(20.0, 20.0);

    let material_after = laid
        .try_find_by_render_type("RenderPhysicalShape")
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
            IconButton::new(SizedBox::square(10.0)).on_pressed(|_cx| {}),
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

/// The middle cascade tier, proven end to end: a configured
/// `icon_button_theme.style.foreground_color` must reach the icon's
/// `IconTheme` — the same coalesce `resolve_property` performs for
/// `IconButton::build`'s widget-level override (see
/// `a_style_foreground_color_override_reaches_the_icons_icon_theme` below),
/// now with a theme-tier value and no widget-level override in the way.
#[test]
fn icon_button_theme_slot_reaches_the_icons_icon_theme() {
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

/// Regression lock for the named divergence `icon_button.rs`'s module docs
/// extend to the theme tier: a state-varying
/// `icon_button_theme.style.foreground_color` is resolved ONCE, against the
/// static enabled/disabled snapshot `IconButton::build` builds itself, and
/// frozen into the icon's `IconTheme` — a REAL hover afterward does not
/// re-resolve it, even though `ButtonStyleButtonCore`'s own `InkWell` DOES
/// track that live hover for its own background/overlay. If a future change
/// starts sharing a live states controller for this icon color, this
/// assertion's expected value would need to flip to `hovered_color` — that
/// is the intended, honest failure mode of a regression-locking test for a
/// named limitation, not a correctness bug this test exists to catch.
#[test]
fn a_hover_varying_icon_button_theme_foreground_color_stays_frozen_at_the_initial_snapshot() {
    let enabled_color = Color::rgb(10, 20, 30);
    let hovered_color = Color::rgb(200, 210, 220);
    let captured = Rc::new(RefCell::new(None));
    let probe = IconThemeProbe {
        captured: Rc::clone(&captured),
    };
    let theme = ThemeData::light().copy_with(ThemeDataOverrides {
        icon_button_theme: Some(IconButtonThemeData {
            style: Some(ButtonStyle {
                foreground_color: Some(WidgetStateProperty::resolve_with(move |states| {
                    Some(if states.contains_state(WidgetState::Hovered) {
                        hovered_color
                    } else {
                        enabled_color
                    })
                })),
                ..Default::default()
            }),
        }),
        ..Default::default()
    });

    let laid = lay_out(
        Theme::new(theme, IconButton::new(probe).on_pressed(|_cx| {})),
        tight(40.0, 40.0),
    );

    let before_hover = captured
        .borrow()
        .clone()
        .expect("IconThemeProbe must have built at least once")
        .color;
    assert_eq!(
        before_hover,
        Some(enabled_color),
        "the initial (non-hovered) snapshot must resolve the enabled branch",
    );

    // A contactless mouse hover (no button held) — the shared harness's
    // `dispatch_pointer_move` is a contact drag and requires a preceding
    // Down; hover is its own event shape, matching what a platform emits.
    laid.dispatch_pointer_hover(20.0, 20.0);

    let after_hover = captured.borrow().clone().unwrap().color;
    assert_eq!(
        after_hover,
        Some(enabled_color),
        "a real hover must NOT change the icon's IconTheme color — the theme-tier \
         foreground_color was resolved once against the static enabled/disabled snapshot and \
         stays frozen, the same named divergence the widget-level style override already \
         carries (see icon_button.rs's module docs)",
    );
}

#[test]
fn a_style_foreground_color_override_reaches_the_icons_icon_theme() {
    let overridden = Color::rgb(200, 10, 90);
    let captured = Rc::new(RefCell::new(None));
    let probe = IconThemeProbe {
        captured: Rc::clone(&captured),
    };

    let _laid = lay_out(
        Theme::new(
            ThemeData::light(),
            IconButton::new(probe)
                .on_pressed(|_cx| {})
                .style(ButtonStyle {
                    foreground_color: Some(WidgetStateProperty::all(Some(overridden))),
                    ..ButtonStyle::default()
                }),
        ),
        tight(40.0, 40.0),
    );

    let resolved = captured
        .borrow()
        .clone()
        .expect("IconThemeProbe must have built at least once");
    assert_eq!(
        resolved.color,
        Some(overridden),
        "a widget-level ButtonStyle.foreground_color override must reach the icon's ambient \
         IconTheme, not just ButtonStyleButtonCore's DefaultTextStyle",
    );
}
