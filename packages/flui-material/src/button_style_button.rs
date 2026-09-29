//! [`ButtonStyleButtonCore`] — the composition machinery shared by every M3
//! button (`ElevatedButton`, `FilledButton`, `OutlinedButton`, `TextButton`).
//!
//! # Flutter parity
//!
//! `material/button_style_button.dart`'s `ButtonStyleButton`/`_ButtonStyleState`
//! (oracle tag `3.44.0`). The oracle is an abstract `StatefulWidget` base
//! class each concrete button subclasses, overriding `defaultStyleOf` /
//! `themeStyleOf`. Rust has no implementation inheritance, so this crate
//! inverts the relationship: each concrete button (`elevated_button.rs` etc.)
//! is a thin [`StatelessView`] that reads
//! the ambient [`Theme`](crate::Theme), computes its own `default_style`
//! table, reads its own `ThemeData` component-theme slot (`elevated_button_theme`
//! and friends), and hands all three to this module's [`ButtonStyleButtonCore`]
//! — the `StatefulView` that owns the interactive-state machinery and the
//! actual composition. **Named reduction**: the oracle also has a standalone
//! `ElevatedButtonTheme` `InheritedTheme` widget per button (so a subtree can
//! override just that button's style without touching the whole
//! `ThemeData`); FLUI V1 has no per-widget `InheritedTheme` wrappers yet, so
//! every concrete button reads only its `ThemeData` slot via `Theme::of` —
//! see each concrete button's own module docs for the simplified
//! `ElevatedButtonTheme.of(context)?.style` chain this collapses to.
//!
//! # The resolve-then-coalesce cascade
//!
//! Oracle: `_ButtonStyleState.build`'s local `effectiveValue`/`resolve`
//! helpers (`button_style_button.dart` `:315-327`) — for each property,
//! `getProperty(widgetStyle)?.resolve(states) ?? getProperty(themeStyle)?.resolve(states)
//! ?? getProperty(defaultStyle)?.resolve(states)`. [`resolve_property`] is
//! the direct Rust translation: three tiers, each independently resolved
//! against the *current* [`WidgetStates`] and coalesced with `Option::or_else`.
//! This is exactly the shape `flui_sdk::widgets::widget_state`'s
//! `option_property_coalesce_chain_mirrors_button_style_button` test
//! demonstrates for a single tier — here extended to three.
//!
//! `theme_style` is threaded in via [`ButtonStyleButtonCore::theme_style`] —
//! each concrete button passes its own `ThemeData` component-theme slot's
//! `style` (when configured) at the same `Theme::of` read it already
//! performs to compute `default_style`. A button whose slot is unset passes
//! nothing, so `theme_style` stays `None` and the cascade falls straight
//! through to `default_style` — unchanged from before this slot was wired.
//!
//! # Composition
//!
//! Oracle: `_ButtonStyleState.build`'s widget tree (`button_style_button.dart`
//! `:497-543`), trimmed to the properties [`crate::ButtonStyle`] actually
//! carries (see that module's docs for the omitted slots this composition
//! therefore also skips — `visual_density`/`tap_target_size` drop the
//! `_InputPadding` wrapper, `alignment` drops the `Align` wrapper, and there
//! is no `Semantics`/`Tooltip` layer). What ships, outermost to innermost:
//!
//! ```text
//! Semantics(container: true, button: true, enabled: on_pressed.is_some())
//!   ConstrainedBox(min/fixed/max size)
//!     Material(color, elevation, shape)
//!       InkWell(overlay_color, shared states controller, enabled = on_pressed.is_some())
//!         Padding(padding)
//!           DefaultTextStyle(text_style with foreground_color folded into its `color`)
//!             child
//! ```
//!
//! The `Semantics` wrapper is outermost, matching the oracle's own
//! `Semantics(container: true, button: widget.isSemanticButton, enabled:
//! widget.enabled, child: _InputPadding(...))` (`button_style_button.dart`
//! `:591-598`) with `_InputPadding` dropped (see above) and
//! `isSemanticButton` narrowed to always-`true` — every concrete button this
//! core composes for is semantically a button in this V1, so there is no
//! per-button override to thread through yet.
//!
//! # Own `WidgetStatesController`, shared with the inner `InkWell`
//!
//! Every resolved property (background/foreground/elevation/…) depends on
//! the *current* interactive state, which only changes through
//! hover/press/focus events the inner `InkWell` observes. So this type owns
//! its own [`WidgetStatesController`] — created once in `create_state`,
//! `Disabled`-synced from `on_pressed.is_some()` before any listener is
//! attached, and handed to the inner `InkWell` via
//! [`InkWell::states_controller`] — mirroring
//! `crate::ink_well::InkWellState`'s own `init_state`/`did_update_view`
//! ordering precisely (see that module's docs for why the sync-before-listen
//! order is load-bearing). Both this state and the inner `InkWell` state end
//! up listening on the *same* controller and syncing the *same* `Disabled`
//! bit from the *same* `enabled` predicate — redundant but harmless (each
//! sync is idempotent), and it is not a divergence: the oracle's own
//! `_ButtonStyleState` and `_InkResponseState` do the identical double-sync
//! on the same shared `MaterialStatesController` (`ButtonStyleButton` passes
//! `statesController: statesController` straight into its `InkWell`).
//!
//! [`overlay_color`](crate::ButtonStyle::overlay_color) is the one property
//! NOT baked to a single value here: it is handed to `InkWell` as a live
//! [`WidgetStateProperty`] (closing over the widget/default styles) so
//! `InkWell`'s own internal rebuilds (e.g. its press-deactivation timer,
//! which fires independently of this type's rebuild — see `ink_well.rs`)
//! keep resolving it fresh, exactly matching the oracle's own
//! `overlayColor: WidgetStateProperty.resolveWith(...)` wrapper.

