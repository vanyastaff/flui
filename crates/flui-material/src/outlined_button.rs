//! [`OutlinedButton`] — an M3 button with an outlined border and no fill.
//!
//! # Flutter parity
//!
//! `material/outlined_button.dart`'s `OutlinedButton` (oracle tag `3.44.0`).
//! `default_style` ports `_OutlinedButtonDefaultsM3` (`outlined_button.dart`
//! `:460-575`) field-by-field, narrowed to the V1 slots [`ButtonStyle`]
//! carries — see that module's docs. Ported: `text_style`, `background_color`
//! (constant transparent), `foreground_color`, `overlay_color`, `elevation`
//! (constant `0.0`), `padding`, `minimum_size`, `maximum_size`, `side`,
//! `shape`. The oracle table sets no default `fixed_size` (its own "No
//! default fixedSize" comment), so that field is left unset here.
//!
//! `side` is this button's whole reason to exist, and it resolves correctly
//! (disabled `onSurface@12%`, focused `primary`, else `outline`, all at the
//! oracle's default `1.0` stroke width) — but see
//! [`ButtonStyle::side`](crate::ButtonStyle::side)'s doc comment: `Material`
//! has no border-side painting path yet, so this V1 `OutlinedButton` does
//! not yet draw a visible outline. A pre-existing deferral (`shape.rs`), not
//! one introduced here.

use flui_types::geometry::px;
use flui_types::styling::{BorderSide, BorderStyle};
use flui_types::{EdgeInsets, Size};
use flui_view::prelude::*;
use flui_widgets::{WidgetState, WidgetStateProperty};

use crate::ThemeData;
use crate::button_style::ButtonStyle;
use crate::button_style_button::{ButtonStyleButtonCore, PressCallback};
use crate::elevated_button::pressed_hovered_focused_overlay;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// An outlined Material 3 button: no fill, a `primary`/`outline` stroke.
/// Use for medium-emphasis actions — see
/// <https://m3.material.io/components/buttons/overview>.
///
/// ```rust
/// use flui_material::OutlinedButton;
/// use flui_widgets::Text;
///
/// let _button = OutlinedButton::new(Text::new("Cancel")).on_pressed(|| {});
/// ```
#[derive(Clone, StatelessView)]
pub struct OutlinedButton {
    on_pressed: Option<PressCallback>,
    style: Option<ButtonStyle>,
    child: BoxedView,
}

impl std::fmt::Debug for OutlinedButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutlinedButton")
            .field("enabled", &self.on_pressed.is_some())
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl OutlinedButton {
    /// An `OutlinedButton` around `child` with no press handler (disabled)
    /// and no style override.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            on_pressed: None,
            style: None,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// Sets the press handler. Presence of a handler is what makes this
    /// button enabled.
    #[must_use]
    pub fn on_pressed(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(std::rc::Rc::new(callback));
        self
    }

    /// Overrides the default style, per property (unset properties keep
    /// falling through to the default).
    #[must_use]
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = Some(style);
        self
    }
}

impl StatelessView for OutlinedButton {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let mut core = ButtonStyleButtonCore::new(default_style(&theme), self.child.clone())
            .style(self.style.clone().unwrap_or_default());
        // Middle cascade tier — see `crate::elevated_button`'s identical
        // "simplified from `ElevatedButtonTheme.of`" note.
        if let Some(theme_style) = theme
            .outlined_button_theme
            .as_ref()
            .and_then(|t| t.style.clone())
        {
            core = core.theme_style(theme_style);
        }
        if let Some(on_pressed) = self.on_pressed.clone() {
            core = core.on_pressed(on_pressed);
        }
        core
    }
}

