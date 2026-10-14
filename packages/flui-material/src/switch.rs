//! [`Switch`] — a binary on/off M3 selection control.
//!
//! # Toggle motion: one animated position drives the whole paint
//!
//! [`SwitchState`] owns one [`AnimatedValue<f64>`] — the switch's position,
//! `0.0` off and `1.0` on — bound to the ambient [`VsyncScope`] and
//! retargeted whenever [`Switch::new`]'s value changes. The painter samples
//! it at paint time (its repaint listenable), so a toggle repaints without
//! rebuilding: the thumb centre slides between the track's inner ends, the
//! thumb radius grows from 8 to 12, and the track fill, track outline and
//! thumb colours each interpolate between their resolved off and on
//! colours. Because every quantity follows the one position, a toggle that
//! interrupts a running one continues from the shown frame with no jump.
//! Under [`Directionality`] right-to-left the thumb's travel is mirrored:
//! on sits at the start (left) end. Reduced motion
//! (`MotionOverride::Reduce`) is the clock's: the new value shows on the
//! first frame.
//!
//! The motion is [`MotionSpec::default`] — 200 ms eased in and out, the
//! Material 3 toggle duration — and the [`MotionBuilder`] methods replace it.
//!
//! No radial-splash reaction painting (substituted by [`InkWell`]'s single
//! resolved-color overlay, see [`crate::Checkbox`]'s module docs). Also **no
//! drag-to-toggle**: a full Material switch flips its value on a horizontal
//! swipe, not just a tap; [`InkWell`] has no horizontal-drag hook (only
//! `on_tap`), so this switch is tap-only, matching [`crate::Checkbox`]'s own
//! tap-only precedent.
//!
//! # Composition: same InkWell-sharing shape as `Checkbox`
//!
//! [`SwitchState`] owns one [`WidgetStatesController`] shared with the
//! [`InkWell`] it builds — `InkWell` manages
//! `Hovered`/`Focused`/`Pressed`/`Disabled`, `Switch` manages `Selected`
//! (`Switch` has no tristate `null` to special-case, unlike `Checkbox`).
//!
//! # Painting: track pill + thumb circle, both real `Canvas` shapes
//!
//! `SwitchPainter` draws the 52×32dp track as a stadium-shaped fill +
//! optional 2dp outline ([`Canvas::draw_rrect`]/[`Canvas::draw_drrect`],
//! the same shape math `CheckboxPainter`'s box border already established)
//! and the thumb as a filled circle ([`Canvas::draw_circle`]) at the
//! animated position.
//!
//! **Named deferral**: the M3 thumb radius follows the position from
//! unselected (`inactiveThumbRadius = 8.0`) to selected
//! (`activeThumbRadius = 12.0`), but the ADDITIONAL press-grow
//! (`pressedThumbRadius = 14.0`) is skipped: it needs its own reaction
//! motion driven by `Pressed`, which this switch does not animate (the
//! state overlay comes from [`InkWell`]) — so `Pressed` does not affect
//! radius here.
//!
//! # Overlay shape: the whole tap target, not a per-thumb circle
//!
//! The M3 radial reaction is a free
//! (unclipped) circle **centered on the thumb's current position**, sized
//! by `splashRadius` (`20.0`). `InkWell`'s single-fill substitution (see
//! [`crate::Checkbox`]'s module docs) has no per-position clip — it paints
//! one shape-clipped fill over its own bounds. This port shapes that fill
//! as [`MaterialShape::Stadium`] over the FULL tap target (the same
//! substitution `Checkbox` makes, sized to the whole control rather than
//! the thumb) — a named, position-independent approximation of the
//! thumb-following splash.
//!
//! # Deferred (named, not silently dropped)
//!
//! - **Reaction/hover/focus-fade animation, drag-to-toggle, thumb
//!   press-grow** — see above. A hover, focus or press colour change snaps;
//!   only the on/off toggle animates.
//! - **`Switch.adaptive`** (`CupertinoSwitch` platform switch) — no
//!   `TargetPlatform` substrate.
//! - **Thumb icons** (`Switch.thumbIcon`) — no icon painted on the thumb.
//! - **`mouse_cursor`, `splash_radius`, `material_tap_target_size`,
//!   `padding`, `track_outline_width` overrides** — V1 always uses the M3
//!   defaults (tap target `60×48`dp = the 52×32dp track plus the M3
//!   default `EdgeInsets.symmetric(horizontal: 4)`, `track_outline_width =
//!   2.0`).
//! - **Widget-level `track_color`/`inactive_thumb_color`/
//!   `inactive_track_color`/thumb images overrides** — only
//!   [`Switch::active_thumb_color`] (the oracle's `activeThumbColor`) ships
//!   at the widget tier; the theme tier ([`crate::SwitchThemeData`]) and the
//!   M3 default tier are both fully state-resolved.
//! - **`focus_node`/`autofocus`** — same whole-substrate `InkWell` gap
//!   [`crate::Checkbox`] already names.

