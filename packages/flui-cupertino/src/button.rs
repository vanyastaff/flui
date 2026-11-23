//! The iOS-style button and its press fade.
//!
//! Implementation notes: the press fade's controller is made at the first
//! press that fades, and each leg starts the next through its
//! `AnimationRunFuture` (ADR-0064); the button's behaviours, focus,
//! activation bindings and semantics are one interactive node (ADR-0322 §1).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::time::Duration;

use flui_sdk::animation::Animatable;

use flui_sdk::animation::{
    Animation, AnimationController, CurvedAnimation, DrivenController, Tween, Vsync, curves,
};
use flui_sdk::geometry::EdgeInsets;
use flui_sdk::painting::Alignment;
use flui_sdk::painting::styling::{BorderRadius, BorderRadiusExt, BoxDecoration, Color};
use flui_sdk::painting::typography::TextStyle;
use flui_sdk::platform::Brightness;
use flui_sdk::semantics::SemanticsRole;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::environment::VsyncScope;
use flui_sdk::view::prelude::*;
use flui_sdk::view::{BoxedView, StatefulView, ViewState};
use flui_sdk::widgets::__private::node::{ActionBinding, ControlTap, Description, InteractiveNode};
use flui_sdk::widgets::prelude::BoxConstraints;
use flui_sdk::widgets::{
    Action, ActionOutcome, ActivateIntent, Align, ButtonActivateIntent, ConstrainedBox,
    DecoratedBox, DefaultTextStyle, FadeTransition, Padding,
};

use crate::CupertinoTheme;
use crate::colors::{CupertinoColor, CupertinoColors};