/// Verbatim, field-by-field port of `_OutlinedButtonDefaultsM3`
/// (`outlined_button.dart`, oracle tag `3.44.0`), narrowed to the V1 slots —
/// see the module docs.
fn default_style(theme: &ThemeData) -> ButtonStyle {
    let colors = theme.color_scheme;

    ButtonStyle {
        text_style: Some(WidgetStateProperty::all(
            theme.text_theme.label_large.clone(),
        )),
        background_color: Some(WidgetStateProperty::all(Some(
            flui_types::Color::TRANSPARENT,
        ))),
        foreground_color: Some(WidgetStateProperty::resolve_with(move |states| {
            Some(if states.contains_state(WidgetState::Disabled) {
                colors.on_surface.with_opacity(0.38)
            } else {
                colors.primary
            })
        })),
        overlay_color: Some(WidgetStateProperty::resolve_with(move |states| {
            pressed_hovered_focused_overlay(states, colors.primary)
        })),
        elevation: Some(WidgetStateProperty::all(Some(0.0))),
        padding: Some(WidgetStateProperty::all(Some(scaled_padding_1x()))),
        minimum_size: Some(WidgetStateProperty::all(Some(Size::new(
            px(64.0),
            px(40.0),
        )))),
        fixed_size: None,
        maximum_size: Some(WidgetStateProperty::all(Some(Size::INFINITY))),
        side: Some(WidgetStateProperty::resolve_with(move |states| {
            let color = if states.contains_state(WidgetState::Disabled) {
                colors.on_surface.with_opacity(0.12)
            } else if states.contains_state(WidgetState::Focused) {
                colors.primary
            } else {
                colors.outline
            };
            Some(BorderSide::new(color, px(1.0), BorderStyle::Solid))
        })),
        shape: Some(WidgetStateProperty::all(Some(MaterialShape::Stadium))),
    }
}

/// `24px` horizontal, `0px` vertical — same 1x-tier padding as
/// [`crate::elevated_button`]. See that module's docs for the `MediaQuery`
/// text-scaler deferral this narrows to the 1x tier.
fn scaled_padding_1x() -> EdgeInsets {
    EdgeInsets::symmetric(px(0.0), px(24.0))
}

#[cfg(test)]
mod tests {
    use flui_widgets::{WidgetState, WidgetStates};

    use super::*;

    fn resolve<T: Clone + Default>(
        property: Option<&WidgetStateProperty<Option<T>>>,
        states: &WidgetStates,
    ) -> Option<T> {
        property.and_then(|p| p.resolve(states))
    }

    /// Oracle citation: `_OutlinedButtonDefaultsM3` (`outlined_button.dart`,
    /// tag `3.44.0`). Default/hovered/pressed/disabled state matrix.
    #[test]
    fn default_style_matches_outlined_button_defaults_m3_state_matrix() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme);

        let none = WidgetStates::NONE;
        let hovered = WidgetStates::from(WidgetState::Hovered);
        let pressed = WidgetStates::from(WidgetState::Pressed);
        let disabled = WidgetStates::from(WidgetState::Disabled);

        assert_eq!(
            resolve(style.background_color.as_ref(), &none),
            Some(flui_types::Color::TRANSPARENT)
        );
        assert_eq!(
            resolve(style.foreground_color.as_ref(), &none),
            Some(colors.primary)
        );
        assert_eq!(
            resolve(style.foreground_color.as_ref(), &disabled),
            Some(colors.on_surface.with_opacity(0.38))
        );
        assert_eq!(resolve(style.overlay_color.as_ref(), &none), None);
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &pressed),
            Some(colors.primary.with_opacity(0.1))
        );
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &hovered),
            Some(colors.primary.with_opacity(0.08))
        );
        assert_eq!(resolve(style.elevation.as_ref(), &none), Some(0.0));
        assert_eq!(resolve(style.elevation.as_ref(), &pressed), Some(0.0));
    }

    /// The whole point of this button: `side` resolves the disabled/focused/
    /// default outline colors at the oracle's default `1.0` stroke width.
    #[test]
    fn default_style_side_resolves_disabled_focused_and_default_outline_colors() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme);

        let none = WidgetStates::NONE;
        let focused = WidgetStates::from(WidgetState::Focused);
        let disabled = WidgetStates::from(WidgetState::Disabled);

        let default_side = resolve(style.side.as_ref(), &none).expect("default side is set");
        assert_eq!(default_side.color, colors.outline);
        assert_eq!(default_side.width, px(1.0));

        let focused_side = resolve(style.side.as_ref(), &focused).expect("focused side is set");
        assert_eq!(focused_side.color, colors.primary);

        let disabled_side = resolve(style.side.as_ref(), &disabled).expect("disabled side is set");
        assert_eq!(disabled_side.color, colors.on_surface.with_opacity(0.12));
    }

    #[test]
    fn default_style_leaves_fixed_size_unset() {
        assert!(default_style(&ThemeData::light()).fixed_size.is_none());
    }
}
