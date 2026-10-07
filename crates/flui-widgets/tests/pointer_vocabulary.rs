//! Owned pointer vocabulary contracts through the production widget pipeline.

use std::{cell::RefCell, rc::Rc};

use crate::common::{lay_out, tight};
use flui_foundation::geometry::{EdgeInsets, Offset, Point};
use flui_interaction::routing::{EventPropagation, HitTestBehavior};
use flui_platform_api::{
    EventTime,
    keyboard::Modifiers,
    pointer::{
        DeviceId, PanZoomEvent, PanZoomPhase, PanZoomTransform, PointerButton, PointerButtons,
        PointerEvent, PointerId, PointerInfo, PointerKind, PointerMove, PointerPosition,
        PointerPress, PointerRelease, PointerRole, PointerSample, ScrollDelta, ScrollEvent,
        ScrollPhase, ScrollPrecision, ScrollUnit,
    },
};
use flui_view::ViewExt;
use flui_widgets::{Listener, ScrollController, Scrollable, SingleChildScrollView, SizedBox};

fn viewer(
    controller: flui_widgets::TransformationController,
    scales: Rc<RefCell<Vec<f64>>>,
) -> flui_widgets::InteractiveViewer {
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, InteractiveViewer};
    InteractiveViewer::new()
        .controller(controller)
        .boundary_margin(EdgeInsets::all(1000.0))
        .on_interaction_update(move |_, details| scales.borrow_mut().push(details.scale))
        .child(ColoredBox::new(Color::rgb(10, 20, 30)))
}

fn pan_zoom(phase: PanZoomPhase) -> PointerEvent {
    PointerEvent::PanZoom(PanZoomEvent::new(
        mouse(),
        EventTime::from_nanos(70),
        position(50.0, 50.0),
        phase,
    ))
}

fn zoom_update(scale: f64) -> PointerEvent {
    pan_zoom(PanZoomPhase::Update(
        PanZoomTransform::try_new(Offset::ZERO, scale, 0.0).expect("positive finite scale"),
    ))
}

fn scale_of(controller: &flui_widgets::TransformationController) -> f64 {
    controller.value().to_col_major_array()[0]
}

fn assert_scale(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "scale {actual}, expected {expected}"
    );
}

fn mouse() -> PointerInfo {
    PointerInfo::new(
        PointerId::try_from(7_u64).expect("valid pointer"),
        PointerKind::Mouse,
    )
    .with_device(DeviceId::try_from(11_u64).expect("valid device"))
    .with_role(PointerRole::Primary)
}

fn position(x: f64, y: f64) -> PointerPosition {
    PointerPosition::try_new(Point::new(x, y)).expect("finite position")
}

pub(crate) fn scroll_claim_preserves_owned_source_units_and_phase() {
    let observed = Rc::new(RefCell::new(Vec::<ScrollEvent>::new()));
    let sink = observed.clone();
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_scroll_claim(move |event| {
                // A claim receives the platform vocabulary itself, not a second
                // structure that silently discards device identity or units.
                let event: &ScrollEvent = event;
                sink.borrow_mut().push(*event);
                EventPropagation::Stop
            })
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    for unit in [ScrollUnit::Pixels, ScrollUnit::Lines, ScrollUnit::Pages] {
        let event = ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(42),
            position(20.0, 30.0),
            ScrollDelta::try_new(unit, 1.25, -0.5).expect("finite delta"),
        )
        .with_precision(ScrollPrecision::Precise)
        .with_phase(ScrollPhase::MomentumChanged)
        .with_modifiers(Modifiers::SHIFT);
        laid.dispatch_pointer_event(&PointerEvent::Scroll(event));
        assert_eq!(observed.borrow().last(), Some(&event), "{unit:?}");
    }
    let terminal = ScrollEvent::new(
        mouse(),
        EventTime::from_nanos(43),
        position(20.0, 30.0),
        ScrollDelta::zero(ScrollUnit::Pages),
    )
    .with_phase(ScrollPhase::MomentumEnded);
    laid.dispatch_pointer_event(&PointerEvent::Scroll(terminal));
    assert_eq!(
        observed.borrow().last(),
        Some(&terminal),
        "zero delta still carries phase"
    );
}

