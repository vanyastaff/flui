//! [`InkWell`] — an M3 state-overlay surface: paints a single resolved
//! overlay color that tracks hover/focus/press, in place of Flutter's ink
//! splash/highlight feature list.
//!
//! # Flutter parity
//!
//! `material/ink_well.dart`'s `InkResponse`/`InkWell` (oracle tag `3.44.0`).
//! `InkWell` extends `InkResponse` with `containedInkWell: true` and a
//! rectangular `highlightShape` — a distinction that only matters to the
//! oracle's ink-feature clipping, which this substrate doesn't have (see
//! below). FLUI ships one type covering both.
//!
//! # Scope: overlay only, no ink-feature registry
//!
//! The oracle paints hover/focus/press as three independently-faded
//! `InkHighlight` features plus splash/ripple `InkFeature`s registered on
//! the ancestor `Material`'s `_RenderInkFeatures`. **This substrate has no
//! feature registry** (see `material.rs`'s module doc) — `InkWell` instead
//! resolves ONE color from `overlay_color` against its current
//! [`WidgetStates`](flui_widgets::WidgetStates) and paints a single shape-clipped local fill
//! (`crate::material::Material` at `elevation: 0`, reused rather than
//! duplicating clip/fill paint code). Consequences:
//!
//! - **No hardcoded opacities.** The oracle's 8%/10%/10% hover/focus/press
//!   defaults live in per-component `_TokenDefaults` (M3 spec tables) that
//!   arrive with a future button family's styles, not here. An `InkWell`
//!   with no `overlay_color` configured paints nothing beyond its child.
//! - **`None` resolution = no overlay layer at all**, not a fallback color.
//!   The oracle falls back through `overlayColor?.resolve(states) ??
//!   highlightColor/focusColor/hoverColor ?? Theme.of(context).<field>`
//!   (`ink_well.dart` `updateHighlight`, `:1035-1046`) — FLUI's `ThemeData`
//!   has no `hoverColor`/`focusColor`/`highlightColor` fields yet to fall
//!   back to, so the chain stops at `None`. Named divergence, revisited
//!   when those theme fields land.
//! - **No ripple/splash.** M3's real splash is the `InkSparkle` shader,
//!   well outside this substrate's scope; this `InkWell` has no
//!   press-triggered ripple animation, just the static resolved
//!   [`WidgetState::Pressed`] fill.
//! - **No cross-widget bleed.** Because there is no ink-feature registry, an
//!   overlay can never paint outside its own `InkWell`'s bounds — see
//!   `material.rs` for the upgrade path.
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
//! resolves any gesture and — Flutter parity — "swallows nothing": its
//! default [`flui_widgets::HitTestBehavior::DeferToChild`] lets an
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
//! fires once the gesture is *recognized* (Flutter parity for a tap: down +
//! up without exceeding touch slop), with no separate down/up primitives to
//! hang two different callbacks on. Architecturally that is the oracle's
//! *synthetic*-activation shape, not its real-pointer shape — there is no
//! "hold duration" for [`WidgetState::Pressed`] to ride on. So `InkWell`
//! applies the oracle's `_activationDuration` mechanism uniformly: on
//! `on_tap`, `Pressed` is set immediately, then cleared after 100ms via a
//! one-shot [`flui_animation::AnimationController`] registered on the
//! ambient [`flui_widgets::animated::VsyncScope`] — "the simplest owner-side
//! timer available" that is already exercised elsewhere in this workspace
//! (`flui-widgets::animated`'s `ImplicitController`), rather than inventing
//! a new timer primitive. Standalone (no `VsyncScope` above this `InkWell`):
//! there is no clock to time the delay against, so `Pressed` is set and
//! immediately cleared with no visible window — a documented degradation,
//! the same shape `GestureDetector` itself already uses for its own
//! deadline-driven gestures without a binding.
//!
//! Full 50ms/200ms highlight fade curves (`getFadeDurationForType`,
//! `ink_well.dart` `:995-1002`) are a named deferral — V1's overlay snaps on
//! and off, no interpolation.
//!
//! # `WidgetStatesController` integration
//!
//! [`InkWell::states_controller`] accepts an external controller (shared
//! with a caller that also wants to read/drive the same state set); absent
//! one, the state owns a private [`WidgetStatesController`] — Flutter
//! parity: `initStatesController`/`MaterialStatesController`.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{
    Animation, AnimationController, AnimationStatus, Scheduler, Vsync, VsyncRegistration,
};
use flui_foundation::Listenable;
use flui_interaction::routing::FocusNode;
use flui_types::Color;
use flui_view::RebuildHandle;
use flui_view::prelude::*;
use flui_widgets::animated::VsyncScope;
use flui_widgets::{
    Focus, GestureDetector, MouseRegion, WidgetState, WidgetStateProperty, WidgetStatesController,
};

