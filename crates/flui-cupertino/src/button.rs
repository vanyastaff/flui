//! [`CupertinoButton`] — the iOS (17) Human Interface Guidelines button.
//!
//! Flutter parity: `cupertino/button.dart` + `cupertino/constants.dart`
//! (oracle tag `3.44.0`). Every geometry constant (padding, border radius,
//! minimum size, tinted opacity, fade timing) is a verbatim port of
//! `constants.dart`'s per-[`CupertinoButtonSize`] tables — pinned by an
//! oracle-diffed const-table test in `tests/button.rs`.
//!
//! ## Honest reduction: one recognized tap, not a real down/move/up sequence
//!
//! The oracle drives its press-opacity animation off `TapGestureRecognizer`'s
//! `onTapDown`/`onTapMove`/`onTapUp`/`onTapCancel` — four independent
//! callbacks, so the fade can start the instant the finger goes down and
//! reverse mid-gesture if the finger drags outside `kCupertinoButtonTapMoveSlop`
//! before lifting. FLUI's [`flui_widgets::GestureDetector`] exposes only
//! `on_tap` (fires once a tap is *recognized* — down + up without exceeding
//! touch slop) and `on_long_press`, with no down/move primitives to hang
//! separate handlers on — the same gap `flui_material::ink_well` documents
//! for its own press-state timing.
//!
//! This port applies the oracle's fade *sequence* uniformly to that single
//! `on_tap` event: on tap, animate toward `CupertinoButton::pressed_opacity` over
//! [`K_FADE_OUT_DURATION`] (`Curves::EaseInOutCubicEmphasized`, matching the
//! oracle's press-down curve), call the handler, then — once that fade
//! completes — animate back to full opacity over [`K_FADE_IN_DURATION`]
//! (`Curves::EaseOutCubic`, the oracle's release curve). The visible result
//! is a brief "flash" per tap instead of a fade that tracks how long the
//! finger is actually held down. `tapMoveSlop`-driven cancel-while-dragging
//! has no equivalent here (named, not silently dropped): there is no drag
//! signal to cancel against.
//!
//! ## Deferred (named, not silently dropped)
//!
//! - **Focus ring** (`RoundedSuperellipseBorder` outline on
//!   `enabled && isFocused`) — `flui-painting` has the superellipse
//!   primitive, but wiring a focus-visible border is out of this crate's V1
//!   scope.
//! - **`WidgetState`-resolved mouse cursor** — the oracle's `_defaultCursor`
//!   is `kIsWeb`-gated (`SystemMouseCursors.click` only on web) and FLUI has
//!   no `MouseCursor` type yet to resolve against. Not "Cupertino doesn't use
//!   `WidgetState`" (the oracle's cursor *and* focus paths do) — just no
//!   consumer for it in this crate yet.
//! - **`onFocusChange`/`autofocus`** — no `FocusableActionDetector`-equivalent
//!   wiring in this pass; [`crate::button`] wraps `GestureDetector` directly.
//! - **Icon theming** — the oracle wraps `child` in an `IconTheme` sized off
//!   the resolved text style; not wired here (no icon-bearing V1 consumer).

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::ext::AnimatableExt;
use flui_animation::ext::AnimationExt;
use flui_animation::{
    Animation, AnimationController, AnimationStatus, Curves, FloatTween, Scheduler, Vsync,
    VsyncRegistration,
};
use flui_types::geometry::{EdgeInsets, Pixels, px};
use flui_types::layout::Alignment;
use flui_types::platform::Brightness;
use flui_types::styling::{BorderRadius, BorderRadiusExt, BoxDecoration, Color};
use flui_types::typography::TextStyle;
use flui_view::RebuildHandle;
use flui_view::prelude::*;
use flui_view::{BoxedView, StatefulView, ViewState};
use flui_widgets::animated::VsyncScope;
use flui_widgets::prelude::BoxConstraints;
use flui_widgets::{
    Align, ConstrainedBox, DecoratedBox, DefaultTextStyle, FadeTransition, GestureDetector,
    Padding, Semantics,
};

