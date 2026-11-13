//! [`InkWell`] — an M3 interactive surface: a state layer that tracks
//! hover/focus/drag and a ripple for each press, painted under the child.
//!
//! # One type for `InkWell` and `InkResponse`
//!
//! A contained, rectangular-highlight `InkWell` and a free-form
//! `InkResponse` only differ in ink clipping. FLUI ships one bounded type:
//! its ink is clipped to [`InkWell::shape`].
//!
//! # Feedback owned by the well
//!
//! The well owns its feedback, the way Compose's `Indication` does: there is
//! no ink-feature registry on an ancestor `Material`, so a ripple can never
//! paint outside the well that started it. [`crate::ripple`] holds the state
//! layer, the waves and their timing. Colours resolve in this order:
//!
//! - [`InkWell::overlay_color`] for the current
//!   [`WidgetStates`](flui_sdk::widgets::WidgetStates), alpha included —
//!   the component styles (buttons, checkbox, …) set it;
//! - otherwise the ambient [`RippleConfiguration`](crate::RippleConfiguration)
//!   colour, else the [`Theme`]'s `on_surface`, at the
//!   [`StateLayerAlpha`](crate::StateLayerAlpha) of the active state.
//!
//! A [`RippleScope`] with no configuration turns both the
//! ripple and the state layer off. Under reduced motion the well paints the
//! state layer only, `Pressed` included, and no expanding wave.
//!
//! # One node for its behaviours
//!
//! The well mounts the catalogs' private interactive node (ADR-0322): one
//! element and one render object carry its tap, its focus node and ring, its
//! activation bindings and the interaction states, where a well used to nest
//! a `GestureDetector`, a `MouseRegion`, `Actions`, `Focus` and `FocusRing`.
//! The node runs the same recognizers, focus node and semantics those
//! wrappers do, so nothing a user or a tool observes changes. Flutter's
//! `InkWell` composes `Actions`, `Focus`, `MouseRegion` and
//! `GestureDetector` the same way; the collapse is FLUI's, inside the
//! catalog.
//!
//! # `enabled` derivation (`isInteractive`)
//!
//! The oracle's `enabled` is `true` when ANY of six tap-family callbacks is
//! set (`isWidgetEnabled`, `ink_well.dart` `:1292-1303`). This well takes
//! [`InkWell::on_tap`] only, so its switch is `on_tap.is_some()`, unless the
//! control around it sets one: a `ListTile` is enabled or not whatever its
//! handler. The switch is the node's (ADR-0322 §2, §3): while it is off
//! [`WidgetState::Disabled`] is set and the semantics say disabled. The
//! well responds only while it is on and has a handler; otherwise no
//! recognizer joins an arena, hover stops updating [`WidgetState::Hovered`]
//! (oracle: `handleMouseEnter` gates `handleHoverChange` on `enabled`), the
//! well cannot take focus, and the activation intents stay declared and
//! switched off, so an Enter reaching it stops there rather than activating
//! an enclosing control. An enabled well with no handler is not disabled.
//! A well is still hit where it is, as its opaque mouse region made it.
//!
//! # Press-state timing
//!
//! [`WidgetState::Pressed`] is the node's from a contact's down to its up or
//! cancel, as the oracle's `handleTapDown`/`handleTap` set and clear it. An
//! activation (a tap, a key, an assistive request) then holds it for
//! `_activationDuration` (100 ms, `ink_well.dart` `:864`, `:897`): the oracle
//! does so for a synthetic activation with no down/up pair, and this well
//! does so for every activation, so a quick tap under reduced motion, where
//! no wave shows it, still shows a pressed state. The well takes a hold on
//! `Pressed` from its controller (`WidgetStatesController::hold_pressed`,
//! which counts it beside the node's contact), runs `on_tap`, then starts the
//! timer that drops the hold: a one-shot
//! [`flui_sdk::animation::AnimationController`] on the ambient
//! [`flui_sdk::view::environment::VsyncScope`]. A controller handed over
//! under the hold gets a hold of its own. Standalone (no
//! `VsyncScope` above this `InkWell`) there is no clock to time it against,
//! so the hold ends at once.
//!
//! The ripple itself follows the pointer contact rather than this timer: a
//! wave starts on the tap recognizer's down and fades out on the contact's
//! up or cancel, or at once when the recognizer gives the contact up (past
//! the slop, or lost the arena).
//!
//! # `WidgetStatesController` integration
//!
//! [`InkWell::states_controller`] accepts an external controller (shared
//! with a caller that also wants to read/drive the same state set); absent
//! one, the state owns a private [`WidgetStatesController`].