use std::sync::Arc;

use flui_sdk::foundation::{Listenable, ListenerId};
use flui_sdk::painting::Color;
use flui_sdk::painting::TextStyle;
use flui_sdk::rendering::BoxConstraints;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::{
    ConstrainedBox, DefaultTextStyle, Padding, Semantics, WidgetState, WidgetStateProperty,
    WidgetStates, WidgetStatesController,
};

use crate::button_style::ButtonStyle;
use crate::ink_well::InkWell;
use crate::material::Material;

pub(crate) use crate::event_callback::PressCallback;

/// Resolves one [`ButtonStyle`] property through the widget → theme →
/// default cascade — see the module docs.
///
/// `default` is `Option`, not a bare reference, because the oracle
/// explicitly allows a concrete button's `defaultStyleOf` to leave some
/// slots unset (`fixed_size` and `side`, in every V1 default table — see
/// `button_style_button.dart`'s `defaultStyleOf` doc comment, "Properties
/// that can be null").
pub(crate) fn resolve_property<T: Clone + Default>(
    states: &WidgetStates,
    widget: Option<&WidgetStateProperty<Option<T>>>,
    theme: Option<&WidgetStateProperty<Option<T>>>,
    default: Option<&WidgetStateProperty<Option<T>>>,
) -> Option<T> {
    widget
        .and_then(|property| property.resolve(states))
        .or_else(|| theme.and_then(|property| property.resolve(states)))
        .or_else(|| default.and_then(|property| property.resolve(states)))
}

/// Resolves every field of one [`ButtonStyle`] against `styles` for
/// `states`, tier-by-tier — the per-property expansion of
/// [`resolve_property`] used by both [`ButtonStyleButtonCoreState::build`]
/// and [`overlay_color_property`].
macro_rules! resolve_field {
    ($states:expr, $widget:expr, $theme:expr, $default:expr, $field:ident) => {
        resolve_property(
            $states,
            $widget.and_then(|s: &ButtonStyle| s.$field.as_ref()),
            $theme.and_then(|s: &ButtonStyle| s.$field.as_ref()),
            $default.and_then(|s: &ButtonStyle| s.$field.as_ref()),
        )
    };
}