use crate::colors::{CupertinoColor, CupertinoColors};
use crate::theme::CupertinoTheme;

/// A user tap/long-press handler. `Rc`-based (owner-local, per ADR-0027) —
/// matches `GestureDetector::on_tap`'s own callback shape.
type ButtonCallback = Rc<dyn Fn()>;

/// `kFadeOutDuration` (`button.dart`, oracle tag `3.44.0`) — the press-in
/// fade's duration.
pub const K_FADE_OUT_DURATION: Duration = Duration::from_millis(120);

/// `kFadeInDuration` (`button.dart`, oracle tag `3.44.0`) — the release fade's
/// duration.
pub const K_FADE_IN_DURATION: Duration = Duration::from_millis(180);

/// `kCupertinoButtonTintedOpacityLight` (`constants.dart`, oracle tag
/// `3.44.0`).
const K_TINTED_OPACITY_LIGHT: f32 = 0.12;
/// `kCupertinoButtonTintedOpacityDark` (`constants.dart`, oracle tag
/// `3.44.0`).
const K_TINTED_OPACITY_DARK: f32 = 0.26;

/// The size of a [`CupertinoButton`]. Flutter parity: `CupertinoButtonSize`
/// (`button.dart`, oracle tag `3.44.0`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CupertinoButtonSize {
    /// A smaller button with round sides and [`crate::text_theme::CupertinoTextThemeData::action_small_text_style`].
    Small,
    /// A medium-sized button with round sides and regular-sized text.
    Medium,
    /// A classic large button with rounded edges and regular-sized text.
    #[default]
    Large,
}

/// `kCupertinoButtonPadding[sizeStyle]` (`constants.dart`, oracle tag
/// `3.44.0`).
fn size_padding(size: CupertinoButtonSize) -> EdgeInsets {
    match size {
        CupertinoButtonSize::Small => EdgeInsets::symmetric(px(6.0), px(12.0)),
        CupertinoButtonSize::Medium => EdgeInsets::symmetric(px(10.0), px(15.0)),
        CupertinoButtonSize::Large => EdgeInsets::symmetric(px(16.0), px(20.0)),
    }
}

/// `kCupertinoButtonSizeBorderRadius[sizeStyle]` (`constants.dart`, oracle
/// tag `3.44.0`).
fn size_border_radius(size: CupertinoButtonSize) -> BorderRadius {
    match size {
        CupertinoButtonSize::Small | CupertinoButtonSize::Medium => {
            BorderRadius::circular(px(40.0))
        }
        CupertinoButtonSize::Large => BorderRadius::circular(px(12.0)),
    }
}

/// `kCupertinoButtonMinSize[sizeStyle]` (`constants.dart`, oracle tag
/// `3.44.0`) — the fallback `build()` uses only when [`CupertinoButton::minimum_size`]
/// is unset. The oracle's constraint expression is
/// `minimumSize?.width ?? kCupertinoButtonMinSize[sizeStyle] ?? kMinInteractiveDimensionCupertino`:
/// the final `kMinInteractiveDimensionCupertino` (44.0) fallback is
/// unreachable in practice (`kCupertinoButtonMinSize` covers every
/// [`CupertinoButtonSize`] variant), so this port omits that dead constant
/// rather than carry unused code — `minimum_size` passes an explicit value
/// (including `0.0`, which genuinely removes the floor) straight through
/// unmodified, exactly as `minimumSize?.width` does.
fn size_min_dimension(size: CupertinoButtonSize) -> f32 {
    match size {
        CupertinoButtonSize::Small => 28.0,
        CupertinoButtonSize::Medium => 32.0,
        CupertinoButtonSize::Large => 44.0,
    }
}