use std::cell::RefCell;
use std::rc::Rc;

use std::time::Duration;

use flui_sdk::animation::MotionPolicy;
use flui_sdk::animation::prelude::*;
use flui_sdk::geometry::Point;
use flui_sdk::interaction::FocusNode;
use flui_sdk::painting::Color;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::environment::VsyncScope;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::interaction::node::{
    ActionBinding, ControlTap, Description, InteractiveNode,
};
use flui_sdk::widgets::{
    Action, ActionOutcome, ActivateIntent, ButtonActivateIntent, ButtonChange, CustomPaint,
    HitTestBehavior, PointerDispatch, PointerEvent, PressHold, WidgetState, WidgetStateProperty,
    WidgetStatesController,
};

use crate::ripple::{Indication, IndicationPainter, IndicationStyle, RippleScope};
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// A user tap handler. `Rc`-based (owner-local, per ADR-0027) — matches
/// `GestureDetector::on_tap`'s callback shape.
type TapCallback = Rc<dyn Fn(&mut flui_sdk::view::EventContext<'_>)>;

/// How an [`InkWell`] shows that it has visible keyboard focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FocusIndicator {
    /// A [`FocusRing`](flui_sdk::widgets::FocusRing) in the inherited style around the well: buttons,
    /// tabs, list rows and every other control without one of its own.
    Ring,
    /// The control draws its own: the M3 halo of a checkbox, switch or radio,
    /// which reads the well's `FocusVisible` state.
    Halo,
}

/// How long the pressed state lingers after a tap — see the module doc's
/// "Press-state timing" section for why this substrate applies it
/// uniformly rather than only on synthetic activation.
const PRESS_DEACTIVATION_DELAY: Duration = Duration::from_millis(100);

/// An M3 interactive surface over a child: a state layer and press ripples,
/// clipped to its shape — see the module docs.
#[derive(Clone, StatefulView)]
pub struct InkWell {
    child: Option<BoxedView>,
    on_tap: Option<TapCallback>,
    overlay_color: WidgetStateProperty<Option<Color>>,
    shape: MaterialShape,
    states_controller: Option<WidgetStatesController>,
    focus_node: Option<Rc<FocusNode>>,
    focus_indicator: FocusIndicator,
    /// The node the control around the well is to assistive technology,
    /// carried by the well's own interactive node in place of a `Semantics`
    /// wrapper (ADR-0322 §1). Shared, so a well that describes nothing pays
    /// one word for it.
    description: Option<Rc<Description>>,
    /// The well's switch, when the control around it sets one: a `ListTile`
    /// is enabled or not whatever its handler. `None` takes the switch from
    /// the handler, as Flutter's `isWidgetEnabled` does.
    enabled: Option<bool>,
}

impl Default for InkWell {
    fn default() -> Self {
        Self::new()
    }
}

impl InkWell {
    /// An `InkWell` with no child, no tap handler (disabled), no overlay
    /// color configured (paints nothing extra), and the plain rectangle
    /// shape. Flutter's `InkWell.child` is nullable: a well without one
    /// sizes to its constraints' smallest box and still takes hits.
    pub fn new() -> Self {
        Self {
            child: None,
            on_tap: None,
            overlay_color: WidgetStateProperty::all(None),
            shape: MaterialShape::default(),
            states_controller: None,
            focus_node: None,
            focus_indicator: FocusIndicator::Ring,
            description: None,
            enabled: None,
        }
    }

