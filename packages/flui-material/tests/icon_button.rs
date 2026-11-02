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
/// `IconButton::build`'s widget-level override, here with a theme-tier value
/// and no widget-level override in the way.
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

/// A toggle button reports its selection as a toggle button's pressed state, both ways; a
/// button never made a toggle reports none.
pub fn a_toggle_icon_button_announces_its_selection() {
    use flui_testing::a11y::{Role, Toggled};

    let announced = |button: IconButton| {
        let mut laid = lay_out(
            Theme::new(ThemeData::light(), button.on_pressed(|_cx| {})),
            tight(40.0, 40.0),
        );
        laid.enable_semantics();
        laid.pump();
        laid.a11y_tree()
            .expect("semantics enabled before the frame")
            .find(Role::Button)
            .unwrap_or_else(|error| panic!("one button: {error}"))
            .toggled()
    };
    let icon = || SizedBox::new(24.0, 24.0);
    assert_eq!(
        announced(IconButton::new(icon()).selected(true)),
        Some(Toggled::True)
    );
    assert_eq!(
        announced(IconButton::new(icon()).selected(false)),
        Some(Toggled::False)
    );
    assert_eq!(
        announced(IconButton::new(icon())),
        None,
        "a plain button is not a toggle"
    );
}

/// A selected toggle's icon takes the scheme's `primary`, an unselected one stays
/// `on_surface_variant`, and a disabled one is dimmed whether selected or not.
pub fn a_selected_icon_button_takes_the_primary_color() {
    let theme = ThemeData::light();
    let colors = theme.color_scheme;
    let icon_color = |button: fn(IconThemeProbe) -> IconButton| {
        let captured = Rc::new(RefCell::new(None));
        let probe = IconThemeProbe {
            captured: Rc::clone(&captured),
        };
        let _laid = lay_out(Theme::new(theme.clone(), button(probe)), tight(40.0, 40.0));
        captured
            .borrow()
            .clone()
            .expect("IconThemeProbe must have built at least once")
            .color
    };
    assert_eq!(
        icon_color(|probe| IconButton::new(probe).selected(true).on_pressed(|_cx| {})),
        Some(colors.primary),
    );
    assert_eq!(
        icon_color(|probe| IconButton::new(probe).selected(false).on_pressed(|_cx| {})),
        Some(colors.on_surface_variant),
    );
    assert_eq!(
        icon_color(|probe| IconButton::new(probe).selected(true)),
        Some(colors.on_surface.with_opacity(0.38)),
        "disabled wins over selected",
    );
}

/// Selecting a toggle with a selected icon cross-fades from the plain icon to it, and only
/// the selected icon's label is heard from the first frame.
pub fn a_toggle_cross_fades_to_its_selected_icon() {
    use std::time::Duration;

    use common::lay_out_animated;
    use flui_sdk::animation::Vsync;
    use flui_sdk::widgets::icon::IconData;
    use flui_sdk::widgets::{Icon, VsyncScope};

    let button = |vsync: &Vsync, selected| {
        VsyncScope::new(
            vsync.clone(),
            Theme::new(
                ThemeData::light(),
                IconButton::new(Icon::new(IconData::new(0xE87E)).semantic_label("Not favorite"))
                    .selected_icon(Icon::new(IconData::new(0xE87D)).semantic_label("Favorite"))
                    .selected(selected)
                    .on_pressed(|_cx| {}),
            ),
        )
    };
    let opacities = |laid: &common::LaidOut| -> Vec<f64> {
        laid.find_all_by_render_type("RenderAnimatedOpacity")
            .into_iter()
            .map(|id| laid.opacity(id))
            .collect()
    };
    let vsync = Vsync::new();
    let mut laid = lay_out_animated(button(&vsync, false), tight(40.0, 40.0), vsync.clone());
    laid.enable_semantics();
    laid.pump_for(Duration::ZERO);
    assert_eq!(opacities(&laid), [1.0], "one icon at rest");

    laid.pump_widget(button(&vsync, true));
    laid.pump_for(Duration::ZERO);
    let tree = laid.a11y_tree().expect("semantics enabled");
    assert!(
        tree.find_by_label("Favorite").is_ok(),
        "the selected icon's label"
    );
    assert!(
        tree.find_by_label("Not favorite").is_err(),
        "the plain icon's label is gone at once"
    );
    laid.pump_for(Duration::from_millis(125));
    let fading = opacities(&laid);
    assert_eq!(fading.len(), 2, "both icons mid-fade: {fading:?}");
    assert!(
        fading.iter().all(|opacity| (opacity - 0.5).abs() < 0.05),
        "half in, half out: {fading:?}"
    );
    laid.pump_for(Duration::from_millis(200));
    assert_eq!(opacities(&laid), [1.0], "the plain icon is gone");
}

/// A hovered toggle lays its state layer in `primary` while selected and in
/// `on_surface_variant` while not.
pub fn a_selected_icon_button_hovers_in_primary() {
    use flui_painting::DrawOp;

    let colors = ThemeData::light().color_scheme;
    let hovered_colors = |selected| -> Vec<Color> {
        let mut laid = lay_out(
            Theme::new(
                ThemeData::light(),
                IconButton::new(SizedBox::new(24.0, 24.0))
                    .selected(selected)
                    .on_pressed(|_cx| {}),
            ),
            tight(40.0, 40.0),
        );
        laid.pump_for(std::time::Duration::from_millis(16));
        laid.dispatch_pointer_hover(20.0, 20.0);
        laid.pump_for(std::time::Duration::from_millis(16));
        laid.pump_for(std::time::Duration::from_millis(16));
        laid.draw_ops()
            .into_iter()
            .filter_map(|command| match command.op {
                DrawOp::Rect { paint, .. }
                | DrawOp::RRect { paint, .. }
                | DrawOp::Circle { paint, .. }
                | DrawOp::Path { paint, .. } => Some(paint.color),
                _ => None,
            })
            .collect()
    };
    let selected = hovered_colors(true);
    assert!(
        selected.contains(&colors.primary.with_opacity(0.08)),
        "{selected:?}"
    );
    let unselected = hovered_colors(false);
    assert!(
        unselected.contains(&colors.on_surface_variant.with_opacity(0.08)),
        "{unselected:?}"
    );
}
