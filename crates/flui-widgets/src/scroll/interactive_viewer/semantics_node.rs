//! The viewer's semantics node.

use std::rc::Rc;

use flui_foundation::geometry::{Matrix4, Vec2};
use flui_semantics::{NumericRange, ScrollUnit as SemanticsScrollUnit};
use flui_view::prelude::*;

use super::boundary::uniform_scale;
use super::types::ZoomIntent;
use super::viewer_core::ViewerCore;
use super::{ViewerConfig, ZOOM_STEP};
use crate::Semantics;

/// The viewer's semantics node: its scale as a numeric range that
/// increment, decrement and set-value adjust, and the scroll actions that pan.
pub(super) fn viewer_semantics(
    core: &Rc<ViewerCore>,
    config: &ViewerConfig,
    matrix: &Matrix4,
) -> Semantics {
    let scale = uniform_scale(matrix);
    let percent = if scale.is_finite() {
        (scale * 100.0).round()
    } else {
        0.0
    };
    let zoom = |intent: ZoomIntent| {
        let core = Rc::clone(core);
        move |_ctx: &mut EventContext<'_>| {
            let _ = core.zoom_step(intent);
        }
    };
    let pan = |direction: Vec2| {
        let core = Rc::clone(core);
        move |_ctx: &mut EventContext<'_>, unit: SemanticsScrollUnit| {
            core.assistive_pan(direction, unit);
        }
    };
    let mut semantics = Semantics::new()
        .value(format!("{percent}%"))
        .on_increment(zoom(ZoomIntent::In))
        .on_decrement(zoom(ZoomIntent::Out))
        .on_scroll_left(pan(Vec2::new(-1.0, 0.0)))
        .on_scroll_right(pan(Vec2::new(1.0, 0.0)))
        .on_scroll_up(pan(Vec2::new(0.0, -1.0)))
        .on_scroll_down(pan(Vec2::new(0.0, 1.0)));
    let (min, max) = (config.min_scale, config.max_scale);
    if min <= max
        && let Ok(range) = NumericRange::try_new(
            scale.clamp(min, max),
            min,
            max,
            (scale * (ZOOM_STEP - 1.0)).max(f64::MIN_POSITIVE),
        )
    {
        let set_core = Rc::clone(core);
        semantics =
            semantics
                .numeric_range(range)
                .on_set_numeric_value(move |_ctx, target: f64| {
                    let current = set_core.scale();
                    set_core.interrupt();
                    let _ = set_core.zoom_by(target / current, None);
                });
    }
    semantics
}
