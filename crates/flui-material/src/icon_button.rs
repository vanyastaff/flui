//! [`IconButton`] — a small M3 button wrapping a single icon child, riding
//! the same `ButtonStyleButtonCore` machinery the text-label button family
//! uses.
//!
//! # Flutter parity
//!
//! `material/icon_button.dart`'s `IconButton` (oracle tag `3.44.0`),
//! standard variant only. In M3 mode the oracle's own `IconButton.build`
//! delegates to `_IconButtonM3 extends ButtonStyleButton` — confirming
//! `IconButton` really does ride the same button-style machinery
//! `ElevatedButton`/`FilledButton`/`OutlinedButton`/`TextButton` do, unlike
//! [`crate::floating_action_button::FloatingActionButton`] (which composes
//! `Material`+`InkWell` directly because its own oracle, `RawMaterialButton`,
//! is a *sibling* of `ButtonStyleButton`, not a subclass built on it). This
//! type is therefore a thin [`StatelessView`] over
//! `ButtonStyleButtonCore`, exactly mirroring `elevated_button.rs`'s own
//! shape.
//!
//! # `default_style`: `_IconButtonDefaultsM3`, standard variant
//!
//! Field-by-field port of `_IconButtonDefaultsM3` (`icon_button.dart`'s
//! generated token block, tag `3.44.0`), narrowed to the V1
//! [`ButtonStyle`] slots (see that module's docs for the omitted-slot list
//! every button in this crate shares):
//!
//! - `background_color` — always `Colors.transparent`.
//! - `foreground_color` — `disabled` → `onSurface@38%`; the oracle's
//!   `selected` branch (`primary`) is skipped — `IconButton.isSelected`/
//!   `selectedIcon` (the toggle variant) is a named V1 deferral, so no state
//!   set this substrate produces ever contains `WidgetState::Selected` — the
//!   fallback is always `onSurfaceVariant`.
//! - `overlay_color` — the oracle's `resolveWith` closure nests a `selected`
//!   branch (never reached here, same reasoning) around the unselected
//!   pressed(10%)/hovered(8%)/focused(10%) ramp — exactly
//!   `crate::elevated_button::pressed_hovered_focused_overlay`'s shape, so
//!   this reuses that helper against `onSurfaceVariant` rather than
//!   duplicating the chain. The oracle's unconditional `Colors.transparent`
//!   fallback (vs. this crate's `None` = "paint no overlay layer", see
//!   `crate::ink_well`'s module docs) is a paint-equivalent substitution: a
//!   transparent fill and no fill layer are visually identical, and the rest
//!   of this button family already makes the same substitution.
//! - `elevation` — always `0.0`.
//! - `padding` — `EdgeInsets::all(8.0)`.
//! - `minimum_size` — `Size(40.0, 40.0)`.
//! - `maximum_size` — `Size::INFINITY`.
//! - `shape` — [`MaterialShape::Stadium`] (a circle at the 40×40 minimum
//!   size, since `Stadium`'s radius is `shortest_side / 2.0`).
//! - `fixed_size`/`side` — unset, matching the oracle's own "// No default
//!   fixedSize" comment and null `side` getter.
//!
//! # Icon color and size: threaded through `IconTheme`, not `ButtonStyle`
//!
//! The oracle's M3 path folds `iconSize` into `ButtonStyle.iconSize` (a
//! slot [`ButtonStyle`] does not carry — a named crate-wide omission, see
//! that module's docs) and `foregroundColor` into `ButtonStyle.iconColor`
//! (ditto), then `IconTheme.merge`s `iconColor ?? resolvedForegroundColor`
//! around the child (`button_style_button.dart`'s `_ButtonStyleState.build`).
//! Neither slot exists yet, so this type wraps `icon` in an [`IconTheme`]
//! *before* handing it to `ButtonStyleButtonCore` — the same technique
//! [`crate::app_bar::AppBar`] already uses to color its own icon/title
//! content.
//!
//! **Not an approximation of the resolved color — a real coalesce.** A
//! naive port would hardcode `default_style`'s own foreground table here,
//! silently dropping a caller's `.style(ButtonStyle { foreground_color: ...
//! })` override (it would still reach `ButtonStyleButtonCore`'s
//! `DefaultTextStyle`, but an `Icon` child reads `IconTheme`, not
//! `DefaultTextStyle` — so the override would visibly do nothing to the
//! icon). Instead, `build` resolves `self.style`'s `foreground_color`
//! against `default_style`'s own, through the identical widget-then-default
//! coalesce `crate::button_style_button::resolve_property` performs inside
//! `ButtonStyleButtonCore` — both tiers are already in hand at `build` time,
//! so no extra plumbing is needed to get the SAME answer
//! `ButtonStyleButtonCore` would fold into `DefaultTextStyle`, just also fed
//! into the icon's `IconTheme`.
//!
//! The coalesce is resolved against a states set built from `disabled` only
//! (never a live, hover/press/focus-tracking one): standard `IconButton`'s
//! `foreground_color` — whether the caller's override or the default table
//! — depends only on `disabled`/`selected` (see above; `selected` is a named
//! deferral), so a static enabled-or-disabled snapshot is sufficient here.
//! A caller whose override varies `foreground_color` by `Pressed`/`Hovered`/
//! `Focused` would see that live variation reach `ButtonStyleButtonCore`'s
//! `DefaultTextStyle` (for a `Text` child) but NOT this icon's `IconTheme`,
//! which only re-resolves on `IconButton`'s own rebuilds — a named
//! divergence, not silently dropped. `icon_size` is always the M3 default
//! (`24.0`); no per-call override exists yet (`IconButton::icon_size` is a
//! natural, additive V1+ follow-up).
//!
//! **This divergence extends verbatim to the theme tier
//! (`icon_button_theme`).** `theme_style`'s `foreground_color` is folded
//! into the SAME `resolve_property` call as `self.style`'s, against the
//! SAME static `disabled`-only snapshot — so a
//! `Theme.icon_button_theme.style.foreground_color` that varies by
//! `Pressed`/`Hovered`/`Focused` behaves identically to a state-varying
//! widget-level override: the live variation reaches
//! `ButtonStyleButtonCore`'s own `DefaultTextStyle`/`InkWell` (which DO hold
//! a live, hover/press/focus-tracking `WidgetStatesController` — see
//! `crate::button_style_button`'s docs) but this icon's `IconTheme` only
//! ever sees the `disabled`/enabled snapshot, never a live
//! `Pressed`/`Hovered`/`Focused` re-resolution. Fixing this for real means
//! sharing `ButtonStyleButtonCoreState`'s own `WidgetStatesController` with
//! this icon color resolution — that controller is private to
//! `button_style_button.rs` and created inside `ButtonStyleButtonCore`'s
//! `create_state`, one layer below where `IconButton::build` runs, so there
//! is no live states seam to read from at this call site today. A named
//! divergence, not silently dropped, same as the widget-tier case above.
//!
//! # Deferred, and named
//!
//! - **`filled`/`filled_tonal`/`outlined` variants** — each is a distinct
//!   `_TokenDefaultsM3` table (`_FilledIconButtonDefaultsM3` and friends);
//!   only the standard (transparent) variant ships.
//! - **`isSelected`/`selectedIcon` (the toggle variant)** — no
//!   `WidgetState::Selected` is ever asserted, so the oracle's `selected`
//!   branches in `foreground_color`/`overlay_color` are unreachable dead
//!   code paths in this port, not wrong ones.
//! - **`icon_size` override**, `visual_density`, `alignment`, `splash_radius`,
//!   `mouse_cursor`, `focus_node`/`autofocus`, `tooltip`, `on_hover`/
//!   `on_long_press`, `states_controller` (external) — no override surface
//!   yet, matching every other V1 button in this crate.

