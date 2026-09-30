//! [`CupertinoButton`] — the iOS (17) Human Interface Guidelines button.
//!
//! Every geometry constant (padding, border radius, minimum size, tinted
//! opacity, fade timing) follows the iOS HIG per-[`CupertinoButtonSize`]
//! tables — pinned by a const-table test in `tests/button.rs`.
//!
//! ## Honest reduction: one recognized tap, not a real down/move/up sequence
//!
//! A full implementation would drive the press-opacity animation off separate
//! tap-down/move/up/cancel callbacks, so the fade can start the instant the
//! finger goes down and reverse mid-gesture if the finger drags outside the
//! tap-move slop before lifting. FLUI's [`flui_sdk::widgets::GestureDetector`] exposes only
//! `on_tap` (fires once a tap is *recognized* — down + up without exceeding
//! touch slop) and `on_long_press`, with no down/move primitives to hang
//! separate handlers on — the same gap `flui-material`'s `ink_well` documents
//! for its own press-state timing.
//!
//! So the fade *sequence* is applied uniformly to that single
//! `on_tap` event: on tap, animate toward `CupertinoButton::pressed_opacity` over
//! [`K_FADE_OUT_DURATION`] (`Curves::EaseInOutCubicEmphasized`, the press-down
//! curve), call the handler, then — once that fade
//! completes — animate back to full opacity over [`K_FADE_IN_DURATION`]
//! (`Curves::EaseOutCubic`, the release curve). The visible result
//! is a brief "flash" per tap instead of a fade that tracks how long the
//! finger is actually held down. Tap-move-slop-driven cancel-while-dragging
//! has no equivalent here (named, not silently dropped): there is no drag
//! signal to cancel against.
//!
//! ## Deferred (named, not silently dropped)
//!
//! - **Focus ring** (`RoundedSuperellipseBorder` outline on
//!   `enabled && isFocused`) — `flui-painting` has the superellipse
//!   primitive, but wiring a focus-visible border is out of this crate's V1
//!   scope.
//! - **`WidgetState`-resolved mouse cursor** — a pointer cursor is only
//!   appropriate on web, and FLUI has no `MouseCursor` type yet to resolve
//!   against. Not "Cupertino doesn't use `WidgetState`" — just no consumer
//!   for it in this crate yet.
//! - **`onFocusChange`/`autofocus`** — no `FocusableActionDetector`-equivalent
//!   wiring in this pass; [`crate::button`] wraps `GestureDetector` directly.
//! - **Icon theming** — wrapping `child` in an `IconTheme` sized off
//!   the resolved text style is not wired here (no icon-bearing V1 consumer).

use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_sdk::animation::ext::AnimatableExt;
use flui_sdk::animation::ext::AnimationExt;
use flui_sdk::animation::{
    Animation, AnimationController, Curves, FloatTween, TickerFuture, UpdateScheduler, Vsync,
    VsyncRegistration,
};
use flui_sdk::geometry::EdgeInsets;
use flui_sdk::painting::Alignment;
use flui_sdk::painting::TextStyle;
use flui_sdk::painting::{BorderRadius, BorderRadiusExt, BoxDecoration, Color};
use flui_sdk::platform::Brightness;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, StatefulView, ViewState};
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::prelude::BoxConstraints;
use flui_sdk::widgets::{
    Align, ConstrainedBox, DecoratedBox, DefaultTextStyle, FadeTransition, GestureDetector,
    Padding, Semantics,
};

use crate::colors::{CupertinoColor, CupertinoColors};
use crate::theme::CupertinoTheme;

/// A user tap/long-press handler. `Rc`-based (owner-local, per ADR-0027) —
/// matches `GestureDetector::on_tap`'s own callback shape.
type ButtonCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>)>;

/// The press-in fade's duration.
pub const K_FADE_OUT_DURATION: Duration = Duration::from_millis(120);

/// The release fade's duration.
pub const K_FADE_IN_DURATION: Duration = Duration::from_millis(180);

/// The tinted background's opacity under a light theme.
const K_TINTED_OPACITY_LIGHT: f64 = 0.12;
/// The tinted background's opacity under a dark theme.
const K_TINTED_OPACITY_DARK: f64 = 0.26;

/// The size of a [`CupertinoButton`].
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

/// The padding for each button size.
fn size_padding(size: CupertinoButtonSize) -> EdgeInsets {
    match size {
        CupertinoButtonSize::Small => EdgeInsets::symmetric(6.0, 12.0),
        CupertinoButtonSize::Medium => EdgeInsets::symmetric(10.0, 15.0),
        CupertinoButtonSize::Large => EdgeInsets::symmetric(16.0, 20.0),
    }
}

