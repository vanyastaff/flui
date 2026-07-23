//! [`Radio`] — a mutually-exclusive-group M3 selection control.
//!
//! # Flutter parity
//!
//! `material/radio.dart`, `material/radio_theme.dart`, and
//! `widgets/toggleable.dart`'s `ToggleableStateMixin`/`ToggleablePainter`
//! (oracle tag `3.44.0`).
//!
//! # API shape: the deprecated-but-functional direct `value`/`groupValue`/
//! `onChanged` triple, not the `RadioGroup` ancestor
//!
//! At the oracle tag, `Radio<T>`'s own `groupValue`/`onChanged` constructor
//! parameters are marked `@Deprecated` in favor of an ancestor `RadioGroup`
//! `InheritedModel` that centralizes group value + change routing for every
//! `Radio<T>` beneath it (`widgets/radio_group.dart`). Both shapes are still
//! present and functional at `3.44.0` — this port targets the direct triple
//! (`value`/`group_value`/`on_changed`), matching Checkbox's/Switch's own
//! self-contained (no ambient-registry) shape, and named as a divergence
//! rather than silently treated as "the whole oracle API": a `RadioGroup`
//! equivalent is a separate, larger feature (an `InheritedModel`-shaped
//! group registry this crate has no precedent for yet), not a narrowing of
//! this type's own scope.
//!
//! # Generics: `Radio<T>` ships as written, not a monomorphic fallback
//!
//! `#[derive(StatefulView)]` (`flui-macros`) forwards the struct's generics
//! through `syn::Generics::split_for_impl` into the generated `impl View`
//! block, so a generic `Radio<T>` mounts through the same `StatefulView`
//! machinery as every non-generic control in this crate — no fight with the
//! view macros materialized. [`RadioState`] itself stays a single
//! non-generic type (it holds no `T`-typed data — `value`/`group_value` live
//! on the view, re-read fresh each `build`), implementing
//! `ViewState<Radio<T>>` once per `T` via a generic `impl` block; this
//! avoids a `PhantomData<T>` marker field entirely. Required bounds: `T:
//! PartialEq + Clone + 'static` (equality for `selected = value ==
//! group_value`, `Clone` to hand an owned value to
//! [`Radio::on_changed`] and to satisfy `#[derive(Clone)]`/`StatefulView`'s
//! own `Clone + 'static` supertrait bound).
//!
//! # V1 scope: static states, no toggle/reaction animation
//!
//! Same shape as [`crate::Checkbox`]'s V1 scope: no `positionController`
//! interpolation (the inner dot snaps straight from absent to
//! `_kInnerRadius`, no `reactionController` radial-splash painting
//! (substituted by [`InkWell`]'s single resolved-color overlay, shaped as
//! [`MaterialShape::Stadium`] over the tap target — the same named
//! approximation of the oracle's `splashRadius`-sized free circle
//! [`crate::Checkbox`]/[`crate::Switch`] already document). Tap-only, no
//! keyboard-arrow group navigation.
//!
//! # Composition: same InkWell-sharing shape as `Checkbox`/`Switch`
//!
//! [`RadioState`] owns one [`WidgetStatesController`] shared with the
//! [`InkWell`] it builds — `InkWell` manages
//! `Hovered`/`Focused`/`Pressed`/`Disabled`, `Radio` manages `Selected`
//! (`value == group_value`, recomputed on every `did_update_view`).
//!
//! # Painting: concentric circles, no dedicated `Circle` decoration needed
//!
//! `flui-painting`'s [`Canvas::draw_circle`] draws both rings directly — the
//! painting-reality-check this control's own module doc calls for resolves
//! favorably without any nested-`DecoratedBox` workaround. `RadioPainter`
//! draws the `8.0`dp outer ring as a **centered stroke**
//! ([`Canvas::draw_circle`] with a [`Paint::stroke`]) rather than
//! [`crate::Checkbox`]'s fill+`drrect`-border approach: the oracle's own
//! default `activeSide`/`inactiveSide` (`radio.dart` `:749-764`) already use
//! `BorderSide.strokeAlignCenter`, which a centered stroke paint is a direct
//! port of (Checkbox's border is inside-aligned instead, hence the
//! `drrect`). The inner dot ([`Canvas::draw_circle`], filled) paints only
//! when selected — the oracle's own `!position.isDismissed` guard
//! (`radio.dart` `:870`) collapses to exactly that under V1's non-animated
//! `position ∈ {0.0, 1.0}`. Both rings share ONE resolved color (the
//! oracle's `activeColor`/`inactiveColor` are themselves just `fillColor`
//! resolved against the selected/unselected state, and — unlike Checkbox's
//! independent `checkColor` — there is no second, separately-themeable
//! color for the dot). The background circle
//! (`_RadioDefaultsM3.backgroundColor`) is fixed `Colors.transparent` with
//! no override surface in this V1, so it is not painted at all rather than
//! drawn as a no-op transparent fill.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Toggle/reaction/hover/focus-fade animation, `RadioGroup` ancestor
//!   routing** — see above.
//! - **`toggleable`** (tap-to-deselect on an already-selected radio) — V1's
//!   tap handler is a plain no-op on an already-selected radio, matching
//!   the oracle's default (`toggleable: false`); no toggle-off path.
//! - **`Radio.adaptive`** (`CupertinoRadio` platform switch) — no
//!   `TargetPlatform` substrate.
//! - **`mouse_cursor`, `splash_radius`, `material_tap_target_size`,
//!   `visual_density`, `side`, `inner_radius`, `background_color`
//!   overrides** — V1 always uses the M3 defaults (`kMinInteractiveDimension`
//!   = 48dp tap target, ring stroke width `2.0`, inner radius `4.5`,
//!   background always transparent).
//! - **Widget-level `fill_color`/`overlay_color` `WidgetStateProperty`
//!   overrides** — only [`Radio::active_color`] (the oracle's plain-`Color`
//!   override) ships at the widget tier; the theme tier
//!   ([`crate::RadioThemeData`]) and the M3 default tier are both fully
//!   state-resolved.
//! - **`focus_node`/`autofocus`** — same whole-substrate `InkWell` gap
//!   [`crate::Checkbox`]/[`crate::Switch`] already name.
//! - **`in_mutually_exclusive_group`/platform-conditional `selected`/`hint`
//!   accessibility fields** — `flui_widgets::Semantics` has no
//!   `in_mutually_exclusive_group` builder method yet (a substrate gap, not
//!   specific to this type); only `.checked()`/`.enabled()` are wired.

