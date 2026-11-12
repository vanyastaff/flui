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
//! [`InkWell::on_tap`] only, so `enabled` is `on_tap.is_some()`, and it is the
//! node's one switch (ADR-0322 §2): while disabled
//! [`WidgetState::Disabled`] is set, no recognizer joins an arena, hover
//! stops updating [`WidgetState::Hovered`] (oracle: `handleMouseEnter` gates
//! `handleHoverChange` on `enabled`), the well cannot take focus, and the
//! activation intents stay declared and switched off, so an Enter reaching
//! it stops there rather than activating an enclosing control. A disabled
//! well is still hit where it is, as its opaque mouse region made it.
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

use flui_sdk::animation::prelude::*;
use flui_sdk::animation::{MotionPolicy, Vsync};
use flui_sdk::geometry::Point;
use flui_sdk::interaction::FocusNode;
use flui_sdk::painting::Color;
use flui_sdk::view::RebuildHandle;
use flui_sdk::view::environment::MediaQuery;
use flui_sdk::view::environment::VsyncScope;
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::interaction::node::{ControlTap, InteractiveNode};
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
type TapCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>)>;

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
    child: BoxedView,
    on_tap: Option<TapCallback>,
    overlay_color: WidgetStateProperty<Option<Color>>,
    shape: MaterialShape,
    states_controller: Option<WidgetStatesController>,
    focus_node: Option<Rc<FocusNode>>,
    focus_indicator: FocusIndicator,
}

impl InkWell {
    /// An `InkWell` around `child` with no tap handler (disabled), no
    /// overlay color configured (paints nothing extra), and the plain
    /// rectangle shape.
    pub fn new(child: impl IntoView) -> Self {
        Self {
            child: BoxedView::new(child.into_view()),
            on_tap: None,
            overlay_color: WidgetStateProperty::all(None),
            shape: MaterialShape::default(),
            states_controller: None,
            focus_node: None,
            focus_indicator: FocusIndicator::Ring,
        }
    }