    /// Sets the child the well paints its ink under.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Some(BoxedView::new(child.into_view()));
        self
    }

    /// Sets the tap handler. Presence of a handler is what makes this
    /// `InkWell` [interactive](self) — see the module doc's `enabled`
    /// section.
    #[must_use]
    pub fn on_tap<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventContext<'_>) -> R + 'static,
    ) -> Self {
        self.on_tap = Some(crate::event_callback::press_callback(callback));
        self
    }

    /// Sets the state-resolved feedback color, alpha included. Where it
    /// resolves to `None`, the ambient ripple configuration's color applies
    /// — see the module doc.
    #[must_use]
    pub fn overlay_color(mut self, overlay_color: WidgetStateProperty<Option<Color>>) -> Self {
        self.overlay_color = overlay_color;
        self
    }

    /// Sets the shape the state layer and ripple are clipped to. Defaults to
    /// a plain rectangle.
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = shape;
        self
    }

    /// Shares an external [`WidgetStatesController`] instead of the private
    /// one this widget otherwise owns.
    #[must_use]
    pub fn states_controller(mut self, controller: WidgetStatesController) -> Self {
        self.states_controller = Some(controller);
        self
    }

    /// Drives an external [`FocusNode`] instead of a widget-owned one.
    /// Exposed primarily so a caller
    /// (or a test) can request focus on this `InkWell` from outside the
    /// widget tree via [`FocusNode::request_focus`].
    #[must_use]
    pub fn focus_node(mut self, node: Rc<FocusNode>) -> Self {
        self.focus_node = Some(node);
        self
    }

    /// How the well shows visible focus; [`FocusIndicator::Ring`] unless a
    /// control draws its own.
    #[must_use]
    pub(crate) fn focus_indicator(mut self, indicator: FocusIndicator) -> Self {
        self.focus_indicator = indicator;
        self
    }

    /// Describes the control the well stands for to assistive technology on
    /// the well's own node: `ButtonStyleButton`'s button, which Flutter wraps
    /// around its `InkWell` as a `Semantics`. The node's switch sets
    /// `enabled`, and its tap and focus add their actions and flags.
    #[must_use]
    pub(crate) fn description(mut self, description: Description) -> Self {
        self.description = Some(Rc::new(description));
        self
    }

    /// Sets the well's switch apart from its handler: an enabled well with
    /// no handler is not disabled, and still mounts no tap, takes no focus
    /// and tracks no pointer (ADR-0322 §3). Defaults to whether
    /// [`InkWell::on_tap`] is set.
    #[must_use]
    pub(crate) fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = Some(enabled);
        self
    }

    /// The well's switch, the one value both its seed of
    /// [`WidgetState::Disabled`] and its node's switch read — see the module
    /// doc's `enabled` section.
    fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(self.on_tap.is_some())
    }

    /// Whether the well responds to interaction: enabled and given a handler.
    fn is_interactive(&self) -> bool {
        self.is_enabled() && self.on_tap.is_some()
    }
}

impl std::fmt::Debug for InkWell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InkWell")
            .field("on_tap", &self.on_tap.is_some())
            .field("enabled", &self.enabled)
            .field("overlay_color", &self.overlay_color)
            .field("shape", &self.shape)
            .finish_non_exhaustive()
    }
}

/// An activation's hold on [`WidgetState::Pressed`] and the
/// [`PRESS_DEACTIVATION_DELAY`] timer that ends it (ADR-0322 §3): live from
/// the activation until the timer settles, the next activation restarts it,
/// or the well is unmounted.
#[derive(Default)]
struct Activation {
    timer: Option<DrivenController>,
    hold: Option<PressHold>,
}

