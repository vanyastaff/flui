//! The application handler a runner passes to `Platform::run` answers with
//! the routes its own run set, and with none of another run's.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use flui_native::{AppHandler, ExitAnswer, HeadlessPlatform, OwnerPlatform, Platform};

use super::{
    OwnerHostClearGuard, app_handler, install_owner_platform, route_exit_policy, route_quit,
    route_wake_deadline,
};

/// A headless run of `handler`, whose bootstrap ran when this returns.
fn run(handler: Rc<dyn AppHandler>) -> Result<(), flui_native::PlatformError> {
    Box::new(HeadlessPlatform::new()).run(handler)
}

/// A run from inside a handler, as a test does, installs its own owner and
/// sets its own routes; the outer run's exit policy, wake deadline and quit
/// still answer with the outer routes once it returns.
fn a_nested_run_leaves_the_outer_runs_routes() {
    let kept: Rc<RefCell<Option<OwnerPlatform>>> = Rc::default();
    let quits = Rc::new(Cell::new(0));
    let deadline = web_time::Instant::now();
    let outer = app_handler({
        let kept = Rc::clone(&kept);
        let quits = Rc::clone(&quits);
        move |owner| {
            route_quit(move || quits.set(quits.get() + 1));
            route_exit_policy(|| true);
            route_wake_deadline(move || Some(deadline));
            let _clear = OwnerHostClearGuard::arm();
            run(app_handler(|owner| {
                install_owner_platform(owner).expect("install the inner owner");
                route_exit_policy(|| false);
                route_wake_deadline(|| None);
                route_quit(|| {});
                Ok(())
            }))
            .expect("the inner run");
            kept.replace(Some(owner));
            Ok(())
        }
    });
    run(Rc::clone(&outer)).expect("the outer run");
    let owner = kept.take().expect("the outer bootstrap ran");
    assert_eq!(outer.exit_policy(&owner), ExitAnswer::End);
    assert_eq!(outer.wake_deadline(&owner), Some(deadline));
    outer.quit_requested(&owner);
    assert_eq!(quits.get(), 1, "the outer quit reached the outer route");
}

/// A bootstrap that fails before it sets a route answers with no route of the
/// run around it: the exit is left to the host, no deadline is asked for,
/// and its quit reaches nothing.
fn a_failed_bootstrap_answers_with_no_other_runs_routes() {
    let kept: Rc<RefCell<Option<OwnerPlatform>>> = Rc::default();
    let failed: Rc<RefCell<Option<Rc<dyn AppHandler>>>> = Rc::default();
    let quits = Rc::new(Cell::new(0));
    let outer = app_handler({
        let kept = Rc::clone(&kept);
        let failed = Rc::clone(&failed);
        let quits = Rc::clone(&quits);
        move |owner| {
            route_quit(move || quits.set(quits.get() + 1));
            route_exit_policy(|| true);
            route_wake_deadline(|| Some(web_time::Instant::now()));
            let inner = app_handler(|_| Err("the bootstrap fails".into()));
            failed.replace(Some(Rc::clone(&inner)));
            assert!(run(inner).is_err(), "the inner bootstrap fails");
            kept.replace(Some(owner));
            Ok(())
        }
    });
    run(outer).expect("the outer run");
    let owner = kept.take().expect("the outer bootstrap ran");
    let inner = failed.take().expect("the inner run was made");
    assert_eq!(inner.exit_policy(&owner), ExitAnswer::HostDecides);
    assert_eq!(inner.wake_deadline(&owner), None);
    inner.quit_requested(&owner);
    assert_eq!(
        quits.get(),
        0,
        "the failed run's quit reached the outer route"
    );
}

