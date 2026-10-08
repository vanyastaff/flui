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
    dispatch_platform_ui_runtime, prepare_replacement_ui_runtime, teardown_platform_ui_runtime,
};
use super::super::window_install::WindowInstall;
use super::{FrameDriver, FrameRegistration, TestFrameDriver};

fn installed_runtime() -> PresentationDispatcher {
    let prepared = prepared_runtime();
    let dispatcher = prepared.dispatcher();
    prepared
        .submit()
        .outcome()
        .expect("idle host")
        .expect("published window");
    dispatcher
}

fn prepared_runtime() -> WindowInstall {
    let runtime = UiRuntime::for_test();
    runtime
        .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
        .expect("mount root");
    prepare_replacement_ui_runtime(
        runtime,
        crate::app::window_test_support::headless_test_window(),
    )
}

fn installed_driver(
    mut installation: WindowInstall,
    sink: ScriptedSink,
    prelude: Option<Box<dyn FnMut()>>,
) -> FrameRegistration {
    let frame = installation
        .frame_driver(FrameDriver::Test(TestFrameDriver {
            installed: None,
            sink,
            prelude,
            resize: None,
        }))
        .expect("prepare frame resources");
    installation
        .submit()
        .outcome()
        .expect("idle host")
        .expect("published window and driver");
    frame
}

fn a_reentrant_rendered_install_publishes_runtime_and_driver_together() {
    let _clear = OwnerHostClearGuard::arm();
    let outer = installed_runtime();
    let installed = Rc::new(RefCell::new(None));
    let completed = Rc::clone(&installed);
    let scenes = Arc::new(AtomicUsize::new(0));
    let delivered_scenes = Arc::clone(&scenes);
    dispatch_platform_ui_runtime(
        outer,
        RuntimeTask::TestCallback(Box::new(move |_| {
            let runtime = UiRuntime::for_test();
            runtime
                .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))
                .expect("mount root");
            let window = crate::app::window_test_support::headless_test_window();
            let mut prepared = super::super::owner_dispatch::prepare_ui_runtime_alongside(
                runtime,
                Arc::clone(&window),
            );
            let dispatcher = prepared.dispatcher();
            let frame = prepared
                .frame_driver(FrameDriver::Test(TestFrameDriver {
                    installed: None,
                    sink: counting_sink(delivered_scenes),
                    prelude: None,
                    resize: None,
                }))
                .expect("prepare frame resources");
            let installation = prepared.submit();
            assert!(
                installation.outcome().is_none(),
                "reentrant admission is not readiness"
            );
            *completed.borrow_mut() = Some((dispatcher, frame.binding, window, installation));
        })),
    )
    .expect("outer operation completes");
    let (dispatcher, binding, _window, installation) = installed
        .borrow_mut()
        .take()
        .expect("installation completed");
    installation
        .outcome()
        .expect("publication and initialization completed")
        .expect("installation succeeded");
    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(binding))
        .expect("new frame accepted");
    assert_eq!(
        scenes.load(Ordering::SeqCst),
        1,
        "the newly published window renders a real scene"
    );
    teardown_platform_ui_runtime();
}

fn counting_sink(count: Arc<AtomicUsize>) -> ScriptedSink {
    ScriptedSink::new(move |_, _| {
        count.fetch_add(1, Ordering::SeqCst);
        SubmitVerdict::Presented
    })
}