use std::rc::Rc;

use flui_sdk::animation::{AnimatedValue, AnimatedValueView, Animation, MotionSpec};
use flui_sdk::foundation::Listenable;
use flui_sdk::geometry::{Point, RRect, Rect, Size};
use flui_sdk::painting::Canvas;
use flui_sdk::painting::Color;
use flui_sdk::painting::Paint;
use flui_sdk::painting::TextDirection;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    CustomPaint, CustomPainter, Directionality, MotionBuilder, Semantics, WidgetState,
    WidgetStateProperty, WidgetStates, WidgetStatesController,
};

use crate::color_scheme::ColorScheme;
use crate::ink_well::InkWell;
use crate::shape::MaterialShape;
use crate::state_color::resolve_state_color;
use crate::theme::Theme;

/// The track's width (M3: `52.0`).
pub const SWITCH_TRACK_WIDTH: f64 = 52.0;

/// The track's height (M3: `32.0`).
pub const SWITCH_TRACK_HEIGHT: f64 = 32.0;

/// The M3 default horizontal padding added to the track to form the tap
/// target, each side: `EdgeInsets.symmetric(horizontal: 4)`.
const TAP_TARGET_HORIZONTAL_PADDING: f64 = 4.0;

/// The tap target's width: the track plus the M3 default horizontal
/// padding on both sides.
pub const SWITCH_TAP_TARGET_WIDTH: f64 = SWITCH_TRACK_WIDTH + TAP_TARGET_HORIZONTAL_PADDING * 2.0;

/// The tap target's height: the minimum interactive dimension, `48.0`
/// (`40.0 + 8.0`) — the `MaterialTapTargetSize.padded` branch this V1
/// always takes (no override yet, matching [`crate::Checkbox`]'s own
/// deferral).
pub const SWITCH_TAP_TARGET_HEIGHT: f64 = 48.0;

/// The thumb radius when selected (or thumb-iconed, which V1 doesn't
/// paint): `24.0 / 2`.
const ACTIVE_THUMB_RADIUS: f64 = 12.0;

/// The thumb radius when unselected: `16.0 / 2`.
const INACTIVE_THUMB_RADIUS: f64 = 8.0;

/// The track border's stroke width, `2.0`.
const TRACK_OUTLINE_WIDTH: f64 = 2.0;

// Compile-time geometry invariants — not runtime tests (every side is
// `const`): the track must fit inside the tap target on both axes, and the
// selected thumb must not overflow the track's rounded ends.
const _: () = assert!(SWITCH_TRACK_WIDTH < SWITCH_TAP_TARGET_WIDTH);
const _: () = assert!(SWITCH_TRACK_HEIGHT < SWITCH_TAP_TARGET_HEIGHT);
const _: () = assert!(ACTIVE_THUMB_RADIUS <= SWITCH_TRACK_HEIGHT / 2.0);