use std::rc::Rc;

use flui_foundation::Listenable;
use flui_rendering::pipeline::Canvas;
use flui_types::geometry::px;
use flui_types::painting::Paint;
use flui_types::styling::Color;
use flui_types::{Point, Size};
use flui_view::prelude::*;
use flui_widgets::{
    CustomPaint, CustomPainter, Semantics, WidgetState, WidgetStateProperty, WidgetStates,
    WidgetStatesController,
};

use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::shape::MaterialShape;
use crate::state_color::resolve_state_color;
use crate::theme::Theme;

/// The outer ring's radius. Flutter parity: `_kOuterRadius` (`radio.dart`,
/// oracle tag `3.44.0`), `8.0`.
const OUTER_RADIUS: f32 = 8.0;

/// The inner (selected) dot's radius. Flutter parity: `_kInnerRadius`,
/// `4.5`.
const INNER_RADIUS: f32 = 4.5;

/// The outer ring's centered stroke width. Flutter parity: the oracle's
/// default `activeSide`/`inactiveSide`, `BorderSide(width: 2.0, strokeAlign:
/// BorderSide.strokeAlignCenter)` (`radio.dart` `:749-764`).
const RING_STROKE_WIDTH: f32 = 2.0;

/// The M3 tap-target side length. Flutter parity: `kMinInteractiveDimension`
/// (`constants.dart`, `48.0`), the `MaterialTapTargetSize.padded` branch
/// `_RadioState.build` always takes in V1 — same deferral
/// [`crate::Checkbox`]/[`crate::Switch`] already make.
pub const RADIO_TAP_TARGET_SIZE: f32 = 48.0;