pub(crate) fn pointer_delivery_preserves_source_and_sample_families() {
    let observed = Rc::new(RefCell::new(Vec::<PointerMove>::new()));
    let sink = observed.clone();
    let laid = lay_out(
        Listener::new()
            .behavior(HitTestBehavior::Opaque)
            .on_pointer_move(move |_, dispatch| {
                if let PointerEvent::Move(event) = dispatch.local {
                    sink.borrow_mut().push(event.clone());
                }
            })
            .child(SizedBox::new(100.0, 100.0)),
        tight(100.0, 100.0),
    );
    let sample = |time, x| PointerSample::new(EventTime::from_nanos(time), position(x, 30.0));
    let down = PointerPress::new(
        mouse(),
        PointerButton::PRIMARY,
        PointerButtons::NONE,
        sample(10, 10.0),
    );
    laid.dispatch_pointer_event(&PointerEvent::Down(down));
    let held = PointerButtons::NONE.with(PointerButton::PRIMARY);
    let event = PointerMove::new(mouse(), held, sample(30, 30.0))
        .with_coalesced(vec![sample(20, 20.0)])
        .with_predicted(vec![sample(40, 40.0)])
        .with_modifiers(Modifiers::CONTROL);
    laid.dispatch_pointer_event(&PointerEvent::Move(event.clone()));
    // Terminal dispatch drains pending Move through GestureBinding.
    let up = PointerRelease::new(mouse(), PointerButton::PRIMARY, held, sample(50, 50.0));
    laid.dispatch_pointer_event(&PointerEvent::Up(up));
    assert_eq!(observed.borrow().as_slice(), &[event]);
}

pub(crate) fn page_scroll_resolves_against_the_actual_viewport() {
    for height in [100.0, 275.0] {
        let controller = ScrollController::new();
        let laid = lay_out(
            Scrollable::new()
                .controller(controller.clone())
                .viewport_builder(Rc::new(|position| {
                    SingleChildScrollView::new()
                        .position(position)
                        .child(SizedBox::new(100.0, 1000.0))
                        .boxed()
                })),
            tight(100.0, height),
        );
        assert_eq!(
            controller.position().viewport_dimension(),
            height,
            "real viewport producer"
        );
        let scroll = ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(60),
            position(50.0, height / 2.0),
            ScrollDelta::try_new(ScrollUnit::Pages, 0.0, 0.5).expect("finite pages"),
        );
        laid.dispatch_pointer_event(&PointerEvent::Scroll(scroll));
        assert_eq!(
            controller.pixels(),
            height * 0.5,
            "half page in {height}px viewport"
        );
    }
}

pub(crate) fn viewer_page_zoom_resolves_against_the_actual_viewport() {
    use flui_painting::styling::Color;
    use flui_widgets::{ColoredBox, InteractiveViewer, TransformationController};

    for height in [100.0, 275.0] {
        let controller = TransformationController::new();
        let laid = lay_out(
            InteractiveViewer::new()
                .controller(controller.clone())
                .child(ColoredBox::new(Color::rgb(10, 20, 30))),
            tight(100.0, height),
        );
        let scroll = ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(60),
            position(50.0, height / 2.0),
            ScrollDelta::try_new(ScrollUnit::Pages, 0.0, -0.5).expect("finite pages"),
        );
        laid.dispatch_pointer_event(&PointerEvent::Scroll(scroll));
        let scale = controller.value().to_col_major_array()[0];
        let expected = (height * 0.5 / 200.0_f64).exp();
        assert!(
            (scale - expected).abs() < 1e-12,
            "half page in {height}px viewer"
        );
    }
}

pub(crate) fn viewer_cumulative_zoom_survives_rebuild_and_resets() {
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let scales = Rc::new(RefCell::new(Vec::new()));
    let mut laid = lay_out(
        viewer(controller.clone(), scales.clone()),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.2));
    assert_scale(scale_of(&controller), 1.2);
    laid.pump_widget(viewer(controller.clone(), scales.clone()));
    laid.pump();
    for cumulative in [1.5, 1.5] {
        laid.dispatch_pointer_event(&zoom_update(cumulative));
        assert_scale(scale_of(&controller), 1.5);
    }
    let observed = scales.borrow();
    assert_eq!(observed.len(), 3);
    for (actual, expected) in observed.iter().zip([1.2, 1.25, 1.0]) {
        assert_scale(*actual, expected);
    }
    drop(observed);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.2));
    assert_scale(scale_of(&controller), 1.8);
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Cancelled));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.1));
    assert_scale(scale_of(&controller), 1.98);
}

