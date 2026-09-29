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

use flui_sdk::geometry::{EdgeInsets, Size};
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{WidgetState, WidgetStateProperty};

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
/// use flui_sdk::widgets::Text;
///
/// let _filled = FilledButton::new(Text::new("Confirm")).on_pressed(|_cx| {});
/// let _tonal = FilledButton::tonal(Text::new("Confirm")).on_pressed(|_cx| {});
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
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_pressed = Some(crate::event_callback::press_callback(callback));
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
        // Middle cascade tier — see `crate::elevated_button`'s identical
        // "simplified from `ElevatedButtonTheme.of`" note; the `tonal`
        // variant shares the same `filled_button_theme` slot as the plain
        // variant, matching the oracle (one `FilledButtonThemeData` for
        // both `FilledButton` constructors).
        if let Some(theme_style) = theme
            .filled_button_theme
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
        // exact check once.
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
        minimum_size: Some(WidgetStateProperty::all(Some(Size::new(64.0, 40.0)))),
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
    EdgeInsets::symmetric(0.0, 24.0)
}

#[cfg(test)]
mod tests {
    use flui_sdk::widgets::{WidgetState, WidgetStates};

    use super::*;

    fn resolve<T: Clone + Default>(
        property: Option<&WidgetStateProperty<Option<T>>>,
        states: &WidgetStates,
    ) -> Option<T> {
        property.and_then(|p| p.resolve(states))
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
}