/// The shared machinery every M3 button composes through: resolves its
/// [`ButtonStyle`] (widget-level `style`, falling through to
/// `default_style`) against the current interactive states, then wires
/// [`Material`] + [`InkWell`] + padding + size constraints + the resolved
/// text style around `child` — see the module docs.
#[derive(Clone, StatefulView)]
pub(crate) struct ButtonStyleButtonCore {
    on_pressed: Option<PressCallback>,
    style: Option<ButtonStyle>,
    theme_style: Option<ButtonStyle>,
    default_style: ButtonStyle,
    child: BoxedView,
}

impl ButtonStyleButtonCore {
    /// `child` wrapped with no press handler (disabled) and no style
    /// override; `default_style` is the concrete button's own
    /// `_TokenDefaultsM3` table, computed by the caller against the ambient
    /// theme.
    pub(crate) fn new(default_style: ButtonStyle, child: BoxedView) -> Self {
        Self {
            on_pressed: None,
            style: None,
            theme_style: None,
            default_style,
            child,
        }
    }

    /// Sets the press handler. Its presence is what makes this button
    /// [`enabled`](Self::is_interactive) — Flutter parity:
    /// `ButtonStyleButton.enabled => onPressed != null || onLongPress != null`
    /// (narrowed to `onPressed`; `onLongPress` is not part of this V1).
    #[must_use]
    pub(crate) fn on_pressed(mut self, callback: PressCallback) -> Self {
        self.on_pressed = Some(callback);
        self
    }

    /// Sets the widget-level style override — the highest-precedence tier
    /// in the resolve cascade.
    #[must_use]
    pub(crate) fn style(mut self, style: ButtonStyle) -> Self {
        self.style = Some(style);
        self
    }

    /// Sets the theme-level style — the middle tier in the resolve cascade,
    /// between [`style`](Self::style) (widget-explicit, highest precedence)
    /// and `default_style` (lowest). The caller passes its own `ThemeData`
    /// component-theme slot's `style` here; leaving this unset (the default
    /// from [`new`](Self::new)) means the cascade skips straight from
    /// `style` to `default_style`, exactly as it did before this tier
    /// existed.
    #[must_use]
    pub(crate) fn theme_style(mut self, style: ButtonStyle) -> Self {
        self.theme_style = Some(style);
        self
    }

    fn is_interactive(&self) -> bool {
        self.on_pressed.is_some()
    }
}

impl std::fmt::Debug for ButtonStyleButtonCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ButtonStyleButtonCore")
            .field("enabled", &self.is_interactive())
            .field("style", &self.style)
            .finish_non_exhaustive()
    }
}

/// Persistent state behind [`ButtonStyleButtonCore`] — see
/// `crate::ink_well::InkWellState`, whose `init_state`/`did_update_view`
/// ordering this mirrors exactly (see the module docs).
pub(crate) struct ButtonStyleButtonCoreState {
    states: WidgetStatesController,
    states_listener: Option<ListenerId>,
    /// Seeded from the initial view in `create_state`; consumed once by
    /// `init_state` (which has no `view` parameter — see `InkWellState`'s
    /// identical `tap_slot` shape for why `create_state` is where a
    /// `ViewState` impl captures anything it needs before its first
    /// build).
    initially_enabled: bool,
    rebuild: Option<RebuildHandle>,
}

impl StatefulView for ButtonStyleButtonCore {
    type State = ButtonStyleButtonCoreState;

    fn create_state(&self) -> Self::State {
        ButtonStyleButtonCoreState {
            states: WidgetStatesController::default(),
            states_listener: None,
            initially_enabled: self.is_interactive(),
            rebuild: None,
        }
    }
}