/// A user tap/long-press handler. `Rc`-based (owner-local, per ADR-0027) —
/// matches the callback shape of the node's `ControlTap::on_tap`.
type ButtonCallback = Rc<dyn Fn(&mut flui_sdk::view::EventContext<'_>)>;

/// The press-in fade's duration.
pub(crate) const FADE_OUT_DURATION: Duration = Duration::from_millis(120);

/// The release fade's duration.
pub(crate) const FADE_IN_DURATION: Duration = Duration::from_millis(180);

/// The tinted background's opacity under a light theme.
const K_TINTED_OPACITY_LIGHT: f64 = 0.12;
/// The tinted background's opacity under a dark theme.
const K_TINTED_OPACITY_DARK: f64 = 0.26;

/// The size of a [`CupertinoButton`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CupertinoButtonSize {
    /// A smaller button with round sides and [`crate::CupertinoTextThemeData::action_small_text_style`].
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

/// An iOS-style button: plain ([`CupertinoButton::new`]), tinted
/// ([`CupertinoButton::tinted`]) or filled ([`CupertinoButton::filled`]).
///
/// [`CupertinoButtonSize`] selects padding, border radius and minimum
/// geometry; the theme and brightness determine the foreground and the fill.
/// A button with a tap or long-press handler is [`enabled`](Self::enabled);
/// without one it paints its disabled colors and announces itself as a
/// disabled button.
///
/// # Focus and activation
///
/// An enabled button takes keyboard focus and draws the inherited focus ring
/// while that focus is shown. Enter, Space or Select on the focused button
/// (an `ActivateIntent` or a `ButtonActivateIntent`) presses it, as Flutter's
/// `FocusableActionDetector` with its `ActivateIntent` action does
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/button.dart:455`).
/// A disabled button does not take focus, and an activation intent stops at
/// it.
///
/// # Press fade
///
/// The fade follows the press, as Flutter's `_handleTapDown`,
/// `_handleTapUp`, `_handleTapCancel` and `_animate` do
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/button.dart:372`).
/// When the tap recognizer shows the press, the button animates toward
/// [`pressed_opacity`](Self::pressed_opacity) over 120 ms
/// (`curves::EASE_IN_OUT_CUBIC_EMPHASIZED`); when the contact lifts or the
/// tap is cancelled, it animates back to full opacity over 180 ms
/// (`curves::EASE_OUT_CUBIC`). A leg that is running is never cut short: the
/// press that ends before its fade-out completes fades back in once it has.
/// The handler runs on the recognized tap, after the release has begun.
///
/// A key activation or an assistive-technology click has no contact: it
/// calls the handler and leaves the opacity as it is, as Flutter's
/// `_handleTap` does
/// (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/button.dart:419`).
///
/// A contact that moves past the tap slop cancels the tap, so the button
/// fades back in and does not call the handler. Flutter's recognizer keeps
/// such a tap (`postAcceptSlopTolerance: null`) and fades out again when
/// the contact returns over the button; here the press ends at the slop.
///
/// # Not implemented
///
/// - No `autofocus`, `focus_node` or focus-change callback: the catalog's
///   interactive node takes none from a package.
/// - The focus ring is the inherited ring style, not Flutter's
///   `RoundedSuperellipseBorder` outline in a colour derived from the fill.
/// - A button with only a long-press handler shows no press fade: a tap
///   family of its own would offer assistive technology a click that does
///   nothing.
/// - It resolves no mouse cursor; FLUI has no `MouseCursor` type to resolve.
/// - It does not wrap its child in an `IconTheme` sized from the text style.
///
/// Flutter reference: `flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/button.dart`.
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
    identifier: Option<std::sync::Arc<str>>,
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
            identifier: None,
        }
    }

    /// A plain button: no background, primary-color text.
    #[must_use]
    pub fn new(child: impl IntoView) -> Self {
        Self::with_style(child, ButtonFillStyle::Plain)
    }

    /// A button with a translucent background derived from
    /// [`crate::CupertinoThemeData::primary_color`].
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

    /// Names the button's node for tests and automation (ADR-0240): exported
    /// as UI Automation's `AutomationId`, never announced. It names the node
    /// the button's semantics form; see `Semantics::identifier`.
    #[must_use]
    pub fn identifier(mut self, identifier: impl Into<std::sync::Arc<str>>) -> Self {
        self.identifier = Some(identifier.into());
        self
    }

    /// Sets the tap handler. Presence of a tap or long-press handler is what
    /// makes this button [`Self::enabled`].
    #[must_use]
    pub fn on_pressed<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventContext<'_>) -> R + 'static,
    ) -> Self {
        self.on_pressed = Some(Rc::new(move |ctx| callback(ctx).report()));
        self
    }

    /// Sets the long-press handler — wired straight to
    /// the long press of the button's node, with no fade
    /// animation tied to it (long press is a wholly separate recognizer from
    /// the tap-driven fade).
    #[must_use]
    pub fn on_long_press<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventContext<'_>) -> R + 'static,
    ) -> Self {
        self.on_long_press = Some(Rc::new(move |ctx| callback(ctx).report()));
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

/// Resolves the background fill: the authored colour, or the theme's primary
/// colour for a tinted or filled button without one.
///
/// A plain or filled button paints that colour as it resolved, its alpha
/// included, so a dynamic colour keeps the alpha of the variant the ambient
/// brightness and contrast pick: `CupertinoColors::SEPARATOR` paints alpha 73
/// under a light theme and 153 under a dark one. A tinted button replaces
/// the alpha with its fixed light or dark opacity. Flutter multiplies a
/// plain or filled fill by the unresolved colour's opacity, the light
/// variant's (`flutter/flutter@3.50.1 packages/flutter/lib/src/cupertino/button.dart:480`),
/// which paints a dark-mode fill with the light alpha; the resolved colour
/// is what the author's dynamic colour asks for.
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
        ButtonFillStyle::Plain | ButtonFillStyle::Filled => base_color,
    })
}