use flui_types::geometry::px;
use flui_types::{Color, EdgeInsets, Size};
use flui_view::prelude::*;
use flui_widgets::{IconTheme, IconThemeData, WidgetState, WidgetStateProperty, WidgetStates};

use crate::ThemeData;
use crate::button_style::ButtonStyle;
use crate::button_style_button::{ButtonStyleButtonCore, PressCallback, resolve_property};
use crate::elevated_button::pressed_hovered_focused_overlay;
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// The standard variant's icon side length. Flutter parity:
/// `_IconButtonDefaultsM3.iconSize`.
pub const ICON_BUTTON_ICON_SIZE: f32 = 24.0;

/// A small M3 button wrapping a single icon child — Flutter's standard
/// `IconButton`. Use for a single, low-emphasis action (an app bar action, a
/// list-item trailing control, …) — see
/// <https://m3.material.io/components/icon-buttons/overview>.
///
/// ```rust
/// use flui_material::IconButton;
/// use flui_widgets::{Icon, IconData};
///
/// let _button = IconButton::new(Icon::new(IconData::new(0xE87D))).on_pressed(|| {});
/// ```
#[derive(Clone, StatelessView)]
pub struct IconButton {
    on_pressed: Option<PressCallback>,
    style: Option<ButtonStyle>,
    icon: BoxedView,
}