use crate::material::Material;
use crate::shape::MaterialShape;

/// A user tap handler. `Rc`-based (owner-local, per ADR-0027) — matches
/// `GestureDetector::on_tap`'s own callback shape.
type TapCallback = Rc<dyn Fn()>;

/// Flutter's `_InkResponseState._activationDuration` — see the module doc's
/// "Press-state timing" section for why this substrate applies it
/// uniformly rather than only on synthetic activation.
const PRESS_DEACTIVATION_DELAY: Duration = Duration::from_millis(100);

/// An M3 interactive-state overlay over a child — see the module docs for
/// what this V1 paints (a single resolved color) and does not (splashes,
/// ripples, cross-widget ink bleed).
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
            child: BoxedView(Box::new(child.into_view())),
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
    pub fn on_tap(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_tap = Some(Rc::new(callback));
        self
    }

    /// Sets the state-resolved overlay color. `None` for a given state set
    /// paints no overlay layer at all — see the module doc.
    #[must_use]
    pub fn overlay_color(mut self, overlay_color: WidgetStateProperty<Option<Color>>) -> Self {
        self.overlay_color = overlay_color;
        self
    }

    /// Sets the shape the overlay fill (and its hit-test bounds) is clipped
    /// to. Defaults to a plain rectangle.
    #[must_use]
    pub fn shape(mut self, shape: MaterialShape) -> Self {
        self.shape = shape;
        self
    }

    /// Shares an external [`WidgetStatesController`] instead of the private
    /// one this widget otherwise owns — Flutter parity:
    /// `InkWell.statesController`.
    #[must_use]
    pub fn states_controller(mut self, controller: WidgetStatesController) -> Self {
        self.states_controller = Some(controller);
        self
    }

    /// Drives an external [`FocusNode`] instead of a widget-owned one —
    /// Flutter parity: `InkWell.focusNode`. Exposed primarily so a caller
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

/// One in-flight press-deactivation timer: the controller, the [`Vsync`] it
/// is registered with, and that registration's id — all three are needed to
/// unregister cleanly (see [`InkWellState::cancel_pending_deactivation`]).
struct PendingDeactivation {
    controller: AnimationController,
    vsync: Vsync,
    registration: VsyncRegistration,
}

/// Persistent state behind [`InkWell`] — see [`StatefulView`]/[`ViewState`].
pub struct InkWellState {
    states: WidgetStatesController,
    states_listener: Option<flui_foundation::ListenerId>,
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
    pending_deactivation: Rc<RefCell<Option<PendingDeactivation>>>,
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
    fn cancel_pending_deactivation(pending: &Rc<RefCell<Option<PendingDeactivation>>>) {
        if let Some(previous) = pending.borrow_mut().take() {
            previous.vsync.unregister(previous.registration);
            previous.controller.dispose();
        }
    }
}

impl StatefulView for InkWell {
    type State = InkWellState;

    fn create_state(&self) -> Self::State {
        InkWellState {
            states: self.states_controller.clone().unwrap_or_default(),
            states_listener: None,
            tap_slot: Rc::new(RefCell::new(self.on_tap.clone())),
            vsync: None,
            rebuild: None,
            pending_deactivation: Rc::new(RefCell::new(None)),
        }
    }
}