/// Resolves the foreground (text/icon) color.
fn resolve_foreground_color(
    view: &CupertinoButton,
    ctx: &dyn BuildContext,
    theme: &crate::CupertinoThemeData,
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

/// What the button's state, its tap and its activation bindings share: the
/// current handler and the press fade.
///
/// The fade's controller is made at the first press that fades: a button
/// nobody presses holds no animation and no frame registration. Nothing
/// here refers back to the tap or the bindings, so the strong references
/// they hold form no cycle; a running leg's continuation holds this core
/// weakly.
struct ButtonCore {
    /// The tap handler, refreshed every build, so an activation calls the
    /// current one.
    on_pressed: RefCell<Option<ButtonCallback>>,
    /// Whether the button fades at all ([`CupertinoButton::pressed_opacity`]
    /// is set), refreshed every build.
    fades: Cell<bool>,
    /// Whether a press holds the button down: the end the fade runs toward,
    /// Flutter's `_buttonHeldDown`.
    held: Cell<bool>,
    controller: RefCell<Option<DrivenController>>,
    /// The ambient clock, read at each dependency change, that a controller
    /// made later is driven by.
    vsync: RefCell<Option<Vsync>>,
    /// Rebuilds the button once its controller exists, so the fade
    /// transition reads it (ADR-0018: scheduled from a press, never from
    /// `build`).
    rebuild: RebuildHandle,
    /// This core, for a running leg's continuation.
    this: Weak<Self>,
}

impl ButtonCore {
    /// The controller, made on the ambient clock if there is none yet. The
    /// slot's borrow ends before the caller drives it.
    fn controller(&self) -> AnimationController {
        let mut slot = self.controller.borrow_mut();
        if let Some(owner) = slot.as_ref() {
            return owner.controller().clone();
        }
        let owner = AnimationController::builder(Duration::from_millis(200))
            .debug_label("CupertinoButton.press")
            .build_on(self.vsync.borrow().as_ref());
        let controller = owner.controller().clone();
        *slot = Some(owner);
        drop(slot);
        self.rebuild
            .schedule(flui_sdk::view::RebuildReason::StateChange);
        controller
    }

    /// The controller's handle, if one was made.
    fn current(&self) -> Option<AnimationController> {
        self.controller
            .borrow()
            .as_ref()
            .map(|owner| owner.controller().clone())
    }

    /// Moves the press to `held`, as Flutter's tap handlers set
    /// `_buttonHeldDown`, and starts the fade toward it. A move that leaves
    /// the side unchanged starts nothing.
    fn hold(&self, held: bool) {
        if self.held.replace(held) == held || !self.fades.get() {
            return;
        }
        self.animate();
    }

    /// Starts the leg toward the held side unless a leg is running, as
    /// Flutter's `_animate` does: a running leg is never cut short, and the
    /// one that completes starts the next if the side changed meanwhile, so
    /// a press that lifts during its fade-out fades back in after it.
    ///
    /// A leg that ends cancelled (its controller disposed) starts nothing,
    /// so a retired fade never runs again.
    fn animate(&self) {
        let controller = self.controller();
        if controller.is_animating() {
            return;
        }
        let held = self.held.get();
        let run = if held {
            controller.animate_to_curved(
                1.0,
                Some(FADE_OUT_DURATION),
                curves::EASE_IN_OUT_CUBIC_EMPHASIZED,
            )
        } else {
            controller.animate_to_curved(0.0, Some(FADE_IN_DURATION), curves::EASE_OUT_CUBIC)
        };
        match run {
            Ok(run) => {
                let this = Weak::clone(&self.this);
                run.when_complete_or_cancel(move |outcome| {
                    if outcome.is_ok()
                        && let Some(core) = this.upgrade()
                        && core.held.get() != held
                    {
                        core.animate();
                    }
                });
            }
            Err(error) => tracing::debug!(?error, "CupertinoButton press fade failed to start"),
        }
    }

    /// The press: what the recognized tap, a key activation and an
    /// assistive-technology click run, as Flutter's `_handleTap`. It calls
    /// the handler and changes no fade: a contact's own down and up drive
    /// that, and an activation without one shows none. The handler runs with
    /// no borrow held, after the tap's up has committed the release, so a
    /// handler that panics leaves the next press able to fade.
    fn activate(&self, ctx: &mut flui_sdk::view::EventContext<'_>) -> ActionOutcome {
        // Cloned out, so the slot is free while the handler runs.
        let handler = self.on_pressed.borrow().clone();
        let Some(handler) = handler else {
            return ActionOutcome::NotPerformed;
        };
        handler(ctx);
        ActionOutcome::Performed
    }
}

impl Action<ActivateIntent> for ButtonCore {
    fn invoke(
        &self,
        ctx: &mut flui_sdk::view::EventContext<'_>,
        _intent: &ActivateIntent,
    ) -> ActionOutcome {
        self.activate(ctx)
    }
}

impl Action<ButtonActivateIntent> for ButtonCore {
    fn invoke(
        &self,
        ctx: &mut flui_sdk::view::EventContext<'_>,
        _intent: &ButtonActivateIntent,
    ) -> ActionOutcome {
        self.activate(ctx)
    }
}

/// Persistent state behind [`CupertinoButton`] — see [`StatefulView`]/
/// [`ViewState`].
pub struct CupertinoButtonState {
    core: Rc<ButtonCore>,
}

impl std::fmt::Debug for CupertinoButtonState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CupertinoButtonState")
            .field("has_controller", &self.core.controller.borrow().is_some())
            .field("held", &self.core.held.get())
            .finish_non_exhaustive()
    }
}

