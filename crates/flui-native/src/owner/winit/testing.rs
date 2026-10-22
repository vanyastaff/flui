//! A real winit loop for the tests of the windows it drives: a window opened
//! through the loop's control lane on an event loop the test built, and a
//! scenario run against it on a worker thread while the loop runs on the
//! test's own.
//!
//! The host's table holds those tests (`hosts::winit`), the one module that
//! names the loop, the window and the platform together.

use std::{
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    sync::Arc,
    thread,
};

use flui_foundation::ClaimOutcome;
use winit::event_loop::EventLoop;

use super::control::control_lane;
use super::real_loop_tests::{install_test_owner_signal, wait_for_running};
use super::{SelfCloseRoute, WinitApp, WinitLoop};
use crate::host_window::HostWindow;
use crate::platform::Platform;
use flui_platform::WindowOptions;

/// Run `event_loop` for `platform`, the loop of `owner_platform`, open a window
/// with `opening` once the loop runs, and run `scenario` against it on a
/// worker thread; the loop quits when the scenario returns, and a panic in it
/// fails the test.
pub(crate) fn run_window_scenario(
    platform: Arc<WinitLoop>,
    owner_platform: Arc<dyn Platform>,
    event_loop: EventLoop<()>,
    opening: WindowOptions,
    scenario: impl FnOnce(Arc<dyn HostWindow>) + Send + 'static,
) {
    install_test_owner_signal(&platform, &event_loop);
    let event_loop_proxy = event_loop.create_proxy();
    let wake_owner: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = event_loop_proxy.send_event(());
    });
    let (control, receiver) = control_lane(wake_owner);
    platform
        .install_control_lane(thread::current().id(), control.clone())
        .expect("first install succeeds");

    let platform_for_worker = Arc::clone(&platform);
    let control_for_worker = control;
    let worker = thread::spawn(move || {
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            wait_for_running(&platform_for_worker);
            let window = match control_for_worker
                .request_open_window(opening)
                .expect("lane accepts the request")
                .wait()
            {
                ClaimOutcome::Delivered(result) => result.expect("window creation succeeds"),
                _ => panic!("the window request is delivered"),
            };
            scenario(window);
        }));
        control_for_worker.request_quit();
        if let Err(payload) = outcome {
            resume_unwind(payload);
        }
    });

    let mut app = WinitApp {
        platform: Arc::clone(&platform),
        owner_platform,
        preference_source: None,
        host_notices: None,
        display_watch: None,
        on_ready: None,
        control: receiver,
        quit_notified: false,
        in_flight_replies: Vec::new(),
        bootstrap_error: None,
        self_close_deadline: None,
        self_close_route: SelfCloseRoute::default(),
    };
    event_loop
        .run_app(&mut app)
        .expect("event loop runs to completion");
    worker.join().expect("worker thread does not panic");
}
