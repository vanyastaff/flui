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
//! # `enabled` derivation (`isInteractive`)
//!
//! The oracle's `enabled` is `true` when ANY of six tap-family callbacks is
//! set (`isWidgetEnabled`, `ink_well.dart` `:1292-1303`) — `onTap`,
//! `onDoubleTap`, `onLongPress`, `onLongPressUp`, `onTapUp`, `onTapDown` (plus
//! a parallel secondary-button check). This substrate wires only
//! [`InkWell::on_tap`] via `GestureDetector`'s own `on_tap`, so `enabled`
//! reduces to `on_tap.is_some()`. When
//! disabled: [`WidgetState::Disabled`] is asserted in the states set, the
//! `GestureDetector` built has no `on_tap` closure at all (so it never
//! resolves any gesture and "swallows nothing": its
//! default [`flui_sdk::widgets::HitTestBehavior::DeferToChild`] lets an
//! unclaimed pointer contact fall through to whatever is behind it), hover
//! stops updating [`WidgetState::Hovered`] (oracle: `handleMouseEnter`
//! gates `handleHoverChange` on `enabled`), and focus is not
//! request-able (`Focus::can_request_focus(false)`).
//!
//! # Press-state timing: the `_activationDuration` shape, applied uniformly
//!
//! The oracle uses two different pressed-deactivation paths:
//! `handleTap`/`handleTapCancel` clear [`WidgetState::Pressed`]
//! **immediately** on a real pointer up/cancel (the down→up duration itself
//! is the visible "pressed" window), while `activateOnIntent` — the
//! keyboard/`Intent`-driven activation path with no real down/up pair —
//! delays the clear by `_activationDuration` (100ms, `ink_well.dart` `:864`,
//! `:897`) specifically so a synthetic, instantaneous activation still
//! *shows* a pressed state for a moment.
//!
//! This substrate wires [`GestureDetector::on_tap`] only — a callback that
//! fires once the gesture is *recognized* (a tap: down +
//! up without exceeding touch slop), with no separate down/up primitives to
//! hang two different callbacks on. Architecturally that is the oracle's
//! *synthetic*-activation shape, not its real-pointer shape — there is no
//! "hold duration" for [`WidgetState::Pressed`] to ride on. So `InkWell`
//! applies the oracle's `_activationDuration` mechanism uniformly: on
//! `on_tap`, `Pressed` is set immediately, then cleared after 100ms via a
//! one-shot [`flui_sdk::animation::AnimationController`] registered on the
//! ambient [`flui_sdk::widgets::animated::VsyncScope`] — "the simplest owner-side
//! timer available" that is already exercised elsewhere in this workspace
//! (`flui-widgets::animated`'s `ImplicitController`), rather than inventing
//! a new timer primitive. Standalone (no `VsyncScope` above this `InkWell`):
//! there is no clock to time the delay against, so `Pressed` is set and
//! immediately cleared with no visible window — a documented degradation,
//! the same shape `GestureDetector` itself already uses for its own
//! deadline-driven gestures without a binding.
//!
//! The ripple itself follows the pointer contact rather than this timer: a
//! wave starts on the primary-button down and fades out on its up or cancel,
//! or at once when the tap recognizer gives the contact up
//! ([`GestureDetector::on_tap_cancel`]: past the slop, or lost the arena).
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
use flui_sdk::view::prelude::*;
use flui_sdk::widgets::animated::VsyncScope;
use flui_sdk::widgets::{
    Actions, ActivateIntent, ButtonActivateIntent, ButtonChange, CallbackAction, CustomPaint,
    Focus, GestureDetector, Listener, MediaQuery, MouseRegion, PointerDispatch, PointerEvent,
    WidgetState, WidgetStateProperty, WidgetStatesController,
};

use crate::ripple::{Indication, IndicationPainter, IndicationStyle, RippleScope};
use crate::shape::MaterialShape;
use crate::theme::Theme;