impl ViewState<InkWell> for InkWellState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // ADR-0018: `rebuild_handle()` is acquired here, fired later (from
        // the states-controller listener and the press-deactivation status
        // listener below) — never called from `build`.
        let rebuild = ctx.rebuild_handle();

        // Flutter parity: `initStatesController` syncs `Disabled` from the
        // widget's `enabled` BEFORE the first build (never inside `build`
        // — the oracle's own doc: mutating a possibly-caller-shared
        // controller and notifying its listeners synchronously mid-build is
        // exactly the "setState during build" hazard Flutter's lifecycle
        // exists to prevent; `did_update_view` below handles every later
        // transition) — AND before `addListener`
        // (`statesController.update(...)` precedes
        // `statesController.addListener(handleStatesControllerChange)` in
        // the oracle body, `ink_well.dart` `:923-924`). That order is load
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

        // A rebuild is needed whenever the states set actually changes
        // (hover/focus/press/disabled) from here on, so the overlay color
        // is re-resolved. Mirrors `_InkResponseState.initStatesController`'s
        // `statesController.addListener(handleStatesControllerChange)`,
        // whose Flutter body is `setState(() {})`.
        let rebuild_for_listener = rebuild.clone();
        self.states_listener = Some(self.states.add_listener(Arc::new(move || {
            rebuild_for_listener.schedule(flui_view::RebuildReason::StateChange);
        })));

        self.vsync = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone());
        self.rebuild = Some(rebuild);
    }

    fn did_update_view(&mut self, old_view: &InkWell, new_view: &InkWell) {
        // Flutter parity: `didUpdateWidget` re-homes the states controller
        // when the caller swaps in a genuinely different one —
        // `widget.statesController != oldWidget.statesController`
        // (`ink_well.dart` `:938-940`). A rebuild that re-clones the SAME
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
                .expect("init_state runs before the first did_update_view");
            self.states_listener = Some(self.states.add_listener(Arc::new(move || {
                rebuild.schedule(flui_view::RebuildReason::StateChange);
            })));
        } else if new_view.is_interactive() != old_view.is_interactive() {
            // Flutter parity: `didUpdateWidget`'s `if (enabled !=
            // isWidgetEnabled(oldWidget))` branch — the controller didn't
            // change, but `enabled` did, so the existing controller needs a
            // resync (`ink_well.dart` `:963-964`). Still never from `build`.
            self.states
                .update(WidgetState::Disabled, !new_view.is_interactive());
        }
    }

    fn build(&self, view: &InkWell, _ctx: &dyn BuildContext) -> impl IntoView {
        let enabled = view.is_interactive();
        self.tap_slot.borrow_mut().clone_from(&view.on_tap);

        let resolved_overlay = view.overlay_color.resolve(&self.states.value());

        let rebuild = self
            .rebuild
            .clone()
            .expect("init_state runs before the first build");

        let mut gesture_detector = GestureDetector::new();
        if enabled {
            let tap_slot = Rc::clone(&self.tap_slot);
            let press_states = self.states.clone();
            let vsync = self.vsync.clone();
            let pending_deactivation = Rc::clone(&self.pending_deactivation);
            gesture_detector = gesture_detector.on_tap(move || {
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
                // runs, so it is set here before `handler()`, not after.
                press_states.update(WidgetState::Pressed, true);
                if let Some(handler) = tap_slot.borrow().clone() {
                    handler();
                }
                InkWellState::cancel_pending_deactivation(&pending_deactivation);
                begin_press_deactivation(
                    &pending_deactivation,
                    &press_states,
                    vsync.clone(),
                    &rebuild,
                );
            });
        }

        let hover_states_enter = self.states.clone();
        let hover_states_exit = self.states.clone();
        let mouse_region = MouseRegion::new()
            // Flutter 3.44 `_InkResponseState` changes hover state on the
            // structural MouseRegion enter/exit transitions. FLUI's
            // presentation-owned MouseTracker also re-hit-tests stationary
            // devices after layout, so a widget appearing beneath an
            // unmoved pointer takes the same path as a physical pointer move.
            .on_enter(move |_device, _position| {
                if enabled {
                    hover_states_enter.update(WidgetState::Hovered, true);
                }
            })
            .on_exit(move |_device, _position| {
                hover_states_exit.update(WidgetState::Hovered, false);
            });

        let focus_states = self.states.clone();
        let mut focus = Focus::new(overlay_content(view, resolved_overlay))
            .can_request_focus(enabled)
            .on_focus_change(move |has_focus| {
                focus_states.update(WidgetState::Focused, has_focus);
            });
        if let Some(node) = &view.focus_node {
            focus = focus.focus_node(Rc::clone(node));
        }

        // GestureDetector wraps MouseRegion wraps Focus wraps the content —
        // outermost to innermost. `GestureDetector` must be OUTERMOST: with
        // it nested inside `MouseRegion` instead, `RenderMouseRegion`'s own
        // `hit_test` (`crates/flui-objects/src/proxy/mouse_region.rs`)
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
        gesture_detector.child(mouse_region.child(focus))
    }

    fn dispose(&mut self) {
        if let Some(id) = self.states_listener.take() {
            self.states.remove_listener(id);
        }
        Self::cancel_pending_deactivation(&self.pending_deactivation);
    }
}