/// The background-fill style. Flutter parity: `_CupertinoButtonStyle`
/// (`button.dart`, oracle tag `3.44.0`) — private in the oracle, exposed here
/// only through the three constructors, matching that scoping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonFillStyle {
    /// No background, primary-color foreground. [`CupertinoButton::new`].
    Plain,
    /// Translucent primary-color background. [`CupertinoButton::tinted`].
    Tinted,
    /// Solid primary-color background, contrasting foreground.
    /// [`CupertinoButton::filled`].
    Filled,
}

/// An iOS-style button — see the module doc for the honest reductions this
/// port makes against the oracle's real down/move/up press tracking.
#[derive(Clone, StatefulView)]
pub struct CupertinoButton {
    child: BoxedView,
    style: ButtonFillStyle,
    size_style: CupertinoButtonSize,
    padding: Option<EdgeInsets>,
    color: Option<CupertinoColor>,
    foreground_color: Option<CupertinoColor>,
    disabled_color: CupertinoColor,
    minimum_size: Option<(f32, f32)>,
    pressed_opacity: Option<f32>,
    border_radius: Option<BorderRadius>,
    alignment: Alignment,
    on_pressed: Option<ButtonCallback>,
    on_long_press: Option<ButtonCallback>,
}

impl CupertinoButton {
    /// Builds a `CupertinoButton` with `fill_style`'s default `disabled_color`
    /// — shared by all three public constructors.
    fn with_style(child: impl IntoView, style: ButtonFillStyle) -> Self {
        let disabled_color = match style {
            ButtonFillStyle::Plain => CupertinoColors::QUATERNARY_SYSTEM_FILL,
            ButtonFillStyle::Tinted | ButtonFillStyle::Filled => {
                CupertinoColors::TERTIARY_SYSTEM_FILL
            }
        };
        Self {
            child: child.into_view().boxed(),
            style,
            size_style: CupertinoButtonSize::default(),
            padding: None,
            color: None,
            foreground_color: None,
            disabled_color: CupertinoColor::Dynamic(disabled_color),
            minimum_size: None,
            pressed_opacity: Some(0.4),
            border_radius: None,
            alignment: Alignment::CENTER,
            on_pressed: None,
            on_long_press: None,
        }
    }