/// A user tap handler. `Rc`-based (owner-local, per ADR-0027) — matches
/// `GestureDetector::on_tap`'s own callback shape.
type TapCallback = Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>)>;

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

/// Persistent state behind [`InkWell`] — see [`StatefulView`]/[`ViewState`].
pub struct InkWellState {
    states: WidgetStatesController,
    states_listener: Option<flui_sdk::foundation::ListenerId>,
    /// Refreshed every `build()` so the tap closure (built once, reused
    /// across rebuilds via `GestureDetector`'s own slot-refresh — see
    /// `build`) always calls the *current* handler, matching
    /// `GestureDetectorState::tap_slot`'s pattern.
    tap_slot: Rc<RefCell<Option<TapCallback>>>,
    /// The ambient vsync, resolved once in `init_state` — `None` means no
    /// `VsyncScope` ancestor, so the press-deactivation delay degrades to
    /// immediate (see the module doc).
    vsync: Option<Vsync>,
    /// `Some` once `init_state` has run — always the case by `build`.
    rebuild: Option<RebuildHandle>,
    pending_deactivation: Rc<RefCell<Option<DrivenController>>>,
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
                &self.pending_deactivation.borrow().is_some(),
            )
            .finish_non_exhaustive()
    }
}

impl InkWellState {
    /// Cancels and disposes any in-flight press-deactivation timer.
    /// Idempotent (a no-op when nothing is pending).
    fn cancel_pending_deactivation(pending: &Rc<RefCell<Option<DrivenController>>>) {
        let taken = pending.borrow_mut().take();
        drop(taken);
    }