/// The content `Focus` wraps: the child alone when there is no overlay to
/// paint, or the child under a flat (`elevation: 0`, hence shadow-free), own
/// `view.shape`-clipped [`Material`] fill of `color`.
fn overlay_content(view: &InkWell, resolved_overlay: Option<Color>) -> BoxedView {
    match resolved_overlay {
        Some(color) => BoxedView(Box::new(
            Material::new(color)
                .shape(view.shape)
                .clip_behavior(flui_types::painting::Clip::AntiAlias)
                .child(view.child.clone()),
        )),
        None => view.child.clone(),
    }
}

/// Starts (or, if one is already in flight, restarts) the
/// [`PRESS_DEACTIVATION_DELAY`] timer that clears [`WidgetState::Pressed`] —
/// see the module doc's "Press-state timing" section.
///
/// `pending` must already be empty (the caller cancels any previous timer
/// first) — this only ever *installs*, never itself cancels.
fn begin_press_deactivation(
    pending: &Rc<RefCell<Option<PendingDeactivation>>>,
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

    let controller = AnimationController::new(PRESS_DEACTIVATION_DELAY, Arc::new(Scheduler::new()));
    let registration = vsync.register(controller.clone());

    // The status listener only needs to be `Send + Sync` (its bound), so it
    // captures the `Send + Sync` states controller and rebuild handle by
    // value — NOT `pending` (an owner-local `Rc<RefCell<_>>`, deliberately
    // left untouched here; the next press's `cancel_pending_deactivation`
    // call disposes this controller then, which is safe to call on an
    // already-completed controller since `AnimationController::dispose` is
    // idempotent).
    let states_for_listener = states.clone();
    let rebuild_for_listener = rebuild.clone();
    controller.add_status_listener(Arc::new(move |status| {
        if status == AnimationStatus::Completed {
            states_for_listener.update(WidgetState::Pressed, false);
            rebuild_for_listener.schedule(flui_view::RebuildReason::AnimationTick);
        }
    }));

    if let Err(error) = controller.forward_from(Some(0.0)) {
        tracing::debug!(?error, "InkWell press-deactivation timer failed to start");
        vsync.unregister(registration);
        states.update(WidgetState::Pressed, false);
        return;
    }

    *pending.borrow_mut() = Some(PendingDeactivation {
        controller,
        vsync,
        registration,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_interactive_reflects_whether_on_tap_is_set() {
        assert!(!InkWell::new(flui_widgets::SizedBox::shrink()).is_interactive());
        assert!(
            InkWell::new(flui_widgets::SizedBox::shrink())
                .on_tap(|| {})
                .is_interactive()
        );
    }

    #[test]
    fn debug_reports_whether_on_tap_is_set_without_the_closure() {
        let debug = format!(
            "{:?}",
            InkWell::new(flui_widgets::SizedBox::shrink()).on_tap(|| {})
        );
        assert!(debug.contains("on_tap: true"));
    }

    #[test]
    fn overlay_content_is_the_bare_child_when_resolution_is_none() {
        use flui_view::View;

        // Mutation-honest: if `overlay_content` stopped checking
        // `resolved_overlay` and always wrapped in `Material`, this test's
        // `None` case would compare a `Material` view-type id instead of
        // `SizedBox`'s and fail.
        let view = InkWell::new(flui_widgets::SizedBox::shrink());
        let content = overlay_content(&view, None);
        assert_eq!(
            content.view_type_id(),
            flui_widgets::SizedBox::shrink().view_type_id()
        );
    }

    #[test]
    fn overlay_content_wraps_in_material_when_resolution_is_some() {
        use flui_view::View;

        let view = InkWell::new(flui_widgets::SizedBox::shrink());
        let content = overlay_content(&view, Some(Color::rgb(1, 2, 3)));
        assert_eq!(
            content.view_type_id(),
            Material::new(Color::BLACK).view_type_id()
        );
    }
}
