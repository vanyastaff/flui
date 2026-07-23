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
//! This is exactly the shape `flui_widgets::widget_state`'s
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
//! ConstrainedBox(min/fixed/max size)
//!   Material(color, elevation, shape)
//!     InkWell(overlay_color, shared states controller, enabled = on_pressed.is_some())
//!       Padding(padding)
//!         DefaultTextStyle(text_style with foreground_color folded into its `color`)
//!           child
//! ```
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

use std::rc::Rc;
use std::sync::Arc;

use flui_foundation::{Listenable, ListenerId};
use flui_rendering::constraints::BoxConstraints;
use flui_types::Color;
use flui_types::typography::TextStyle;
use flui_view::RebuildHandle;
use flui_view::prelude::*;
use flui_widgets::{
    ConstrainedBox, DefaultTextStyle, Padding, WidgetState, WidgetStateProperty, WidgetStates,
    WidgetStatesController,
};

use crate::button_style::ButtonStyle;
use crate::ink_well::InkWell;
use crate::material::Material;

/// A no-argument press handler — the button-family counterpart to
/// `InkWell`'s own `Rc<dyn Fn()>` tap callback (owner-local, per ADR-0027).
pub(crate) type PressCallback = Rc<dyn Fn()>;

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
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        let rebuild = ctx.rebuild_handle();

        // Sync BEFORE listening — see `InkWellState::init_state`'s module-doc
        // rationale, which applies verbatim here.
        self.states
            .update(WidgetState::Disabled, !self.initially_enabled);

        let rebuild_for_listener = rebuild.clone();
        self.states_listener = Some(self.states.add_listener(Arc::new(move || {
            rebuild_for_listener.schedule(flui_view::RebuildReason::StateChange);
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
        .unwrap_or(flui_types::Size::ZERO);
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
        .unwrap_or(flui_types::Size::INFINITY);
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
            ink_well = ink_well.on_tap(move || on_pressed());
        }

        ConstrainedBox::new(constraints).child(
            Material::new(background_color)
                .elevation(elevation)
                .shape(shape)
                .child(ink_well),
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
    minimum: flui_types::Size,
    maximum: flui_types::Size,
    fixed: Option<flui_types::Size>,
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
    use flui_widgets::{WidgetStateConstraint, WidgetStates};

    use super::*;

    fn all_property<T: Clone>(value: T) -> WidgetStateProperty<Option<T>> {
        WidgetStateProperty::all(Some(value))
    }

    /// `overlay_color_property`'s theme tier, resolved for a `Pressed` state
    /// — the live `WidgetStateProperty` handed to `InkWell` (see the
    /// function's own doc comment: it closes over all three cascade tiers,
    /// same as `ButtonStyleButtonCoreState::build`'s per-property
    /// resolution). Mutation-honest: with no widget-level `overlay_color`
    /// set, the resolved value must be the THEME tier's color, not the
    /// default tier's — reverting `theme_style.as_ref()` back to a
    /// hardcoded `None` (its state before `theme_style` was wired) would
    /// resolve `default_overlay` here instead of `themed_overlay`, failing
    /// this assertion.
    #[test]
    fn overlay_color_property_resolves_the_theme_tier_for_a_pressed_state() {
        let themed_overlay = Color::rgb(9, 9, 9);
        let default_overlay = Color::rgb(1, 1, 1);
        let theme_style = ButtonStyle {
            overlay_color: Some(all_property(themed_overlay)),
            ..Default::default()
        };
        let default_style = ButtonStyle {
            overlay_color: Some(all_property(default_overlay)),
            ..Default::default()
        };

        let property = overlay_color_property(None, Some(theme_style), default_style);
        let resolved = property.resolve(&WidgetStates::from(WidgetState::Pressed));

        assert_eq!(
            resolved,
            Some(themed_overlay),
            "a theme-configured overlay_color, with no widget-level override, must reach the \
             live WidgetStateProperty handed to InkWell — not fall through to the default tier",
        );
    }

    /// Companion coverage: an explicit widget-level `overlay_color` still
    /// wins over a configured theme tier, matching every other property's
    /// widget > theme > default precedence.
    #[test]
    fn overlay_color_property_widget_tier_wins_over_the_theme_tier() {
        let widget_overlay = Color::rgb(2, 2, 2);
        let themed_overlay = Color::rgb(9, 9, 9);
        let widget_style = ButtonStyle {
            overlay_color: Some(all_property(widget_overlay)),
            ..Default::default()
        };
        let theme_style = ButtonStyle {
            overlay_color: Some(all_property(themed_overlay)),
            ..Default::default()
        };

        let property = overlay_color_property(
            Some(widget_style),
            Some(theme_style),
            ButtonStyle::default(),
        );
        let resolved = property.resolve(&WidgetStates::from(WidgetState::Pressed));

        assert_eq!(resolved, Some(widget_overlay));
    }

    #[test]
    fn widget_tier_wins_when_all_three_are_set() {
        let widget = all_property(1_u32);
        let default = all_property(3_u32);
        let resolved = resolve_property(&WidgetStates::NONE, Some(&widget), None, Some(&default));
        assert_eq!(resolved, Some(1));
    }

    #[test]
    fn default_tier_is_used_when_widget_and_theme_are_absent() {
        let default = all_property(3_u32);
        let resolved: Option<u32> =
            resolve_property(&WidgetStates::NONE, None, None, Some(&default));
        assert_eq!(resolved, Some(3));
    }

    /// Mutation-honest: if the coalesce stopped checking the widget
    /// property's OWN resolution and instead treated "widget property is
    /// present" as sufficient, this would return `Some(1)` from a widget
    /// property that only covers `Pressed` while the button is unpressed —
    /// it must fall through to `default` instead.
    #[test]
    fn a_widget_property_that_resolves_none_for_this_state_falls_through_to_default() {
        let widget: WidgetStateProperty<Option<u32>> = WidgetStateProperty::from_map([(
            WidgetStateConstraint::Is(flui_widgets::WidgetState::Pressed),
            Some(1_u32),
        )]);
        let default = all_property(3_u32);
        let resolved = resolve_property(&WidgetStates::NONE, Some(&widget), None, Some(&default));
        assert_eq!(resolved, Some(3));
    }

    #[test]
    fn nothing_set_anywhere_resolves_to_none() {
        let resolved: Option<u32> = resolve_property(&WidgetStates::NONE, None, None, None);
        assert_eq!(resolved, None);
    }

    #[test]
    fn fold_foreground_into_text_style_overrides_the_text_styles_own_color() {
        let base = TextStyle::new().with_color(Color::rgb(1, 1, 1));
        let folded = fold_foreground_into_text_style(Some(base), Some(Color::rgb(9, 9, 9)));
        assert_eq!(folded.color, Some(Color::rgb(9, 9, 9)));
    }

    #[test]
    fn fold_foreground_into_text_style_keeps_the_base_color_when_no_foreground_is_resolved() {
        let base = TextStyle::new().with_color(Color::rgb(1, 1, 1));
        let folded = fold_foreground_into_text_style(Some(base.clone()), None);
        assert_eq!(folded.color, base.color);
    }

    // ------------------------------------------------------------------
    // effective_constraints — min/max envelope + fixed-size clamping
    // ------------------------------------------------------------------

    fn size(width: f32, height: f32) -> flui_types::Size {
        flui_types::Size::new(
            flui_types::geometry::px(width),
            flui_types::geometry::px(height),
        )
    }

    #[test]
    fn no_fixed_size_passes_minimum_and_maximum_through_unpinned() {
        let constraints = effective_constraints(size(64.0, 40.0), size(200.0, 100.0), None);
        assert_eq!(constraints.min_width, flui_types::geometry::px(64.0));
        assert_eq!(constraints.max_width, flui_types::geometry::px(200.0));
        assert_eq!(constraints.min_height, flui_types::geometry::px(40.0));
        assert_eq!(constraints.max_height, flui_types::geometry::px(100.0));
    }

    /// A `fixed_size` inside `[minimum, maximum]` pins `min == max` at
    /// exactly that value on both axes.
    #[test]
    fn fixed_size_inside_the_envelope_pins_min_and_max_to_it() {
        let constraints =
            effective_constraints(size(64.0, 40.0), size(200.0, 100.0), Some(size(90.0, 60.0)));
        assert_eq!(constraints.min_width, flui_types::geometry::px(90.0));
        assert_eq!(constraints.max_width, flui_types::geometry::px(90.0));
        assert_eq!(constraints.min_height, flui_types::geometry::px(60.0));
        assert_eq!(constraints.max_height, flui_types::geometry::px(60.0));
    }

    /// Mutation-honest — the bug this test would have caught: a
    /// `fixed_size` (10×10) SMALLER than `minimum` (64×40) must clamp UP to
    /// the minimum before pinning, not pin directly to 10×10 (which would
    /// invert `min > max` against a `maximum` of 200×100 anyway, but more
    /// importantly silently produces a button smaller than its own declared
    /// minimum — the oracle's `effectiveConstraints.constrain(resolvedFixedSize)`
    /// step this function ports).
    #[test]
    fn fixed_size_smaller_than_minimum_is_clamped_up_to_the_minimum() {
        let constraints =
            effective_constraints(size(64.0, 40.0), size(200.0, 100.0), Some(size(10.0, 10.0)));
        assert_eq!(constraints.min_width, flui_types::geometry::px(64.0));
        assert_eq!(constraints.max_width, flui_types::geometry::px(64.0));
        assert_eq!(constraints.min_height, flui_types::geometry::px(40.0));
        assert_eq!(constraints.max_height, flui_types::geometry::px(40.0));
    }

    /// Symmetric case: a `fixed_size` LARGER than `maximum` clamps down.
    #[test]
    fn fixed_size_larger_than_maximum_is_clamped_down_to_the_maximum() {
        let constraints = effective_constraints(
            size(64.0, 40.0),
            size(200.0, 100.0),
            Some(size(500.0, 500.0)),
        );
        assert_eq!(constraints.min_width, flui_types::geometry::px(200.0));
        assert_eq!(constraints.max_width, flui_types::geometry::px(200.0));
        assert_eq!(constraints.min_height, flui_types::geometry::px(100.0));
        assert_eq!(constraints.max_height, flui_types::geometry::px(100.0));
    }

    /// An infinite `fixed_size` axis is ignored on that axis (Flutter
    /// parity: "Fixed size dimensions whose value is double.infinity are
    /// ignored", `ButtonStyle.fixedSize`'s doc comment) — the other axis
    /// still pins.
    #[test]
    fn an_infinite_fixed_axis_leaves_that_axis_at_the_envelope() {
        let constraints = effective_constraints(
            size(64.0, 40.0),
            size(200.0, 100.0),
            Some(flui_types::Size::new(
                flui_types::Pixels::INFINITY,
                flui_types::geometry::px(60.0),
            )),
        );
        assert_eq!(constraints.min_width, flui_types::geometry::px(64.0));
        assert_eq!(constraints.max_width, flui_types::geometry::px(200.0));
        assert_eq!(constraints.min_height, flui_types::geometry::px(60.0));
        assert_eq!(constraints.max_height, flui_types::geometry::px(60.0));
    }
}