/// A value-change callback: the next boolean value. `Rc`-based (owner-local,
/// per ADR-0027) — matches [`InkWell`]'s own callback shape.
type SwitchChangeCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>, bool)>;

/// A Material Design binary (on/off) switch.
///
/// ```rust
/// use flui_material::Switch;
///
/// let _off = Switch::new(false).on_changed(|_cx, _next| { /* ... */ });
/// let _disabled = Switch::new(true);
/// ```
///
/// A change of `value` animates the thumb and track over the switch's motion,
/// [`MotionSpec::default`] unless a [`MotionBuilder`] method replaces it:
///
/// ```rust
/// use flui_material::Switch;
/// use flui_sdk::widgets::MotionBuilder;
/// use std::time::Duration;
///
/// let _slow = Switch::new(true).duration(Duration::from_millis(400));
/// ```
#[derive(Clone, StatefulView)]
pub struct Switch {
    value: bool,
    on_changed: Option<SwitchChangeCallback>,
    active_thumb_color: Option<Color>,
    motion: MotionSpec,
}

impl std::fmt::Debug for Switch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Switch")
            .field("value", &self.value)
            .field("is_interactive", &self.is_interactive())
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

impl MotionBuilder for Switch {
    fn motion_mut(&mut self) -> &mut MotionSpec {
        &mut self.motion
    }
}

impl Switch {
    /// Creates a switch at `value`, no change handler (disabled), no
    /// overrides.
    #[must_use]
    pub fn new(value: bool) -> Self {
        Self {
            value,
            on_changed: None,
            active_thumb_color: None,
            motion: MotionSpec::default(),
        }
    }

    /// Sets the change handler. Presence of a handler is what makes this
    /// switch interactive — `None` (the default) renders disabled and
    /// swallows taps. On tap, fires with `!value`.
    #[must_use]
    pub fn on_changed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>, bool) -> R + 'static,
    ) -> Self {
        self.on_changed = Some(crate::event_callback::value_callback(callback));
        self
    }

    /// Overrides the thumb color used when this switch is on (and enabled).
    #[must_use]
    pub fn active_thumb_color(mut self, color: Color) -> Self {
        self.active_thumb_color = Some(color);
        self
    }

    /// Whether this switch responds to taps (`on_changed` is set).
    fn is_interactive(&self) -> bool {
        self.on_changed.is_some()
    }
}

/// Persistent state behind [`Switch`] — owns the [`WidgetStatesController`]
/// shared with the [`InkWell`] this view builds. See [`crate::checkbox::CheckboxState`]'s
/// doc comment for why `Selected` lives here while
/// `Hovered`/`Focused`/`Pressed`/`Disabled` are `InkWell`'s to manage.
/// It also owns the animated toggle position (see the module docs).
pub struct SwitchState {
    states: WidgetStatesController,
    states_listener: Option<flui_sdk::foundation::ListenerId>,
    position: AnimatedValue<f64>,
}

impl std::fmt::Debug for SwitchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SwitchState")
            .field("states", &self.states)
            .field("position", &self.position.value())
            .finish_non_exhaustive()
    }
}

/// The toggle position for `value`: `1.0` on, `0.0` off.
fn switch_position(value: bool) -> f64 {
    if value { 1.0 } else { 0.0 }
}

impl StatefulView for Switch {
    type State = SwitchState;

    fn create_state(&self) -> Self::State {
        let initial = if self.value {
            WidgetStates::from(WidgetState::Selected)
        } else {
            WidgetStates::NONE
        };
        let position = AnimatedValue::with_debug_label(
            "Switch",
            switch_position(self.value),
            self.motion.clone(),
            None,
        )
        .expect("BUG: a switch position is 0 or 1, always finite");
        SwitchState {
            states: WidgetStatesController::new(initial),
            states_listener: None,
            position,
        }
    }
}

