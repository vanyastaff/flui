//! [`InteractiveViewerState`]: the mounted viewer's element state, its build and
//! its event handlers, and the zoom keys.

use std::rc::Rc;

use flui_animation::{DrivenController, Vsync};
use flui_foundation::geometry::{Axis, Offset, Vec2};
use flui_foundation::panic::FirstPanic;
use flui_interaction::focus::{FocusNode, FocusNodeRegistration};
use flui_interaction::gesture::{GestureEndReason, Velocity};
use flui_interaction::gesture::{
    ScaleEndDetails, ScaleStartDetails, ScaleStartMode, ScaleUpdateDetails,
};
use flui_interaction::pointer::EventPropagation;
use flui_interaction::pointer::HitTestBehavior;
use flui_interaction::{events::PointerEvent, pointer::WheelPreferencesProvider};
use flui_platform::TargetPlatform;
use flui_platform::input::pointer::TouchpadHoldPhase;
use flui_platform::{
    input::keyboard::Modifiers,
    input::pointer::{PanZoomEvent, PanZoomPhase, ScrollEvent},
};
use flui_rendering::pipeline::PipelineCell;
use flui_scheduler::PostFrameHandle;
use flui_view::prelude::*;
use flui_view::{BoxedView, IntoView, ViewExt, ViewState};

use super::InteractiveViewer;
use super::alternatives::{AlternativesMount, PanAction, ZoomAction};
use super::boundary::{align_to_axis, dominant_axis, gesture_transform, uniform_scale};
use super::fling::Motion;
use super::leaf::ViewerCallbacks;
use super::semantics_node::viewer_semantics;
use super::types::{
    InteractionEndDetails, InteractionStartDetails, InteractionUpdateDetails, PanAxis,
    WheelScaleGate, ZoomIntent,
};
use super::viewer_core::ViewerCore;
use super::zoom_gestures::{double_tap, wheel_zoom};
use crate::anchored_box::AnchoredBox;
use crate::scroll::nested::{NestedScrollScope, ScrollDevice, ScrollEndReason};
use crate::{
    Actions, AnimatedBuilder, ClipRect, Focus, GestureDetector, Listener, Shortcuts,
    SingleActivator, Transform, TransformSlot,
};
use flui_view::environment::{GestureArenaScope, VsyncScope};

/// Persistent state for [`InteractiveViewer`].
pub struct InteractiveViewerState {
    pub(super) core: Rc<ViewerCore>,
    /// The node the arrow keys reach this viewer through, from a focused row
    /// inside it.
    pub(super) keys: Rc<FocusNode>,
    /// Ends the keyboard sequence a held arrow began when the key is
    /// cancelled instead of released here.
    pub(super) _key_cancel: FocusNodeRegistration,
    pub(super) writer: WriterSource,
    pub(super) post_frame: Option<PostFrameHandle>,
    pub(super) controller: DrivenController,
    /// Identity only: the driven controller owns the registry seat.
    pub(super) clock: Option<Vsync>,
    pub(super) wheel_preferences: Option<WheelPreferencesProvider>,
}

impl std::fmt::Debug for InteractiveViewerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractiveViewerState")
            .field("config", &self.core.config.get())
            .finish_non_exhaustive()
    }
}

