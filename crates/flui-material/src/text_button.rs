//! [`TextButton`] — an M3 button with no outline or fill, the lowest-emphasis
//! member of the button family.
//!
//! # Flutter parity
//!
//! `material/text_button.dart`'s `TextButton` (oracle tag `3.44.0`).
//! `default_style` ports `_TextButtonDefaultsM3` (`text_button.dart`
//! `:493-596`) field-by-field, narrowed to the V1 slots [`ButtonStyle`]
//! carries — see that module's docs. Ported: `text_style`, `background_color`
//! (constant transparent), `foreground_color`, `overlay_color`, `elevation`
//! (constant `0.0`), `padding`, `minimum_size`, `maximum_size`, `shape`. The
//! oracle table sets no default `side` or `fixed_size` (its own "No default
//! fixedSize"/"No default side" comments), so neither field is populated
//! here.
//!
//! `padding` is the one slot that differs in *shape*, not just color, from
//! its siblings: `_scaledPadding` in `text_button.dart` uses `12px`
//! horizontal / `8px` vertical at the M3 1x tier, versus `24px`/`0px` for
//! `ElevatedButton`/`FilledButton`/`OutlinedButton` — text buttons ship
//! tighter by design (no fill or outline to visually separate from
//! surrounding content).

use flui_types::geometry::px;
use flui_types::{Color, EdgeInsets, Size};
use flui_view::prelude::*;
use flui_widgets::{WidgetState, WidgetStateProperty};

use crate::ThemeData;
use crate::button_style::ButtonStyle;
use crate::button_style_button::{ButtonStyleButtonCore, PressCallback};
use crate::elevated_button::pressed_hovered_focused_overlay;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// A Material 3 button with no outline or fill. Use for the
/// lowest-emphasis actions, e.g. a dialog's dismissive action — see
/// <https://m3.material.io/components/buttons/overview>.
///
/// ```rust
/// use flui_material::TextButton;
/// use flui_widgets::Text;
///
/// let _button = TextButton::new(Text::new("Learn more")).on_pressed(|| {});
/// ```
#[derive(Clone, StatelessView)]
pub struct TextButton {
    on_pressed: Option<PressCallback>,
    style: Option<ButtonStyle>,
    child: BoxedView,
}

impl std::fmt::Debug for TextButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextButton")
            .field("enabled", &self.on_pressed.is_some())
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl TextButton {
    /// A `TextButton` around `child` with no press handler (disabled) and no
    /// style override.
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

impl StatelessView for TextButton {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let mut core = ButtonStyleButtonCore::new(default_style(&theme), self.child.clone())
            .style(self.style.clone().unwrap_or_default());
        if let Some(on_pressed) = self.on_pressed.clone() {
            core = core.on_pressed(on_pressed);
        }
        core
    }
}

/// Verbatim, field-by-field port of `_TextButtonDefaultsM3`
/// (`text_button.dart`, oracle tag `3.44.0`), narrowed to the V1 slots — see
/// the module docs.
fn default_style(theme: &ThemeData) -> ButtonStyle {
    let colors = theme.color_scheme;

    ButtonStyle {
        text_style: Some(WidgetStateProperty::all(
            theme.text_theme.label_large.clone(),
        )),
        background_color: Some(WidgetStateProperty::all(Some(Color::TRANSPARENT))),
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
        side: None,
        shape: Some(WidgetStateProperty::all(Some(MaterialShape::Stadium))),
    }
}

/// `12px` horizontal, `8px` vertical — the M3 1x tier of `TextButton`'s own
/// `_scaledPadding` (`text_button.dart`, oracle tag `3.44.0`), tighter than
/// the `24px`/`0px` its filled/outlined siblings use. See
/// `crate::elevated_button`'s docs for the shared `MediaQuery` text-scaler
/// deferral this narrows to the 1x tier.
fn scaled_padding_1x() -> EdgeInsets {
    EdgeInsets::symmetric(px(8.0), px(12.0))
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

    /// Oracle citation: `_TextButtonDefaultsM3` (`text_button.dart`, tag
    /// `3.44.0`). Default/hovered/pressed/disabled state matrix.
    #[test]
    fn default_style_matches_text_button_defaults_m3_state_matrix() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme);

        let none = WidgetStates::NONE;
        let hovered = WidgetStates::from(WidgetState::Hovered);
        let pressed = WidgetStates::from(WidgetState::Pressed);
        let disabled = WidgetStates::from(WidgetState::Disabled);

        assert_eq!(
            resolve(style.background_color.as_ref(), &none),
            Some(Color::TRANSPARENT)
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
        assert_eq!(resolve(style.elevation.as_ref(), &disabled), Some(0.0));
    }

    #[test]
    fn default_style_padding_is_tighter_than_the_filled_siblings() {
        let padding = resolve(
            default_style(&ThemeData::light()).padding.as_ref(),
            &WidgetStates::NONE,
        )
        .expect("padding is set");
        assert_eq!(padding, EdgeInsets::symmetric(px(8.0), px(12.0)));
    }

    #[test]
    fn default_style_leaves_fixed_size_and_side_unset() {
        let style = default_style(&ThemeData::light());
        assert!(style.fixed_size.is_none());
        assert!(style.side.is_none());
    }
}