impl SwitchState {
    /// Follows the ambient frame clock.
    fn bind(&mut self, ctx: &dyn LifecycleContext) {
        if let Err(error) = self.position.rebind(VsyncScope::maybe_of(ctx).as_ref()) {
            tracing::error!(%error, "Switch lost its frame registry");
        }
    }
}

impl ViewState<Switch> for SwitchState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.bind(ctx);
        // The handle is consumed directly by the listener closure; nothing
        // else needs to re-read it later, so it is not stored on `self`.
        let rebuild = ctx.rebuild_handle();
        self.states_listener = Some(self.states.add_listener(std::rc::Rc::new(move || {
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.bind(ctx);
    }

    fn did_update_view(&mut self, old_view: &Switch, new_view: &Switch) {
        if old_view.value != new_view.value {
            self.states.update(WidgetState::Selected, new_view.value);
        }
        let target = switch_position(new_view.value);
        if (old_view.value != new_view.value || old_view.motion != new_view.motion)
            && let Err(error) = self.position.retarget(target, new_view.motion.clone())
        {
            // The value still lands: a refused motion shows it at once.
            tracing::warn!(%error, "Switch motion refused; showing the new value");
            if let Err(error) = self.position.snap_to(target) {
                tracing::error!(%error, "Switch could not show its value");
            }
        }
    }

    fn build(&self, view: &Switch, ctx: &dyn BuildContext) -> impl IntoView {
        let theme = Theme::of(ctx);
        let switch_theme = theme.switch_theme.clone();
        let colors = theme.color_scheme;

        let states = self.states.value();

        // Both ends of the toggle resolve against the live hover/focus/press/
        // disabled states; the painter interpolates between them.
        let resolve_end = |states: WidgetStates| SwitchColors {
            thumb: resolve_switch_thumb_color(
                view.active_thumb_color,
                switch_theme.as_ref().and_then(|t| t.thumb_color.as_ref()),
                &colors,
                states,
            ),
            track: resolve_state_color(
                switch_theme.as_ref().and_then(|t| t.track_color.as_ref()),
                &states,
            )
            .unwrap_or_else(|| switch_default_track_color(&colors, states)),
            track_outline: resolve_state_color(
                switch_theme
                    .as_ref()
                    .and_then(|t| t.track_outline_color.as_ref()),
                &states,
            )
            .unwrap_or_else(|| switch_default_track_outline_color(&colors, states)),
        };
        let off = resolve_end(states.without_state(WidgetState::Selected));
        let on = resolve_end(states.with_state(WidgetState::Selected));

        let theme_overlay = switch_theme.as_ref().and_then(|t| t.overlay_color.clone());
        let overlay_color = WidgetStateProperty::resolve_with(move |live_states: &WidgetStates| {
            resolve_state_color(theme_overlay.as_ref(), live_states)
                .or_else(|| switch_default_overlay_color(&colors, *live_states))
        });

        let painter: std::rc::Rc<dyn CustomPainter> = std::rc::Rc::new(SwitchPainter {
            position: self.position.animation(),
            off,
            on,
            rtl: Directionality::maybe_of(ctx) == Some(TextDirection::Rtl),
        });

        let interactive = view.is_interactive();
        let next_value = !view.value;
        let on_changed = view.on_changed.clone();
        let mut ink_well = InkWell::new(
            CustomPaint::new()
                .size(Size::new(SWITCH_TAP_TARGET_WIDTH, SWITCH_TAP_TARGET_HEIGHT))
                .painter(painter),
        )
        .shape(MaterialShape::Stadium)
        .overlay_color(overlay_color)
        .states_controller(self.states.clone());
        if interactive {
            ink_well = ink_well.on_tap(move |cx| {
                if let Some(handler) = &on_changed {
                    handler(cx, next_value);
                }
            });
        }

        Semantics::new()
            .switch(view.value)
            .enabled(interactive)
            .child(ink_well)
    }

    fn dispose(&mut self) {
        if let Some(id) = self.states_listener.take() {
            self.states.remove_listener(id);
        }
        self.position.dispose();
    }
}

/// Resolves [`Switch`]'s thumb color through the widget -> theme -> default
/// cascade, then alpha-blends the result over `colors.surface` — extracted
/// as its own pure function (not left inline in `build`) specifically so
/// both the tier-precedence order AND the surface-blend fix are
/// unit-testable without mounting a widget tree. The cascade is
/// `switchTheme.thumbColor?.resolve ?? defaults.thumbColor.resolve`,
/// `active_thumb_color` substituting
/// only when [`WidgetState::Selected`] AND NOT [`WidgetState::Disabled`]
/// (same shape `crate::checkbox`'s own `resolve_checkbox_fill_color` uses
/// for its `activeColor` gate), composed with a
/// `Color.alphaBlend(thumbColor, surfaceColor)` — the blend keeps a translucent thumb (e.g. the
/// disabled+unselected `onSurface@38%` default tier) from letting the
/// track paint underneath it show through.
fn resolve_switch_thumb_color(
    active_thumb_color: Option<Color>,
    theme_thumb_color: Option<&WidgetStateProperty<Option<Color>>>,
    colors: &ColorScheme,
    states: WidgetStates,
) -> Color {
    let widget_active_override = (!states.contains_state(WidgetState::Disabled)
        && states.contains_state(WidgetState::Selected))
    .then_some(active_thumb_color)
    .flatten();

    let resolved = widget_active_override
        .or_else(|| resolve_state_color(theme_thumb_color, &states))
        .unwrap_or_else(|| switch_default_thumb_color(colors, states));

    resolved.blend_over(colors.surface)
}

/// `_SwitchDefaultsM3.thumbColor` (`switch.dart`, oracle tag `3.44.0`).
fn switch_default_thumb_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Disabled) {
        return if states.contains_state(WidgetState::Selected) {
            colors.surface
        } else {
            colors.on_surface.with_opacity(0.38)
        };
    }
    if states.contains_state(WidgetState::Selected) {
        if states.contains_state(WidgetState::Pressed)
            || states.contains_state(WidgetState::Hovered)
            || states.contains_state(WidgetState::Focused)
        {
            return colors.primary_container;
        }
        return colors.on_primary;
    }
    if states.contains_state(WidgetState::Pressed)
        || states.contains_state(WidgetState::Hovered)
        || states.contains_state(WidgetState::Focused)
    {
        return colors.on_surface_variant;
    }
    colors.outline
}

