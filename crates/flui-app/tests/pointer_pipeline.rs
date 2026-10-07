//! Backend vocabulary conversion reaches the production widget input path.

use flui_painting::styling::Color;
use flui_platform::shared::input_vocabulary::pointer_event;
use flui_platform_api::{
    EventTime,
    pointer::{PanZoomPhase, PointerEvent, PointerKind},
};
use flui_testing::widgets::{lay_out, tight};
use flui_widgets::{ColoredBox, InteractiveViewer, TransformationController};
use ui_events::pointer as upstream;

fn legacy_backend_pinch_ticks_reach_the_viewer_as_independent_steps() {
    let controller = TransformationController::new();
    let laid = lay_out(
        InteractiveViewer::new()
            .controller(controller.clone())
            .child(ColoredBox::new(Color::rgb(10, 20, 30))),
        tight(100.0, 100.0),
    );
    for time in [10, 20] {
        let mut state = upstream::PointerState::default();
        state.time = time;
        state.position.x = 50.0;
        state.position.y = 50.0;
        let native_translation = upstream::PointerEvent::Gesture(upstream::PointerGestureEvent {
            pointer: upstream::PointerInfo {
                pointer_id: upstream::PointerId::new(u64::MAX),
                pointer_type: upstream::PointerType::Touch,
                persistent_device_id: None,
            },
            gesture: upstream::PointerGesture::Pinch(0.1),
            state,
        });
        // This is the existing public production bridge called by the backends,
        // not a second converter implemented by the test.
        let event = pointer_event(&native_translation, EventTime::from_nanos(time))
            .expect("finite backend translation");
        let PointerEvent::PanZoom(pan_zoom) = &event else {
            panic!("pinch translation must produce pan zoom");
        };
        assert_eq!(pan_zoom.pointer().kind, PointerKind::Trackpad);
        let PanZoomPhase::Update(transform) = pan_zoom.phase else {
            panic!("legacy backend produces an isolated Update without Start");
        };
        assert!((transform.scale() - 1.1).abs() < 1e-7);
        laid.dispatch_pointer_event(&event);
    }
    let scale = controller.value().to_col_major_array()[0];
    // Upstream pinch fractions are f32, so allow their representation error.
    // Two independent ten-percent steps compose; they do not form two readings
    // of the same cumulative 1.1 transform.
    assert!(
        (scale - 1.21).abs() < 1e-7,
        "two backend pinch ticks: {scale}"
    );
}

#[test]
fn backend_pointer_pipeline_contract() {
    crate::run_table(
        "backend_pointer_pipeline_contract",
        &[(
            "legacy_backend_pinch_ticks_reach_the_viewer_as_independent_steps",
            legacy_backend_pinch_ticks_reach_the_viewer_as_independent_steps,
        )],
    );
}