/// What the well's state, its tap, its key bindings and its deactivation
/// timer share, in one allocation: the current handler, the activation and
/// the indication. The tap and the bindings are made once per state and read
/// this at event time, so a rebuild allocates none of them.
///
/// Nothing this holds refers back to the tap or the bindings, so the
/// strong references they hold form no cycle; the timer's listener holds it
/// weakly.
struct Core {
    /// Refreshed every `build()`, so an activation always calls the
    /// *current* handler.
    on_tap: RefCell<Option<TapCallback>>,
    /// The last activation: its hold on `Pressed` and the timer that ends
    /// it.
    activation: RefCell<Activation>,
    /// The state layer and ripple, shared with the painter.
    indication: Rc<Indication>,
    rebuild: RebuildHandle,
    /// This core, for the deactivation timer's listener.
    this: std::rc::Weak<Self>,
}

impl Core {
    /// Cancels and disposes any in-flight press-deactivation timer.
    /// Idempotent (a no-op when nothing is pending). The slot borrow ends
    /// before the timer is dropped.
    fn cancel_pending_deactivation(&self) {
        let taken = self.activation.borrow_mut().timer.take();
        drop(taken);
    }

    /// Ends the activation's hold on `Pressed`, if one is live. The slot
    /// borrow ends before the controller's listeners run.
    fn release_press_hold(&self) {
        let taken = self.activation.borrow_mut().hold.take();
        drop(taken);
    }

    /// One activation for a tap and for the keyboard: the oracle's
    /// `activateOnIntent` (`ink_well.dart` `:883-900`) is both what its
    /// `ActivateIntent`/`ButtonActivateIntent` actions run and what the tap
    /// runs. A well with no handler is disabled, and its node delivers
    /// neither; the check keeps an activation that arrives anyway from
    /// showing a press.
    fn activate(&self, ctx: &mut flui_sdk::view::EventContext<'_>) -> ActionOutcome {
        // Cloned out, so the slot is free while the handler runs.
        let Some(handler) = self.on_tap.borrow().clone() else {
            return ActionOutcome::NotPerformed;
        };
        // Oracle order (`ink_well.dart` `activateOnIntent`, `:864-900`):
        // `WidgetState.pressed` is set FIRST, THEN `widget.onTap?.call()`
        // fires, and only after that does the hold start. A handler that
        // reads the states set (e.g. to resolve its own overlay) observes
        // `Pressed`.
        let hold = self.indication.states().hold_pressed();
        handler(ctx);
        self.cancel_pending_deactivation();
        // The new hold is taken before the old one goes, so `Pressed` stays
        // set across a second activation.
        let outgoing = self.activation.borrow_mut().hold.replace(hold);
        drop(outgoing);
        self.begin_press_deactivation();
        ActionOutcome::Performed
    }

    /// Starts the [`PRESS_DEACTIVATION_DELAY`] timer that ends the
    /// activation's hold on [`WidgetState::Pressed`] — see the module doc's
    /// "Press-state timing" section.
    ///
    /// The activation's timer must already be empty (the caller cancels any
    /// previous timer first): this only ever installs, never cancels.
    fn begin_press_deactivation(&self) {
        let Some(vsync) = self.indication.vsync() else {
            // No ambient VsyncScope: no clock to time the delay against. The
            // simplest available degrade is immediate deactivation —
            // documented in the module doc, the same shape `GestureDetector`
            // already uses for its own deadline-driven gestures without a
            // binding.
            self.release_press_hold();
            return;
        };

        let owner = AnimationController::builder(PRESS_DEACTIVATION_DELAY)
            .debug_label("InkWell.press")
            .behavior(flui_sdk::animation::AnimationBehavior::Preserve)
            .build_on(Some(&vsync));
        let controller = owner.controller().clone();

        // Completion changes the pressed state. The next press withdraws and
        // disposes the pending timer through `cancel_pending_deactivation`.
        let core = std::rc::Weak::clone(&self.this);
        let rebuild = self.rebuild.clone();
        // This callback lives until its owned timer is canceled or retired.
        controller
            .subscribe_status(Rc::new(move |status| {
                if status == AnimationStatus::ForwardSettled {
                    if let Some(core) = core.upgrade() {
                        core.release_press_hold();
                    }
                    rebuild.schedule(flui_sdk::view::RebuildReason::AnimationTick);
                }
            }))
            .detach();

        if let Err(error) = controller.forward_from(Some(0.0)) {
            tracing::debug!(?error, "InkWell press-deactivation timer failed to start");
            self.release_press_hold();
            return;
        }

        let outgoing = self.activation.borrow_mut().timer.replace(owner);
        drop(outgoing);
    }

