//! Owned pointer vocabulary contracts through the production widget pipeline.

use std::{cell::RefCell, rc::Rc};

use crate::common::{lay_out, tight};
use flui_foundation::geometry::Point;
use flui_interaction::routing::EventPropagation;
use flui_platform_api::{
    EventTime,
    keyboard::Modifiers,
    pointer::{
        DeviceId, PointerButton, PointerButtons, PointerEvent, PointerId, PointerInfo, PointerKind,
        PointerMove, PointerPosition, PointerPress, PointerRelease, PointerRole, PointerSample,
        ScrollDelta, ScrollEvent, ScrollPhase, ScrollPrecision, ScrollUnit,
    },
};
use flui_view::IntoView;
use flui_widgets::{Listener, ScrollController, Scrollable, SingleChildScrollView, SizedBox};

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
    use flui_foundation::Color;
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