pub(crate) fn viewer_unstarted_pinch_updates_remain_independent_steps() {
    use flui_interaction::events::make_pinch_gesture_event;
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let scales = Rc::new(RefCell::new(Vec::new()));
    let laid = lay_out(
        viewer(controller.clone(), scales.clone()),
        tight(100.0, 100.0),
    );
    // The legacy backend bridge has no Start/End to send. Its two 10% ticks
    // are independent steps, while a started stream carries cumulative values.
    for _ in 0..2 {
        laid.dispatch_pointer_event(
            &make_pinch_gesture_event(Offset::new(50.0, 50.0), 0.1)
                .expect("finite synthetic legacy pinch"),
        );
    }
    assert_scale(scale_of(&controller), 1.21);
    assert_eq!(scales.borrow().len(), 2);
    for step in scales.borrow().iter() {
        assert_scale(*step, 1.1);
    }
}

pub(crate) fn viewer_extreme_zoom_reports_the_finite_applied_change() {
    use flui_widgets::TransformationController;
    let controller = TransformationController::new();
    let scales = Rc::new(RefCell::new(Vec::new()));
    let laid = lay_out(
        viewer(controller.clone(), scales.clone()),
        tight(100.0, 100.0),
    );
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    for (cumulative, expected) in [
        (f64::from_bits(1), 0.8),
        (1e300, 2.5),
        (f64::from_bits(1), 0.8),
    ] {
        let before = scale_of(&controller);
        laid.dispatch_pointer_event(&zoom_update(cumulative));
        assert_scale(scale_of(&controller), expected);
        assert!(
            controller
                .value()
                .to_col_major_array()
                .iter()
                .all(|value| value.is_finite())
        );
        let applied = *scales.borrow().last().expect("update callback");
        assert!(applied.is_finite() && applied > 0.0);
        // The documented callback reports the applied change, including clamps.
        // A saturated unrepresentable raw ratio would be an invented result.
        assert_scale(applied, expected / before);
    }
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::End));
    laid.dispatch_pointer_event(&pan_zoom(PanZoomPhase::Start));
    laid.dispatch_pointer_event(&zoom_update(1.25));
    assert_scale(scale_of(&controller), 1.0);
}

pub(crate) fn viewer_page_overflow_and_empty_viewport_recover() {
    use flui_widgets::TransformationController;
    for height in [0.0, 100.0] {
        let controller = TransformationController::new();
        let scales = Rc::new(RefCell::new(Vec::new()));
        let mut laid = lay_out(
            SizedBox::new(100.0, height).child(viewer(controller.clone(), scales.clone())),
            crate::common::loose(100.0),
        );
        let scroll = |delta| {
            PointerEvent::Scroll(ScrollEvent::new(
                mouse(),
                EventTime::from_nanos(80),
                position(50.0, 0.0),
                ScrollDelta::try_new(ScrollUnit::Pages, 0.0, delta).expect("finite pages"),
            ))
        };
        laid.dispatch_pointer_event(&scroll(-f64::MAX));
        assert_scale(scale_of(&controller), 1.0);
        assert!(
            scales.borrow().is_empty(),
            "no interaction for empty/overflowing page geometry"
        );
        // Reconcile the same viewer under a non-empty actual viewport before
        // the next event, retaining its controller and owner-local state.
        laid.pump_widget(
            SizedBox::new(100.0, 100.0).child(viewer(controller.clone(), scales.clone())),
        );
        laid.pump();
        laid.dispatch_pointer_event(&PointerEvent::Scroll(ScrollEvent::new(
            mouse(),
            EventTime::from_nanos(81),
            position(50.0, 50.0),
            ScrollDelta::try_new(ScrollUnit::Pages, 0.0, -0.5).expect("finite pages"),
        )));
        assert_scale(scale_of(&controller), (50.0 / 200.0_f64).exp());
        assert_eq!(scales.borrow().len(), 1);
    }
}