    /// What a key activation runs: a wave from the centre, released at
    /// once, then the activation. A disabled well's node keeps these
    /// bindings declared and switched off, so the intent stops at the well.
    fn activate_by_key(&self, ctx: &mut flui_sdk::view::EventContext<'_>) -> ActionOutcome {
        if self.on_tap.borrow().is_none() {
            return ActionOutcome::NotPerformed;
        }
        // A key activation has no contact: its wave grows from the centre
        // and is released at once.
        self.indication.press(None, None);
        self.indication.release(None);
        self.activate(ctx)
    }
}

impl Action<ActivateIntent> for Core {
    fn invoke(
        &self,
        ctx: &mut flui_sdk::view::EventContext<'_>,
        _intent: &ActivateIntent,
    ) -> ActionOutcome {
        self.activate_by_key(ctx)
    }
}

impl Action<ButtonActivateIntent> for Core {
    fn invoke(
        &self,
        ctx: &mut flui_sdk::view::EventContext<'_>,
        _intent: &ButtonActivateIntent,
    ) -> ActionOutcome {
        self.activate_by_key(ctx)
    }
}

/// Persistent state behind [`InkWell`] — see [`StatefulView`]/[`ViewState`].
pub struct InkWellState {
    states: WidgetStatesController,
    states_listener: Option<flui_sdk::foundation::Subscription>,
    core: Rc<Core>,
    /// The tap and the wave it drives, made once: it reads the handler from
    /// `core` at event time. The node is handed it while the well is
    /// enabled.
    tap: Rc<ControlTap>,
}

