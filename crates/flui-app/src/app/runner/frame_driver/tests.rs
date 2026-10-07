//! Private driver fault matrix. Scene assertions use the product pump/sink;
//! the seam injects native prelude and capture destruction failures without GPU.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::{cell::RefCell, rc::Rc};

use flui_runtime::{sink::SubmitVerdict, testing::ScriptedSink, ui_runtime::UiRuntime};

use super::super::host::OwnerHostClearGuard;
use super::super::owner_dispatch::{
    DispatchError, PresentationDispatcher, RuntimeTask, close_this_window,
    dispatch_platform_ui_runtime, install_platform_ui_runtime,
    install_single_window_terminal_wiring, teardown_platform_ui_runtime,
};
use super::{FrameDriver, FrameRegistration, TestFrameDriver, install_frame_driver};

fn installed_runtime() -> PresentationDispatcher {
    let runtime = UiRuntime::for_test();
    runtime
        .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
        .expect("mount root");
    install_platform_ui_runtime(
        runtime,
        &crate::app::window_test_support::headless_test_window(),
    )
}

fn installed_driver(
    dispatcher: PresentationDispatcher,
    sink: ScriptedSink,
    prelude: Option<Box<dyn FnMut()>>,
) -> FrameRegistration {
    install_frame_driver(
        dispatcher,
        FrameDriver::Test(TestFrameDriver { sink, prelude }),
    )
    .expect("install frame resources")
}

fn counting_sink(count: Arc<AtomicUsize>) -> ScriptedSink {
    ScriptedSink::new(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        SubmitVerdict::Presented
    })
}

fn a_registered_driver_delivers_the_product_scene() {
    let _clear = OwnerHostClearGuard::arm();
    let dispatcher = installed_runtime();
    let scenes = Arc::new(AtomicUsize::new(0));
    let frame = installed_driver(dispatcher, counting_sink(Arc::clone(&scenes)), None);
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
        .expect("deliver frame");
    assert_eq!(
        scenes.load(Ordering::SeqCst),
        1,
        "the product scene left through the installed sink"
    );
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
        .expect("idle delivery");
    assert_eq!(
        scenes.load(Ordering::SeqCst),
        1,
        "idle delivery did not fabricate scene damage"
    );
    teardown_platform_ui_runtime();
}

fn an_old_frame_binding_never_reaches_a_new_runtime() {
    let _clear = OwnerHostClearGuard::arm();
    let old_dispatcher = installed_runtime();
    let old_scenes = Arc::new(AtomicUsize::new(0));
    let old_frame = installed_driver(old_dispatcher, counting_sink(Arc::clone(&old_scenes)), None);
    let new_dispatcher = installed_runtime();
    let new_scenes = Arc::new(AtomicUsize::new(0));
    let new_frame = installed_driver(new_dispatcher, counting_sink(Arc::clone(&new_scenes)), None);
    assert_eq!(
        dispatch_platform_ui_runtime(old_dispatcher, RuntimeTask::Frame(old_frame.binding)),
        Err(DispatchError::StaleRuntime)
    );
    assert_eq!(
        dispatch_platform_ui_runtime(new_dispatcher, RuntimeTask::Frame(old_frame.binding)),
        Err(DispatchError::StalePresentation)
    );
    dispatch_platform_ui_runtime(new_dispatcher, RuntimeTask::Frame(new_frame.binding))
        .expect("current delivery");
    assert_eq!(
        old_scenes.load(Ordering::SeqCst),
        0,
        "stale scene never submitted"
    );
    assert_eq!(
        new_scenes.load(Ordering::SeqCst),
        1,
        "new binding delivered its own scene"
    );
    teardown_platform_ui_runtime();
}

fn close_refuses_async_publication_without_cancelling_the_accepted_frame() {
    let _clear = OwnerHostClearGuard::arm();
    let dispatcher = installed_runtime();
    let scenes = Arc::new(AtomicUsize::new(0));
    let authority = Rc::new(RefCell::new(None::<super::FrameLiveness>));
    let published = Rc::new(parking_lot::Mutex::new(None));
    let results = Rc::new(RefCell::new(None));
    let frame = installed_driver(
        dispatcher,
        counting_sink(Arc::clone(&scenes)),
        Some(Box::new({
            let authority = Rc::clone(&authority);
            let published = Rc::clone(&published);
            let results = Rc::clone(&results);
            move || {
                let authority = authority.borrow().clone().expect("registered authority");
                let before = authority.publish(&published, 1);
                close_this_window(dispatcher);
                let after = authority.publish(&published, 2);
                *results.borrow_mut() = Some((before, after));
            }
        })),
    );
    *authority.borrow_mut() = Some(frame.liveness);
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
        .expect("accepted frame completes before close");
    assert_eq!(*results.borrow(), Some((Ok(None), Err(2))));
    assert_eq!(
        *published.lock(),
        Some(1),
        "late result did not replace resources"
    );
    assert_eq!(
        scenes.load(Ordering::SeqCst),
        1,
        "close preserved the accepted frame"
    );
    teardown_platform_ui_runtime();
}