/// A runner that routes no exit policy leaves the exit to the host, as a run
/// with no handler does.
fn a_run_that_routes_no_exit_leaves_it_to_the_host() {
    let kept: Rc<RefCell<Option<OwnerPlatform>>> = Rc::default();
    let handler = app_handler({
        let kept = Rc::clone(&kept);
        move |owner| {
            kept.replace(Some(owner));
            Ok(())
        }
    });
    run(Rc::clone(&handler)).expect("the run");
    let owner = kept.take().expect("the bootstrap ran");
    assert_eq!(handler.exit_policy(&owner), ExitAnswer::HostDecides);
}

#[test]
fn app_handler_routes() {
    crate::table_test::run_table(
        "app handler routes",
        &[
            (
                "a nested run leaves the outer run's routes",
                a_nested_run_leaves_the_outer_runs_routes as fn(),
            ),
            (
                "a failed bootstrap answers with no other run's routes",
                a_failed_bootstrap_answers_with_no_other_runs_routes as fn(),
            ),
            (
                "a run that routes no exit leaves it to the host",
                a_run_that_routes_no_exit_leaves_it_to_the_host as fn(),
            ),
        ],
    );
}

/// ADR-0258 §4 on the desktop runner: the web host has no headless window
/// teardown to drive these rows through.
#[cfg(not(target_arch = "wasm32"))]
mod handler_containment {
    use super::*;

    /// A routed headless window of an installed owner, torn down on drop.
    struct Routed {
        native: flui_native::MockWindow,
        window: Rc<dyn flui_platform::PlatformWindow>,
        routes: Rc<crate::app::runner::window_routes::WindowRoutes>,
        _clear: OwnerHostClearGuard,
    }

    impl Routed {
        fn open() -> Self {
            use crate::app::runner::window_routes::OwnerOpenRouted as _;

            let clear = OwnerHostClearGuard::arm();
            run(app_handler(|owner| {
                install_owner_platform(owner)?;
                Ok(())
            }))
            .expect("headless run");
            let host_window = super::super::with_owner_platform(|owner| {
                owner.open_routed(flui_platform::WindowOptions::default())
            })
            .expect("an installed owner")
            .and_then(flui_native::WindowOpen::try_ready)
            .expect("headless windows are ready");
            let native = host_window
                .as_any()
                .downcast_ref::<flui_native::MockWindow>()
                .expect("headless window")
                .clone();
            let window: Rc<dyn flui_platform::PlatformWindow> = host_window;
            let routes = crate::app::runner::window_routes::WindowRoutes::of(&*window);
            Self {
                native,
                window,
                routes,
                _clear: clear,
            }
        }
    }

    impl Drop for Routed {
        fn drop(&mut self) {
            crate::app::runner::owner_dispatch::teardown_platform_ui_runtime();
        }
    }

    const APPLICATION_PANIC: &str = "the application fails";

    /// The loop's terminal failure is the application's panic, taken so the next
    /// row starts with none.
    fn assert_held() {
        let held = crate::app::runner::terminal::take_terminal()
            .expect("the panic is held as the loop's terminal failure");
        assert_eq!(
            flui_foundation::panic::payload_text(held.as_ref()),
            Some(APPLICATION_PANIC)
        );
    }

    fn window_input() {
        let routed = Routed::open();
        // The capability snapshot held since the window opened is handed over
        // when the route is set, outside any platform call.
        routed.routes.route_input(|event| {
            assert!(
                !matches!(event, flui_platform::PlatformInput::Ime(_)),
                "{APPLICATION_PANIC}"
            );
            flui_platform::DispatchEventResult::default()
        });
        routed
            .native
            .inject_event(flui_platform::PlatformInput::Ime(
                flui_platform::ImeEvent::Enabled,
            ));
        assert_held();
    }

    fn window_state_changed() {
        let routed = Routed::open();
        routed
            .routes
            .route_state_changed(|| panic!("{APPLICATION_PANIC}"));
        routed.native.simulate_resize(320.0, 240.0);
        assert_held();
    }

