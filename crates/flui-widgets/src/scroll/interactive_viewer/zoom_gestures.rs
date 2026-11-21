//! The pointer-driven zoom paths: the double tap and the wheel tick.

use flui_foundation::geometry::Offset;
use flui_interaction::gesture::{GestureEndReason, Velocity};
use flui_interaction::pointer::EventPropagation;
use flui_platform::input::pointer::{ScrollEvent, ScrollUnit};
use flui_view::prelude::*;

use super::boundary::{clamp_scale, clamp_translation};
use super::types::{InteractionEndDetails, InteractionStartDetails, InteractionUpdateDetails};
use super::viewer_core::ViewerCore;

/// A double tap: the first command about the tapped point, or, at the
/// maximum scale, back out to the minimum.
pub(super) fn double_tap(core: &ViewerCore, ctx: &mut EventContext<'_>) {
    let config = core.config.get();
    let at_max = core.scale() >= config.max_scale * (1.0 - 1e-9);
    if at_max {
        let focal = core.focal_hint.take();
        core.interrupt();
        let _ = core.zoom_by(config.min_scale / core.scale(), focal);
        return;
    }
    let command = core.command.borrow().clone();
    if let Some(command) = command {
        let _ = command.perform(ctx);
    }
    core.focal_hint.set(None);
}

/// A wheel tick that zooms about the pointer.
pub(super) fn wheel_zoom(
    core: &ViewerCore,
    ctx: &mut EventContext<'_>,
    data: &ScrollEvent,
) -> EventPropagation {
    let config = core.config.get();
    let geometry = core.geometry();
    let pixels_per_unit = match data.delta.unit() {
        ScrollUnit::Pixels => 1.0,
        // Raw rotation and already normalized lines both retain the authored
        // zoom step. Scroll preferences govern translation, independently of
        // the zoom divisor.
        ScrollUnit::Detents | ScrollUnit::Lines => 53.0,
        ScrollUnit::Pages => {
            let Some((viewport, _)) = geometry else {
                return EventPropagation::Continue;
            };
            let height = viewport.height();
            if !height.is_finite() || height <= 0.0 {
                return EventPropagation::Continue;
            }
            height
        }
        _ => return EventPropagation::Continue,
    };
    let delta = data.delta.y() * pixels_per_unit;
    if delta == 0.0 || !delta.is_finite() {
        // Ignore horizontal-only wheel scroll.
        return EventPropagation::Continue;
    }

    let position = data.position.get();
    let position = Offset::new(position.x, position.y);
    let scale_change = (-delta / config.scale_factor).exp();
    if !scale_change.is_finite() || scale_change <= 0.0 {
        return EventPropagation::Continue;
    }
    core.interrupt();
    let callbacks = core.callbacks();
    if let Some(callback) = &callbacks.start {
        callback(
            ctx,
            InteractionStartDetails {
                focal_point: position,
                local_focal_point: position,
            },
        );
    }

    let controller = core.controller();
    let value_before_zoom = controller.value();
    if config.scale_enabled
        && let Some((viewport, boundary)) = geometry
    {
        let scene_before = controller.to_scene(position);
        let scaled = clamp_scale(
            controller.value(),
            scale_change,
            config.min_scale,
            config.max_scale,
            viewport,
            boundary,
        );
        controller.set_value(scaled);

        // Keep the same scene point under the cursor before and after the
        // scale.
        let scene_after = controller.to_scene(position);
        let correction = Offset::new(
            scene_after.dx - scene_before.dx,
            scene_after.dy - scene_before.dy,
        );
        let translated = clamp_translation(controller.value(), correction, viewport, boundary);
        controller.set_value(translated);
    }

    if let Some(callback) = &callbacks.update {
        callback(
            ctx,
            InteractionUpdateDetails {
                focal_point: position,
                local_focal_point: position,
                scale: scale_change,
                focal_point_delta: Offset::ZERO,
            },
        );
    }
    if let Some(callback) = &callbacks.end {
        callback(
            ctx,
            InteractionEndDetails {
                reason: GestureEndReason::Completed,
                velocity: Velocity::ZERO,
                scale_velocity: 0.0,
            },
        );
    }
    // Claim only when the viewer actually zoomed — the same shape as the
    // scrollable's can-move predicate. Scaling disabled, or a zoom the
    // boundary/min/max clamp collapsed to a no-op (e.g. zoom-out at identity
    // with a zero boundary margin), leaves the tick to an enclosing
    // scrollable; the interaction callbacks above still observed it even
    // then.
    if controller.value().m == value_before_zoom.m {
        EventPropagation::Continue
    } else {
        EventPropagation::Stop
    }
}

/// `moved` limited to `[0, wanted]` (or `[wanted, 0]`): what a boundary
/// correction adds beyond or against a step is not the step's.
pub(super) fn toward(moved: f64, wanted: f64) -> f64 {
    if !moved.is_finite() || wanted == 0.0 {
        0.0
    } else if wanted > 0.0 {
        moved.clamp(0.0, wanted)
    } else {
        moved.clamp(wanted, 0.0)
    }
}