struct ReentrantCapture(Arc<AtomicUsize>);

fn native_terminal_callbacks_refuse_late_publication() {
    use flui_platform::{Platform, traits::PlatformWindow};

    for quit in [false, true] {
        let _clear = OwnerHostClearGuard::arm();
        Box::new(flui_platform::HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                let owner = Rc::new(owner);
                let window: Arc<dyn PlatformWindow> = owner
                    .open_window(flui_platform::WindowOptions::default())?
                    .try_ready()?;
                owner.shared().set_exit_policy_hook(Box::new(|| false));
                let runtime = UiRuntime::for_test();
                runtime.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))?;
                let dispatcher = install_platform_ui_runtime(runtime, &window);
                install_single_window_terminal_wiring(&window, &owner.shared(), dispatcher);
                let scenes = Arc::new(AtomicUsize::new(0));
                let frame = installed_driver(
                    dispatcher,
                    counting_sink(Arc::clone(&scenes)),
                    Some(Box::new(move || {
                        if quit {
                            owner.quit();
                        } else {
                            window.close();
                        }
                    })),
                );
                dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
                    .expect("already accepted frame finishes");
                let resource = parking_lot::Mutex::new(None);
                assert_eq!(
                    frame.liveness.publish(&resource, 42),
                    Err(42),
                    "quit={quit}"
                );
                assert_eq!(scenes.load(Ordering::SeqCst), 1, "quit={quit}");
                teardown_platform_ui_runtime();
                Ok(())
            }))
            .expect("headless terminal callback");
    }
}

impl Drop for ReentrantCapture {
    fn drop(&mut self) {
        // Reenter through the same host API, proving retirement is outside TLS.
        teardown_platform_ui_runtime();
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn a_leased_driver_is_retired_after_reentrant_teardown() {
    let _clear = OwnerHostClearGuard::arm();
    let dispatcher = installed_runtime();
    let retired = Arc::new(AtomicUsize::new(0));
    let capture = ReentrantCapture(Arc::clone(&retired));
    let sink = ScriptedSink::new(move |_, _| {
        let _ = &capture;
        SubmitVerdict::Presented
    });
    let frame = installed_driver(
        dispatcher,
        sink,
        Some(Box::new(teardown_platform_ui_runtime)),
    );
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
        .expect("accepted delivery finishes cleanup");
    assert_eq!(
        retired.load(Ordering::SeqCst),
        1,
        "leased resources retire once after the callback"
    );
    assert_eq!(
        dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding)),
        Err(DispatchError::RuntimeUnavailable)
    );
    let next = installed_runtime();
    let scenes = Arc::new(AtomicUsize::new(0));
    let frame = installed_driver(next, counting_sink(Arc::clone(&scenes)), None);
    dispatch_platform_ui_runtime(next, RuntimeTask::Frame(frame.binding))
        .expect("next incarnation runs");
    assert_eq!(scenes.load(Ordering::SeqCst), 1);
    teardown_platform_ui_runtime();
}

struct FailingCapture(Arc<AtomicUsize>);

impl Drop for FailingCapture {
    fn drop(&mut self) {
        teardown_platform_ui_runtime();
        self.0.fetch_add(1, Ordering::SeqCst);
        panic!("frame capture failure");
    }
}

fn a_driver_failure_remains_first_when_retirement_also_fails() {
    let _clear = OwnerHostClearGuard::arm();
    let dispatcher = installed_runtime();
    let retired = Arc::new(AtomicUsize::new(0));
    let capture = FailingCapture(Arc::clone(&retired));
    let sink = ScriptedSink::new(move |_, _| {
        let _ = &capture;
        SubmitVerdict::Presented
    });
    let frame = installed_driver(
        dispatcher,
        sink,
        Some(Box::new(|| {
            teardown_platform_ui_runtime();
            panic!("frame prelude failure");
        })),
    );
    let failure = catch_unwind(AssertUnwindSafe(|| {
        dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
            .expect("admitted frame");
    }))
    .expect_err("prelude failed");
    assert_eq!(
        failure.downcast_ref::<&str>(),
        Some(&"frame prelude failure")
    );
    assert_eq!(
        retired.load(Ordering::SeqCst),
        1,
        "competing cleanup still ran once"
    );
    let next = installed_runtime();
    let scenes = Arc::new(AtomicUsize::new(0));
    let frame = installed_driver(next, counting_sink(Arc::clone(&scenes)), None);
    dispatch_platform_ui_runtime(next, RuntimeTask::Frame(frame.binding))
        .expect("host makes progress after containment");
    assert_eq!(scenes.load(Ordering::SeqCst), 1);
    teardown_platform_ui_runtime();
}

