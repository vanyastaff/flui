//! [`FilledButton`] — a filled M3 button that does not elevate on press, plus
//! its `tonal` variant.
//!
//! # Flutter parity
//!
//! `material/filled_button.dart`'s `FilledButton` (oracle tag `3.44.0`).
//! `default_style` ports `_FilledButtonDefaultsM3`/`_FilledTonalButtonDefaultsM3`
//! (`filled_button.dart` `:531-671` / `:672-810`) field-by-field, narrowed to
//! the V1 slots [`ButtonStyle`] carries — see that module's docs. Ported:
//! `text_style`, `background_color`, `foreground_color`, `overlay_color`,
//! `elevation`, `padding`, `minimum_size`, `maximum_size`, `shape`. Neither
//! table sets a default `side` or `fixed_size` (both oracle tables' own "No
//! default fixedSize"/"No default side" comments), so neither field is
//! populated here.

use flui_types::geometry::px;
use flui_types::{EdgeInsets, Size};
use flui_view::prelude::*;
use flui_widgets::{WidgetState, WidgetStateProperty};

use crate::ThemeData;
use crate::button_style::ButtonStyle;
use crate::button_style_button::{ButtonStyleButtonCore, PressCallback};
use crate::elevated_button::pressed_hovered_focused_overlay;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// Which of the two `_TokenDefaultsM3` tables a [`FilledButton`] resolves
/// against — see `default_style`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FilledButtonVariant {
    /// `_FilledButtonDefaultsM3`: `primary`/`onPrimary` fill.
    Filled,
    /// `_FilledTonalButtonDefaultsM3`: `secondaryContainer`/
    /// `onSecondaryContainer` fill. Flutter parity: `FilledButton.tonal`.
    Tonal,
}

/// A filled Material 3 button that does not elevate on press. Use for
/// important, final actions — see
/// <https://m3.material.io/components/buttons/overview>. Construct the
/// secondary "filled tonal" variant with [`FilledButton::tonal`].
///
/// ```rust
/// use flui_material::FilledButton;
/// use flui_widgets::Text;
///
/// let _filled = FilledButton::new(Text::new("Confirm")).on_pressed(|| {});
/// let _tonal = FilledButton::tonal(Text::new("Confirm")).on_pressed(|| {});
/// ```
#[derive(Clone, StatelessView)]
pub struct FilledButton {
    on_pressed: Option<PressCallback>,
    style: Option<ButtonStyle>,
    variant: FilledButtonVariant,
    child: BoxedView,
}

impl std::fmt::Debug for FilledButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilledButton")
            .field("enabled", &self.on_pressed.is_some())
            .field("style", &self.style)
            .field("variant", &self.variant)
            .finish_non_exhaustive()
    }
}