impl ViewState<InteractiveViewer> for InteractiveViewerState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        *self.core.pipeline.borrow_mut() = ctx.pipeline_owner();
        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = ctx.pipeline_owner();
        let unchanged = match (&*self.core.pipeline.borrow(), &incoming) {
            (None, None) => true,
            (Some(current), Some(incoming)) => current.ptr_eq(incoming),
            _ => false,
        };
        if !unchanged {
            self.core.fling.stop(ScrollEndReason::Cancelled);
        }
        *self.core.pipeline.borrow_mut() = incoming;
        self.post_frame = ctx.post_frame_handle();
        self.writer = ctx.writer_source();
        let leaf = &self.core.leaf;
        *leaf.parent.borrow_mut() = NestedScrollScope::parent_of(ctx);
        leaf.plane.set_pipeline(
            self.core
                .pipeline
                .borrow()
                .as_ref()
                .map(PipelineCell::downgrade),
        );
        leaf.sequences.bind(GestureArenaScope::of(ctx));
        *leaf.settings.borrow_mut() = GestureArenaScope::settings_or_default_of(ctx);
        self.wheel_preferences = Some(GestureArenaScope::wheel_preferences_of(ctx));
        self.bind_vsync(ctx);
    }

    fn did_update_view(&mut self, old: &InteractiveViewer, view: &InteractiveViewer) {
        let core = &self.core;
        let incoming = view.config;
        let outgoing = core.config.replace(incoming);
        let callbacks = core.callbacks.replace(ViewerCallbacks::of(view));
        drop(callbacks);
        let controller = core.controller.replace(view.controller.clone());
        drop(controller);
        if !old.controller.ptr_eq(&view.controller)
            || outgoing.boundary_margin != incoming.boundary_margin
            || outgoing.pan_enabled != incoming.pan_enabled
            || outgoing.pan_axis != incoming.pan_axis
            || outgoing.min_scale != incoming.min_scale
            || outgoing.max_scale != incoming.max_scale
            || outgoing.rotation_enabled != incoming.rotation_enabled
            || old.alignment != view.alignment
        {
            core.fling.stop(ScrollEndReason::Cancelled);
        }
    }

    fn dispose(&mut self) {
        self.core.fling.close();
        self.core.leaf.retire();
        self.controller.dispose();
    }

    fn build(&self, view: &InteractiveViewer, ctx: &dyn BuildContext) -> impl IntoView {
        let core = Rc::clone(&self.core);
        let content = self.content(view);
        let mount = AlternativesMount {
            alternatives: view.alternatives.clone(),
            core: Rc::clone(&core),
            content,
        };
        let key_core = Rc::clone(&core);
        let keys = Focus::new(mount)
            .focus_node(Rc::clone(&self.keys))
            .can_request_focus(false)
            .on_key_event(move |_ctx, event| key_core.key_pan(event));
        let mut shortcuts = Shortcuts::new(keys);
        for (activator, intent) in zoom_keys(flui_view::environment::PlatformConvention::of(ctx)) {
            shortcuts = shortcuts.shortcut(activator, intent);
        }
        Actions::new(shortcuts)
            .action(ZoomAction(Rc::clone(&core)))
            .action(PanAction(core))
    }
}

/// The keys that zoom a focused viewer on `platform`: `+` (or `=`, its
/// unshifted key) and `-`, alone or with the platform's command key. The
/// collision table checks them on every platform.
pub(crate) fn zoom_keys(platform: TargetPlatform) -> Vec<(SingleActivator, ZoomIntent)> {
    let apple = matches!(platform, TargetPlatform::MacOS | TargetPlatform::iOS);
    let command = |activator: SingleActivator| {
        if apple {
            activator.meta()
        } else {
            activator.control()
        }
    };
    let mut keys = Vec::new();
    for (character, intent) in [
        ("+", ZoomIntent::In),
        ("=", ZoomIntent::In),
        ("-", ZoomIntent::Out),
    ] {
        keys.push((SingleActivator::character_ignoring_shift(character), intent));
        keys.push((
            command(SingleActivator::character_ignoring_shift(character)),
            intent,
        ));
    }
    keys
}