const _: () = assert!(OUTER_RADIUS * 2.0 < RADIO_TAP_TARGET_SIZE);
const _: () = assert!(INNER_RADIUS < OUTER_RADIUS);

/// A value-change callback: the newly-selected value. `Rc`-based
/// (owner-local, per ADR-0027) — matches [`InkWell`]'s own callback shape.
type RadioChangeCallback<T> = Rc<dyn Fn(T)>;

/// A Material Design radio button for selecting one value out of a
/// mutually-exclusive group. See the module docs' "API shape" section for
/// why this ships the direct `value`/`group_value`/`on_changed` triple
/// rather than an ambient `RadioGroup` registry.
///
/// ```rust
/// use flui_material::Radio;
///
/// #[derive(Clone, PartialEq)]
/// enum Season {
///     Spring,
///     Summer,
/// }
///
/// let _selected = Radio::new(Season::Spring, Some(Season::Spring)).on_changed(|_next| { /* ... */ });
/// let _unselected = Radio::new(Season::Summer, Some(Season::Spring));
/// ```
#[derive(Clone, StatefulView)]
pub struct Radio<T>
where
    T: PartialEq + Clone + 'static,
{
    value: T,
    group_value: Option<T>,
    on_changed: Option<RadioChangeCallback<T>>,
    active_color: Option<Color>,
}

impl<T: PartialEq + Clone + std::fmt::Debug + 'static> std::fmt::Debug for Radio<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Radio")
            .field("value", &self.value)
            .field("group_value", &self.group_value)
            .field("is_interactive", &self.is_interactive())
            .finish_non_exhaustive()
    }
}

impl<T: PartialEq + Clone + 'static> Radio<T> {
    /// Creates a radio for `value` in a group currently at `group_value` —
    /// selected when the two compare equal. No change handler (disabled),
    /// no overrides.
    #[must_use]
    pub fn new(value: T, group_value: Option<T>) -> Self {
        Self {
            value,
            group_value,
            on_changed: None,
            active_color: None,
        }
    }

    /// Sets the change handler. Presence of a handler is what makes this
    /// radio interactive — `None` (the default) renders disabled and
    /// swallows taps. Fires with a clone of [`Self::new`]'s `value` when
    /// tapped while NOT already selected; a no-op tap on an already-selected
    /// radio (see the module docs' `toggleable` deferral). Flutter parity:
    /// `Radio.onChanged`.
    #[must_use]
    pub fn on_changed(mut self, callback: impl Fn(T) + 'static) -> Self {
        self.on_changed = Some(Rc::new(callback));
        self
    }

    /// Overrides the ring/dot color used when this radio is selected (and
    /// enabled). Flutter parity: `Radio.activeColor`.
    #[must_use]
    pub fn active_color(mut self, color: Color) -> Self {
        self.active_color = Some(color);
        self
    }

    /// Whether this radio responds to taps. Flutter parity:
    /// `ToggleableStateMixin.isInteractive` (`onChanged != null`).
    fn is_interactive(&self) -> bool {
        self.on_changed.is_some()
    }

    /// Whether `value` matches the group's current value. Flutter parity:
    /// the selection test `RawRadio`'s `_RawRadioState` applies internally
    /// (`value == groupValue`, via `RadioGroupRegistry`/direct `groupValue`
    /// comparison).
    fn is_selected(&self) -> bool {
        self.group_value.as_ref() == Some(&self.value)
    }
}