/// `_SwitchDefaultsM3.trackColor` (`switch.dart`, oracle tag `3.44.0`).
fn switch_default_track_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Disabled) {
        return if states.contains_state(WidgetState::Selected) {
            colors.on_surface.with_opacity(0.12)
        } else {
            colors.surface_container_highest.with_opacity(0.12)
        };
    }
    if states.contains_state(WidgetState::Selected) {
        return colors.primary;
    }
    colors.surface_container_highest
}

/// `_SwitchDefaultsM3.trackOutlineColor` (`switch.dart`, oracle tag
/// `3.44.0`).
fn switch_default_track_outline_color(colors: &ColorScheme, states: WidgetStates) -> Color {
    if states.contains_state(WidgetState::Selected) {
        return Color::TRANSPARENT;
    }
    if states.contains_state(WidgetState::Disabled) {
        return colors.on_surface.with_opacity(0.12);
    }
    colors.outline
}

/// `_SwitchDefaultsM3.overlayColor` (`switch.dart`, oracle tag `3.44.0`).
/// Returns `None` where the oracle returns `null` — see [`InkWell`]'s own
/// "`None` resolution = no overlay layer at all" contract.
fn switch_default_overlay_color(colors: &ColorScheme, states: WidgetStates) -> Option<Color> {
    if states.contains_state(WidgetState::Selected) {
        if states.contains_state(WidgetState::Pressed) {
            return Some(colors.primary.with_opacity(0.1));
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
        return Some(colors.on_surface.with_opacity(0.1));
    }
    if states.contains_state(WidgetState::Hovered) {
        return Some(colors.on_surface.with_opacity(0.08));
    }
    if states.contains_state(WidgetState::Focused) {
        return Some(colors.on_surface.with_opacity(0.1));
    }
    None
}

/// The colours of one end of the toggle, resolved for the live states with
/// `Selected` forced off or on.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SwitchColors {
    thumb: Color,
    track: Color,
    track_outline: Color,
}