struct RetirementCapture {
    retired: Arc<AtomicUsize>,
    failure: Option<&'static str>,
}

impl Drop for RetirementCapture {
    fn drop(&mut self) {
        teardown_platform_ui_runtime();
        self.retired.fetch_add(1, Ordering::SeqCst);
        if let Some(failure) = self.failure {
            std::panic::panic_any(failure);
        }
    }
}

fn native_owners_retire_independently_after_registry_removal() {
    assert_native_retirement_failures(close_this_window);
    assert_native_retirement_failures(|_| teardown_platform_ui_runtime());
}

fn assert_native_retirement_failures(retire: fn(PresentationDispatcher)) {
    for (driver_failure, handler_failure) in [
        (None, None),
        (Some("driver retirement"), None),
        (None, Some("handler retirement")),
        (Some("driver retirement"), Some("handler retirement")),
    ] {
        let _clear = OwnerHostClearGuard::arm();
        let window = crate::app::window_test_support::headless_test_window();
        let dispatcher = install_platform_ui_runtime(UiRuntime::for_test(), &window);
        let retired = Arc::new(AtomicUsize::new(0));
        let driver_capture = RetirementCapture {
            retired: Arc::clone(&retired),
            failure: driver_failure,
        };
        installed_driver(
            dispatcher,
            ScriptedSink::new(move |_, _| {
                let _ = &driver_capture;
                SubmitVerdict::Presented
            }),
            None,
        );
        let handler_capture = RetirementCapture {
            retired: Arc::clone(&retired),
            failure: handler_failure,
        };
        super::super::install_close_request_wiring(
            dispatcher.address,
            &window,
            Some(crate::CloseRequestHandler::new(move |_| {
                let _ = &handler_capture;
                crate::CloseResponse::Close
            })),
        );
        let failure = catch_unwind(AssertUnwindSafe(|| retire(dispatcher))).err();
        assert_eq!(
            failure.is_some(),
            driver_failure.or(handler_failure).is_some(),
            "retirement reports exactly the injected failure"
        );
        assert_eq!(
            failure
                .as_ref()
                .and_then(|payload| payload.downcast_ref::<&str>())
                .copied(),
            driver_failure.or(handler_failure),
            "the first retirement failure stays authoritative"
        );
        assert_eq!(
            retired.load(Ordering::SeqCst),
            2,
            "both native owners retired"
        );
        let next = installed_runtime();
        let scenes = Arc::new(AtomicUsize::new(0));
        let frame = installed_driver(next, counting_sink(Arc::clone(&scenes)), None);
        dispatch_platform_ui_runtime(next, RuntimeTask::Frame(frame.binding))
            .expect("next runtime delivers after retirement");
        assert_eq!(scenes.load(Ordering::SeqCst), 1);
        teardown_platform_ui_runtime();
    }
}

#[test]
fn installed_frame_driver_contract() {
    crate::table_test::run_table(
        "installed_frame_driver_contract",
        &[
            (
                "a_registered_driver_delivers_the_product_scene",
                a_registered_driver_delivers_the_product_scene as fn(),
            ),
            (
                "an_old_frame_binding_never_reaches_a_new_runtime",
                an_old_frame_binding_never_reaches_a_new_runtime as fn(),
            ),
            (
                "close_refuses_async_publication_without_cancelling_the_accepted_frame",
                close_refuses_async_publication_without_cancelling_the_accepted_frame as fn(),
            ),
            (
                "native_terminal_callbacks_refuse_late_publication",
                native_terminal_callbacks_refuse_late_publication as fn(),
            ),
            (
                "a_leased_driver_is_retired_after_reentrant_teardown",
                a_leased_driver_is_retired_after_reentrant_teardown as fn(),
            ),
            (
                "native_owners_retire_independently_after_registry_removal",
                native_owners_retire_independently_after_registry_removal as fn(),
            ),
            (
                "a_driver_failure_remains_first_when_retirement_also_fails",
                a_driver_failure_remains_first_when_retirement_also_fails as fn(),
            ),
        ],
    );
}