/// Persistent state behind every `Radio<T>` — owns the
/// [`WidgetStatesController`] shared with the [`InkWell`] this view builds.
/// Deliberately non-generic: it holds no `T`-typed data (`value`/
/// `group_value` live on the view, re-read fresh each `build`) — see the
/// module docs' "Generics" section for why this avoids a `PhantomData<T>`
/// marker. See [`crate::checkbox::CheckboxState`]'s doc comment for why
/// `Selected` lives here while `Hovered`/`Focused`/`Pressed`/`Disabled` are
/// `InkWell`'s to manage.
pub struct RadioState {
    states: WidgetStatesController,
    states_listener: Option<flui_foundation::ListenerId>,
}

impl std::fmt::Debug for RadioState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RadioState")
            .field("states", &self.states)
            .finish_non_exhaustive()
    }
}

impl<T: PartialEq + Clone + 'static> StatefulView for Radio<T> {
    type State = RadioState;

    fn create_state(&self) -> Self::State {
        let initial = if self.is_selected() {
            WidgetStates::from(WidgetState::Selected)
        } else {
            WidgetStates::NONE
        };
        RadioState {
            states: WidgetStatesController::new(initial),
            states_listener: None,
        }
    }
}

impl<T: PartialEq + Clone + 'static> ViewState<Radio<T>> for RadioState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // The handle is consumed directly by the listener closure; nothing
        // else needs to re-read it later, so it is not stored on `self`.
        let rebuild = ctx.rebuild_handle();
        self.states_listener = Some(self.states.add_listener(std::sync::Arc::new(move || {
            rebuild.schedule(flui_view::RebuildReason::StateChange);
        })));
    }

    fn did_update_view(&mut self, old_view: &Radio<T>, new_view: &Radio<T>) {
        let was_selected = old_view.is_selected();
        let is_selected = new_view.is_selected();
        if was_selected != is_selected {
            self.states.update(WidgetState::Selected, is_selected);
        }
    }

    fn build(&self, view: &Radio<T>, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let radio_theme = theme.radio_theme.clone();
        let colors = theme.color_scheme;

        let states = self.states.value();

        let ring_color = resolve_radio_ring_color(
            view.active_color,
            radio_theme.as_ref().and_then(|t| t.fill_color.as_ref()),
            &colors,
            states,
        );

        let theme_overlay = radio_theme.as_ref().and_then(|t| t.overlay_color.clone());
        let overlay_color = WidgetStateProperty::resolve_with(move |live_states: &WidgetStates| {
            resolve_state_color(theme_overlay.as_ref(), live_states)
                .or_else(|| radio_default_overlay_color(&colors, *live_states))
        });

        let selected = view.is_selected();
        let painter: std::sync::Arc<dyn CustomPainter> = std::sync::Arc::new(RadioPainter {
            ring_color,
            selected,
        });

        let interactive = view.is_interactive();
        let next_value = view.value.clone();
        let on_changed = view.on_changed.clone();
        let mut ink_well = InkWell::new(
            CustomPaint::new()
                .size(Size::new(
                    px(RADIO_TAP_TARGET_SIZE),
                    px(RADIO_TAP_TARGET_SIZE),
                ))
                .painter(painter),
        )
        .shape(MaterialShape::Stadium)
        .overlay_color(overlay_color)
        .states_controller(self.states.clone());
        if interactive {
            ink_well = ink_well.on_tap(move || {
                if selected {
                    return;
                }
                if let Some(handler) = &on_changed {
                    handler(next_value.clone());
                }
            });
        }

        Semantics::new()
            .checked(selected)
            .enabled(interactive)
            .child(ink_well)
    }

    fn dispose(&mut self) {
        if let Some(id) = self.states_listener.take() {
            self.states.remove_listener(id);
        }
    }
}