impl InteractiveViewerState {
    /// The transformed, clipped and recognized child, rebuilt whenever the
    /// transform changes.
    #[expect(
        clippy::too_many_lines,
        reason = "one gesture-wiring closure set; splitting it fragments the capture set"
    )]
    pub(super) fn content(&self, view: &InteractiveViewer) -> BoxedView {
        let core = Rc::clone(&self.core);
        let post_frame = self.post_frame.clone();
        let writer = self.writer.clone();
        let wheel_preferences = self.wheel_preferences.clone();
        let clip_behavior = view.clip_behavior;
        let alignment = view.alignment;
        let child = view.child.clone();
        let listenable = core.controller().listenable();

        AnimatedBuilder::new(listenable, move || {
            let config = core.config.get();
            let matrix = core.controller().value();

            // -- Contacts and the native pinch (one transform slot) --------
            let start_core = Rc::clone(&core);
            let on_start = move |ctx: &mut EventContext<'_>, details: ScaleStartDetails| {
                let core = &start_core;
                // A gesture that catches a snap-back continues from where the
                // scene shows.
                core.fling.stop(ScrollEndReason::Preempted);
                core.catch_stretch();
                core.gesture.reset();
                core.gesture
                    .pan_start_local
                    .set(Some(details.local_focal_point));
                let native = details.pointer_count == 0;
                core.gesture.native.set(native);
                if !native {
                    core.leaf.begin_contact();
                    // A contact gives the viewer the keys, as a drag gives a
                    // scrollable its own.
                    let row = core.row_focus.borrow().clone();
                    if let Some(row) = row {
                        let _ = row.request_focus();
                    }
                }
                if let Some(callback) = core.callbacks().start {
                    callback(
                        ctx,
                        InteractionStartDetails {
                            focal_point: details.focal_point,
                            local_focal_point: details.local_focal_point,
                        },
                    );
                }
            };

            let update_core = Rc::clone(&core);
            let on_update = move |ctx: &mut EventContext<'_>, details: ScaleUpdateDetails| {
                let core = &update_core;
                let config = core.config.get();
                let before = core.controller().value();
                let gesture = &core.gesture;
                // In logarithms: the ratio of two extreme cumulative factors overflows.
                let log_ratio = details.scale.ln() - gesture.scale.replace(details.scale).ln();
                let rotation = details.rotation - gesture.rotation.replace(details.rotation);
                let delta = details.focal_point_delta;
                if config.pan_axis == PanAxis::Aligned
                    && gesture.current_axis.get().is_none()
                    && let Some(start_local) = gesture.pan_start_local.get()
                {
                    let total = details.local_focal_point - start_local;
                    if total != Offset::ZERO {
                        gesture.current_axis.set(Some(dominant_axis(total)));
                    }
                }
                let locked = core.lock(delta);
                // Scale and rotate about the focal point where it was; the
                // focal motion follows as a pan.
                let log_ratio = if config.scale_enabled && log_ratio.is_finite() {
                    log_ratio
                } else {
                    0.0
                };
                let rotation = if config.rotation_enabled {
                    rotation
                } else {
                    0.0
                };
                if log_ratio != 0.0 || rotation != 0.0 {
                    core.pinch(details.local_focal_point - delta, log_ratio, rotation);
                }
                if gesture.native.get() {
                    if config.pan_enabled {
                        let _ = core.translate(locked, true);
                    }
                } else if delta != Offset::ZERO {
                    let _ = core.leaf.contact_step(
                        Vec2::new(-delta.dx, -delta.dy),
                        details.local_focal_point,
                        |offer| core.consume(offer, core.leaf.contact_holds()),
                    );
                }
                if let Some(callback) = core.callbacks().update {
                    callback(
                        ctx,
                        InteractionUpdateDetails {
                            focal_point: details.focal_point,
                            local_focal_point: details.local_focal_point,
                            scale: uniform_scale(&core.controller().value())
                                / uniform_scale(&before),
                            focal_point_delta: if config.pan_enabled {
                                locked
                            } else {
                                Offset::ZERO
                            },
                        },
                    );
                }
            };

            let end_core = Rc::clone(&core);
            let end_post_frame = post_frame.clone();
            let on_end = move |ctx: &mut EventContext<'_>, details: ScaleEndDetails| {
                let core = &end_core;
                let config = core.config.get();
                let velocity = core.lock(details.focal_fling_velocity().pixels_per_second);
                core.gesture.reset();
                let native = core.gesture.native.replace(false);
                let overlay = core.overlay.get();
                // A participant that fails at the release still leaves the
                // sequence to the run below, which owns its end; the failure
                // resumes after it.
                let mut failure = FirstPanic::new();
                let sequence = if native {
                    None
                } else {
                    core.leaf
                        .release(Vec2::new(-velocity.dx, -velocity.dy), &mut failure)
                };
                failure.contain(|| {
                    if !overlay.is_rest() {
                        // Pulled past the bounds: the scene comes back, and no
                        // inertia runs from outside them.
                        core.fling.snap_back(
                            core,
                            Motion::Release(overlay),
                            sequence.map(|(sequence, _)| sequence),
                            end_post_frame.clone(),
                        );
                    } else if native {
                        if config.pan_enabled {
                            core.fling
                                .start(core, velocity, None, end_post_frame.clone());
                        }
                    } else if let Some((sequence, held)) = sequence {
                        let own = Offset::new(
                            if held[0] { velocity.dx } else { 0.0 },
                            if held[1] { velocity.dy } else { 0.0 },
                        );
                        let own = if config.pan_enabled {
                            own
                        } else {
                            Offset::ZERO
                        };
                        core.fling
                            .start(core, own, Some(sequence), end_post_frame.clone());
                    }
                });
                if let Some(callback) = core.callbacks().end {
                    failure.contain(|| {
                        callback(
                            ctx,
                            InteractionEndDetails {
                                reason: GestureEndReason::Completed,
                                velocity: details.focal_velocity,
                                scale_velocity: details.velocity,
                            },
                        );
                    });
                }
                failure.resume();
            };

            let cancel_core = Rc::clone(&core);
            let cancel_post_frame = post_frame.clone();
            let on_cancel = move |ctx: &mut EventContext<'_>| {
                let core = &cancel_core;
                core.fling.stop(ScrollEndReason::Cancelled);
                core.gesture.reset();
                core.gesture.native.set(false);
                core.leaf.cancel();
                let overlay = core.overlay.get();
                if !overlay.is_rest() {
                    core.fling.snap_back(
                        core,
                        Motion::Release(overlay),
                        None,
                        cancel_post_frame.clone(),
                    );
                }
                if let Some(callback) = core.callbacks().end {
                    callback(
                        ctx,
                        InteractionEndDetails {
                            reason: GestureEndReason::Cancelled,
                            velocity: Velocity::ZERO,
                            scale_velocity: 0.0,
                        },
                    );
                }
            };

            // Reject an idle native claimant before it can mutate its actor.
            // Once admitted, GestureDetector owns the source through terminal.
            let admission_core = Rc::clone(&core);
            let native_admission = move |event: &PanZoomEvent, translation: Offset<f64>| {
                let core = &admission_core;
                let config = core.config.get();
                let PanZoomPhase::Update(transform) = event.phase else {
                    return false;
                };
                let Some((viewport, boundary)) = core.geometry() else {
                    return false;
                };
                // Translation is the focal point moving since Start
                // (ADR-0217 §4).
                let point = event.position.get();
                let delta = if config.pan_enabled {
                    match config.pan_axis {
                        PanAxis::Horizontal => align_to_axis(translation, Axis::Horizontal),
                        PanAxis::Vertical => align_to_axis(translation, Axis::Vertical),
                        PanAxis::Aligned if translation != Offset::ZERO => {
                            align_to_axis(translation, dominant_axis(translation))
                        }
                        _ => translation,
                    }
                } else {
                    Offset::ZERO
                };
                let focal = Offset::new(point.x, point.y);
                let before = core.base();
                gesture_transform(
                    before,
                    focal,
                    delta,
                    if config.scale_enabled {
                        transform.scale()
                    } else {
                        1.0
                    },
                    if config.rotation_enabled {
                        transform.rotation()
                    } else {
                        0.0
                    },
                    config.min_scale,
                    config.max_scale,
                    viewport,
                    boundary,
                )
                .is_some_and(|next| next != before)
            };

            // -- Wheel (Listener::on_scroll_claim) -------------------------
            //
            // The viewer goes through the arbitrated claim walk rather than
            // acting on the raw pointer signal: acting directly would make a
            // viewer nested in a scrollable both zoom and scroll on one wheel
            // tick. When the tick will actually zoom or pan, the viewer
            // claims it and the outer scrollable stays still.
            let wheel_core = Rc::clone(&core);
            let wheel_writer = writer.clone();
            let wheel_preferences = wheel_preferences.clone();
            let scroll_claim = move |data: &ScrollEvent| {
                wheel_writer.write(|ctx| {
                    let core = &wheel_core;
                    if core.config.get().wheel_scale_gate == WheelScaleGate::CtrlWheel
                        && !data.modifiers.contains(Modifiers::CONTROL)
                    {
                        let panned = core.wheel_pan(data, wheel_preferences.as_ref());
                        core.leaf.wheel_latch();
                        return if panned {
                            EventPropagation::Stop
                        } else {
                            EventPropagation::Continue
                        };
                    }
                    wheel_zoom(core, ctx, data)
                })
            };

            let mut transform = Transform::new(matrix);
            if let Some(inner_child) = child.clone().into_inner() {
                transform = transform.child(AnchoredBox::new(core.anchor.clone(), inner_child));
            }
            if let Some(alignment) = alignment {
                transform = transform.alignment(alignment);
            }
            let clipped = ClipRect::new()
                .clip_behavior(clip_behavior)
                .child(transform);

            let mut recognized = GestureDetector::new()
                .behavior(HitTestBehavior::Opaque)
                .transform(
                    TransformSlot::new()
                        .start_mode(ScaleStartMode::PanOrScale)
                        .native_admission(native_admission)
                        .on_start(on_start)
                        .on_update(on_update)
                        .on_end(on_end)
                        .on_cancel(on_cancel),
                );
            if config.double_tap_zoom {
                let hint_core = Rc::clone(&core);
                let tap_core = Rc::clone(&core);
                let zoom_core = Rc::clone(&core);
                recognized = recognized
                    .on_double_tap_down(move |_ctx, details| {
                        hint_core.focal_hint.set(Some(details.local_position));
                    })
                    .on_double_tap(move |ctx| double_tap(&tap_core, ctx))
                    // A trackpad's smart zoom is the same gesture.
                    .on_smart_zoom(move |ctx, details| {
                        zoom_core.focal_hint.set(Some(details.local_position));
                        double_tap(&zoom_core, ctx);
                    });
            }
            let recognized = recognized.child(clipped);

            let down_core = Rc::clone(&core);
            let listener = Listener::new()
                .on_pointer_down(move |_, dispatch| {
                    if let PointerEvent::Down(press) = dispatch.local {
                        down_core.leaf.device.set(Some(ScrollDevice::Pointer {
                            kind: press.pointer.kind,
                            device: press.pointer.device,
                        }));
                    }
                    down_core.fling.stop_friction();
                })
                // Fingers resting on a touchpad stop the inertia, as a press does.
                .on_pointer_signal({
                    let core = Rc::clone(&core);
                    move |_, dispatch| {
                        if let PointerEvent::TouchpadHold(hold) = dispatch.local
                            && hold.phase == TouchpadHoldPhase::Begin
                        {
                            core.fling.stop_friction();
                        }
                    }
                })
                .on_scroll_claim(scroll_claim)
                .child(recognized);
            viewer_semantics(&core, &config, &matrix)
                .child(AnchoredBox::new(core.leaf.plane.anchor(), listener))
        })
        .boxed()
    }

    pub(super) fn bind_vsync(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = VsyncScope::maybe_of(ctx);
        if VsyncScope::same_registry(self.clock.as_ref(), incoming.as_ref()) {
            return;
        }
        self.clock = incoming;
        let fling = &self.core.fling;
        fling.clock_bound.set(false);
        fling.stop(ScrollEndReason::Cancelled);
        let rebound = self.controller.rebind(self.clock.as_ref());
        fling.clock_bound.set(self.controller.is_bound());
        if let Err(error) = rebound {
            tracing::error!(%error, "InteractiveViewer lost its frame registry");
        }
    }
}
