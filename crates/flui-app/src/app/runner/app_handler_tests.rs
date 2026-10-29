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
