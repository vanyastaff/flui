//! What the runner's window handler does before and after its routes point
//! at a presentation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_platform::input_capabilities::InputCapabilities;
use flui_platform::{
    Availability, CloseDecision, DispatchEventResult, ImeEvent, OwnerThread, PlatformInput,
    SurfaceChange, WindowHandler,
};

use super::WindowRoutes;

fn owner(routes: &Rc<WindowRoutes>) -> OwnerThread {
    OwnerThread::holding(routes)
}

/// Before its input route the handler takes no input, keeps the capability
/// snapshot, and hands the route the newest capabilities first.
fn the_input_route_hears_the_capabilities_first() {
    let routes = WindowRoutes::new();
    let pen = InputCapabilities::builder()
        .pen(Availability::Native)
        .build();
    let early = routes.input(
        owner(&routes),
        PlatformInput::Capabilities(InputCapabilities::UNAVAILABLE),
    );
    assert_eq!(early, DispatchEventResult::default());
    let ignored = routes.input(owner(&routes), PlatformInput::Ime(ImeEvent::Enabled));
    assert_eq!(
        ignored,
        DispatchEventResult::default(),
        "nothing takes it yet"
    );
    routes.input(owner(&routes), PlatformInput::Capabilities(pen));
    let heard = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&heard);
    routes.route_input(move |input| {
        log.borrow_mut().push(input);
        DispatchEventResult::resolved(false, true)
    });
    routes.input(owner(&routes), PlatformInput::Ime(ImeEvent::Enabled));
    let heard = heard.borrow();
    assert_eq!(heard.len(), 2, "{heard:?}");
    assert!(matches!(heard[0], PlatformInput::Capabilities(c) if c == pen));
    assert!(matches!(heard[1], PlatformInput::Ime(_)));
}

/// A window that closed before its close route was set still runs it, once,
/// when it is set; a close request with no route closes the window.
fn a_close_heard_early_runs_its_route() {
    let routes = WindowRoutes::new();
    assert_eq!(routes.close_requested(owner(&routes)), CloseDecision::Close);
    routes.closed(owner(&routes));
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
        routes.surface_changed(owner(&routes), SurfaceChange::Release);
    }));
    assert!(failed.is_err());
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        routes.surface_changed(owner(&routes), SurfaceChange::Rebuild);
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
    routes.state_changed(owner(&routes));
    routes.state_changed(owner(&routes));
    assert!(successor.get(), "the route set from inside its call lost");
}

#[test]
fn window_routes_contract() {
    let rows: &[(&str, fn())] = &[
        (
            "the input route hears the capabilities first",
            the_input_route_hears_the_capabilities_first,
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