impl StatefulView for CupertinoButton {
    type State = CupertinoButtonState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let rebuild = ctx.rebuild_handle();
        CupertinoButtonState {
            core: Rc::new_cyclic(|this| ButtonCore {
                on_pressed: RefCell::new(None),
                fades: Cell::new(false),
                held: Cell::new(false),
                controller: RefCell::new(None),
                vsync: RefCell::new(None),
                rebuild,
                this: Weak::clone(this),
            }),
        }
    }
}

impl ViewState<CupertinoButton> for CupertinoButtonState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.core.vsync.borrow_mut() = VsyncScope::maybe_of(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let vsync = VsyncScope::maybe_of(ctx);
        if let Some(controller) = self.core.controller.borrow_mut().as_mut()
            && let Err(error) = controller.rebind(vsync.as_ref())
        {
            tracing::error!(%error, "CupertinoButton lost its frame registry");
        }
        *self.core.vsync.borrow_mut() = vsync;
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

        let opacity: std::rc::Rc<dyn Animation<f64>> = match self.core.current() {
            Some(controller) => {
                let pressed_opacity = view.pressed_opacity.unwrap_or(1.0);
                let curved = std::rc::Rc::new(CurvedAnimation::new(
                    Rc::new(controller) as std::rc::Rc<dyn Animation<f64>>,
                    curves::DECELERATE,
                ));
                std::rc::Rc::new(
                    Tween::new(1.0, pressed_opacity)
                        .animate(curved as std::rc::Rc<dyn Animation<f64>>),
                )
            }
            None => std::rc::Rc::new(flui_sdk::animation::ConstantAnimation::new(1.0)),
        };

        // The replaced handler is dropped after the slot's borrow ends: its
        // `Drop` is the author's.
        let replaced = self.core.on_pressed.replace(view.on_pressed.clone());
        drop(replaced);
        self.core.fades.set(view.pressed_opacity.is_some());

        // Flutter's `Semantics(button: true)`, `RawGestureDetector` and
        // `FocusableActionDetector` around the button are one interactive
        // node here (ADR-0322 §1): its description is the button, announced
        // as one whether enabled or not (a disabled button is still a
        // button, just an inert one), named by the identifier (ADR-0240);
        // its switch says `enabled`, and its press group adds the click and,
        // with a long press, the context menu. The node hosts the button's
        // focus node and draws the ring while that focus is shown; Enter,
        // Space and Select on the focused button reach the bindings through
        // the root shortcuts (ADR-0079). A disabled node cannot take focus
        // and keeps the bindings declared and switched off, so the intent
        // stops at the button.
        let description =
            Description::new(SemanticsRole::Button).identifier(view.identifier.clone());
        let faded =
            ConstrainedBox::new(constraints).child(FadeTransition::new(opacity).child(decorated));

        let mut node = InteractiveNode::new(faded)
            .enabled(enabled)
            .semantics(description)
            .focusable("CupertinoButton")
            .focus_ring()
            .bindings([
                ActionBinding::shared::<ActivateIntent, ButtonCore>(&self.core),
                ActionBinding::shared::<ButtonActivateIntent, ButtonCore>(&self.core),
            ]);
        if enabled {
            let mut tap = ControlTap::new();
            // The fade follows the press the tap recognizer shows (Flutter's
            // `_handleTapDown`/`_handleTapUp`/`_handleTapCancel`); only a
            // button with a tap handler has the tap family, so a
            // long-press-only button offers assistive technology no click.
            if view.on_pressed.is_some() {
                let (down, up, cancel, pressed) = (
                    Rc::clone(&self.core),
                    Rc::clone(&self.core),
                    Rc::clone(&self.core),
                    Rc::clone(&self.core),
                );
                tap = tap
                    .on_tap_down(move |_ctx, _details| down.hold(true))
                    .on_tap_up(move |_ctx, _details| up.hold(false))
                    .on_tap_cancel(move |_ctx, _details| cancel.hold(false))
                    .on_tap(move |ctx| {
                        pressed.activate(ctx);
                    });
            }
            if let Some(on_long_press) = view.on_long_press.clone() {
                tap = tap.on_long_press(move |ctx| on_long_press(ctx));
            }
            node = node.tap(tap);
        }
        node
    }

    fn dispose(&mut self) {
        // Dropping the owner cancels a running leg, whose continuation then
        // starts nothing.
        let controller = self.core.controller.borrow_mut().take();
        drop(controller);
    }
}