/// Paints the switch's track (fill + border) and thumb at the animated
/// toggle position, sampled at paint time, with no radial-reaction paint
/// (the overlay comes from [`InkWell`] instead — see the module docs'
/// "Overlay shape" section).
#[derive(Debug, Clone)]
struct SwitchPainter {
    /// `0.0` off, `1.0` on; repaints this painter as it moves.
    position: AnimatedValueView<f64>,
    off: SwitchColors,
    on: SwitchColors,
    /// The thumb travels from the right end (off) to the left end (on).
    rtl: bool,
}

impl CustomPainter for SwitchPainter {
    fn paint(&self, canvas: &mut Canvas, size: Size) {
        // A spring may overshoot; the thumb stays on its track.
        let t = self.position.value().clamp(0.0, 1.0);
        let track_color = Color::lerp(self.off.track, self.on.track, t);
        let track_outline_color = Color::lerp(self.off.track_outline, self.on.track_outline, t);
        let thumb_color = Color::lerp(self.off.thumb, self.on.thumb, t);
        let thumb_radius = INACTIVE_THUMB_RADIUS + (ACTIVE_THUMB_RADIUS - INACTIVE_THUMB_RADIUS) * t;

        let track_origin_x = (size.width - SWITCH_TRACK_WIDTH) / 2.0;
        let track_origin_y = (size.height - SWITCH_TRACK_HEIGHT) / 2.0;

        let track_rect = Rect::from_ltrb(
            track_origin_x,
            track_origin_y,
            track_origin_x + SWITCH_TRACK_WIDTH,
            track_origin_y + SWITCH_TRACK_HEIGHT,
        );
        let track_rrect = RRect::from_rect_circular(track_rect, SWITCH_TRACK_HEIGHT / 2.0);

        canvas.draw_rrect(track_rrect, &Paint::fill(track_color));

        if TRACK_OUTLINE_WIDTH > 0.0 {
            let inner_rrect = track_rrect.inflate(-TRACK_OUTLINE_WIDTH);
            canvas.draw_drrect(track_rrect, inner_rrect, &Paint::fill(track_outline_color));
        }

        // `trackInnerStart = trackHeight / 2.0`,
        // `trackInnerEnd = trackWidth - trackInnerStart` — the thumb's
        // center travels between these two track-local x-coordinates, from
        // the start end (off) to the far end (on); right-to-left mirrors it.
        let track_inner_start = SWITCH_TRACK_HEIGHT / 2.0;
        let track_inner_end = SWITCH_TRACK_WIDTH - track_inner_start;
        let visual = if self.rtl { 1.0 - t } else { t };
        let thumb_center_x =
            track_origin_x + track_inner_start + (track_inner_end - track_inner_start) * visual;
        let thumb_center_y = track_origin_y + SWITCH_TRACK_HEIGHT / 2.0;

        canvas.draw_circle(
            Point::new(thumb_center_x, thumb_center_y),
            thumb_radius,
            &Paint::fill(thumb_color),
        );
    }

    fn should_repaint(&self, old_delegate: &dyn CustomPainter) -> bool {
        // The position repaints through `repaint`; only the endpoints and
        // direction come from a rebuild.
        old_delegate
            .as_any()
            .downcast_ref::<Self>()
            .is_none_or(|old| old.off != self.off || old.on != self.on || old.rtl != self.rtl)
    }

    fn repaint(&self) -> Option<Rc<dyn Listenable>> {
        Some(Rc::new(self.position.clone()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