    /// A plain button: no background, primary-color text. Flutter parity:
    /// `CupertinoButton(...)`.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self::with_style(child, ButtonFillStyle::Plain)
    }

    /// A button with a translucent background derived from
    /// [`crate::theme::CupertinoThemeData::primary_color`]. Flutter parity:
    /// `CupertinoButton.tinted`.
    #[must_use]
    pub fn tinted(child: impl IntoView) -> Self {
        Self::with_style(child, ButtonFillStyle::Tinted)
    }

    /// A button with a solid, opaque background. Flutter parity:
    /// `CupertinoButton.filled`.
    #[must_use]
    pub fn filled(child: impl IntoView) -> Self {
        Self::with_style(child, ButtonFillStyle::Filled)
    }

    /// Sets [`CupertinoButtonSize`] (default [`CupertinoButtonSize::Large`]).
    #[must_use]
    pub fn size_style(mut self, size_style: CupertinoButtonSize) -> Self {
        self.size_style = size_style;
        self
    }

    /// Overrides the padding inside the button's bounds. Defaults to
    /// `size_padding` for [`Self::size_style`].
    #[must_use]
    pub fn padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Sets the background color. `None` (the default for
    /// [`CupertinoButton::new`]) paints no background at all for the plain
    /// style; [`CupertinoButton::tinted`]/[`CupertinoButton::filled`] fall
    /// back to the theme's primary color.
    #[must_use]
    pub fn color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Sets the text/icon color. Defaults to the theme's primary color
    /// (contrasting color for [`CupertinoButton::filled`]) when enabled, and
    /// [`CupertinoColors::TERTIARY_LABEL`] when disabled.
    #[must_use]
    pub fn foreground_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.foreground_color = Some(color.into());
        self
    }

    /// Overrides the background color used while disabled. Ignored unless a
    /// background color is otherwise painted.
    #[must_use]
    pub fn disabled_color(mut self, color: impl Into<CupertinoColor>) -> Self {
        self.disabled_color = color.into();
        self
    }

    /// Overrides the minimum `(width, height)` of the button. Defaults to
    /// `size_min_dimension` on both axes for [`Self::size_style`].
    #[must_use]
    pub fn minimum_size(mut self, width: f32, height: f32) -> Self {
        self.minimum_size = Some((width, height));
        self
    }

    /// Sets the opacity the button fades to while pressed (default `0.4`).
    /// `None` disables the fade animation entirely.
    #[must_use]
    pub fn pressed_opacity(mut self, pressed_opacity: Option<f32>) -> Self {
        self.pressed_opacity = pressed_opacity;
        self
    }

    /// Overrides the corner radius. Defaults to `size_border_radius` for
    /// [`Self::size_style`].
    #[must_use]
    pub fn border_radius(mut self, border_radius: BorderRadius) -> Self {
        self.border_radius = Some(border_radius);
        self
    }

    /// Sets how the button's child is aligned within it (default
    /// [`Alignment::CENTER`]).
    #[must_use]
    pub fn alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Sets the tap handler. Presence of a tap or long-press handler is what
    /// makes this button [`Self::enabled`].
    #[must_use]
    pub fn on_pressed(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_pressed = Some(Rc::new(callback));
        self
    }

    /// Sets the long-press handler — wired straight to
    /// [`flui_widgets::GestureDetector::on_long_press`], with no fade
    /// animation tied to it (matching the oracle: `LongPressGestureRecognizer`
    /// is a wholly separate recognizer from the tap-driven fade).
    #[must_use]
    pub fn on_long_press(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_long_press = Some(Rc::new(callback));
        self
    }

    /// Whether the button responds to interaction. Flutter parity:
    /// `CupertinoButton.enabled`.
    #[must_use]
    pub fn enabled(&self) -> bool {
        self.on_pressed.is_some() || self.on_long_press.is_some()
    }
}

impl std::fmt::Debug for CupertinoButton {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoButton")
            .field("style", &self.style)
            .field("size_style", &self.size_style)
            .field("enabled", &self.enabled())
            .finish_non_exhaustive()
    }
}