impl ViewState<ButtonStyleButtonCore> for ButtonStyleButtonCoreState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let rebuild = ctx.rebuild_handle();

        // Sync BEFORE listening — see `InkWellState::init_state`'s module-doc
        // rationale, which applies verbatim here.
        self.states
            .update(WidgetState::Disabled, !self.initially_enabled);

        let rebuild_for_listener = rebuild.clone();
        self.states_listener = Some(self.states.add_listener(Arc::new(move || {
            rebuild_for_listener.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));

        self.rebuild = Some(rebuild);
    }

    fn did_update_view(
        &mut self,
        old_view: &ButtonStyleButtonCore,
        new_view: &ButtonStyleButtonCore,
    ) {
        if new_view.is_interactive() != old_view.is_interactive() {
            self.states
                .update(WidgetState::Disabled, !new_view.is_interactive());
        }
    }

    fn build(&self, view: &ButtonStyleButtonCore, _ctx: &dyn BuildContext) -> impl IntoView {
        let states = self.states.value();
        let widget_style = view.style.as_ref();
        let default_style = Some(&view.default_style);
        // Set by the caller via `theme_style` when its `ThemeData`
        // component-theme slot is configured — see the module docs.
        let theme_style = view.theme_style.as_ref();

        let background_color = resolve_field!(
            &states,
            widget_style,
            theme_style,
            default_style,
            background_color
        )
        .unwrap_or(Color::TRANSPARENT);
        let foreground_color = resolve_field!(
            &states,
            widget_style,
            theme_style,
            default_style,
            foreground_color
        );
        let elevation =
            resolve_field!(&states, widget_style, theme_style, default_style, elevation)
                .unwrap_or(0.0);
        let padding = resolve_field!(&states, widget_style, theme_style, default_style, padding)
            .unwrap_or_default();
        let minimum_size = resolve_field!(
            &states,
            widget_style,
            theme_style,
            default_style,
            minimum_size
        )
        .unwrap_or(flui_sdk::geometry::Size::ZERO);
        let fixed_size = resolve_field!(
            &states,
            widget_style,
            theme_style,
            default_style,
            fixed_size
        );
        let maximum_size = resolve_field!(
            &states,
            widget_style,
            theme_style,
            default_style,
            maximum_size
        )
        .unwrap_or(flui_sdk::geometry::Size::INFINITY);
        // `side` is NOT resolved here: `Material`/`MaterialShape` has no
        // border-side painting path yet (see `ButtonStyle::side`'s doc
        // comment), so nothing in this composition would consume it. Each
        // button's `default_style` (and any widget-level override) still
        // carries the slot — `OutlinedButton`'s own tests exercise its
        // resolution directly — this composition step just has nothing to
        // do with the result yet.
        let shape = resolve_field!(&states, widget_style, theme_style, default_style, shape)
            .unwrap_or_default();
        let text_style = resolve_field!(
            &states,
            widget_style,
            theme_style,
            default_style,
            text_style
        );

        let text_style = fold_foreground_into_text_style(text_style, foreground_color);

        let constraints = effective_constraints(minimum_size, maximum_size, fixed_size);

        let overlay_color = overlay_color_property(
            view.style.clone(),
            view.theme_style.clone(),
            view.default_style.clone(),
        );

        let mut ink_well = InkWell::new(
            Padding::new(padding).child(DefaultTextStyle::new(text_style, view.child.clone())),
        )
        .shape(shape)
        .overlay_color(overlay_color)
        .states_controller(self.states.clone());
        if let Some(on_pressed) = view.on_pressed.clone() {
            ink_well = ink_well.on_tap(move |cx| on_pressed(cx));
        }

        Semantics::new()
            .container(true)
            .button(true)
            .enabled(view.is_interactive())
            .child(
                ConstrainedBox::new(constraints).child(
                    Material::new(background_color)
                        .elevation(elevation)
                        .shape(shape)
                        .child(ink_well),
                ),
            )
    }

    fn dispose(&mut self) {
        if let Some(id) = self.states_listener.take() {
            self.states.remove_listener(id);
        }
    }
}