impl FilledButton {
    /// A filled `FilledButton` around `child` with no press handler
    /// (disabled) and no style override.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            on_pressed: None,
            style: None,
            variant: FilledButtonVariant::Filled,
            child: BoxedView(Box::new(child.into_view())),
        }
    }

    /// The "filled tonal" variant: `secondaryContainer`/`onSecondaryContainer`
    /// in place of `primary`/`onPrimary`. Flutter parity: `FilledButton.tonal`.
    pub fn tonal(child: impl IntoView) -> Self {
        Self {
            variant: FilledButtonVariant::Tonal,
            ..Self::new(child)
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

impl StatelessView for FilledButton {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let mut core =
            ButtonStyleButtonCore::new(default_style(&theme, self.variant), self.child.clone())
                .style(self.style.clone().unwrap_or_default());
        if let Some(on_pressed) = self.on_pressed.clone() {
            core = core.on_pressed(on_pressed);
        }
        core
    }
}

/// Verbatim, field-by-field port of `_FilledButtonDefaultsM3`
/// (`variant: Filled`) / `_FilledTonalButtonDefaultsM3` (`variant: Tonal`)
/// (`filled_button.dart`, oracle tag `3.44.0`), narrowed to the V1 slots —
/// see the module docs.
fn default_style(theme: &ThemeData, variant: FilledButtonVariant) -> ButtonStyle {
    let colors = theme.color_scheme;
    let (background, foreground) = match variant {
        FilledButtonVariant::Filled => (colors.primary, colors.on_primary),
        FilledButtonVariant::Tonal => (colors.secondary_container, colors.on_secondary_container),
    };

    ButtonStyle {
        text_style: Some(WidgetStateProperty::all(
            theme.text_theme.label_large.clone(),
        )),
        background_color: Some(WidgetStateProperty::resolve_with(move |states| {
            Some(if states.contains_state(WidgetState::Disabled) {
                colors.on_surface.with_opacity(0.12)
            } else {
                background
            })
        })),
        foreground_color: Some(WidgetStateProperty::resolve_with(move |states| {
            Some(if states.contains_state(WidgetState::Disabled) {
                colors.on_surface.with_opacity(0.38)
            } else {
                foreground
            })
        })),
        overlay_color: Some(WidgetStateProperty::resolve_with(move |states| {
            pressed_hovered_focused_overlay(states, foreground)
        })),
        // Oracle order matters: `disabled` and `pressed` are checked BEFORE
        // `hovered`, so a state set containing both `Pressed` and `Hovered`
        // (an ordinary mouse press on an already-hovered button) resolves
        // the pressed value (0.0), never the hover value (1.0). Early
        // returns (not an `if`/`else if` chain — clippy rightly flags
        // adjacent identical `0.0` bodies there) still preserve the
        // oracle's exact branch order: `disabled` → `pressed` → `hovered` →
        // `focused` → the unconditional fallback (`_FilledButtonDefaultsM3
        // .elevation`'s chain, `filled_button.dart`, tag `3.44.0`; same
        // table shape as `_FilledTonalButtonDefaultsM3.elevation`). A
        // collapsed `!disabled && hovered` condition already dropped this
        // exact check once — see the mutation-honest
        // `elevation_checks_pressed_before_hovered` test below.
        elevation: Some(WidgetStateProperty::resolve_with(move |states| {
            if states.contains_state(WidgetState::Disabled) {
                return Some(0.0);
            }
            if states.contains_state(WidgetState::Pressed) {
                return Some(0.0);
            }
            if states.contains_state(WidgetState::Hovered) {
                return Some(1.0);
            }
            // Focused, and the oracle's unconditional fallback, both
            // resolve to 0.0.
            Some(0.0)
        })),
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

/// `24px` horizontal, `0px` vertical — same 1x-tier padding as
/// [`crate::elevated_button`] (both oracle tables call the identical
/// `_scaledPadding` shape with `padding1x = 24.0`). See that module's docs
/// for the `MediaQuery` text-scaler deferral this narrows to the 1x tier.
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

    /// Oracle citation: `_FilledButtonDefaultsM3` (`filled_button.dart`, tag
    /// `3.44.0`). Default/hovered/pressed/disabled state matrix.
    #[test]
    fn filled_default_style_matches_filled_button_defaults_m3_state_matrix() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme, FilledButtonVariant::Filled);

        let none = WidgetStates::NONE;
        let hovered = WidgetStates::from(WidgetState::Hovered);
        let pressed = WidgetStates::from(WidgetState::Pressed);
        let disabled = WidgetStates::from(WidgetState::Disabled);

        assert_eq!(
            resolve(style.background_color.as_ref(), &none),
            Some(colors.primary)
        );
        assert_eq!(
            resolve(style.background_color.as_ref(), &disabled),
            Some(colors.on_surface.with_opacity(0.12))
        );
        assert_eq!(
            resolve(style.foreground_color.as_ref(), &none),
            Some(colors.on_primary)
        );
        assert_eq!(
            resolve(style.foreground_color.as_ref(), &disabled),
            Some(colors.on_surface.with_opacity(0.38))
        );
        assert_eq!(resolve(style.overlay_color.as_ref(), &none), None);
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &pressed),
            Some(colors.on_primary.with_opacity(0.1))
        );
        assert_eq!(resolve(style.elevation.as_ref(), &none), Some(0.0));
        assert_eq!(resolve(style.elevation.as_ref(), &hovered), Some(1.0));
        assert_eq!(resolve(style.elevation.as_ref(), &pressed), Some(0.0));
        assert_eq!(resolve(style.elevation.as_ref(), &disabled), Some(0.0));
    }

    /// A disabled-but-hovered state must not resolve the hover elevation —
    /// `disabled` is checked first in the oracle's own conditional chain.
    #[test]
    fn filled_elevation_disabled_wins_over_hovered() {
        let theme = ThemeData::light();
        let style = default_style(&theme, FilledButtonVariant::Filled);
        let disabled_and_hovered =
            WidgetStates::from(WidgetState::Disabled).with_state(WidgetState::Hovered);
        assert_eq!(
            resolve(style.elevation.as_ref(), &disabled_and_hovered),
            Some(0.0)
        );
    }

    /// Pressed-first resolver order: a state set containing both `Pressed`
    /// and `Hovered` (an ordinary mouse press on an already-hovered button)
    /// resolves the pressed elevation (0.0), not hover's (1.0).
    /// Mutation-honest: this is exactly the case a collapsed
    /// `!disabled && hovered` condition gets wrong (it checked only
    /// `hovered`, so `{Pressed, Hovered}` resolved 1.0 instead of 0.0) —
    /// run that mutation against `default_style` below to see this test
    /// fail.
    #[test]
    fn filled_elevation_checks_pressed_before_hovered() {
        let theme = ThemeData::light();
        let style = default_style(&theme, FilledButtonVariant::Filled);
        let pressed_and_hovered =
            WidgetStates::from(WidgetState::Pressed).with_state(WidgetState::Hovered);
        assert_eq!(
            resolve(style.elevation.as_ref(), &pressed_and_hovered),
            Some(0.0)
        );
    }

    /// Same pressed-before-hovered check for the tonal variant's identical
    /// elevation table shape.
    #[test]
    fn tonal_elevation_checks_pressed_before_hovered() {
        let theme = ThemeData::light();
        let style = default_style(&theme, FilledButtonVariant::Tonal);
        let pressed_and_hovered =
            WidgetStates::from(WidgetState::Pressed).with_state(WidgetState::Hovered);
        assert_eq!(
            resolve(style.elevation.as_ref(), &pressed_and_hovered),
            Some(0.0)
        );
    }

    /// Oracle citation: `_FilledTonalButtonDefaultsM3` (`filled_button.dart`,
    /// tag `3.44.0`). Default/pressed/disabled state matrix.
    #[test]
    fn tonal_default_style_matches_filled_tonal_button_defaults_m3_state_matrix() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme, FilledButtonVariant::Tonal);

        let none = WidgetStates::NONE;
        let pressed = WidgetStates::from(WidgetState::Pressed);
        let disabled = WidgetStates::from(WidgetState::Disabled);

        assert_eq!(
            resolve(style.background_color.as_ref(), &none),
            Some(colors.secondary_container)
        );
        assert_eq!(
            resolve(style.foreground_color.as_ref(), &none),
            Some(colors.on_secondary_container)
        );
        assert_eq!(
            resolve(style.background_color.as_ref(), &disabled),
            Some(colors.on_surface.with_opacity(0.12))
        );
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &pressed),
            Some(colors.on_secondary_container.with_opacity(0.1))
        );
    }

    #[test]
    fn tonal_constructor_selects_the_tonal_variant() {
        let button = FilledButton::tonal(flui_widgets::SizedBox::shrink());
        assert_eq!(button.variant, FilledButtonVariant::Tonal);
    }

    #[test]
    fn new_constructor_selects_the_filled_variant() {
        let button = FilledButton::new(flui_widgets::SizedBox::shrink());
        assert_eq!(button.variant, FilledButtonVariant::Filled);
    }

    #[test]
    fn both_variants_leave_fixed_size_and_side_unset() {
        let theme = ThemeData::light();
        for variant in [FilledButtonVariant::Filled, FilledButtonVariant::Tonal] {
            let style = default_style(&theme, variant);
            assert!(style.fixed_size.is_none());
            assert!(style.side.is_none());
        }
    }
}
