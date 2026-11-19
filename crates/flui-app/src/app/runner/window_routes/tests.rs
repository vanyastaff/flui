//! What the runner's window handler does before and after its routes point
//! at a presentation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_platform::input::capabilities::InputCapabilities;
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
/// the frame request was declined when it arrived, and one that arrives
/// after the route is answered by the route.
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
    let answer = Rc::new(Cell::new(FrameOutcome::Produced));
    let answering = Rc::clone(&answer);
    routes.route_frame_requested(move |request| {
        log.borrow_mut().push(request.vsync);
        answering.get()
    });
    assert_eq!(
        *frames.borrow(),
        [EventTime::from_nanos(2)],
        "the newest request"
    );
    // A routed request is answered by its route: a route that ran no frame
    // says so, and the window does not answer `Produced` in its place.
    for outcome in [
        FrameOutcome::Produced,
        FrameOutcome::Declined,
        FrameOutcome::Failed,
    ] {
        answer.set(outcome);
        assert_eq!(
            routes.frame_requested(FrameRequest::new(EventTime::from_nanos(3))),
            outcome,
            "the route's answer is the window's"
        );
    }

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
/// panics stays, so the next call still reaches the presentation. The panic
/// does not leave the method: the first is held as the loop's terminal
/// failure.
fn a_route_survives_its_panic_and_yields_to_its_successor() {
    let routes = WindowRoutes::new();
    let calls = Rc::new(Cell::new(0));
    let count = Rc::clone(&calls);
    routes.route_surface_changed(move |_| {
        count.set(count.get() + 1);
        panic!("the route fails");
    });
    routes.surface_changed(SurfaceChange::Release);
    routes.surface_changed(SurfaceChange::Rebuild);
    assert_eq!(calls.get(), 2, "the panicking route was lost");
    let held = super::super::terminal::take_terminal().expect("the route's panic is held");
    assert_eq!(
        flui_foundation::panic::payload_text(held.as_ref()),
        Some("the route fails")
    );

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

/// A close request before the close route keeps the window and is asked of
/// the route exactly once when it is set, however many came; granted, the
/// window is closed then, refused, it is not.
fn a_close_request_before_its_route_is_asked_once() {
    for (answer, closes) in [(CloseDecision::Close, 1), (CloseDecision::KeepOpen, 0)] {
        let routes = WindowRoutes::new();
        assert_eq!(routes.close_requested(), CloseDecision::KeepOpen);
        assert_eq!(routes.close_requested(), CloseDecision::KeepOpen);
        let asked = Rc::new(Cell::new(0));
        let closed = Rc::new(Cell::new(0));
        let (count, closing) = (Rc::clone(&asked), Rc::clone(&closed));
        routes.route_close_requested(
            move || {
                count.set(count.get() + 1);
                answer
            },
            move || closing.set(closing.get() + 1),
        );
        assert_eq!(
            asked.get(),
            1,
            "{answer:?}: the held request reached the route once"
        );
        assert_eq!(
            closed.get(),
            closes,
            "{answer:?}: the close follows the answer"
        );
        assert_eq!(
            routes.close_requested(),
            answer,
            "a later request asks the route"
        );
        assert_eq!(asked.get(), 2);
        assert_eq!(
            closed.get(),
            closes,
            "{answer:?}: a later request is its caller's"
        );
    }
}

/// A frame route that panics answers `Failed`, so the platform keeps what
/// waited on the frame owed; the panic is not the loop's terminal failure.
fn a_failed_frame_is_answered_and_not_terminal() {
    let routes = WindowRoutes::new();
    routes.route_frame_requested(|_| panic!("the frame fails"));
    assert_eq!(
        routes.frame_requested(FrameRequest::new(EventTime::from_nanos(1))),
        FrameOutcome::Failed
    );
    assert!(
        super::super::terminal::take_terminal().is_none(),
        "a failed frame ended the loop"
    );
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
            "a close request before its route is asked once",
            a_close_request_before_its_route_is_asked_once,
        ),
        (
            "a close heard early runs its route",
            a_close_heard_early_runs_its_route,
        ),
        (
            "a route survives its panic and yields to its successor",
            a_route_survives_its_panic_and_yields_to_its_successor,
        ),
        (
            "a failed frame is answered and not terminal",
            a_failed_frame_is_answered_and_not_terminal,
        ),
    ];
    let failed: Vec<_> = rows
        .iter()
        .filter(|(_, row)| std::panic::catch_unwind(*row).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(failed.is_empty(), "{failed:?}");
}

