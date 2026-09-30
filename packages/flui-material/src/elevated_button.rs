//! [`ElevatedButton`] — a filled M3 button whose `Material` elevates when
//! pressed.
//!
//! # Defaults and scope
//!
//! `default_style` holds the M3 elevated-button defaults, narrowed to the V1
//! slots [`ButtonStyle`] carries — see that module's docs for the full
//! omitted-slot list. Populated fields: `text_style`, `background_color`, `foreground_color`,
//! `overlay_color`, `elevation`, `padding`, `minimum_size`, `maximum_size`,
//! `shape`. Deferred alongside every other V1 button:
//! `icon_color`/`icon_size`, `mouse_cursor`,
//! `visual_density`/`tap_target_size`, `animation_duration`/`enable_feedback`/
//! `splash_factory`. The M3 elevated button sets no default `side` or
//! `fixed_size` either, so neither field is populated here.

use flui_sdk::geometry::{EdgeInsets, Size};
use flui_sdk::painting::Color;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{WidgetState, WidgetStateProperty};

use crate::ThemeData;
use crate::button_style::ButtonStyle;
use crate::button_style_button::{ButtonStyleButtonCore, PressCallback};
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// A filled Material 3 button whose `Material` elevates on press. Use for
/// important actions in flat, low-emphasis layouts — see
/// <https://m3.material.io/components/buttons/overview>.
///
/// ```rust
/// use flui_material::ElevatedButton;
/// use flui_sdk::widgets::Text;
///
/// let _button = ElevatedButton::new(Text::new("Save")).on_pressed(|_cx| {});
/// ```
#[derive(Clone, StatelessView)]
pub struct ElevatedButton {
    on_pressed: Option<PressCallback>,
    style: Option<ButtonStyle>,
    child: BoxedView,
}

impl std::fmt::Debug for ElevatedButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ElevatedButton")
            .field("enabled", &self.on_pressed.is_some())
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl ElevatedButton {
    /// An `ElevatedButton` around `child` with no press handler (disabled)
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
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_pressed = Some(crate::event_callback::press_callback(callback));
        self
    }

    /// Overrides the default style, per property (unset properties keep
    /// falling through to the default) — see
    /// `crate::button_style_button`'s resolve-then-coalesce docs.
    #[must_use]
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = Some(style);
        self
    }
}

impl StatelessView for ElevatedButton {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let mut core = ButtonStyleButtonCore::new(default_style(&theme), self.child.clone())
            .style(self.style.clone().unwrap_or_default());
        // Middle cascade tier: FLUI V1 has no per-button `InheritedTheme`
        // wrapper yet, so this reads the button style off the ambient
        // `ThemeData`'s `elevated_button_theme` slot instead (see
        // `crate::button_style_button`'s module docs for this reduction,
        // shared by every button in this crate).
        if let Some(theme_style) = theme
            .elevated_button_theme
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

/// Verbatim, field-by-field port of `_ElevatedButtonDefaultsM3`
/// (`elevated_button.dart`, oracle tag `3.44.0`), narrowed to the V1 slots —
/// see the module docs.
fn default_style(theme: &ThemeData) -> ButtonStyle {
    let colors = theme.color_scheme;

    ButtonStyle {
        text_style: Some(WidgetStateProperty::all(
            theme.text_theme.label_large.clone(),
        )),
        background_color: Some(WidgetStateProperty::resolve_with(move |states| {
            Some(if states.contains_state(WidgetState::Disabled) {
                colors.on_surface.with_opacity(0.12)
            } else {
                colors.surface_container_low
            })
        })),
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
        elevation: Some(WidgetStateProperty::resolve_with(move |states| {
            Some(if states.contains_state(WidgetState::Disabled) {
                0.0
            } else if states.contains_state(WidgetState::Pressed) {
                1.0
            } else if states.contains_state(WidgetState::Hovered) {
                3.0
            } else {
                // Focused, and the oracle's unconditional fallback, both
                // resolve to 1.0.
                1.0
            })
        })),
        padding: Some(WidgetStateProperty::all(Some(scaled_padding_1x()))),
        minimum_size: Some(WidgetStateProperty::all(Some(Size::new(64.0, 40.0)))),
        fixed_size: None,
        maximum_size: Some(WidgetStateProperty::all(Some(Size::INFINITY))),
        side: None,
        shape: Some(WidgetStateProperty::all(Some(MaterialShape::Stadium))),
    }
}

/// `24px` horizontal, `0px` vertical — the `effectiveTextScale <= 1` tier of
/// `ButtonStyleButton.scaledPadding` (oracle: `elevated_button.dart`'s
/// private `_scaledPadding`, tag `3.44.0`, `useMaterial3` branch). FLUI has
/// no `MediaQuery` text-scaler consumer yet (named deferral, shared with
/// every V1 button below), so only the 1x tier is ported; the 2x/3x lerp
/// tiers arrive alongside that consumer.
pub(crate) fn scaled_padding_1x() -> EdgeInsets {
    EdgeInsets::symmetric(0.0, 24.0)
}

/// The pressed(10%)/hovered(8%)/focused(10%) overlay ramp every V1 button's
/// `_TokenDefaultsM3.overlayColor` shares, differing only in `base_color`.
/// Oracle order matters: pressed is checked FIRST, so a state set containing
/// both `Pressed` and `Hovered` resolves the pressed opacity, not hover's.
/// `None` (no interactive state active) paints no overlay layer at all — see
/// `crate::ink_well`'s module docs on why `None` is not a fallback color.
pub(crate) fn pressed_hovered_focused_overlay(
    states: &flui_sdk::widgets::WidgetStates,
    base_color: Color,
) -> Option<Color> {
    if states.contains_state(WidgetState::Pressed) {
        Some(base_color.with_opacity(0.1))
    } else if states.contains_state(WidgetState::Hovered) {
        Some(base_color.with_opacity(0.08))
    } else if states.contains_state(WidgetState::Focused) {
        Some(base_color.with_opacity(0.1))
    } else {
        None
    }
}