impl std::fmt::Debug for InkWellState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InkWellState")
            .field("states", &self.states)
            .field(
                "has_pending_deactivation",
                &self.core.activation.borrow().timer.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl InkWellState {
    /// Listen to the current states controller: rebuild, and move the state
    /// layer, on every change.
    fn listen_to_states(&mut self) {
        let indication = Rc::downgrade(&self.core.indication);
        let rebuild = self.core.rebuild.clone();
        self.states_listener = Some(self.states.listen(Rc::new(move || {
            if let Some(indication) = indication.upgrade() {
                indication.sync();
            }
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));
    }

    /// Read the ambient clock, ripple configuration, theme colour and motion
    /// policy, registering a dependency on each.
    fn read_ambient(&self, ctx: &dyn LifecycleContext) {
        let vsync = VsyncScope::maybe_of(ctx);
        let has_clock = vsync.is_some();
        let indication = &self.core.indication;
        indication.bind(vsync);
        let configuration = RippleScope::of(ctx);
        let content_color = Theme::color_scheme_of(ctx).map(|scheme| scheme.on_surface);
        let reduce_motion = MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce);
        indication.update_style(|style| {
            style.configuration = configuration;
            style.content_color = content_color;
            style.reduce_motion = reduce_motion;
            style.has_clock = has_clock;
        });
    }
}

impl StatefulView for InkWell {
    type State = InkWellState;

    fn create_state(&self, ctx: &dyn LifecycleContext) -> Self::State {
        let states = self.states_controller.clone().unwrap_or_default();
        // The node writes `Disabled` (ADR-0322 §3) when it mounts, inside this
        // well's first build. Seeding the same value here, from the same
        // switch and before the listener exists, keeps that write from
        // notifying the well while it builds.
        states.update(WidgetState::Disabled, !self.is_enabled());
        let indication = Indication::new(
            states.clone(),
            IndicationStyle {
                overlay_color: self.overlay_color.clone(),
                configuration: None,
                content_color: None,
                reduce_motion: false,
                has_clock: false,
                focus_layer: self.focus_indicator == FocusIndicator::Halo,
            },
        );
        let core = Rc::new_cyclic(|this| Core {
            on_tap: RefCell::new(self.on_tap.clone()),
            activation: RefCell::default(),
            indication,
            // Fired later, from the states-controller listener and the
            // press-deactivation timer, never from `build` (ADR-0018).
            rebuild: ctx.rebuild_handle(),
            this: std::rc::Weak::clone(this),
        });
        InkWellState {
            states,
            states_listener: None,
            tap: Rc::new(press_feedback(&core)),
            core,
        }
    }
}

impl ViewState<InkWell> for InkWellState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // From here on every real change of the states set (hover/focus/
        // press/disabled) moves the state layer and rebuilds.
        self.listen_to_states();

        self.read_ambient(ctx);
        self.core.indication.set_rebuild(self.core.rebuild.clone());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.read_ambient(ctx);
        let pending = self.core.activation.borrow_mut().timer.take();
        if let Some(mut owner) = pending {
            if let Err(error) = owner.rebind(self.core.indication.vsync().as_ref()) {
                tracing::error!(%error, "InkWell lost its frame registry");
            }
            let displaced = {
                let slot = &mut self.core.activation.borrow_mut().timer;
                if slot.is_none() {
                    slot.replace(owner)
                } else {
                    Some(owner)
                }
            };
            drop(displaced);
        }
    }

    fn did_update_view(&mut self, old_view: &InkWell, new_view: &InkWell) {
        // A disabled well ends its activation: the hold goes with it, so
        // enabling the well again inside the delay shows no press.
        if !new_view.is_interactive() {
            self.core.cancel_pending_deactivation();
            self.core.release_press_hold();
        }
        // Re-home the states controller when the caller swaps in a
        // genuinely different one. A rebuild that re-clones the SAME
        // external controller, or leaves both `None` (keeping the private
        // one this state already owns), is not a swap: `WidgetStatesController`
        // has no `PartialEq`, so identity is `is_same`, not value equality.
        let controller_changed = match (&old_view.states_controller, &new_view.states_controller) {
            (Some(old), Some(new)) => !old.is_same(new),
            (None, None) => false,
            _ => true,
        };

        if controller_changed {
            drop(self.states_listener.take());

            self.states = new_view.states_controller.clone().unwrap_or_default();
            self.core.indication.set_states(self.states.clone());
            // A hold under way follows the activation to the new controller.
            if self.core.activation.borrow().hold.is_some() {
                let moved = self.states.hold_pressed();
                let outgoing = self.core.activation.borrow_mut().hold.replace(moved);
                drop(outgoing);
            }

            // Seeded before the listener, as in `create_state` (oracle:
            // `initStatesController` runs again on a controller swap,
            // `ink_well.dart` `:943`): the node writes the same value to the
            // controller it is handed during this build.
            self.states
                .update(WidgetState::Disabled, !new_view.is_enabled());

            self.listen_to_states();
        } else if new_view.is_enabled() != old_view.is_enabled() {
            // The node writes the new `Disabled` during this build; seeded
            // here, that write notifies nobody mid-build.
            self.states
                .update(WidgetState::Disabled, !new_view.is_enabled());
        }

        // A new overlay color or controller retargets the state layer.
        let overlay_color = new_view.overlay_color.clone();
        let focus_layer = new_view.focus_indicator == FocusIndicator::Halo;
        self.core.indication.update_style(|style| {
            style.overlay_color = overlay_color;
            style.focus_layer = focus_layer;
        });
    }

    fn build(&self, view: &InkWell, _ctx: &dyn BuildContext) -> impl IntoView {
        let interactive = view.is_interactive();
        // The replaced handler is dropped after the slot's borrow ends: its
        // `Drop` is the author's.
        let replaced = self.core.on_tap.replace(view.on_tap.clone());
        drop(replaced);

        // An idle well attaches no painter, so it records nothing; a state
        // change or a press rebuilds it with one.
        let indication = &self.core.indication;
        let active = indication.is_active();
        indication.set_painted(active);
        let mut content = CustomPaint::new();
        if active {
            content = content.painter(IndicationPainter {
                indication: Rc::clone(indication),
                shape: view.shape,
            });
        }
        if let Some(child) = &view.child {
            content = content.child(child.clone());
        }

        // `_actionMap` around the focus node (`ink_well.dart` `:852-855`,
        // `:1386-1389`): Enter, Space and Select on the focused well reach
        // these through the root shortcuts (ADR-0079). The actions run inside
        // the key event's dispatch and hand its `ctx` to the activation, the
        // same one a tap's recognizer opens.
        //
        // `Focused` follows the primary focus and `FocusVisible` the focus a
        // user can see (ADR-0079): a pointer press that focuses the well
        // leaves no ring, halo or overlay behind. The node writes both, with
        // `Hovered`, `Pressed` and `Disabled`, to the well's controller.
        // Its hit-test behaviour is the opaque mouse region's the well had:
        // hit over its whole box, enabled or not.
        let mut node = InteractiveNode::new(content)
            .behavior(HitTestBehavior::Opaque)
            .enabled(view.is_enabled())
            .states(self.states.clone())
            .bindings([
                ActionBinding::shared::<ActivateIntent, Core>(&self.core),
                ActionBinding::shared::<ButtonActivateIntent, Core>(&self.core),
            ]);
        node = match &view.focus_node {
            Some(focus_node) => node.focus_node(Rc::clone(focus_node)),
            None => node.focusable("InkWell"),
        };
        if let Some(description) = &view.description {
            node = node.semantics(Description::clone(description));
        }
        if view.focus_indicator == FocusIndicator::Ring {
            node = node.focus_ring();
        }
        if interactive {
            node = node.tap(Rc::clone(&self.tap));
        }
        node
    }

    fn dispose(&mut self) {
        drop(self.states_listener.take());
        self.core.cancel_pending_deactivation();
        self.core.release_press_hold();
        self.core.indication.dispose();
    }
}