    /// A failed frame is answered `Failed` and is not terminal (ADR-0258 §4).
    fn window_frame_requested() {
        let routed = Routed::open();
        routed
            .routes
            .route_frame_requested(|_| panic!("{APPLICATION_PANIC}"));
        routed.window.request_redraw();
        assert!(
            crate::app::runner::terminal::take_terminal().is_none(),
            "a failed frame ended the loop"
        );
    }

    fn window_close_requested() {
        let routed = Routed::open();
        routed
            .routes
            .route_close_requested(|| panic!("{APPLICATION_PANIC}"), || {});
        assert!(
            !routed.native.simulate_close(),
            "a close whose handler panicked keeps the window"
        );
        assert_held();
    }

    fn window_closed() {
        let routed = Routed::open();
        routed
            .routes
            .route_close_requested(|| flui_platform::CloseDecision::Close, || {});
        routed.routes.route_closed(|| panic!("{APPLICATION_PANIC}"));
        assert!(routed.native.simulate_close(), "the window closes");
        assert_held();
    }

    fn window_surface_changed() {
        let routed = Routed::open();
        routed
            .routes
            .route_surface_changed(|_| panic!("{APPLICATION_PANIC}"));
        routed
            .native
            .simulate_surface_change(flui_platform::SurfaceChange::Release);
        assert_held();
    }

    /// An application handler method whose route panics, called the way the
    /// platform calls it.
    fn app_method(set: fn(), call: fn(&dyn AppHandler, &OwnerPlatform)) {
        let kept: Rc<RefCell<Option<OwnerPlatform>>> = Rc::default();
        let handler = app_handler({
            let kept = Rc::clone(&kept);
            move |owner| {
                set();
                kept.replace(Some(owner));
                Ok(())
            }
        });
        run(Rc::clone(&handler)).expect("the run");
        let owner = kept.take().expect("the bootstrap ran");
        call(&*handler, &owner);
        assert_held();
    }

    fn app_quit_requested() {
        app_method(
            || route_quit(|| panic!("{APPLICATION_PANIC}")),
            AppHandler::quit_requested,
        );
    }

    fn app_reopen() {
        app_method(
            || super::super::route_reopen(|| panic!("{APPLICATION_PANIC}")),
            AppHandler::reopen,
        );
    }

    fn app_exit_policy() {
        app_method(
            || route_exit_policy(|| panic!("{APPLICATION_PANIC}")),
            |handler, owner| {
                assert_eq!(
                    handler.exit_policy(owner),
                    ExitAnswer::End,
                    "a failed exit policy ends the loop"
                );
            },
        );
    }

    fn app_wake_deadline() {
        app_method(
            || route_wake_deadline(|| panic!("{APPLICATION_PANIC}")),
            |handler, owner| assert_eq!(handler.wake_deadline(owner), None),
        );
    }

    fn app_ready() {
        let handler = app_handler(|_| panic!("{APPLICATION_PANIC}"));
        run(handler).expect("a bootstrap that panicked returns from the run");
        assert_held();
    }

    /// ADR-0258 §4: application code that panics inside a handler method never
    /// unwinds out of the platform's call (the call point's `or_resume`, which on
    /// Win32, AppKit and UIKit is an abort). One row per method of `AppHandler`
    /// and `WindowHandler`; a row fails when the panic leaves the method.
    #[test]
    fn an_application_panic_never_leaves_a_handler() {
        crate::table_test::run_table(
            "an application panic never leaves a handler",
            &[
                ("AppHandler::ready", app_ready as fn()),
                ("AppHandler::quit_requested", app_quit_requested),
                ("AppHandler::reopen", app_reopen),
                ("AppHandler::exit_policy", app_exit_policy),
                ("AppHandler::wake_deadline", app_wake_deadline),
                ("WindowHandler::input", window_input),
                ("WindowHandler::state_changed", window_state_changed),
                ("WindowHandler::frame_requested", window_frame_requested),
                ("WindowHandler::close_requested", window_close_requested),
                ("WindowHandler::closed", window_closed),
                ("WindowHandler::surface_changed", window_surface_changed),
            ],
        );
    }
}