/// Folds `foreground_color` into `text_style`'s own `color` — Flutter
/// parity: `resolvedTextStyle?.copyWith(color: resolvedForegroundColor)`
/// (`button_style_button.dart` `:539`). `foreground_color` takes precedence
/// over whatever color `text_style` already carries (see
/// [`ButtonStyle::foreground_color`](crate::ButtonStyle::foreground_color)'s
/// doc comment).
fn fold_foreground_into_text_style(
    text_style: Option<TextStyle>,
    foreground_color: Option<Color>,
) -> TextStyle {
    let text_style = text_style.unwrap_or_default();
    match foreground_color {
        Some(color) => text_style.with_color(color),
        None => text_style,
    }
}

/// Builds the [`BoxConstraints`] the button's `ConstrainedBox` enforces from
/// its three resolved size slots. Flutter parity:
/// `_ButtonStyleState.build`'s `effectiveConstraints` construction
/// (`button_style_button.dart` `:517-533`, oracle tag `3.44.0`), narrowed to
/// the V1 slots (no `visualDensity` adjustment — see
/// `crate::button_style`'s module docs).
///
/// `minimum`/`maximum` build the base envelope; `fixed`, if present, is
/// clamped INTO that already-built envelope (`effectiveConstraints.constrain
/// (resolvedFixedSize)`) before pinning `min == max` on each of its finite
/// axes — clamping first, not pinning `fixed` directly, is load-bearing: a
/// `fixed` smaller than `minimum` (or larger than `maximum`) would otherwise
/// invert `min > max` into a malformed [`BoxConstraints`] instead of
/// clamping to the envelope's edge, matching the oracle's own behavior.
fn effective_constraints(
    minimum: flui_sdk::geometry::Size,
    maximum: flui_sdk::geometry::Size,
    fixed: Option<flui_sdk::geometry::Size>,
) -> BoxConstraints {
    let mut constraints =
        BoxConstraints::new(minimum.width, maximum.width, minimum.height, maximum.height);
    let Some(fixed) = fixed else {
        return constraints;
    };

    let clamped = constraints.constrain(fixed);
    if fixed.width.is_finite() {
        constraints.min_width = clamped.width;
        constraints.max_width = clamped.width;
    }
    if fixed.height.is_finite() {
        constraints.min_height = clamped.height;
        constraints.max_height = clamped.height;
    }
    constraints
}

/// Builds the live [`WidgetStateProperty`] handed to the inner `InkWell` —
/// see the module docs on why `overlay_color` is not baked to a single
/// value like every other property. Closes over all three cascade tiers
/// (widget/theme/default), same as [`ButtonStyleButtonCoreState::build`]'s
/// own per-property resolution, so a theme-configured `overlay_color`
/// reaches the live property too.
fn overlay_color_property(
    widget_style: Option<ButtonStyle>,
    theme_style: Option<ButtonStyle>,
    default_style: ButtonStyle,
) -> WidgetStateProperty<Option<Color>> {
    WidgetStateProperty::resolve_with(move |states: &WidgetStates| {
        resolve_field!(
            states,
            widget_style.as_ref(),
            theme_style.as_ref(),
            Some(&default_style),
            overlay_color
        )
    })
}

#[cfg(test)]
mod tests {
    use flui_sdk::widgets::WidgetStates;

    use super::*;

    fn all_property<T: Clone>(value: T) -> WidgetStateProperty<Option<T>> {
        WidgetStateProperty::all(Some(value))
    }

    #[test]
    fn widget_tier_wins_when_all_three_are_set() {
        let widget = all_property(1_u32);
        let default = all_property(3_u32);
        let resolved = resolve_property(&WidgetStates::NONE, Some(&widget), None, Some(&default));
        assert_eq!(resolved, Some(1));
    }

    // ------------------------------------------------------------------
    // effective_constraints — min/max envelope + fixed-size clamping
    // ------------------------------------------------------------------
}