/// Resolves [`Radio`]'s ring/inner-dot color through the widget -> theme ->
/// default cascade — extracted as its own pure function (not left inline
/// in `build`) specifically so the tier-precedence order is unit-testable
/// without mounting a widget tree. Same shape `crate::checkbox`'s own
/// `resolve_checkbox_fill_color` uses: `active_color` only substitutes
/// when [`WidgetState::Selected`] AND NOT [`WidgetState::Disabled`].
fn resolve_radio_ring_color(
    active_color: Option<Color>,
    theme_fill_color: Option<&WidgetStateProperty<Option<Color>>>,
    colors: &ColorScheme,
    states: WidgetStates,
) -> Color {
    let widget_active_override = (!states.contains_state(WidgetState::Disabled)
        && states.contains_state(WidgetState::Selected))
    .then_some(active_color)
    .flatten();

    widget_active_override
        .or_else(|| resolve_state_color(theme_fill_color, &states))
        .unwrap_or_else(|| radio_default_fill_color(colors, states))
}

/// `_RadioDefaultsM3.fillColor` (`radio.dart`, oracle tag `3.44.0`).
fn radio_default_fill_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Selected) {
        return if states.contains_state(WidgetState::Disabled) {
            colors.on_surface.with_opacity(0.38)
        } else {
            colors.primary
        };
    }
    if states.contains_state(WidgetState::Disabled) {
        return colors.on_surface.with_opacity(0.38);
    }
    if states.contains_state(WidgetState::Pressed)
        || states.contains_state(WidgetState::Hovered)
        || states.contains_state(WidgetState::Focused)
    {
        return colors.on_surface;
    }
    colors.on_surface_variant
}

/// `_RadioDefaultsM3.overlayColor` (`radio.dart`, oracle tag `3.44.0`).
/// Returns `None` where the oracle returns `Colors.transparent` — see
/// [`InkWell`]'s own "`None` resolution = no overlay layer at all" contract.
fn radio_default_overlay_color(colors: &ColorScheme, states: WidgetStates) -> Option<Color> {
    if states.contains_state(WidgetState::Selected) {
        if states.contains_state(WidgetState::Pressed) {
            return Some(colors.on_surface.with_opacity(0.1));
        }
        if states.contains_state(WidgetState::Hovered) {
            return Some(colors.primary.with_opacity(0.08));
        }
        if states.contains_state(WidgetState::Focused) {
            return Some(colors.primary.with_opacity(0.1));
        }
        return None;
    }
    if states.contains_state(WidgetState::Pressed) {
        return Some(colors.primary.with_opacity(0.1));
    }
    if states.contains_state(WidgetState::Hovered) {
        return Some(colors.on_surface.with_opacity(0.08));
    }
    if states.contains_state(WidgetState::Focused) {
        return Some(colors.on_surface.with_opacity(0.1));
    }
    None
}

/// Paints the radio's concentric circles, always at the fully-settled shape
/// (see the module docs' V1-scope section). Flutter parity: `_RadioPainter`
/// (`radio.dart` `:797-877`), evaluated at `position ∈ {0.0, 1.0}`
/// throughout with no `ToggleablePainter.paintRadialReaction` call (the
/// overlay comes from [`InkWell`] instead).
#[derive(Debug, Clone, PartialEq)]
struct RadioPainter {
    ring_color: Color,
    selected: bool,
}

