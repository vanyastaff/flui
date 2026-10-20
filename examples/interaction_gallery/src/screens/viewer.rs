//! An `InteractiveViewer` over a painted colour grid: pinch, Ctrl+wheel or
//! double-tap to zoom, drag to pan, or the Zoom in and Zoom out buttons.

use std::rc::Rc;

use flui::prelude::*;
use flui::widgets::{
    Alternative, AnimatedBuilder, ControlSpec, GestureAlternatives, GestureCommand,
    InteractiveViewer, TransformationController, WheelScaleGate, ZoomIntent,
};

use super::page;
use crate::painters::{GridPainter, grid_size};
use crate::style;

/// How far in and out the viewer goes.
const MIN_SCALE: f64 = 0.5;
const MAX_SCALE: f64 = 4.0;

#[derive(Clone, StatefulView)]
pub(crate) struct ViewerScreen;

pub(crate) struct ViewerState {
    transform: TransformationController,
}

impl StatefulView for ViewerScreen {
    type State = ViewerState;

    fn create_state(&self) -> Self::State {
        ViewerState {
            transform: TransformationController::new(),
        }
    }
}

impl ViewState<ViewerScreen> for ViewerState {
    fn build(&self, _view: &ViewerScreen, _ctx: &dyn BuildContext) -> impl IntoView {
        let alternatives = GestureAlternatives::new(Alternative::VisibleControl(
            ControlSpec::new(
                GestureCommand::new("Zoom in", ZoomIntent::In),
                Padding::symmetric(12.0, 8.0).child(style::tinted("Zoom in", style::ACCENT)),
            )
            .control(
                GestureCommand::new("Zoom out", ZoomIntent::Out),
                Padding::symmetric(12.0, 8.0).child(style::tinted("Zoom out", style::ACCENT)),
            ),
        ))
        .expect("BUG: the two zoom commands have distinct labels")
        .target("Colour grid");
        let viewer = InteractiveViewer::new(alternatives)
            .controller(self.transform.clone())
            .min_scale(MIN_SCALE)
            .max_scale(MAX_SCALE)
            .wheel_scale_gate(WheelScaleGate::CtrlWheel)
            .child(
                CustomPaint::new()
                    .size(grid_size())
                    .painter(Rc::new(GridPainter)),
            );

        let transform = self.transform.clone();
        let readout = AnimatedBuilder::new(self.transform.listenable(), move || {
            let matrix = transform.value();
            let m = matrix.to_col_major_array();
            let scale = m[0].hypot(m[1]);
            let (x, y, _) = matrix.translation_component();
            style::readout(format!(
                "Zoom {}%, offset {} by {} px",
                (scale * 100.0).round(),
                x.round(),
                y.round()
            ))
        });
        page(
            "Zoom and pan",
            "Pinch, Ctrl+wheel or double-tap the grid to zoom, and drag it to pan. With the grid \
             focused, + and - zoom and the arrow keys pan.",
            readout,
            ClipRRect::circular(12.0).child(viewer),
        )
    }
}
