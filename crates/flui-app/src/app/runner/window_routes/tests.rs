//! What the runner's window handler does before and after its routes point
//! at a presentation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    Availability, CloseDecision, DispatchEventResult, EventTime, FrameOutcome, FrameRequest,
    ImeEvent, PlatformInput, SurfaceChange, WindowHandler,
};

use super::WindowRoutes;

/// Every input that arrives before the input route is kept in order and
/// answered with its deferred form; the route hears them all, in order, when
/// it is set, then the input after them.
fn the_input_route_hears_everything_before_it_in_order() {
    let routes = WindowRoutes::new();
    let pen = InputCapabilities::builder()
        .pen(Availability::Native)
        .build();
    for early in [
        PlatformInput::Capabilities(InputCapabilities::UNAVAILABLE),
        PlatformInput::Ime(ImeEvent::Enabled),
        PlatformInput::Capabilities(pen),
    ] {
        assert_eq!(
            routes.input(early),
            DispatchEventResult::DEFERRED,
            "its outcome is not known yet"
        );
    }
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&heard);
    routes.route_input(move |input| {
        log.borrow_mut().push(input);
        DispatchEventResult::resolved(false, true)
    });
    assert_eq!(
        routes.input(PlatformInput::Ime(ImeEvent::Disabled)),
        DispatchEventResult::resolved(false, true),
        "a routed input is answered by the route"
    );
    let heard = heard.borrow();
    assert_eq!(heard.len(), 4, "{heard:?}");
    assert!(
        matches!(heard[0], PlatformInput::Capabilities(c) if c == InputCapabilities::UNAVAILABLE)
    );
    assert!(matches!(heard[1], PlatformInput::Ime(ImeEvent::Enabled)));
    assert!(matches!(heard[2], PlatformInput::Capabilities(c) if c == pen));
    assert!(matches!(heard[3], PlatformInput::Ime(ImeEvent::Disabled)));
}

/// A state change, a frame request and a surface change that arrive before
/// their route are kept, the newest of each, and handed over when it is set;
/// the frame request was declined when it arrived.
fn state_frame_and_surface_wait_for_their_routes() {
    let routes = WindowRoutes::new();
    routes.state_changed();
    routes.state_changed();
    assert_eq!(
        routes.frame_requested(FrameRequest::new(EventTime::from_nanos(1))),
        FrameOutcome::Declined
    );
    routes.frame_requested(FrameRequest::new(EventTime::from_nanos(2)));
    routes.surface_changed(SurfaceChange::Release);
    routes.surface_changed(SurfaceChange::Rebuild);

    let states = Rc::new(Cell::new(0));
    let count = Rc::clone(&states);
    routes.route_state_changed(move || count.set(count.get() + 1));
    assert_eq!(states.get(), 1, "one state change was owed");

    let frames = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&frames);
    routes.route_frame_requested(move |request| log.borrow_mut().push(request.vsync));
    assert_eq!(
        *frames.borrow(),
        [EventTime::from_nanos(2)],
        "the newest request"
    );
    assert_eq!(
        routes.frame_requested(FrameRequest::new(EventTime::from_nanos(3))),
        FrameOutcome::Produced
    );

    let surfaces = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&surfaces);
    routes.route_surface_changed(move |change| log.borrow_mut().push(change));
    assert_eq!(
        *surfaces.borrow(),
        [SurfaceChange::Rebuild],
        "the surface the window needs now"
    );
}

/// A window that closed before its close route was set still runs it, once,
/// when it is set; a close request with no route keeps the window.
fn a_close_heard_early_runs_its_route() {
    let routes = WindowRoutes::new();
    assert_eq!(routes.close_requested(), CloseDecision::KeepOpen);
    routes.closed();
    let ran = Rc::new(Cell::new(0));
    let count = Rc::clone(&ran);
    routes.route_closed(move || count.set(count.get() + 1));
    assert_eq!(ran.get(), 1, "the close heard before the route was lost");
}

/// A route that sets its successor from inside its call is replaced; one that
/// panics stays, so the next call still reaches the presentation.
fn a_route_survives_its_panic_and_yields_to_its_successor() {
    let routes = WindowRoutes::new();
    let calls = Rc::new(Cell::new(0));
    let count = Rc::clone(&calls);
    routes.route_surface_changed(move |_| {
        count.set(count.get() + 1);
        panic!("the route fails");
    });
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        routes.surface_changed(SurfaceChange::Release);
    }));
    assert!(failed.is_err());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        routes.surface_changed(SurfaceChange::Rebuild);
    }));
    assert_eq!(calls.get(), 2, "the panicking route was lost");

    let successor = Rc::new(Cell::new(false));
    let reached = Rc::clone(&successor);
    let weak = Rc::downgrade(&routes);
    routes.route_state_changed(move || {
        let reached = Rc::clone(&reached);
        weak.upgrade()
            .expect("live routes")
            .route_state_changed(move || reached.set(true));
    });
    routes.state_changed();
    routes.state_changed();
    assert!(successor.get(), "the route set from inside its call lost");
}

#[test]
fn window_routes_contract() {
    let rows: &[(&str, fn())] = &[
        (
            "the input route hears everything before it in order",
            the_input_route_hears_everything_before_it_in_order,
        ),
        (
            "state, frame and surface wait for their routes",
            state_frame_and_surface_wait_for_their_routes,
        ),
        (
            "a close heard early runs its route",
            a_close_heard_early_runs_its_route,
        ),
        (
            "a route survives its panic and yields to its successor",
            a_route_survives_its_panic_and_yields_to_its_successor,
        ),
    ];
    let failed: Vec<_> = rows
        .iter()
        .filter(|(_, row)| std::panic::catch_unwind(*row).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{failed:?}");
}