    /// Sets the tap handler. Presence of a handler is what makes this
    /// `InkWell` [interactive](self) — see the module doc's `enabled`
    /// section.
    #[must_use]
    pub fn on_tap<R: flui_sdk::view::EventOutcome>(
        mut self,
        callback: impl Fn(&mut flui_sdk::view::EventCx<'_>) -> R + 'static,
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

    /// Whether this `InkWell` responds to interaction — see the module
    /// doc's `enabled` section.
    fn is_interactive(&self) -> bool {
        self.on_tap.is_some()
    }
}

impl std::fmt::Debug for InkWell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InkWell")
            .field("on_tap", &self.on_tap.is_some())
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

/// Persistent state behind [`InkWell`] — see [`StatefulView`]/[`ViewState`].
pub struct InkWellState {
    states: WidgetStatesController,
    states_listener: Option<flui_sdk::foundation::Subscription>,
    /// Refreshed every `build()`, so an activation always calls the
    /// *current* handler.
    tap_slot: Rc<RefCell<Option<TapCallback>>>,
    /// The ambient vsync, resolved in `create_state` and again on each
    /// dependency change — `None` means no
    /// `VsyncScope` ancestor, so the press-deactivation delay degrades to
    /// immediate (see the module doc).
    vsync: Option<Vsync>,
    rebuild: RebuildHandle,
    /// The last activation: its hold on `Pressed` and the timer that ends
    /// it, shared with the activation closure and the timer's listener.
    activation: Rc<RefCell<Activation>>,
    /// The state layer and ripple, shared with the pointer callbacks and the
    /// painter.
    indication: Rc<Indication>,
}

impl std::fmt::Debug for InkWellState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InkWellState")
            .field("states", &self.states)
            .field(
                "has_pending_deactivation",
                &self.activation.borrow().timer.is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl InkWellState {
    /// Cancels and disposes any in-flight press-deactivation timer.
    /// Idempotent (a no-op when nothing is pending).
    fn cancel_pending_deactivation(activation: &RefCell<Activation>) {
        let taken = activation.borrow_mut().timer.take();
        drop(taken);
    }

    /// Ends the activation's hold on `Pressed`, if one is live. The slot
    /// borrow ends before the controller's listeners run.
    fn release_press_hold(activation: &RefCell<Activation>) {
        let taken = activation.borrow_mut().hold.take();
        drop(taken);
    }

    /// Listen to the current states controller: rebuild, and move the state
    /// layer, on every change.
    fn listen_to_states(&mut self, rebuild: RebuildHandle) {
        let indication = Rc::downgrade(&self.indication);
        self.states_listener = Some(self.states.listen(Rc::new(move || {
            if let Some(indication) = indication.upgrade() {
                indication.sync();
            }
            rebuild.schedule(flui_sdk::view::RebuildReason::StateChange);
        })));
    }

    /// Read the ambient clock, ripple configuration, theme colour and motion
    /// policy, registering a dependency on each.
    fn read_ambient(&mut self, ctx: &dyn LifecycleContext) {
        self.vsync = VsyncScope::maybe_of(ctx);
        self.indication.bind(self.vsync.clone());
        let configuration = RippleScope::of(ctx);
        let content_color = Theme::color_scheme_of(ctx).map(|scheme| scheme.on_surface);
        let reduce_motion = MediaQuery::motion_of(ctx) == Some(MotionPolicy::Reduce);
        let has_clock = self.vsync.is_some();
        self.indication.update_style(|style| {
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
        InkWellState {
            states,
            states_listener: None,
            tap_slot: Rc::new(RefCell::new(self.on_tap.clone())),
            // Fired later, from the states-controller listener and the
            // press-deactivation timer, never from `build` (ADR-0018).
            rebuild: ctx.rebuild_handle(),
            vsync: VsyncScope::maybe_of(ctx),
            activation: Rc::default(),
            indication,
        }
    }
}

impl ViewState<InkWell> for InkWellState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // The node writes `Disabled` (ADR-0322 §2) when it mounts, inside this
        // well's first build. Seeding the same value here, before the listener
        // exists, keeps that write from notifying the well while it builds.
        // `create_state` seeded `tap_slot` from the initial view, so `enabled`
        // is derivable without one.
        let initially_enabled = self.tap_slot.borrow().is_some();
        self.states
            .update(WidgetState::Disabled, !initially_enabled);

        // From here on every real change of the states set (hover/focus/
        // press/disabled) moves the state layer and rebuilds.
        self.listen_to_states(self.rebuild.clone());

        self.read_ambient(ctx);
        self.indication.set_rebuild(self.rebuild.clone());
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.read_ambient(ctx);
        let pending = self.activation.borrow_mut().timer.take();
        if let Some(mut owner) = pending {
            if let Err(error) = owner.rebind(self.vsync.as_ref()) {
                tracing::error!(%error, "InkWell lost its frame registry");
            }
            let displaced = {
                let slot = &mut self.activation.borrow_mut().timer;
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
            Self::cancel_pending_deactivation(&self.activation);
            Self::release_press_hold(&self.activation);
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
            self.indication.set_states(self.states.clone());
            // A hold under way follows the activation to the new controller.
            if self.activation.borrow().hold.is_some() {
                let moved = self.states.hold_pressed();
                let outgoing = self.activation.borrow_mut().hold.replace(moved);
                drop(outgoing);
            }

            // Seeded before the listener, as in `init_state` (oracle:
            // `initStatesController` runs again on a controller swap,
            // `ink_well.dart` `:943`): the node writes the same value to the
            // controller it is handed during this build.
            self.states
                .update(WidgetState::Disabled, !new_view.is_interactive());

            self.listen_to_states(self.rebuild.clone());
        } else if new_view.is_interactive() != old_view.is_interactive() {
            // The node writes the new `Disabled` during this build; seeded
            // here, that write notifies nobody mid-build.
            self.states
                .update(WidgetState::Disabled, !new_view.is_interactive());
        }

        // A new overlay color or controller retargets the state layer.
        let overlay_color = new_view.overlay_color.clone();
        let focus_layer = new_view.focus_indicator == FocusIndicator::Halo;
        self.indication.update_style(|style| {
            style.overlay_color = overlay_color;
            style.focus_layer = focus_layer;
        });
    }

    fn build(&self, view: &InkWell, _ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = view.is_interactive();
        self.tap_slot.borrow_mut().clone_from(&view.on_tap);

        let rebuild = self.rebuild.clone();

        // One activation for a tap and for the keyboard: the oracle's
        // `activateOnIntent` (`ink_well.dart` `:883-900`) is both what its
        // `ActivateIntent`/`ButtonActivateIntent` actions run and what the
        // tap runs.
        let activate: Option<TapCallback> = enabled.then(|| {
            let tap_slot = Rc::clone(&self.tap_slot);
            let press_states = self.states.clone();
            let vsync = self.vsync.clone();
            let activation = Rc::clone(&self.activation);
            Rc::new(move |ctx: &mut flui_sdk::view::EventCx<'_>| {
                // Oracle order (`ink_well.dart` `activateOnIntent`, `:864-900`):
                // `WidgetState.pressed` is set FIRST, THEN `widget.onTap?.call()`
                // fires, and only after that does the hold start. A handler
                // that reads the states set (e.g. to resolve its own overlay)
                // observes `Pressed`.
                let hold = press_states.hold_pressed();
                let handler = tap_slot.borrow().clone();
                if let Some(handler) = handler {
                    handler(ctx);
                }
                InkWellState::cancel_pending_deactivation(&activation);
                // The new hold is taken before the old one goes, so `Pressed`
                // stays set across a second activation.
                let outgoing = activation.borrow_mut().hold.replace(hold);
                drop(outgoing);
                begin_press_deactivation(&activation, vsync.clone(), &rebuild);
            }) as Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>)>
        });

        // An idle well attaches no painter, so it records nothing; a state
        // change or a press rebuilds it with one.
        let active = self.indication.is_active();
        self.indication.set_painted(active);
        let mut content = CustomPaint::new();
        if active {
            content = content.painter(IndicationPainter {
                indication: Rc::clone(&self.indication),
                shape: view.shape,
            });
        }
        let content = content.child(view.child.clone());

        // `_actionMap` around the focus node (`ink_well.dart` `:852-855`,
        // `:1386-1389`): Enter, Space and Select on the focused well reach
        // these through the root shortcuts (ADR-0079). The actions run inside
        // the key event's dispatch and hand its `ctx` to the activation, the
        // same one a tap's recognizer opens.
        let keyed = Rc::new(KeyActivation {
            activate: activate.clone(),
            indication: Rc::clone(&self.indication),
        });
        // `Focused` follows the primary focus and `FocusVisible` the focus a
        // user can see (ADR-0079): a pointer press that focuses the well
        // leaves no ring, halo or overlay behind. The node writes both, with
        // `Hovered`, `Pressed` and `Disabled`, to the well's controller.
        // Its hit-test behaviour is the opaque mouse region's the well had:
        // hit over its whole box, enabled or not.
        let mut node = InteractiveNode::new(content)
            .behavior(HitTestBehavior::Opaque)
            .enabled(enabled)
            .states(self.states.clone())
            .action::<ActivateIntent, KeyActivation>(&keyed)
            .action::<ButtonActivateIntent, KeyActivation>(&keyed);
        node = match &view.focus_node {
            Some(focus_node) => node.focus_node(Rc::clone(focus_node)),
            None => node.focusable("InkWell"),
        };
        if view.focus_indicator == FocusIndicator::Ring {
            node = node.focus_ring();
        }
        match activate {
            Some(activate) => node.tap(press_feedback(&self.indication, activate)),
            None => node,
        }
    }

    fn dispose(&mut self) {
        drop(self.states_listener.take());
        Self::cancel_pending_deactivation(&self.activation);
        Self::release_press_hold(&self.activation);
        self.indication.dispose();
    }
}

/// What a key activation runs: a wave from the centre, released at once,
/// then the activation. A disabled well's node keeps these bindings declared
/// and switched off, so the intent stops at the well.
struct KeyActivation {
    activate: Option<TapCallback>,
    indication: Rc<Indication>,
}

impl KeyActivation {
    fn run(&self, ctx: &mut flui_sdk::view::EventCx<'_>) -> ActionOutcome {
        let Some(activate) = &self.activate else {
            return ActionOutcome::NotPerformed;
        };
        // A key activation has no contact: its wave grows from the centre
        // and is released at once.
        self.indication.press(None, None);
        self.indication.release(None);
        activate(ctx);
        ActionOutcome::Performed
    }
}

impl Action<ActivateIntent> for KeyActivation {
    fn invoke(
        &self,
        ctx: &mut flui_sdk::view::EventCx<'_>,
        _intent: &ActivateIntent,
    ) -> ActionOutcome {
        self.run(ctx)
    }
}

impl Action<ButtonActivateIntent> for KeyActivation {
    fn invoke(
        &self,
        ctx: &mut flui_sdk::view::EventCx<'_>,
        _intent: &ButtonActivateIntent,
    ) -> ActionOutcome {
        self.run(ctx)
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
fn press_feedback(indication: &Rc<Indication>, activate: TapCallback) -> ControlTap {
    let (down, up, cancel) = (
        Rc::clone(indication),
        Rc::clone(indication),
        Rc::clone(indication),
    );
    let (on_up, on_button_up, on_cancel) = (
        Rc::clone(indication),
        Rc::clone(indication),
        Rc::clone(indication),
    );
    ControlTap::new()
        .on_tap_down(move |_ctx, details| {
            down.press(
                Some(Point::new(
                    details.local_position.dx,
                    details.local_position.dy,
                )),
                Some(details.pointer.get()),
            );
        })
        .on_tap_up(move |_ctx, details| {
            up.release(Some(details.pointer.get()));
        })
        .on_tap(move |ctx| activate(ctx))
        .on_tap_cancel(move |_ctx, details| {
            cancel.cancel(Some(details.pointer.get()));
        })
        // An `Up` is the last held button coming up, so it always ends the
        // press. A button released while another is still held arrives as a
        // `ButtonChange` (through a move): only the primary's ends the press
        // there.
        .on_pointer_up(move |_ctx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::Up(release) = dispatch.local {
                on_up.release(Some(release.pointer.id.get()));
            }
        })
        .on_pointer_move(move |_ctx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::ButtonChange(ButtonChange::Released(release)) = dispatch.local
                && release.button().number().get() == 1
            {
                on_button_up.release(Some(release.pointer.id.get()));
            }
        })
        .on_pointer_cancel(move |_ctx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::Cancel(cancel) = dispatch.local {
                on_cancel.cancel(Some(cancel.pointer.id.get()));
            }
        })
}

/// Starts (or, if one is already in flight, restarts) the
/// [`PRESS_DEACTIVATION_DELAY`] timer that ends the activation's hold on
/// [`WidgetState::Pressed`] —
/// see the module doc's "Press-state timing" section.
///
/// The activation's timer must already be empty (the caller cancels any
/// previous timer
/// first) — this only ever *installs*, never itself cancels.
fn begin_press_deactivation(
    activation: &Rc<RefCell<Activation>>,
    vsync: Option<Vsync>,
    rebuild: &RebuildHandle,
) {
    let Some(vsync) = vsync else {
        // No ambient VsyncScope: no clock to time the delay against. The
        // simplest available degrade is immediate deactivation — documented
        // in the module doc, the same shape `GestureDetector` already uses
        // for its own deadline-driven gestures without a binding.
        InkWellState::release_press_hold(activation);
        return;
    };

    let owner = AnimationController::builder(PRESS_DEACTIVATION_DELAY)
        .debug_label("InkWell.press")
        .behavior(flui_sdk::animation::AnimationBehavior::Preserve)
        .build_on(Some(&vsync));
    let controller = owner.controller().clone();

    // Completion changes the pressed state. The next press withdraws and
    // disposes the pending timer through cancel_pending_deactivation.
    let activation_for_listener = Rc::downgrade(activation);
    let rebuild_for_listener = rebuild.clone();
    // This callback lives until its owned timer is canceled or retired.
    controller
        .subscribe_status(std::rc::Rc::new(move |status| {
            if status == AnimationStatus::ForwardSettled {
                if let Some(activation) = activation_for_listener.upgrade() {
                    InkWellState::release_press_hold(&activation);
                }
                rebuild_for_listener.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }
        }))
        .detach();

    if let Err(error) = controller.forward_from(Some(0.0)) {
        tracing::debug!(?error, "InkWell press-deactivation timer failed to start");
        InkWellState::release_press_hold(activation);
        return;
    }

    let outgoing = activation.borrow_mut().timer.replace(owner);
    drop(outgoing);
}