    /// Listen to the current states controller: rebuild, and move the state
    /// layer, on every change.
    fn listen_to_states(&mut self, rebuild: RebuildHandle) {
        let indication = Rc::downgrade(&self.indication);
        self.states_listener = Some(self.states.add_listener(Rc::new(move || {
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

    fn create_state(&self) -> Self::State {
        let states = self.states_controller.clone().unwrap_or_default();
        let indication = Indication::new(
            states.clone(),
            IndicationStyle {
                overlay_color: self.overlay_color.clone(),
                configuration: None,
                content_color: None,
                reduce_motion: false,
                has_clock: false,
            },
        );
        InkWellState {
            states,
            states_listener: None,
            tap_slot: Rc::new(RefCell::new(self.on_tap.clone())),
            vsync: None,
            rebuild: None,
            pending_deactivation: Rc::new(RefCell::new(None)),
            indication,
        }
    }
}

impl ViewState<InkWell> for InkWellState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        // ADR-0018: `rebuild_handle()` is acquired here, fired later (from
        // the states-controller listener and the press-deactivation status
        // listener below) — never called from `build`.
        let rebuild = ctx.rebuild_handle();

        // Sync `Disabled` from the widget's `enabled` BEFORE the first build
        // (never inside `build`: mutating a possibly-caller-shared
        // controller and notifying its listeners synchronously mid-build is
        // exactly the "rebuild during build" hazard the lifecycle exists to
        // prevent; `did_update_view` below handles every later transition)
        // — AND before adding the listener. That order is load
        // bearing, not incidental: this very first sync almost always IS a
        // real change (a freshly-constructed controller starts at
        // `WidgetStates::NONE`, so an initially-disabled `InkWell` — the
        // common case, since `on_tap` is often set after construction —
        // flips `Disabled` on), and updating before a listener exists means
        // that one certain transition notifies nobody and schedules no
        // spurious rebuild for an element that hasn't even had its first
        // build yet; the identical update after `did_update_view` re-homes
        // a controller is a genuine, listener-visible change instead (see
        // below). `init_state` has no `view` parameter, but `create_state`
        // already seeded `tap_slot` from the initial view, so `enabled` is
        // derivable from it without one.
        let initially_enabled = self.tap_slot.borrow().is_some();
        self.states
            .update(WidgetState::Disabled, !initially_enabled);

        // From here on every real change of the states set (hover/focus/
        // press/disabled) moves the state layer and rebuilds.
        self.listen_to_states(rebuild.clone());

        self.read_ambient(ctx);
        self.indication.set_rebuild(rebuild.clone());
        self.rebuild = Some(rebuild);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.read_ambient(ctx);
        let pending = self.pending_deactivation.borrow_mut().take();
        if let Some(mut owner) = pending {
            if let Err(error) = owner.rebind(self.vsync.as_ref()) {
                tracing::error!(%error, "InkWell lost its frame registry");
            }
            let displaced = {
                let mut slot = self.pending_deactivation.borrow_mut();
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
            if let Some(id) = self.states_listener.take() {
                self.states.remove_listener(id);
            }

            self.states = new_view.states_controller.clone().unwrap_or_default();
            self.indication.set_states(self.states.clone());

            // Same order as `init_state`, same reason: sync BEFORE
            // `add_listener` so re-homing onto a controller that hasn't
            // seen this `InkWell`'s `enabled` yet (oracle: `initStatesController`
            // runs again on a controller swap, `ink_well.dart` `:943`)
            // doesn't self-notify a listener that was only just attached.
            self.states
                .update(WidgetState::Disabled, !new_view.is_interactive());

            let rebuild = self
                .rebuild
                .clone()
                .expect("BUG: init_state runs before the first did_update_view");
            self.listen_to_states(rebuild);
        } else if new_view.is_interactive() != old_view.is_interactive() {
            // The controller didn't change, but `enabled` did, so the
            // existing controller needs a resync. Still never from `build`.
            self.states
                .update(WidgetState::Disabled, !new_view.is_interactive());
        }

        // A new overlay color or controller retargets the state layer.
        let overlay_color = new_view.overlay_color.clone();
        self.indication
            .update_style(|style| style.overlay_color = overlay_color);
    }

    fn build(&self, view: &InkWell, _ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = view.is_interactive();
        self.tap_slot.borrow_mut().clone_from(&view.on_tap);

        let rebuild = self
            .rebuild
            .clone()
            .expect("init_state runs before the first build");

        let mut gesture_detector = GestureDetector::new();
        // One activation for a tap and for the keyboard: the oracle's
        // `activateOnIntent` (`ink_well.dart` `:883-900`) is both what its
        // `ActivateIntent`/`ButtonActivateIntent` actions run and the shape
        // this substrate's single `on_tap` callback already has.
        let activate: Option<TapCallback> = enabled.then(|| {
            let tap_slot = Rc::clone(&self.tap_slot);
            let press_states = self.states.clone();
            let vsync = self.vsync.clone();
            let pending_deactivation = Rc::clone(&self.pending_deactivation);
            Rc::new(move |cx: &mut flui_sdk::view::EventCx<'_>| {
                // Oracle order (`ink_well.dart` `activateOnIntent`, `:864-900`
                // — the synthetic/no-real-down-up activation path this
                // substrate's single `on_tap` callback architecturally
                // matches, per the module doc's "Press-state timing"
                // section): `_startNewSplash` sets `WidgetState.pressed`
                // true FIRST, THEN `widget.onTap?.call()` fires, and only
                // after that does the delayed-clear `_activationTimer`
                // start. A handler that reads the states set (e.g. to
                // resolve its own overlay) must observe `Pressed` — the
                // oracle guarantees it is already set by the time `onTap`
                // runs, so it is set here before `handler(cx)`, not after.
                press_states.update(WidgetState::Pressed, true);
                if let Some(handler) = tap_slot.borrow().clone() {
                    handler(cx);
                }
                InkWellState::cancel_pending_deactivation(&pending_deactivation);
                begin_press_deactivation(
                    &pending_deactivation,
                    &press_states,
                    vsync.clone(),
                    &rebuild,
                );
            }) as Rc<dyn Fn(&mut flui_sdk::view::EventCx<'_>)>
        });
        if let Some(activate) = &activate {
            let activate = Rc::clone(activate);
            let (down, up, cancel) = (
                Rc::clone(&self.indication),
                Rc::clone(&self.indication),
                Rc::clone(&self.indication),
            );
            // The wave follows the press the tap recognizer shows: it starts
            // at the down (at once when no rival holds the contact, else after
            // the press timeout or at the win) and ends with that contact's up
            // or cancel. A contact that was never shown as pressed started no
            // wave and ends none.
            gesture_detector = gesture_detector
                .on_tap_down(move |_cx, details| {
                    down.press(
                        Some(Point::new(
                            details.local_position.dx,
                            details.local_position.dy,
                        )),
                        Some(details.pointer.get()),
                    );
                })
                .on_tap_up(move |_cx, details| {
                    up.release(Some(details.pointer.get()));
                })
                .on_tap(move |cx| activate(cx))
                .on_tap_cancel(move |_cx, details| {
                    cancel.cancel(Some(details.pointer.get()));
                });
        }

        let hover_states_enter = self.states.clone();
        let hover_states_exit = self.states.clone();
        let mouse_region = MouseRegion::new()
            // Hover state changes on the structural MouseRegion enter/exit
            // transitions. FLUI's presentation-owned MouseTracker also re-hit-tests stationary
            // devices after layout, so a widget appearing beneath an
            // unmoved pointer takes the same path as a physical pointer move.
            .on_enter(move |_cx, _device, _position| {
                if enabled {
                    hover_states_enter.update(WidgetState::Hovered, true);
                }
            })
            .on_exit(move |_cx, _device, _position| {
                hover_states_exit.update(WidgetState::Hovered, false);
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
        let focus_states = self.states.clone();
        let mut focus = Focus::new(content)
            .can_request_focus(enabled)
            .on_focus_change(move |_cx, has_focus| {
                focus_states.update(WidgetState::Focused, has_focus);
            });
        if let Some(node) = &view.focus_node {
            focus = focus.focus_node(Rc::clone(node));
        }
        // `_actionMap` around the `Focus` (`ink_well.dart` `:852-855`,
        // `:1386-1389`): Enter, Space and Select on the focused well reach
        // these through the root shortcuts (ADR-0079). A disabled well
        // declares neither, so the keys keep bubbling. The actions run inside
        // the key event's dispatch and hand its `cx` to the activation, the
        // same one a tap's recognizer opens.
        let mut actions = Actions::new(focus);
        if let Some(activate) = &activate {
            // A key activation has no contact: its wave grows from the
            // centre and is released at once.
            let keyed = {
                let activate = Rc::clone(activate);
                let indication = Rc::clone(&self.indication);
                Rc::new(move |cx: &mut flui_sdk::view::EventCx<'_>| {
                    indication.press(None, None);
                    indication.release(None);
                    activate(cx);
                })
            };
            let on_activate = Rc::clone(&keyed);
            let on_button_activate = keyed;
            actions = actions
                .action(CallbackAction::new(move |cx, _: &ActivateIntent| {
                    on_activate(cx);
                }))
                .action(CallbackAction::new(move |cx, _: &ButtonActivateIntent| {
                    on_button_activate(cx);
                }));
        }

        // GestureDetector wraps MouseRegion wraps Focus wraps the content —
        // outermost to innermost. `GestureDetector` must be OUTERMOST: with
        // it nested inside `MouseRegion` instead, `RenderMouseRegion`'s own
        // `hit_test` (`crates/flui-objects/src/interaction/mouse_region.rs`)
        // returns `hit_target && self.opaque`, and under `Opaque` behavior
        // (both widgets' default) that return value is `true` regardless of
        // whether the inner `GestureDetector`'s `Listener` was hit —
        // confirmed empirically (a synthetic down+up recognized zero taps
        // through `MouseRegion(GestureDetector(child))`, but the identical
        // event pair recognized one tap through `GestureDetector(MouseRegion(child))`).
        // This is a divergence from the oracle's own `Focus > MouseRegion >
        // GestureDetector > CustomPaint` layering (there, the raw gesture
        // listener is innermost) — driven by this render pipeline's
        // hit-test contract, not a stylistic choice.
        //
        // The ripple's contact listener sits between the two: it defers to
        // the opaque `MouseRegion`, so it sees every contact on the well, and
        // it only observes, so recognition is untouched.
        let surface = mouse_region.child(actions);
        if enabled {
            gesture_detector.child(contact_listener(&self.indication).child(surface))
        } else {
            gesture_detector.child(surface)
        }
    }

    fn dispose(&mut self) {
        if let Some(id) = self.states_listener.take() {
            self.states.remove_listener(id);
        }
        Self::cancel_pending_deactivation(&self.pending_deactivation);
        self.indication.dispose();
    }
}

/// Ends a wave with the raw end of its contact: the last button's up, the
/// primary button's release under another held button, or the pointer's
/// cancel. The wave itself starts with the tap recognizer's down, which also
/// ends it when the contact is lost to a rival.
fn contact_listener(indication: &Rc<Indication>) -> Listener {
    let on_up = Rc::clone(indication);
    let on_button_up = Rc::clone(indication);
    let on_cancel = Rc::clone(indication);
    Listener::new()
        // An `Up` is the last held button coming up, so it always ends the
        // press. A button released while another is still held arrives as a
        // `ButtonChange` (through `on_pointer_move`): only the primary's ends
        // the press there.
        .on_pointer_up(move |_cx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::Up(release) = dispatch.local {
                on_up.release(Some(release.pointer.id.get()));
            }
        })
        .on_pointer_move(move |_cx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::ButtonChange(ButtonChange::Released(release)) = dispatch.local
                && release.button().number().get() == 1
            {
                on_button_up.release(Some(release.pointer.id.get()));
            }
        })
        .on_pointer_cancel(move |_cx, dispatch: PointerDispatch<'_>| {
            if let PointerEvent::Cancel(cancel) = dispatch.local {
                on_cancel.cancel(Some(cancel.pointer.id.get()));
            }
        })
}

/// Starts (or, if one is already in flight, restarts) the
/// [`PRESS_DEACTIVATION_DELAY`] timer that clears [`WidgetState::Pressed`] —
/// see the module doc's "Press-state timing" section.
///
/// `pending` must already be empty (the caller cancels any previous timer
/// first) — this only ever *installs*, never itself cancels.
fn begin_press_deactivation(
    pending: &Rc<RefCell<Option<DrivenController>>>,
    states: &WidgetStatesController,
    vsync: Option<Vsync>,
    rebuild: &RebuildHandle,
) {
    let Some(vsync) = vsync else {
        // No ambient VsyncScope: no clock to time the delay against. The
        // simplest available degrade is immediate deactivation — documented
        // in the module doc, the same shape `GestureDetector` already uses
        // for its own deadline-driven gestures without a binding.
        states.update(WidgetState::Pressed, false);
        return;
    };

    let owner = AnimationController::builder(PRESS_DEACTIVATION_DELAY)
        .debug_label("InkWell.press")
        .behavior(flui_sdk::animation::AnimationBehavior::Preserve)
        .build_on(Some(&vsync));
    let controller = owner.controller().clone();

    // Completion changes the pressed state. The next press withdraws and
    // disposes the pending timer through cancel_pending_deactivation.
    let states_for_listener = states.clone();
    let rebuild_for_listener = rebuild.clone();
    // This callback lives until its owned timer is canceled or retired.
    controller
        .subscribe_status(std::rc::Rc::new(move |status| {
            if status == AnimationStatus::ForwardSettled {
                states_for_listener.update(WidgetState::Pressed, false);
                rebuild_for_listener.schedule(flui_sdk::view::RebuildReason::AnimationTick);
            }
        }))
        .detach();

    if let Err(error) = controller.forward_from(Some(0.0)) {
        tracing::debug!(?error, "InkWell press-deactivation timer failed to start");
        states.update(WidgetState::Pressed, false);
        return;
    }

    let outgoing = pending.borrow_mut().replace(owner);
    drop(outgoing);
}