/// The tap and the wave it drives. The wave follows the press the tap
/// recognizer shows: it starts at the down (at once when no rival holds the
/// contact, else after the press timeout or at the win) and ends with that
/// contact's up or cancel. A contact that was never shown as pressed started
/// no wave and ends none.
///
/// The raw ends of the contact end the wave too: the last held button's up,
/// the primary button's release under another held button, or the
/// pointer's cancel. They arrive before the recognizer sees the same event,
/// and only observe, so recognition is untouched.
///
/// Made once per state: the tap reads the current handler from `core`.
fn press_feedback(core: &Rc<Core>) -> ControlTap {
    let (down, up, tap, cancel) = (
        Rc::clone(core),
        Rc::clone(core),
        Rc::clone(core),
        Rc::clone(core),
    );
    let (on_up, on_button_up, on_cancel) = (Rc::clone(core), Rc::clone(core), Rc::clone(core));
    ControlTap::new()
        .on_tap_down(move |_ctx, details| {
            down.indication.press(
                Some(Point::new(
                    details.local_position.dx,
                    details.local_position.dy,
                )),
                Some(details.pointer.get()),
            );
        })
        .on_tap_up(move |_ctx, details| {
            up.indication.release(Some(details.pointer.get()));
        })
        .on_tap(move |ctx| {
            tap.activate(ctx);
        })
        .on_tap_cancel(move |_ctx, details| {
            cancel.indication.cancel(Some(details.pointer.get()));
        })
        // An `Up` is the last held button coming up, so it always ends the
        // press. A button released while another is still held arrives as a
        // `ButtonChange` (through a move): only the primary's ends the press
        // there.
        .on_pointer_up(move |_ctx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::Up(release) = dispatch.local {
                on_up.indication.release(Some(release.pointer.id.get()));
            }
        })
        .on_pointer_move(move |_ctx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::ButtonChange(ButtonChange::Released(release)) = dispatch.local
                && release.button().number().get() == 1
            {
                on_button_up.indication.release(Some(release.pointer.id.get()));
            }
        })
        .on_pointer_cancel(move |_ctx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::Cancel(cancel) = dispatch.local {
                on_cancel.indication.cancel(Some(cancel.pointer.id.get()));
            }
        })
}