/// The border radius for each button size.
fn size_border_radius(size: CupertinoButtonSize) -> BorderRadius {
    match size {
        CupertinoButtonSize::Small | CupertinoButtonSize::Medium => BorderRadius::circular(40.0),
        CupertinoButtonSize::Large => BorderRadius::circular(12.0),
    }
}

/// The minimum dimension for each button size — the fallback `build()` uses
/// only when [`CupertinoButton::minimum_size`] is unset. A further 44.0
/// minimum-interactive-dimension fallback would be unreachable (this covers
/// every [`CupertinoButtonSize`] variant), so it is omitted —
/// `minimum_size` passes an explicit value (including `0.0`, which genuinely
/// removes the floor) straight through unmodified.
fn size_min_dimension(size: CupertinoButtonSize) -> f64 {
    match size {
        CupertinoButtonSize::Small => 28.0,
        CupertinoButtonSize::Medium => 32.0,
        CupertinoButtonSize::Large => 44.0,
    }
}

/// The background-fill style, exposed only through the three constructors.
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

/// An iOS-style button — see the module doc for the honest reductions made
/// against real down/move/up press tracking.
#[derive(Clone, StatefulView)]
pub struct CupertinoButton {
    child: BoxedView,
    style: ButtonFillStyle,
    size_style: CupertinoButtonSize,
    padding: Option<EdgeInsets>,
    color: Option<CupertinoColor>,
    foreground_color: Option<CupertinoColor>,
    disabled_color: CupertinoColor,
    minimum_size: Option<(f64, f64)>,
    pressed_opacity: Option<f64>,
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

    /// A plain button: no background, primary-color text.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self::with_style(child, ButtonFillStyle::Plain)
    }

    /// A button with a translucent background derived from
    /// [`crate::theme::CupertinoThemeData::primary_color`].
    #[must_use]
    pub fn tinted(child: impl IntoView) -> Self {
        Self::with_style(child, ButtonFillStyle::Tinted)
    }

    /// A button with a solid, opaque background.
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
    pub fn minimum_size(mut self, width: f64, height: f64) -> Self {
        self.minimum_size = Some((width, height));
        self
    }

    /// Sets the opacity the button fades to while pressed (default `0.4`).
    /// `None` disables the fade animation entirely.
    #[must_use]
    pub fn pressed_opacity(mut self, pressed_opacity: Option<f64>) -> Self {
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
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_pressed = Some(Rc::new(move |cx| callback(cx).report()));
        self
    }

    /// Sets the long-press handler — wired straight to
    /// [`flui_sdk::widgets::GestureDetector::on_long_press`], with no fade
    /// animation tied to it (long press is a wholly separate recognizer from
    /// the tap-driven fade).
    #[must_use]
    pub fn on_long_press<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_long_press = Some(Rc::new(move |cx| callback(cx).report()));
        self
    }

    /// Whether the button responds to interaction.
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

/// Resolves the background fill.
///
/// ## Why the `Plain`/`Filled` alpha comes from `view.color`, not the resolved color
///
/// For `Tinted`, the multiplier is a fixed light/dark constant — no
/// surprises. For `Plain`/`Filled`, the alpha is read from the **original,
/// never-resolved** `view.color`, not from the resolved color. A dynamic
/// color's own alpha is that of its light/normal variant; resolving it
/// against the ambient brightness yields a separate value. So this
/// multiplier is always the color's light-variant alpha, even when the
/// background itself resolved to the dark variant.
///
/// This looks like a bug (a dark-mode background painted with the light
/// alpha) but is deliberate: `CupertinoColors::SEPARATOR` is alpha 73 light /
/// 153 dark, and a `SEPARATOR`-colored `Plain`/`Filled` button under a Dark
/// theme renders at alpha 73. The alpha is a direct channel copy
/// (`Color::with_alpha`, `u8`) rather than an `f64` opacity round trip — same
/// source byte, no float-precision risk.
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

/// Resolves the foreground (text/icon) color.
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

/// Starts the press-in fade on tap, returning the run's [`TickerFuture`] so
/// the caller can chain the release fade onto it — `None` if it did not
/// start a run at all. Extracted from the `on_tap` closure so "does a tap
/// with `pressed_opacity: None` actually start the controller" is
/// unit-testable without mounting a render tree (see the tests below).
///
/// `pressed_opacity: None` means [`CupertinoButton::pressed_opacity`]'s
/// contract — "disables the fade animation entirely". Driving the controller
/// anyway would tick invisibly (the tween's begin/end both collapse to
/// `1.0`). Skipping the run here has no observable paint difference —
/// `build`'s `opacity` `FloatTween` also collapses to `1.0..=1.0` in that
/// case — it only removes wasted ticking and rebuild-scheduling.
fn start_press_fade(
    controller: &AnimationController,
    pressed_opacity: Option<f64>,
    rebuild: Option<&RebuildHandle>,
) -> Option<TickerFuture> {
    pressed_opacity?;
    let curve: Arc<dyn flui_sdk::animation::Curve + Send + Sync> =
        Arc::new(Curves::EaseInOutCubicEmphasized);
    let outcome = controller.animate_to_curved(1.0, Some(K_FADE_OUT_DURATION), curve);
    if let Err(error) = &outcome {
        tracing::debug!(?error, "CupertinoButton press fade failed to start");
    }
    if let Some(rebuild) = rebuild {
        rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
    }
    outcome.ok()
}