fn a_registered_driver_delivers_the_product_scene() {
    let _clear = OwnerHostClearGuard::arm();
    let installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let scenes = Arc::new(AtomicUsize::new(0));
    let frame = installed_driver(installation, counting_sink(Arc::clone(&scenes)), None);
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
    let old_installation = prepared_runtime();
    let old_dispatcher = old_installation.dispatcher();
    let old_scenes = Arc::new(AtomicUsize::new(0));
    let old_frame = installed_driver(
        old_installation,
        counting_sink(Arc::clone(&old_scenes)),
        None,
    );
    let new_installation = prepared_runtime();
    let new_dispatcher = new_installation.dispatcher();
    let new_scenes = Arc::new(AtomicUsize::new(0));
    let new_frame = installed_driver(
        new_installation,
        counting_sink(Arc::clone(&new_scenes)),
        None,
    );
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
    let installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let scenes = Arc::new(AtomicUsize::new(0));
    let authority = Rc::new(RefCell::new(None::<super::FrameLiveness>));
    let published = Rc::new(parking_lot::Mutex::new(None));
    let results = Rc::new(RefCell::new(None));
    let frame = installed_driver(
        installation,
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
                let mut installation = prepare_replacement_ui_runtime(runtime, Arc::clone(&window));
                let dispatcher = installation.dispatcher();
                installation.terminal_callbacks(&owner.shared());
                let scenes = Arc::new(AtomicUsize::new(0));
                let frame = installation
                    .frame_driver(FrameDriver::Test(TestFrameDriver {
                        installed: None,
                        sink: counting_sink(Arc::clone(&scenes)),
                        resize: None,
                        prelude: Some(Box::new(move || {
                            if quit {
                                owner.quit();
                            } else {
                                window.close();
                            }
                        })),
                    }))
                    .expect("prepare native driver");
                installation
                    .submit()
                    .outcome()
                    .expect("fresh host completes inline")
                    .expect("installed");
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

fn a_prepared_single_window_rejects_close_and_quit_before_publication() {
    use flui_platform::{Platform, traits::PlatformWindow};
    for quit in [false, true] {
        let _clear = OwnerHostClearGuard::arm();
        Box::new(flui_platform::HeadlessPlatform::new())
            .run(Box::new(move |owner| {
                let window: Arc<dyn PlatformWindow> = owner
                    .open_window(flui_platform::WindowOptions::default())?
                    .try_ready()?;
                owner.shared().set_exit_policy_hook(Box::new(|| false));
                let mut prepared =
                    prepare_replacement_ui_runtime(UiRuntime::for_test(), Arc::clone(&window));
                prepared.terminal_callbacks(&owner.shared());
                let frame = prepared.frame_driver(FrameDriver::Test(TestFrameDriver {
                    installed: None,
                    sink: counting_sink(Arc::new(AtomicUsize::new(0))),
                    prelude: None,
                    resize: None,
                }))?;
                assert!(
                    !frame.liveness.is_live(),
                    "preparation grants no async publication authority"
                );
                if quit {
                    owner.quit();
                } else {
                    window.close();
                }
                let result = prepared
                    .submit()
                    .outcome()
                    .expect("fresh host settles refusal inline");
                assert!(matches!(
                    result,
                    Err(super::super::installed_host::InstallError::WindowClosed)
                ));
                assert!(
                    !frame.liveness.is_live(),
                    "terminal preparation never publishes its driver"
                );
                teardown_platform_ui_runtime();
                Ok(())
            }))
            .expect("headless preparation");
    }
}

fn a_leased_driver_is_retired_after_reentrant_teardown() {
    let _clear = OwnerHostClearGuard::arm();
    let installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let retired = Arc::new(AtomicUsize::new(0));
    let capture = ReentrantCapture(Arc::clone(&retired));
    let sink = ScriptedSink::new(move |_, _| {
        let _ = &capture;
        SubmitVerdict::Presented
    });
    let frame = installed_driver(
        installation,
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
    let next_installation = prepared_runtime();
    let next = next_installation.dispatcher();
    let scenes = Arc::new(AtomicUsize::new(0));
    let frame = installed_driver(next_installation, counting_sink(Arc::clone(&scenes)), None);
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
    let installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let retired = Arc::new(AtomicUsize::new(0));
    let capture = FailingCapture(Arc::clone(&retired));
    let sink = ScriptedSink::new(move |_, _| {
        let _ = &capture;
        SubmitVerdict::Presented
    });
    let frame = installed_driver(
        installation,
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
    let next_installation = prepared_runtime();
    let next = next_installation.dispatcher();
    let scenes = Arc::new(AtomicUsize::new(0));
    let frame = installed_driver(next_installation, counting_sink(Arc::clone(&scenes)), None);
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

fn native_operation_retains_owners_after_the_last_external_handle_is_released() {
    use super::super::native_bindings::{NativeBindings, PreparedNativeWindow};
    use crate::app::close_request::PreparedCloseRequest;
    use crate::app::window_registry::PreparedWindowRegistration;

    for (resize_failure, driver_failure, handler_failure) in [
        (false, None, None),
        (false, Some("driver retirement"), None),
        (false, None, Some("handler retirement")),
        (false, Some("driver retirement"), Some("handler retirement")),
        (true, Some("driver retirement"), Some("handler retirement")),
    ] {
        let _clear = OwnerHostClearGuard::arm();
        let runtime = UiRuntime::for_test();
        let address = flui_foundation::PresentationAddress {
            ui_runtime_id: runtime.id(),
            presentation_id: runtime.presentation_id(),
        };
        let window = crate::app::window_test_support::headless_test_window();
        let native = NativeBindings::new();
        let retired = Arc::new(AtomicUsize::new(0));
        let driver_capture = RetirementCapture {
            retired: Arc::clone(&retired),
            failure: driver_failure,
        };
        let handler_capture = RetirementCapture {
            retired: Arc::clone(&retired),
            failure: handler_failure,
        };
        let router = native.close_requests();
        let close = PreparedCloseRequest::new(
            address,
            &window,
            Some(crate::app::CloseRequestHandler::new(move |_| {
                let _ = &handler_capture;
                crate::app::CloseResponse::Close
            })),
        );
        let owner = Rc::new(RefCell::new(Some(native)));
        let resize_owner = Rc::clone(&owner);
        let resize_retired = Arc::clone(&retired);
        let operation = owner.borrow().as_ref().expect("native owner").clone();
        let (frame, registration) = super::PreparedFrame::new(
            address,
            FrameDriver::Test(TestFrameDriver {
                installed: None,
                sink: ScriptedSink::new(move |_, _| {
                    let _ = &driver_capture;
                    SubmitVerdict::Presented
                }),
                prelude: None,
                resize: Some(Box::new(move |_, _| {
                    let outgoing = resize_owner.borrow_mut().take();
                    drop(outgoing);
                    assert_eq!(resize_retired.load(Ordering::SeqCst), 0);
                    if resize_failure {
                        std::panic::panic_any("resize failure");
                    }
                })),
            }),
        );
        operation
            .publish(
                address,
                PreparedNativeWindow {
                    registration: PreparedWindowRegistration::new(&window),
                    frame: Some(frame),
                    close: Some(close),
                },
            )
            .unwrap_or_else(|_| panic!("publish complete native window"));
        let failure = catch_unwind(AssertUnwindSafe(move || {
            operation.resize(
                address,
                flui_foundation::geometry::Size::new(80.0, 60.0),
                1.0,
            );
        }))
        .err();
        assert_eq!(
            failure
                .as_ref()
                .and_then(|payload| payload.downcast_ref::<&str>())
                .copied(),
            if resize_failure {
                Some("resize failure")
            } else {
                driver_failure.or(handler_failure)
            }
        );
        assert_eq!(
            retired.load(Ordering::SeqCst),
            2,
            "both native owners released"
        );
        assert!(!registration.liveness.is_live());
        assert!(
            router.take(address).is_none(),
            "outliving router has no stale handler"
        );
        assert!(owner.borrow().is_none());
    }
}

fn resize_uses_the_installed_driver_lease_through_failure_and_teardown() {
    use super::super::owner_dispatch::RuntimeEvent;
    use std::cell::Cell;

    for (close, fail, retire_failure) in [
        (false, false, None),
        (false, true, None),
        (true, false, None),
        (true, true, None),
        (true, true, Some("resize resource retirement")),
    ] {
        let _clear = OwnerHostClearGuard::arm();
        let mut installation = prepared_runtime();
        let dispatcher = installation.dispatcher();
        let retired = Arc::new(AtomicUsize::new(0));
        let capture = RetirementCapture {
            retired: Arc::clone(&retired),
            failure: retire_failure,
        };
        let scenes = Arc::new(AtomicUsize::new(0));
        let scene_count = Arc::clone(&scenes);
        let sink = ScriptedSink::new(move |_, _| {
            let _ = &capture;
            scene_count.fetch_add(1, Ordering::SeqCst);
            SubmitVerdict::Presented
        });
        let calls = Rc::new(Cell::new(0));
        let resize_calls = Rc::clone(&calls);
        let during_resize = Arc::clone(&retired);
        let frame = installation
            .frame_driver(FrameDriver::Test(TestFrameDriver {
                installed: None,
                sink,
                prelude: None,
                resize: Some(Box::new(move |size, scale| {
                    assert_eq!(size, flui_foundation::geometry::Size::new(320.0, 200.0));
                    assert_eq!(scale, 2.0);
                    resize_calls.set(resize_calls.get() + 1);
                    if close {
                        teardown_platform_ui_runtime();
                    }
                    assert_eq!(
                        during_resize.load(Ordering::SeqCst),
                        0,
                        "active resize retains native resources"
                    );
                    if fail && resize_calls.get() == 1 {
                        std::panic::panic_any("resize failure");
                    }
                })),
            }))
            .expect("prepare resize driver");
        installation
            .submit()
            .outcome()
            .expect("idle host")
            .expect("published resize driver");
        let resize = || {
            dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Event(RuntimeEvent::Resized {
                    size: flui_foundation::geometry::Size::new(320.0, 200.0),
                    scale_factor: 2.0,
                }),
            )
        };
        let failure = catch_unwind(AssertUnwindSafe(resize)).err();
        assert_eq!(
            failure
                .as_ref()
                .and_then(|failure| failure.downcast_ref::<&str>())
                .copied(),
            if fail { Some("resize failure") } else { None }
        );
        assert_eq!(calls.get(), 1, "resize reached the installed driver");
        if close {
            assert_eq!(retired.load(Ordering::SeqCst), 1);
            assert!(!frame.liveness.is_live());
            let next_installation = prepared_runtime();
            let next = next_installation.dispatcher();
            let next_frame =
                installed_driver(next_installation, counting_sink(Arc::clone(&scenes)), None);
            dispatch_platform_ui_runtime(next, RuntimeTask::Frame(next_frame.binding))
                .expect("new binding runs");
        } else {
            resize().expect("resize restored after failure");
            assert_eq!(calls.get(), 2);
            dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
                .expect("driver still produces frames");
        }
        assert_eq!(scenes.load(Ordering::SeqCst), 1);
        teardown_platform_ui_runtime();
        assert_eq!(retired.load(Ordering::SeqCst), 1);
    }
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
        let mut installation = prepare_replacement_ui_runtime(UiRuntime::for_test(), window);
        let dispatcher = installation.dispatcher();
        let retired = Arc::new(AtomicUsize::new(0));
        let driver_capture = RetirementCapture {
            retired: Arc::clone(&retired),
            failure: driver_failure,
        };
        installation
            .frame_driver(FrameDriver::Test(TestFrameDriver {
                installed: None,
                sink: ScriptedSink::new(move |_, _| {
                    let _ = &driver_capture;
                    SubmitVerdict::Presented
                }),
                prelude: None,
                resize: None,
            }))
            .expect("prepare retirement driver");
        let handler_capture = RetirementCapture {
            retired: Arc::clone(&retired),
            failure: handler_failure,
        };
        installation.close_requests(Some(crate::app::CloseRequestHandler::new(move |_| {
            let _ = &handler_capture;
            crate::app::CloseResponse::Close
        })));
        installation
            .submit()
            .outcome()
            .expect("idle host completes installation")
            .expect("joint driver and close handler publication");
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
        let next_installation = prepared_runtime();
        let next = next_installation.dispatcher();
        let scenes = Arc::new(AtomicUsize::new(0));
        let frame = installed_driver(next_installation, counting_sink(Arc::clone(&scenes)), None);
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
            #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
            (
                "agent_notification_follows_live_native_initialization",
                agent_notification_follows_live_native_initialization as fn(),
            ),
            (
                "unfinished_installation_closes_its_native_window",
                unfinished_installation_closes_its_native_window as fn(),
            ),
            (
                "unpublished_window_retires_native_resources_independently",
                unpublished_window_retires_native_resources_independently as fn(),
            ),
            (
                "a_reentrant_rendered_install_publishes_runtime_and_driver_together",
                a_reentrant_rendered_install_publishes_runtime_and_driver_together as fn(),
            ),
            (
                "native_operation_retains_owners_after_the_last_external_handle_is_released",
                native_operation_retains_owners_after_the_last_external_handle_is_released
                    as fn(),
            ),
            (
                "resize_uses_the_installed_driver_lease_through_failure_and_teardown",
                resize_uses_the_installed_driver_lease_through_failure_and_teardown as fn(),
            ),
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
                "a_prepared_single_window_rejects_close_and_quit_before_publication",
                a_prepared_single_window_rejects_close_and_quit_before_publication as fn(),
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

fn unpublished_window_retires_native_resources_independently() {
    struct Capture {
        retired: Arc<AtomicUsize>,
        failure: Option<&'static str>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.retired.fetch_add(1, Ordering::SeqCst);
            if let Some(failure) = self.failure {
                std::panic::panic_any(failure);
            }
        }
    }
    for (deferred, incoming_failure) in [(false, false), (true, false), (false, true), (true, true)]
    {
        for (driver_failure, handler_failure) in [
            (None, None),
            (Some("unpublished driver"), None),
            (None, Some("unpublished handler")),
            (Some("unpublished driver"), Some("unpublished handler")),
        ] {
            let _clear = OwnerHostClearGuard::arm();
            let outer = installed_runtime();
            let retired = Arc::new(AtomicUsize::new(0));
            let driver = Capture {
                retired: Arc::clone(&retired),
                failure: driver_failure,
            };
            let handler = Capture {
                retired: Arc::clone(&retired),
                failure: handler_failure,
            };
            let mut installation = super::super::owner_dispatch::prepare_ui_runtime_alongside(
                UiRuntime::for_test(),
                crate::app::window_test_support::headless_test_window(),
            );
            let frame = installation
                .frame_driver(FrameDriver::Test(TestFrameDriver {
                    installed: None,
                    sink: ScriptedSink::new(move |_, _| {
                        let _ = &driver;
                        SubmitVerdict::Presented
                    }),
                    prelude: None,
                    resize: None,
                }))
                .expect("prepare unpublished driver");
            installation.close_requests(Some(crate::app::CloseRequestHandler::new(move |_| {
                let _ = &handler;
                crate::app::CloseResponse::Close
            })));
            let failure = catch_unwind(AssertUnwindSafe(|| {
                if deferred {
                    dispatch_platform_ui_runtime(
                        outer,
                        RuntimeTask::TestCallback(Box::new(move |_| {
                            let receipt = installation.submit();
                            assert!(receipt.outcome().is_none());
                            drop(receipt);
                            assert!(!incoming_failure, "preparation failure");
                        })),
                    )
                    .expect("admit cancellation");
                } else if incoming_failure {
                    let _installation = installation;
                    panic!("preparation failure");
                } else {
                    drop(installation);
                }
            }))
            .err();
            assert_eq!(
                failure
                    .as_ref()
                    .and_then(|failure| failure.downcast_ref::<&str>())
                    .copied(),
                if incoming_failure {
                    Some("preparation failure")
                } else {
                    driver_failure.or(handler_failure)
                }
            );
            if deferred && incoming_failure {
                // The failed callback preserves its accepted FIFO tail. The
                // next owner opportunity retires the cancelled proposal.
                let continuation = catch_unwind(AssertUnwindSafe(|| {
                    dispatch_platform_ui_runtime(
                        outer,
                        RuntimeTask::TestCallback(Box::new(|_| {})),
                    )
                    .expect("resume accepted cancellation");
                }))
                .err();
                assert_eq!(
                    continuation
                        .as_ref()
                        .and_then(|failure| failure.downcast_ref::<&str>())
                        .copied(),
                    driver_failure.or(handler_failure)
                );
            }
            assert_eq!(
                retired.load(Ordering::SeqCst),
                2,
                "both prepared resources retire; deferred={deferred}"
            );
            assert!(!frame.liveness.is_live());
            let next = prepared_runtime();
            let dispatcher = next.dispatcher();
            let scenes = Arc::new(AtomicUsize::new(0));
            let frame = installed_driver(next, counting_sink(Arc::clone(&scenes)), None);
            dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
                .expect("host recovers after abandoned resources");
            assert_eq!(scenes.load(Ordering::SeqCst), 1);
        }
    }
}

fn unfinished_installation_closes_its_native_window() {
    use super::super::owner_dispatch::RuntimeEvent;
    for (deferred, publish) in [(false, false), (true, false), (false, true)] {
        let _clear = OwnerHostClearGuard::arm();
        let outer = installed_runtime();
        let window = crate::app::window_test_support::headless_test_window();
        let closed = Arc::new(AtomicUsize::new(0));
        let closing = Arc::clone(&closed);
        let installation = super::super::owner_dispatch::prepare_ui_runtime_alongside(
            UiRuntime::for_test(),
            Arc::clone(&window),
        );
        installation.on_close(move || {
            dispatch_platform_ui_runtime(
                outer,
                RuntimeTask::Event(RuntimeEvent::WindowFocus(false)),
            )
            .expect("native close can reenter the same owner without a registry borrow");
            closing.fetch_add(1, Ordering::SeqCst);
        });
        if publish {
            installation
                .submit()
                .outcome()
                .expect("idle host completes publication")
                .expect("successful window installation");
        } else if deferred {
            dispatch_platform_ui_runtime(
                outer,
                RuntimeTask::TestCallback(Box::new(move |_| {
                    let receipt = installation.submit();
                    assert!(receipt.outcome().is_none());
                    drop(receipt);
                })),
            )
            .expect("drain cancelled installation");
        } else {
            drop(installation);
        }
        assert_eq!(
            closed.load(Ordering::SeqCst),
            usize::from(!publish),
            "only unfinished installation owns native closure; deferred={deferred}, publish={publish}"
        );
        if publish {
            window.close();
            assert_eq!(
                closed.load(Ordering::SeqCst),
                1,
                "published window stays open until explicitly closed"
            );
        }
        drop(window);
    }
}

#[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
fn agent_notification_follows_live_native_initialization() {
    use flui_platform::traits::PlatformWindow;
    use flui_view::dev_agent::{AgentWindow, DevAgentHook};

    #[derive(Clone, Copy, Debug)]
    enum DuringInstall {
        Keep,
        Close,
        Teardown,
        Panic,
        CloseAndPanic,
        CloseFromAgent,
    }

    struct Agent {
        opened: Arc<AtomicUsize>,
        live: Arc<AtomicUsize>,
        close: Option<Arc<dyn PlatformWindow>>,
    }
    impl DevAgentHook for Agent {
        fn attach(&mut self) -> bool {
            true
        }
        fn detach(&mut self) {}
        fn window_opened(&mut self, window: AgentWindow) {
            self.opened.fetch_add(1, Ordering::SeqCst);
            self.live
                .fetch_add(usize::from(window.is_open()), Ordering::SeqCst);
            if let Some(native) = self.close.take() {
                native.close();
            }
        }
    }

    for case in [
        DuringInstall::Keep,
        DuringInstall::Close,
        DuringInstall::Teardown,
        DuringInstall::Panic,
        DuringInstall::CloseAndPanic,
        DuringInstall::CloseFromAgent,
    ] {
        let _clear = OwnerHostClearGuard::arm();
        let window = crate::app::window_test_support::headless_test_window();
        let opened = Arc::new(AtomicUsize::new(0));
        let live = Arc::new(AtomicUsize::new(0));
        let agent = crate::app::dev_agent::DevAgent::new(Agent {
            opened: Arc::clone(&opened),
            live: Arc::clone(&live),
            close: matches!(case, DuringInstall::CloseFromAgent).then(|| Arc::clone(&window)),
        });
        let _attachment = agent.attach().expect("attach development agent");
        let runtime = UiRuntime::for_test();
        runtime
            .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
            .expect("mount root");
        let agent_window = agent
            .vend(&runtime, runtime.presentation_id())
            .expect("vend mounted presentation");
        let mut installation = prepare_replacement_ui_runtime(runtime, Arc::clone(&window));
        installation.dev_agent(Some((agent, agent_window)));
        let native = Arc::clone(&window);
        installation
            .frame_driver(FrameDriver::Test(TestFrameDriver {
                sink: ScriptedSink::always_presents(),
                installed: Some(Box::new(move || match case {
                    DuringInstall::Keep | DuringInstall::CloseFromAgent => {}
                    DuringInstall::Close => native.close(),
                    DuringInstall::Teardown => teardown_platform_ui_runtime(),
                    DuringInstall::Panic => panic!("native initialization failed"),
                    DuringInstall::CloseAndPanic => {
                        native.close();
                        panic!("native initialization failed");
                    }
                })),
                prelude: None,
                resize: None,
            }))
            .expect("prepare native driver");
        let completed = catch_unwind(AssertUnwindSafe(|| installation.submit()));
        if matches!(case, DuringInstall::Panic | DuringInstall::CloseAndPanic) {
            let failure = completed.err().expect("native failure stays authoritative");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&"native initialization failed")
            );
        } else {
            let result = completed
                .expect("installation is contained")
                .outcome()
                .expect("initialization completed");
            assert_eq!(
                result.is_ok(),
                matches!(case, DuringInstall::Keep),
                "{case:?}"
            );
        }
        assert_eq!(
            opened.load(Ordering::SeqCst),
            usize::from(matches!(
                case,
                DuringInstall::Keep | DuringInstall::CloseFromAgent
            )),
            "only live native initialization can notify the agent: {case:?}"
        );
        assert_eq!(
            live.load(Ordering::SeqCst),
            opened.load(Ordering::SeqCst),
            "every notified window is live at handover: {case:?}"
        );
        teardown_platform_ui_runtime();
        let next = prepared_runtime();
        let dispatcher = next.dispatcher();
        let scenes = Arc::new(AtomicUsize::new(0));
        let frame = installed_driver(next, counting_sink(Arc::clone(&scenes)), None);
        dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(frame.binding))
            .expect("the next installation can render after recovery");
        assert_eq!(scenes.load(Ordering::SeqCst), 1, "{case:?}");
        teardown_platform_ui_runtime();
    }
}