impl CustomPainter for RadioPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        let center = Point::new(px(size.width.get() / 2.0), px(size.height.get() / 2.0));

        canvas.draw_circle(
            center,
            px(OUTER_RADIUS),
            &Paint::stroke(self.ring_color, RING_STROKE_WIDTH),
        );

        if self.selected {
            canvas.draw_circle(center, px(INNER_RADIUS), &Paint::fill(self.ring_color));
        }
    }

    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old != self)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ------------------------------------------------------------------
    // Construction / builder surface / selection
    // ------------------------------------------------------------------

    #[test]
    fn new_leaves_every_override_unset_and_is_not_interactive() {
        let radio = Radio::new(1_u32, None);
        assert!(radio.active_color.is_none());
        assert!(!radio.is_interactive());
    }

    #[test]
    fn on_changed_makes_the_radio_interactive() {
        let radio = Radio::new(1_u32, None).on_changed(|_| {});
        assert!(radio.is_interactive());
    }

    #[test]
    fn is_selected_true_when_value_matches_group_value() {
        assert!(Radio::new(1_u32, Some(1_u32)).is_selected());
    }

    #[test]
    fn is_selected_false_when_value_differs_from_group_value() {
        assert!(!Radio::new(1_u32, Some(2_u32)).is_selected());
    }

    #[test]
    fn is_selected_false_when_group_value_is_none() {
        assert!(!Radio::new(1_u32, None).is_selected());
    }

    // ------------------------------------------------------------------
    // M3 default token tables — per-state probes, oracle branch order
    // ------------------------------------------------------------------

    fn light() -> ColorScheme {
        ColorScheme::light()
    }

    // ------------------------------------------------------------------
    // resolve_radio_ring_color — tier precedence (widget > theme >
    // default), mutation-run: each probe below was verified to fail
    // against a deliberately broken cascade (widget/theme tier short-
    // circuited, or the `!Disabled && Selected` gate dropped) before being
    // confirmed against the real implementation.
    // ------------------------------------------------------------------

    #[test]
    fn theme_tier_beats_the_m3_default_when_no_widget_override_is_set() {
        let states = WidgetStates::from(WidgetState::Selected);
        let theme_color = WidgetStateProperty::all(Some(Color::rgb(9, 9, 9)));
        let resolved = resolve_radio_ring_color(None, Some(&theme_color), &light(), states);
        assert_eq!(resolved, Color::rgb(9, 9, 9));
        assert_ne!(resolved, radio_default_fill_color(&light(), states));
    }

    #[test]
    fn widget_override_wins_over_theme_and_default_when_selected_and_enabled() {
        let states = WidgetStates::from(WidgetState::Selected);
        let theme_color = WidgetStateProperty::all(Some(Color::rgb(9, 9, 9)));
        let resolved = resolve_radio_ring_color(
            Some(Color::rgb(1, 1, 1)),
            Some(&theme_color),
            &light(),
            states,
        );
        assert_eq!(resolved, Color::rgb(1, 1, 1));
    }

    #[test]
    fn widget_override_is_ignored_when_disabled_even_if_selected() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        let resolved = resolve_radio_ring_color(Some(Color::rgb(1, 1, 1)), None, &light(), states);
        assert_ne!(resolved, Color::rgb(1, 1, 1));
        assert_eq!(resolved, radio_default_fill_color(&light(), states));
    }

    #[test]
    fn widget_override_is_ignored_when_unselected() {
        let resolved = resolve_radio_ring_color(
            Some(Color::rgb(1, 1, 1)),
            None,
            &light(),
            WidgetStates::NONE,
        );
        assert_ne!(resolved, Color::rgb(1, 1, 1));
        assert_eq!(
            resolved,
            radio_default_fill_color(&light(), WidgetStates::NONE)
        );
    }

    #[test]
    fn default_fill_color_unselected_enabled_default_is_on_surface_variant() {
        assert_eq!(
            radio_default_fill_color(&light(), WidgetStates::NONE),
            light().on_surface_variant
        );
    }

    #[test]
    fn default_fill_color_selected_enabled_is_primary() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(radio_default_fill_color(&light(), states), light().primary);
    }

    #[test]
    fn default_fill_color_selected_hovered_is_still_primary() {
        // Branch-order pin: every selected+enabled sub-branch (pressed/
        // hovered/focused/plain) resolves to the SAME color in the oracle's
        // own table — confirms the collapsed `selected && !disabled =>
        // primary` shape this port uses is faithful, not an accidental
        // simplification.
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Hovered);
        assert_eq!(radio_default_fill_color(&light(), states), light().primary);
    }

    #[test]
    fn default_fill_color_selected_disabled_is_faded_on_surface() {
        // Combined pin: Selected+Disabled takes the disabled color, not
        // primary.
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Disabled);
        assert_eq!(
            radio_default_fill_color(&light(), states),
            light().on_surface.with_opacity(0.38)
        );
    }

    #[test]
    fn default_fill_color_unselected_disabled_is_faded_on_surface() {
        let states = WidgetStates::from(WidgetState::Disabled);
        assert_eq!(
            radio_default_fill_color(&light(), states),
            light().on_surface.with_opacity(0.38)
        );
    }

    #[test]
    fn default_fill_color_unselected_hovered_is_on_surface() {
        let states = WidgetStates::from(WidgetState::Hovered);
        assert_eq!(
            radio_default_fill_color(&light(), states),
            light().on_surface
        );
    }

    #[test]
    fn default_overlay_color_selected_hovered_is_primary_at_8_percent() {
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Hovered);
        assert_eq!(
            radio_default_overlay_color(&light(), states),
            Some(light().primary.with_opacity(0.08))
        );
    }

    #[test]
    fn default_overlay_color_selected_pressed_is_on_surface_at_10_percent() {
        // Branch-order/value pin: selected+pressed uses `onSurface`, NOT
        // `primary` — the one branch in this table that breaks the
        // otherwise-uniform selected-tier color, so it needs its own probe.
        let states = WidgetStates::from(WidgetState::Selected).with_state(WidgetState::Pressed);
        assert_eq!(
            radio_default_overlay_color(&light(), states),
            Some(light().on_surface.with_opacity(0.1))
        );
    }

    #[test]
    fn default_overlay_color_unselected_pressed_is_primary_at_10_percent() {
        let states = WidgetStates::from(WidgetState::Pressed);
        assert_eq!(
            radio_default_overlay_color(&light(), states),
            Some(light().primary.with_opacity(0.1))
        );
    }

    #[test]
    fn default_overlay_color_selected_default_is_none() {
        let states = WidgetStates::from(WidgetState::Selected);
        assert_eq!(radio_default_overlay_color(&light(), states), None);
    }

    #[test]
    fn default_overlay_color_unselected_default_is_none() {
        assert_eq!(
            radio_default_overlay_color(&light(), WidgetStates::NONE),
            None
        );
    }

    // ------------------------------------------------------------------
    // Painter should_repaint / geometry
    // ------------------------------------------------------------------

    fn painter(selected: bool) -> RadioPainter {
        RadioPainter {
            ring_color: Color::BLACK,
            selected,
        }
    }

    #[test]
    fn should_repaint_is_false_for_an_identical_delegate() {
        let old = painter(true);
        let new = painter(true);
        assert!(!new.should_repaint(&old));
    }

    #[test]
    fn should_repaint_is_true_when_selected_changes() {
        let old = painter(false);
        let new = painter(true);
        assert!(new.should_repaint(&old));
    }

    /// Proves the painter is actually invoked (via a real [`Canvas`]/
    /// [`flui_painting::display_list::DrawCommand`]) and that the inner
    /// dot is drawn if and only if `selected` — the oracle's own
    /// `!position.isDismissed` guard (`radio.dart` `:870`), which V1's
    /// non-animated `position ∈ {0.0, 1.0}` collapses to exactly this
    /// boolean. Mutation-run: deleting the `if self.selected` guard in
    /// `RadioPainter::paint` (always drawing the dot) was confirmed to make
    /// the `unselected` half of this test fail before being reverted.
    #[test]
    fn inner_dot_is_present_only_when_selected() {
        use flui_painting::display_list::DrawCommand;

        let size = Size::new(px(RADIO_TAP_TARGET_SIZE), px(RADIO_TAP_TARGET_SIZE));

        for (selected, expected_circle_count) in [(false, 1_usize), (true, 2_usize)] {
            let mut canvas = Canvas::new();
            painter(selected).paint(&mut canvas, size);

            let circle_count = canvas
                .display_list()
                .iter()
                .filter(|command| matches!(command, DrawCommand::DrawCircle { .. }))
                .count();
            assert_eq!(
                circle_count, expected_circle_count,
                "selected={selected}: expected {expected_circle_count} drawn circle(s) \
                 (the outer ring, plus the inner dot only when selected)",
            );
        }
    }
}