/// Resolves the background fill, per `_CupertinoButtonState.build`'s
/// `backgroundColor` computation (`button.dart`, oracle tag `3.44.0`):
///
/// ```dart
/// final Color? backgroundColor =
///     (widget.color == null ? ... : CupertinoDynamicColor.maybeResolve(widget.color, context))
///         ?.withOpacity(widget._style == tinted ? ... : widget.color?.opacity ?? 1.0);
/// ```
///
/// ## Why the `Plain`/`Filled` alpha comes from `view.color`, not the resolved color
///
/// For `Tinted`, the multiplier is a fixed light/dark constant — no
/// surprises. For `Plain`/`Filled`, the oracle multiplies by `widget.color?.opacity`,
/// which reads the **original, never-resolved** `widget.color` — not the
/// `CupertinoDynamicColor.maybeResolve` result assigned to `backgroundColor`
/// two lines above. `CupertinoDynamicColor.opacity` forwards to its private
/// `_effectiveColor`, which every public constructor seeds with `color` (the
/// light/normal variant) and which is only ever replaced by calling
/// `resolveFrom` — a call that returns a **new** instance, leaving the
/// original `widget.color` untouched. So this multiplier is always the
/// color's light-variant alpha, even when `backgroundColor` itself resolved
/// to the dark variant.
///
/// This looks like a bug (a dark-mode background painted with the light
/// alpha) but is the oracle's real, verified behavior: `CupertinoColors.separator`
/// is alpha 73 light / 153 dark (`colors.dart`, oracle tag `3.44.0`), and a
/// `.separator`-colored `Plain`/`Filled` button under a Dark theme really
/// does render at alpha 73 in real Flutter — see
/// `background_dynamic_color_keeps_the_light_variants_alpha_under_a_dark_theme`
/// in `tests/button.rs`, which mounts a `.separator`-colored button under a
/// Dark theme and pins this exact (surprising but oracle-faithful) value.
/// Ported here as a direct alpha-channel copy (`Color::with_alpha`, `u8`)
/// rather than the oracle's `opacity` (`f64`) round trip through
/// `with_opacity` — same source byte, no float-precision risk.
fn resolve_background_color(
    view: &CupertinoButton,
    ctx: &dyn BuildContext,
    primary_color: Color,
) -> Option<Color> {
    let base = match view.color {
        Some(color) => Some(color.resolve(ctx)),
        None => match view.style {
            ButtonFillStyle::Plain => None,
            ButtonFillStyle::Tinted | ButtonFillStyle::Filled => Some(primary_color),
        },
    };

    base.map(|base_color| match view.style {
        ButtonFillStyle::Tinted => {
            let opacity = if CupertinoTheme::brightness_of(ctx) == Brightness::Light {
                K_TINTED_OPACITY_LIGHT
            } else {
                K_TINTED_OPACITY_DARK
            };
            base_color.with_opacity(opacity)
        }
        ButtonFillStyle::Plain | ButtonFillStyle::Filled => {
            let alpha = match view.color {
                Some(CupertinoColor::Static(color)) => color.a,
                Some(CupertinoColor::Dynamic(dynamic)) => dynamic.color.a,
                None => 255,
            };
            base_color.with_alpha(alpha)
        }
    })
}

/// Resolves the foreground (text/icon) color, per
/// `_CupertinoButtonState.build`'s `effectiveForegroundColor` computation.
fn resolve_foreground_color(
    view: &CupertinoButton,
    ctx: &dyn BuildContext,
    theme: &crate::theme::CupertinoThemeData,
    primary_color: Color,
    enabled: bool,
) -> Color {
    if let Some(color) = view.foreground_color {
        return color.resolve(ctx);
    }
    match (view.style, enabled) {
        (ButtonFillStyle::Filled, _) => theme.primary_contrasting_color().resolve(ctx),
        (_, true) => primary_color,
        (_, false) => CupertinoColors::TERTIARY_LABEL.resolve_from(ctx),
    }
}

/// Starts the press-in fade on tap — `true` if it actually started a run.
/// Extracted from the `on_tap` closure so "does a tap with `pressed_opacity:
/// None` actually start the controller" is unit-testable without mounting a
/// render tree (see the tests below).
///
/// `pressed_opacity: None` means [`CupertinoButton::pressed_opacity`]'s
/// contract — "disables the fade animation entirely" — a stronger,
/// FLUI-authored promise than the oracle's own doc ("opacity will not change
/// on pressed"); the oracle's `_animate()` has no such guard and always
/// drives the `AnimationController` on tap, even when `pressedOpacity` is
/// `null` (the tween's begin/end both collapse to `1.0`, so the run ticks
/// invisibly). Skipping the run here has no observable paint difference —
/// `build`'s `opacity` `FloatTween` also collapses to `1.0..=1.0` in that
/// case — it only removes wasted ticking, rebuild-scheduling, and
/// status-listener work, so this does not diverge from the oracle's visible
/// behavior, only from its incidental cost.
fn start_press_fade(
    controller: &AnimationController,
    pressed_opacity: Option<f32>,
    rebuild: Option<&RebuildHandle>,
) -> bool {
    if pressed_opacity.is_none() {
        return false;
    }
    let curve: Arc<dyn flui_animation::Curve + Send + Sync> =
        Arc::new(Curves::EaseInOutCubicEmphasized);
    if let Err(error) = controller.animate_to_curved(1.0, Some(K_FADE_OUT_DURATION), curve) {
        tracing::debug!(?error, "CupertinoButton press fade failed to start");
    }
    if let Some(rebuild) = rebuild {
        rebuild.schedule(flui_view::RebuildReason::StateChange);
    }
    true
}