/// The runner paths that open a window, each in an application run on the
/// headless platform. Input waits for its route only between a window's open
/// and the end of the owner turn that installs it: once that turn is over
/// the route is set and has heard everything, so the held queue never
/// outlives it.
#[cfg(all(
    feature = "testing",
    not(target_os = "android"),
    not(target_os = "ios"),
    not(target_arch = "wasm32")
))]
mod runner_paths {
    use flui_platform::PlatformWindow;
    use flui_widgets::SizedBox;

    use super::WindowRoutes;
    use crate::app::runner::owner_dispatch::{RuntimeTask, dispatch_platform_ui_runtime};
    use crate::app::runner::secondary_window::{
        open_secondary_window_impl, open_window_with_content_impl,
    };
    use crate::app::runtime::WindowPolicy;
    use crate::app::{AppConfig, Application};
    use crate::testing::{HeadlessRun, HeadlessRunHandle};

    fn started() -> HeadlessRunHandle {
        HeadlessRun::start(Application::new(), SizedBox::shrink())
    }

    /// How many inputs wait for the window's input route; `None` once the
    /// route is set and has heard them all, so input reaches the
    /// presentation as it arrives.
    fn waiting(window: &(impl PlatformWindow + ?Sized)) -> Option<usize> {
        let routes = WindowRoutes::of(window);
        let waiting = routes.held.borrow().inputs.len();
        (waiting > 0 || !routes.input.is_set()).then_some(waiting)
    }

    fn the_main_window() {
        let run = started();
        let windows = run.open_windows();
        assert_eq!(windows.len(), 1, "the main window opens as the run starts");
        assert_eq!(
            waiting(&windows[0]),
            None,
            "the main window is installed by the call that opens it"
        );
        run.join().expect("the run ends");
    }

    fn a_bare_secondary_window() {
        let run = started();
        let (dispatcher, window) =
            open_secondary_window_impl(AppConfig::new(), WindowPolicy::Isolated)
                .expect("the open is admitted")
                .expect("outside an owner turn the window opens at once");
        assert_eq!(
            waiting(&*window),
            None,
            "a bare secondary window is installed by the call that opens it"
        );

        // Asked for from inside an owner turn, the window is published when
        // that turn ends.
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(|_| {
                let opened = open_secondary_window_impl(AppConfig::new(), WindowPolicy::Isolated)
                    .expect("the open is admitted");
                assert!(opened.is_none(), "a turn is running: the window waits");
            })),
        )
        .expect("the turn that asks for the window");
        let windows = run.open_windows();
        assert_eq!(windows.len(), 3, "main, and both secondary windows");
        assert_eq!(
            waiting(&windows[2]),
            None,
            "a window asked for inside an owner turn is installed by the end of it"
        );
        run.join().expect("the run ends");
    }

    fn a_secondary_window_with_content() {
        let run = started();
        let _installed = open_window_with_content_impl(
            AppConfig::new(),
            WindowPolicy::Isolated,
            SizedBox::shrink(),
        )
        .expect("the open is admitted");
        let windows = run.open_windows();
        assert_eq!(windows.len(), 2, "the window is open before its content");
        assert_eq!(
            waiting(&windows[1]),
            Some(1),
            "until the owner's next turn the window holds its capability snapshot"
        );
        run.run_owner_turn();
        assert_eq!(
            waiting(&windows[1]),
            None,
            "the owner's next turn installs the content and the input route"
        );
        run.join().expect("the run ends");
    }

    #[test]
    fn every_runner_path_routes_input_by_the_end_of_its_installing_turn() {
        crate::table_test::run_table(
            "every_runner_path_routes_input_by_the_end_of_its_installing_turn",
            &[
                ("the main window", the_main_window as fn()),
                ("a bare secondary window", a_bare_secondary_window as fn()),
                (
                    "a secondary window with content",
                    a_secondary_window_with_content as fn(),
                ),
            ],
        );
    }
}