impl std::fmt::Debug for IconButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IconButton")
            .field("enabled", &self.on_pressed.is_some())
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

impl IconButton {
    /// An `IconButton` around `icon` with no press handler (disabled) and no
    /// style override.
    #[must_use]
    pub fn new(icon: impl IntoView) -> Self {
        Self {
            on_pressed: None,
            style: None,
            icon: BoxedView(Box::new(icon.into_view())),
        }
    }

    /// Sets the press handler. Presence of a handler is what makes this
    /// button enabled.
    #[must_use]
    pub fn on_pressed(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(std::rc::Rc::new(callback));
        self
    }

    /// Overrides the default style, per property — see
    /// `crate::button_style_button`'s resolve-then-coalesce docs.
    #[must_use]
    pub fn style(mut self, style: ButtonStyle) -> Self {
        self.style = Some(style);
        self
    }

    fn is_interactive(&self) -> bool {
        self.on_pressed.is_some()
    }
}

impl StatelessView for IconButton {
    fn build(&self, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let default = default_style(&theme);
        // Middle cascade tier — see `crate::elevated_button`'s identical
        // "simplified from `ElevatedButtonTheme.of`" note.
        let theme_style = theme
            .icon_button_theme
            .as_ref()
            .and_then(|t| t.style.clone());

        // A static enabled-or-disabled snapshot — see the module docs'
        // "Icon color and size" section for why that's sufficient for
        // `foreground_color` specifically, and where it stops being exact.
        let states = if self.is_interactive() {
            WidgetStates::NONE
        } else {
            WidgetStates::from(WidgetState::Disabled)
        };

        // The SAME widget-then-theme-then-default coalesce
        // `ButtonStyleButtonCore` performs internally (`resolve_property`)
        // — computed here too so a caller's `.style(ButtonStyle {
        // foreground_color: .. })` override, OR a theme-configured
        // `icon_button_theme`, reaches the icon's `IconTheme`, not just
        // `DefaultTextStyle`. The `unwrap_or` fallback is unreachable in
        // practice: `default_style` always sets `foreground_color`, so the
        // coalesce always resolves `Some` — kept only because
        // `resolve_property` returns `Option`.
        let icon_color = resolve_property(
            &states,
            self.style
                .as_ref()
                .and_then(|style| style.foreground_color.as_ref()),
            theme_style
                .as_ref()
                .and_then(|style| style.foreground_color.as_ref()),
            default.foreground_color.as_ref(),
        )
        .unwrap_or(Color::TRANSPARENT);

        let themed_icon = IconTheme::new(
            IconThemeData {
                color: Some(icon_color),
                size: Some(ICON_BUTTON_ICON_SIZE),
                ..IconThemeData::default()
            },
            self.icon.clone(),
        );

        let mut core = ButtonStyleButtonCore::new(default, themed_icon.boxed())
            .style(self.style.clone().unwrap_or_default());
        if let Some(theme_style) = theme_style {
            core = core.theme_style(theme_style);
        }
        if let Some(on_pressed) = self.on_pressed.clone() {
            core = core.on_pressed(on_pressed);
        }
        core
    }
}