#[cfg(test)]
mod tests {
    use flui_sdk::widgets::SizedBox;
    use flui_testing::widgets::{SharedSignal, lay_out, tight};

    use super::*;

    /// A well whose switch a test sets apart from its handler, as a
    /// `ListTile` does.
    #[derive(Clone, StatefulView)]
    struct SwitchedWell {
        enabled: SharedSignal<bool>,
        handler: bool,
        states: WidgetStatesController,
    }

    struct SwitchedWellState {
        enabled: SharedSignal<bool>,
    }

    impl StatefulView for SwitchedWell {
        type State = SwitchedWellState;
        fn create_state(&self, _ctx: &dyn LifecycleContext) -> Self::State {
            SwitchedWellState {
                enabled: self.enabled.clone(),
            }
        }
    }

    impl ViewState<SwitchedWell> for SwitchedWellState {
        fn init_state(&mut self, ctx: &dyn LifecycleContext) {
            self.enabled.bind(ctx);
        }

        fn build(&self, view: &SwitchedWell, ctx: &dyn BuildContext) -> impl IntoView {
            let well = InkWell::new()
                .child(SizedBox::new(80.0, 40.0))
                .states_controller(view.states.clone())
                .enabled(self.enabled.get(ctx));
            if view.handler {
                well.on_tap(|_ctx| {})
            } else {
                well
            }
        }
    }

    /// The well's switch, not its handler, writes `Disabled`: an enabled
    /// well without a handler is not disabled, and turning the switch with
    /// the handler kept brings the controller along, from the seed before
    /// the build as from the node within it.
    ///
    /// Red-check: take the switch from the handler alone (`is_enabled`
    /// ignoring `enabled`), and the well without a handler is disabled.
    #[test]
    fn the_switch_writes_disabled_whatever_the_handler() {
        for handler in [true, false] {
            let enabled = SharedSignal::new(true);
            let states = WidgetStatesController::default();
            let mut laid = lay_out(
                SwitchedWell {
                    enabled: enabled.clone(),
                    handler,
                    states: states.clone(),
                },
                tight(80.0, 40.0),
            );
            let disabled = || states.value().contains_state(WidgetState::Disabled);
            assert!(!disabled(), "handler: {handler}: enabled is not disabled");
            enabled.set(false);
            laid.pump();
            assert!(disabled(), "handler: {handler}: turned off");
            enabled.set(true);
            laid.pump();
            assert!(!disabled(), "handler: {handler}: turned on again");
        }
    }

    /// An enabled well without a handler is not operated: it mounts no
    /// mouse part and no pointer hovers it, as Flutter's `InkWell` without
    /// callbacks.
    ///
    /// Red-check: give the node's mouse part to every enabled control (its
    /// press not asked for), and the well without a handler mounts one.
    #[test]
    fn an_enabled_well_without_a_handler_tracks_no_pointer() {
        for handler in [true, false] {
            let states = WidgetStatesController::default();
            let mut laid = lay_out(
                SwitchedWell {
                    enabled: SharedSignal::new(true),
                    handler,
                    states: states.clone(),
                },
                tight(80.0, 40.0),
            );
            let node = laid
                .try_find_by_render_type("RenderInteractive")
                .expect("the well's node");
            assert_eq!(
                laid.render_node(node).get::<bool>("has_mouse_part").ok() == Some(true),
                handler,
                "handler: {handler}: the mouse part"
            );
            laid.dispatch_pointer_hover(40.0, 20.0);
            laid.pump();
            assert_eq!(
                states.value().contains_state(WidgetState::Hovered),
                handler,
                "handler: {handler}: hovered"
            );
        }
    }
}