/// Starts the release fade once `press_fade` genuinely completes — never on
/// cancellation (a second tap superseding this one must not start a
/// release for the one it displaced). Extracted from the `on_tap` closure
/// for the same reason as [`start_press_fade`]: a direct, controller-only
/// unit test proves "the release starts exactly once per tap" without
/// mounting a render tree.
///
/// Chained on `press_fade`'s own [`TickerFuture`] (ADR-0064), not a status
/// listener: a status listener has no way to tell "the press fade just
/// landed" from "the release fade just landed" now that direction is chosen
/// by the method (`animate_to_curved` reports `Completed` at BOTH ends,
/// issue #1171) — a listener watching `Completed` unconditionally
/// re-triggers itself once the release it started lands. Chaining off the
/// return of the leg just started is one-shot, unlike a persistent listener.
fn chain_release_fade(controller: &AnimationController, press_fade: TickerFuture) {
    let release_controller = controller.clone();
    press_fade.when_complete_or_cancel(move |outcome| {
        if outcome.is_ok() {
            let curve: Arc<dyn flui_sdk::animation::Curve + Send + Sync> =
                Arc::new(Curves::EaseOutCubic);
            if let Err(error) =
                release_controller.animate_to_curved(0.0, Some(K_FADE_IN_DURATION), curve)
            {
                tracing::debug!(?error, "CupertinoButton release fade failed to start");
            }
        }
    });
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
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // ADR-0018: `rebuild_handle()` acquired here, fired later from
        // `start_press_fade` (called from the `on_tap` handler in `build`,
        // never from `build` itself).
        self.rebuild = Some(ctx.rebuild_handle());

        let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) else {
            // No ambient VsyncScope: no clock to animate the fade against.
            // Tapping still fires the handler with no visible fade — the same
            // degrade `flui-material`'s `ink_well` documents for its own
            // `VsyncScope`-less case.
            return;
        };

        let controller =
            AnimationController::new(Duration::from_millis(200), &UpdateScheduler::new());
        let registration = vsync.register(controller.clone());

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
        let decoration = BoxDecoration::<f64>::new()
            .set_color(fill_color)
            .set_border_radius(Some(
                view.border_radius
                    .unwrap_or_else(|| size_border_radius(view.size_style)),
            ));

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
        let constraints = BoxConstraints::new(min_width, f64::MAX, min_height, f64::MAX);

        let padding = view
            .padding
            .unwrap_or_else(|| size_padding(view.size_style));

        let content = Align::new(view.alignment)
            .width_factor(1.0)
            .height_factor(1.0)
            .child(DefaultTextStyle::new(text_style, view.child.clone()));

        let decorated = DecoratedBox::new(decoration).child(Padding::new(padding).child(content));

        let opacity: Arc<dyn Animation<f64>> = match &self.controller {
            Some(controller) => {
                let pressed_opacity = view.pressed_opacity.unwrap_or(1.0);
                let curved = Arc::new(Arc::new(controller.clone()).curved(Curves::Decelerate));
                Arc::new(
                    FloatTween::new(1.0, pressed_opacity)
                        .animate(curved as Arc<dyn Animation<f64>>),
                )
            }
            None => Arc::new(flui_sdk::animation::ConstantAnimation::new(1.0)),
        };

        // The button role is applied unconditionally, not gated on `enabled`
        // (a disabled button is still announced as a button, just an inert one).
        let faded = Semantics::new()
            .button(true)
            .child(ConstrainedBox::new(constraints).child(FadeTransition::new(opacity, decorated)));

        let mut gesture_detector = GestureDetector::new();
        if enabled {
            if let Some(on_pressed) = view.on_pressed.clone() {
                let controller = self.controller.clone();
                let rebuild = self.rebuild.clone();
                let pressed_opacity = view.pressed_opacity;
                gesture_detector = gesture_detector.on_tap(move |cx| {
                    if let Some(controller) = &controller
                        && let Some(future) =
                            start_press_fade(controller, pressed_opacity, rebuild.as_ref())
                    {
                        chain_release_fade(controller, future);
                    }
                    on_pressed(cx);
                });
            }
            if let Some(on_long_press) = view.on_long_press.clone() {
                gesture_detector = gesture_detector.on_long_press(move |cx| on_long_press(cx));
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