/// Field-by-field port of `_IconButtonDefaultsM3`, standard variant
/// (`icon_button.dart`, oracle tag `3.44.0`), narrowed to the V1 slots — see
/// the module docs.
fn default_style(theme: &ThemeData) -> ButtonStyle {
    let colors = theme.color_scheme;

    ButtonStyle {
        background_color: Some(WidgetStateProperty::all(Some(Color::TRANSPARENT))),
        foreground_color: Some(WidgetStateProperty::resolve_with(move |states| {
            Some(if states.contains_state(WidgetState::Disabled) {
                colors.on_surface.with_opacity(0.38)
            } else {
                colors.on_surface_variant
            })
        })),
        overlay_color: Some(WidgetStateProperty::resolve_with(move |states| {
            pressed_hovered_focused_overlay(states, colors.on_surface_variant)
        })),
        elevation: Some(WidgetStateProperty::all(Some(0.0))),
        padding: Some(WidgetStateProperty::all(Some(EdgeInsets::all(px(8.0))))),
        minimum_size: Some(WidgetStateProperty::all(Some(Size::new(
            px(40.0),
            px(40.0),
        )))),
        fixed_size: None,
        maximum_size: Some(WidgetStateProperty::all(Some(Size::INFINITY))),
        side: None,
        shape: Some(WidgetStateProperty::all(Some(MaterialShape::Stadium))),
        ..ButtonStyle::default()
    }
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

    /// Oracle citation: `_IconButtonDefaultsM3` (`icon_button.dart`, tag
    /// `3.44.0`). Default/disabled state matrix — the parity core this test
    /// exists to prove.
    #[test]
    fn default_style_matches_icon_button_defaults_m3_state_matrix() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme);

        let none = WidgetStates::NONE;
        let disabled = WidgetStates::from(WidgetState::Disabled);

        assert_eq!(
            resolve(style.background_color.as_ref(), &none),
            Some(Color::TRANSPARENT),
            "background_color is always transparent, enabled or not",
        );
        assert_eq!(
            resolve(style.background_color.as_ref(), &disabled),
            Some(Color::TRANSPARENT),
        );

        assert_eq!(
            resolve(style.foreground_color.as_ref(), &none),
            Some(colors.on_surface_variant),
        );
        assert_eq!(
            resolve(style.foreground_color.as_ref(), &disabled),
            Some(colors.on_surface.with_opacity(0.38)),
        );

        assert_eq!(resolve(style.elevation.as_ref(), &none), Some(0.0));
        assert_eq!(resolve(style.elevation.as_ref(), &disabled), Some(0.0));

        assert_eq!(
            resolve(style.minimum_size.as_ref(), &none),
            Some(Size::new(px(40.0), px(40.0))),
        );
        assert_eq!(
            resolve(style.maximum_size.as_ref(), &none),
            Some(Size::INFINITY),
        );
        assert_eq!(
            resolve(style.padding.as_ref(), &none),
            Some(EdgeInsets::all(px(8.0))),
        );
        assert_eq!(
            resolve(style.shape.as_ref(), &none),
            Some(MaterialShape::Stadium),
        );
    }

    #[test]
    fn default_style_leaves_fixed_size_and_side_unset() {
        let style = default_style(&ThemeData::light());
        assert!(style.fixed_size.is_none());
        assert!(style.side.is_none());
    }

    #[test]
    fn default_style_no_text_style_is_set() {
        // Flutter parity: `_IconButtonDefaultsM3` never overrides
        // `textStyle` (the oracle's own "// No default text style" comment).
        let style = default_style(&ThemeData::light());
        assert!(style.text_style.is_none());
    }

    /// Overlay ordered-chain coverage: pressed(10%)/hovered(8%)/focused(10%),
    /// matching `_IconButtonDefaultsM3.overlayColor`'s unselected branch —
    /// same shape `elevated_button.rs`'s own overlay tests pin, repeated
    /// here against `onSurfaceVariant` rather than `primary` since the base
    /// color differs.
    #[test]
    fn overlay_color_matches_the_pressed_hovered_focused_ramp() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme);

        let none = WidgetStates::NONE;
        let pressed = WidgetStates::from(WidgetState::Pressed);
        let hovered = WidgetStates::from(WidgetState::Hovered);
        let focused = WidgetStates::from(WidgetState::Focused);

        assert_eq!(resolve(style.overlay_color.as_ref(), &none), None);
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &pressed),
            Some(colors.on_surface_variant.with_opacity(0.1)),
        );
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &hovered),
            Some(colors.on_surface_variant.with_opacity(0.08)),
        );
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &focused),
            Some(colors.on_surface_variant.with_opacity(0.1)),
        );
    }

    /// Mutation-honest ordered-chain coverage: a combined pressed+hovered
    /// state must resolve through the pressed branch (10%), not hover's
    /// lower 8% — the combined-state pin every token table in this crate
    /// carries.
    #[test]
    fn overlay_color_checks_pressed_before_hovered() {
        let theme = ThemeData::light();
        let colors = theme.color_scheme;
        let style = default_style(&theme);

        let pressed_and_hovered =
            WidgetStates::from(WidgetState::Pressed).with_state(WidgetState::Hovered);
        assert_eq!(
            resolve(style.overlay_color.as_ref(), &pressed_and_hovered),
            Some(colors.on_surface_variant.with_opacity(0.1)),
        );
    }

    #[test]
    fn is_disabled_when_no_press_handler_is_set() {
        assert!(!IconButton::new(flui_widgets::SizedBox::shrink()).is_interactive());
        assert!(
            IconButton::new(flui_widgets::SizedBox::shrink())
                .on_pressed(|| {})
                .is_interactive()
        );
    }
}