/// Persistent state behind [`CupertinoButton`] — see [`StatefulView`]/
/// [`ViewState`].
pub struct CupertinoButtonState {
    /// `None` when there is no ambient [`VsyncScope`] — see the module doc's
    /// press-fade section; the button still responds to taps, it just has no
    /// clock to animate the fade against.
    controller: Option<AnimationController>,
    vsync: Option<Vsync>,
    registration: Option<VsyncRegistration>,
    rebuild: Option<RebuildHandle>,
}

impl std::fmt::Debug for CupertinoButtonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoButtonState")
            .field("has_controller", &self.controller.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for CupertinoButton {
    type State = CupertinoButtonState;

    fn create_state(&self) -> Self::State {
        CupertinoButtonState {
            controller: None,
            vsync: None,
            registration: None,
            rebuild: None,
        }
    }
}

impl ViewState<CupertinoButton> for CupertinoButtonState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // ADR-0018: `rebuild_handle()` acquired here, fired later from the
        // status listener below — never called from `build`.
        self.rebuild = Some(ctx.rebuild_handle());

        let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) else {
            // No ambient VsyncScope: no clock to animate the fade against.
            // Tapping still fires the handler with no visible fade — the same
            // degrade `flui_material::ink_well` documents for its own
            // `VsyncScope`-less case.
            return;
        };

        let controller =
            AnimationController::new(Duration::from_millis(200), Arc::new(Scheduler::new()));
        let registration = vsync.register(controller.clone());

        // Oracle: `_animate()`'s `ticker.then(...)` re-invokes `_animate` if
        // `_buttonHeldDown` changed mid-run. This port's single `on_tap`
        // event has only one transition to chain: once the press-in fade
        // reaches its target (`AnimationStatus::Completed` == reached the
        // upper bound), start the release fade back down automatically.
        let release_controller = controller.clone();
        controller.add_status_listener(Arc::new(move |status| {
            if status == AnimationStatus::Completed {
                let curve: Arc<dyn flui_animation::Curve + Send + Sync> =
                    Arc::new(Curves::EaseOutCubic);
                if let Err(error) =
                    release_controller.animate_to_curved(0.0, Some(K_FADE_IN_DURATION), curve)
                {
                    tracing::debug!(?error, "CupertinoButton release fade failed to start");
                }
            }
        }));

        self.controller = Some(controller);
        self.vsync = Some(vsync);
        self.registration = Some(registration);
    }

    fn build(&self, view: &CupertinoButton, ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = view.enabled();
        let theme = CupertinoTheme::of(ctx);
        let primary_color = theme.primary_color().resolve(ctx);

        let background_color = resolve_background_color(view, ctx, primary_color);
        let foreground_color = resolve_foreground_color(view, ctx, &theme, primary_color, enabled);

        let text_style = if view.size_style == CupertinoButtonSize::Small {
            theme.text_theme().action_small_text_style()
        } else {
            theme.text_theme().action_text_style()
        };
        let text_style = TextStyle {
            color: Some(foreground_color),
            ..text_style
        };

        let fill_color = match background_color {
            Some(_) if !enabled => Some(view.disabled_color.resolve(ctx)),
            other => other,
        };
        let decoration = BoxDecoration::<Pixels> {
            color: fill_color,
            border_radius: Some(
                view.border_radius
                    .unwrap_or_else(|| size_border_radius(view.size_style)),
            ),
            ..BoxDecoration::new()
        };

        // An explicit `minimum_size` (including `(0.0, 0.0)`, which
        // genuinely removes the floor) passes straight through unmodified —
        // see `size_min_dimension`'s doc for why no other fallback runs on
        // top of a caller-supplied value.
        let (min_width, min_height) = view.minimum_size.unwrap_or_else(|| {
            (
                size_min_dimension(view.size_style),
                size_min_dimension(view.size_style),
            )
        });
        let constraints =
            BoxConstraints::new(px(min_width), Pixels::MAX, px(min_height), Pixels::MAX);

        let padding = view
            .padding
            .unwrap_or_else(|| size_padding(view.size_style));

        let content = Align::new(view.alignment)
            .width_factor(1.0)
            .height_factor(1.0)
            .child(DefaultTextStyle::new(text_style, view.child.clone()));

        let decorated = DecoratedBox::new(decoration).child(Padding::new(padding).child(content));

        let opacity: Arc<dyn Animation<f32>> = match &self.controller {
            Some(controller) => {
                let pressed_opacity = view.pressed_opacity.unwrap_or(1.0);
                let curved = Arc::new(Arc::new(controller.clone()).curved(Curves::Decelerate));
                Arc::new(
                    FloatTween::new(1.0, pressed_opacity)
                        .animate(curved as Arc<dyn Animation<f32>>),
                )
            }
            None => Arc::new(flui_animation::ConstantAnimation::new(1.0)),
        };

        // Flutter parity: `Semantics(button: true, child: ConstrainedBox(...))`
        // — applied unconditionally, not gated on `enabled` (a disabled
        // button is still announced as a button, just an inert one).
        let faded = Semantics::new()
            .button(true)
            .child(ConstrainedBox::new(constraints).child(FadeTransition::new(opacity, decorated)));

        let mut gesture_detector = GestureDetector::new();
        if enabled {
            if let Some(on_pressed) = view.on_pressed.clone() {
                let controller = self.controller.clone();
                let rebuild = self.rebuild.clone();
                let pressed_opacity = view.pressed_opacity;
                gesture_detector = gesture_detector.on_tap(move || {
                    if let Some(controller) = &controller {
                        start_press_fade(controller, pressed_opacity, rebuild.as_ref());
                    }
                    on_pressed();
                });
            }
            if let Some(on_long_press) = view.on_long_press.clone() {
                gesture_detector = gesture_detector.on_long_press(move || on_long_press());
            }
        }

        gesture_detector.child(faded)
    }

    fn dispose(&mut self) {
        if let (Some(vsync), Some(registration)) = (self.vsync.take(), self.registration.take()) {
            vsync.unregister(registration);
        }
        if let Some(controller) = self.controller.take() {
            controller.dispose();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- const-table geometry (oracle-diffed) ---------------------------

    #[test]
    fn size_padding_matches_the_oracle_table() {
        assert_eq!(
            size_padding(CupertinoButtonSize::Small),
            EdgeInsets::symmetric(px(6.0), px(12.0))
        );
        assert_eq!(
            size_padding(CupertinoButtonSize::Medium),
            EdgeInsets::symmetric(px(10.0), px(15.0))
        );
        assert_eq!(
            size_padding(CupertinoButtonSize::Large),
            EdgeInsets::symmetric(px(16.0), px(20.0))
        );
    }

    #[test]
    fn size_border_radius_matches_the_oracle_table() {
        assert_eq!(
            size_border_radius(CupertinoButtonSize::Small),
            BorderRadius::circular(px(40.0))
        );
        assert_eq!(
            size_border_radius(CupertinoButtonSize::Medium),
            BorderRadius::circular(px(40.0))
        );
        assert_eq!(
            size_border_radius(CupertinoButtonSize::Large),
            BorderRadius::circular(px(12.0))
        );
    }

    #[test]
    fn size_min_dimension_matches_the_oracle_table() {
        assert_eq!(size_min_dimension(CupertinoButtonSize::Small), 28.0);
        assert_eq!(size_min_dimension(CupertinoButtonSize::Medium), 32.0);
        assert_eq!(size_min_dimension(CupertinoButtonSize::Large), 44.0);
    }

    #[test]
    fn fade_durations_and_tinted_opacities_match_the_oracle() {
        assert_eq!(K_FADE_OUT_DURATION, Duration::from_millis(120));
        assert_eq!(K_FADE_IN_DURATION, Duration::from_millis(180));
        assert_eq!(K_TINTED_OPACITY_LIGHT, 0.12);
        assert_eq!(K_TINTED_OPACITY_DARK, 0.26);
    }

    // ---- construction / defaults -----------------------------------------

    #[test]
    fn plain_button_defaults_to_quaternary_system_fill_disabled_color() {
        let button = CupertinoButton::new(flui_widgets::SizedBox::shrink());
        assert_eq!(
            button.disabled_color,
            CupertinoColor::Dynamic(CupertinoColors::QUATERNARY_SYSTEM_FILL)
        );
    }

    #[test]
    fn tinted_and_filled_default_to_tertiary_system_fill_disabled_color() {
        let tinted = CupertinoButton::tinted(flui_widgets::SizedBox::shrink());
        let filled = CupertinoButton::filled(flui_widgets::SizedBox::shrink());
        assert_eq!(
            tinted.disabled_color,
            CupertinoColor::Dynamic(CupertinoColors::TERTIARY_SYSTEM_FILL)
        );
        assert_eq!(
            filled.disabled_color,
            CupertinoColor::Dynamic(CupertinoColors::TERTIARY_SYSTEM_FILL)
        );
    }

    #[test]
    fn button_with_no_handlers_is_disabled() {
        assert!(!CupertinoButton::new(flui_widgets::SizedBox::shrink()).enabled());
    }

    #[test]
    fn on_pressed_makes_the_button_enabled() {
        assert!(
            CupertinoButton::new(flui_widgets::SizedBox::shrink())
                .on_pressed(|| {})
                .enabled()
        );
    }

    #[test]
    fn on_long_press_alone_also_makes_the_button_enabled() {
        assert!(
            CupertinoButton::new(flui_widgets::SizedBox::shrink())
                .on_long_press(|| {})
                .enabled()
        );
    }

    #[test]
    fn default_pressed_opacity_is_0_4() {
        let button = CupertinoButton::new(flui_widgets::SizedBox::shrink());
        assert_eq!(button.pressed_opacity, Some(0.4));
    }

    #[test]
    fn debug_reports_style_size_and_enabled_without_the_closures() {
        let debug = format!(
            "{:?}",
            CupertinoButton::filled(flui_widgets::SizedBox::shrink()).on_pressed(|| {})
        );
        assert!(debug.contains("Filled"));
        assert!(debug.contains("enabled: true"));
    }

    // ---- start_press_fade (pressed_opacity(None) truly starts no run) ----

    fn fresh_controller() -> AnimationController {
        AnimationController::new(Duration::from_millis(200), Arc::new(Scheduler::new()))
    }

    /// Red-check: delete the `pressed_opacity.is_none()` guard in
    /// `start_press_fade` (call `animate_to_curved` unconditionally, as the
    /// oracle's own `_animate()` does) — this assertion fails because the
    /// controller starts animating.
    #[test]
    fn pressed_opacity_none_starts_no_controller_run() {
        let controller = fresh_controller();
        let started = start_press_fade(&controller, None, None);
        assert!(
            !started,
            "pressed_opacity(None) must not start the press-fade run"
        );
        assert!(
            !controller.is_animating(),
            "the controller must never begin animating when pressed_opacity is None"
        );
    }

    #[test]
    fn pressed_opacity_some_starts_the_controller_run() {
        let controller = fresh_controller();
        let started = start_press_fade(&controller, Some(0.4), None);
        assert!(
            started,
            "pressed_opacity(Some(_)) must start the press-fade run"
        );
        assert!(
            controller.is_animating(),
            "animate_to_curved should leave the controller animating immediately after starting"
        );
    }
}
