//! Private driver fault matrix. Scene assertions use the product pump/sink;
//! the seam injects native prelude and capture destruction failures without GPU.

use crate::app::runner::window_routes::OwnerOpenRouted as _;
use flui_testing::window_commands::AdmitAndApply as _;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use flui_runtime::{sink::SubmitVerdict, testing::ScriptedSink, ui_runtime::UiRuntime};

use super::super::host::OwnerHostClearGuard;
use super::super::owner_dispatch::{
    DispatchError, PresentationDispatcher, RuntimeTask, close_this_window, dispatch_platform_frame,
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
                Rc::clone(&window),
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
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::Frame(binding, flui_native::frame_request_now()),
    )
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
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
    )
    .expect("deliver frame");
    assert_eq!(
        scenes.load(Ordering::SeqCst),
        1,
        "the product scene left through the installed sink"
    );
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
    )
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
        dispatch_platform_ui_runtime(
            old_dispatcher,
            RuntimeTask::Frame(old_frame.binding, flui_native::frame_request_now())
        ),
        Err(DispatchError::StaleRuntime)
    );
    assert_eq!(
        dispatch_platform_ui_runtime(
            new_dispatcher,
            RuntimeTask::Frame(old_frame.binding, flui_native::frame_request_now())
        ),
        Err(DispatchError::StalePresentation)
    );
    dispatch_platform_ui_runtime(
        new_dispatcher,
        RuntimeTask::Frame(new_frame.binding, flui_native::frame_request_now()),
    )
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

/// A driver that counts the frames it runs, whether or not one has a scene
/// to present, installed for `installation`.
fn frame_counting_driver(installation: WindowInstall) -> (FrameRegistration, Rc<Cell<u32>>) {
    let frames = Rc::new(Cell::new(0));
    let ran = Rc::clone(&frames);
    let frame = installed_driver(
        installation,
        counting_sink(Arc::default()),
        Some(Box::new(move || ran.set(ran.get() + 1))),
    );
    (frame, frames)
}

/// What a window's frame route answers the platform: `Produced` for a frame
/// the owner ran, or took into the turn that is running, and `Declined` for a
/// request whose presentation is gone. The platform asks again for work due
/// at one instant after a frame that ran, so a refused request answered
/// `Produced` would have it ask on every turn of its loop.
fn a_frame_request_is_answered_by_what_became_of_it() {
    use flui_platform::FrameOutcome;

    let _clear = OwnerHostClearGuard::arm();
    let replaced = prepared_runtime();
    let replaced_dispatcher = replaced.dispatcher();
    let replaced_frame = installed_driver(replaced, counting_sink(Arc::default()), None);
    let installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let (frame, frames) = frame_counting_driver(installation);
    let binding = frame.binding;
    let ask =
        move || dispatch_platform_frame(dispatcher, binding, flui_native::frame_request_now());

    // (case, dispatcher, binding, answer)
    let rows = [
        (
            "a runtime that was replaced",
            replaced_dispatcher,
            replaced_frame.binding,
            FrameOutcome::Declined,
        ),
        (
            "a binding of another presentation",
            dispatcher,
            replaced_frame.binding,
            FrameOutcome::Declined,
        ),
        (
            "the installed presentation",
            dispatcher,
            binding,
            FrameOutcome::Produced,
        ),
    ];
    for (case, dispatcher, binding, answer) in rows {
        assert_eq!(
            dispatch_platform_frame(dispatcher, binding, flui_native::frame_request_now()),
            answer,
            "{case}"
        );
    }
    assert_eq!(
        frames.get(),
        1,
        "only the installed presentation ran a frame"
    );

    // Asked from inside the owner turn that is running, the frame joins that
    // turn: the owner accepted it, so it counts, and the turn runs it once.
    let nested = Rc::new(Cell::new(None));
    let (answered, counted) = (Rc::clone(&nested), Rc::clone(&frames));
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |_| {
            answered.set(Some((ask(), counted.get())));
        })),
    )
    .expect("the outer turn");
    assert_eq!(
        nested.get(),
        Some((FrameOutcome::Produced, 1)),
        "a frame queued behind the running turn is accepted before it runs"
    );
    assert_eq!(
        frames.get(),
        2,
        "the turn ran the frame it took: counted once, run once"
    );

    // A window calls its handler on the thread that opened it: a request from
    // another one is this crate's defect, never a quiet refusal.
    let off_thread = std::thread::spawn(ask)
        .join()
        .expect_err("a frame request off the owner thread was answered");
    assert!(
        flui_foundation::panic::payload_text(off_thread.as_ref())
            .is_some_and(|text| text.starts_with("BUG:")),
        "the request off the owner thread names its invariant"
    );

    // A presentation whose close the owner accepted refuses the frame asked
    // for behind it, and once it is closed no runtime is left to ask.
    let closing = Rc::new(Cell::new(None));
    let answered = Rc::clone(&closing);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |_| {
            close_this_window(dispatcher);
            answered.set(Some(ask()));
        })),
    )
    .expect("the closing turn");
    assert_eq!(
        closing.get(),
        Some(FrameOutcome::Declined),
        "a closing presentation"
    );
    assert_eq!(ask(), FrameOutcome::Declined, "a host with no runtime left");
    assert_eq!(frames.get(), 2, "a refused request ran a frame");

    // A frame the running turn took is dropped when the host shuts before
    // the turn reaches it. The platform counted it once; the request that
    // count brings is refused, so nothing is asked for twice.
    let installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let (frame, frames) = frame_counting_driver(installation);
    let binding = frame.binding;
    let ask =
        move || dispatch_platform_frame(dispatcher, binding, flui_native::frame_request_now());
    let shut = Rc::new(Cell::new(None));
    let answered = Rc::clone(&shut);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::TestCallback(Box::new(move |_| {
            answered.set(Some(ask()));
            teardown_platform_ui_runtime();
        })),
    )
    .expect("the turn that shuts the host");
    assert_eq!(
        shut.get(),
        Some(FrameOutcome::Produced),
        "a frame queued behind the turn that shuts the host"
    );
    assert_eq!(frames.get(), 0, "a shut host ran the frame it had queued");
    assert_eq!(ask(), FrameOutcome::Declined, "a shut host");
}

fn close_refuses_async_publication_without_cancelling_the_accepted_frame() {
    let _clear = OwnerHostClearGuard::arm();
    let installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let scenes = Arc::new(AtomicUsize::new(0));
    let authority = Rc::new(RefCell::new(None::<super::FrameLiveness>));
    let results = Rc::new(RefCell::new(None));
    let frame = installed_driver(
        installation,
        counting_sink(Arc::clone(&scenes)),
        Some(Box::new({
            let authority = Rc::clone(&authority);
            let results = Rc::clone(&results);
            move || {
                let authority = authority.borrow().clone().expect("registered authority");
                let before = authority.is_live();
                close_this_window(dispatcher);
                let after = authority.is_live();
                *results.borrow_mut() = Some((before, after));
            }
        })),
    );
    *authority.borrow_mut() = Some(frame.liveness);
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
    )
    .expect("accepted frame completes before close");
    assert_eq!(
        *results.borrow(),
        Some((true, false)),
        "an async result is admitted before the close and refused after it"
    );
    assert_eq!(
        scenes.load(Ordering::SeqCst),
        1,
        "close preserved the accepted frame"
    );
    teardown_platform_ui_runtime();
}

struct ReentrantCapture(Arc<AtomicUsize>);

/// Run a headless platform whose bootstrap only keeps the owner capability,
/// then `scenario` with it on the same thread, as the owner's later turns run:
/// the quit a scenario asks for reaches the handler at once, not after a
/// bootstrap still running.
fn after_bootstrap(
    scenario: impl FnOnce(Rc<flui_native::OwnerPlatform>) -> Result<(), flui_native::BootstrapError>,
) {
    use flui_native::Platform;

    let kept = Rc::new(std::cell::RefCell::new(None));
    let keep = Rc::clone(&kept);
    Box::new(flui_native::testing::HeadlessPlatform::new())
        .run(crate::app::runner::app_handler(move |owner| {
            keep.replace(Some(owner));
            Ok(())
        }))
        .expect("headless run");
    let owner = Rc::new(kept.take().expect("the bootstrap kept the owner"));
    scenario(owner).expect("the scenario");
}

fn native_terminal_callbacks_refuse_late_publication() {
    use flui_platform::PlatformWindow;

    for quit in [false, true] {
        let _clear = OwnerHostClearGuard::arm();
        after_bootstrap(move |owner| {
            let window: Rc<dyn PlatformWindow> = owner
                .open_routed(flui_platform::WindowOptions::default())?
                .try_ready()?;
            let runtime = UiRuntime::for_test();
            runtime.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))?;
            let mut installation = prepare_replacement_ui_runtime(runtime, Rc::clone(&window));
            let dispatcher = installation.dispatcher();
            installation.terminal_callbacks();
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
                            window.close_now();
                        }
                    })),
                }))
                .expect("prepare native driver");
            installation
                .submit()
                .outcome()
                .expect("fresh host completes inline")
                .expect("installed");
            dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
            )
            .expect("already accepted frame finishes");
            assert!(
                !frame.liveness.is_live(),
                "an async result is refused once the window is gone: quit={quit}"
            );
            assert_eq!(scenes.load(Ordering::SeqCst), 1, "quit={quit}");
            teardown_platform_ui_runtime();
            Ok(())
        });
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
    use flui_platform::PlatformWindow;
    for quit in [false, true] {
        let _clear = OwnerHostClearGuard::arm();
        after_bootstrap(move |owner| {
            let window: Rc<dyn PlatformWindow> = owner
                .open_routed(flui_platform::WindowOptions::default())?
                .try_ready()?;
            let mut prepared =
                prepare_replacement_ui_runtime(UiRuntime::for_test(), Rc::clone(&window));
            prepared.terminal_callbacks();
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
                window.close_now();
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
        });
    }
}

/// Android's Back: `Destroy` fires the window's native close, which retires its
/// sole presentation and with it the UI runtime; the loop then exits and runs
/// the quit hook. A `quit()` with no `Destroy` before it leaves the close to
/// the hook.
fn a_quit_after_the_native_close_dispatches_no_second_close() {
    use flui_platform::PlatformWindow;

    for native_close in [true, false] {
        let _clear = OwnerHostClearGuard::arm();
        after_bootstrap(move |owner| {
            let window: Rc<dyn PlatformWindow> = owner
                .open_routed(flui_platform::WindowOptions::default())?
                .try_ready()?;
            let runtime = UiRuntime::for_test();
            runtime.attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))?;
            let installation = prepare_replacement_ui_runtime(runtime, Rc::clone(&window));
            installation.terminal_callbacks();
            installation
                .submit()
                .outcome()
                .expect("fresh host completes inline")
                .expect("installed");
            let runtimes = || {
                super::super::host::APP_RUNTIME
                    .with(|slot| slot.borrow().installed_host.logical().runtime_count())
            };
            if native_close {
                window.close_now();
                assert_eq!(runtimes(), 0, "the native close retired the runtime");
            }
            let ((), log) = flui_testing::log_capture::capture(|| owner.quit());
            assert!(
                log.contains("Quitting headless platform"),
                "vacuous-pass guard: the quit ran inside the capture\n{log}"
            );
            let complaints = log
                .records()
                .iter()
                .filter(|record| record.level <= tracing::Level::WARN)
                .count();
            assert_eq!(
                complaints,
                0,
                "native_close={native_close}\n{}",
                log.render_at_least(tracing::Level::WARN)
            );
            assert_eq!(runtimes(), 0, "native_close={native_close}");
            teardown_platform_ui_runtime();
            Ok(())
        });
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
    dispatch_platform_ui_runtime(
        dispatcher,
        RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
    )
    .expect("accepted delivery finishes cleanup");
    assert_eq!(
        retired.load(Ordering::SeqCst),
        1,
        "leased resources retire once after the callback"
    );
    assert_eq!(
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::Frame(frame.binding, flui_native::frame_request_now())
        ),
        Err(DispatchError::RuntimeUnavailable)
    );
    let next_installation = prepared_runtime();
    let next = next_installation.dispatcher();
    let scenes = Arc::new(AtomicUsize::new(0));
    let frame = installed_driver(next_installation, counting_sink(Arc::clone(&scenes)), None);
    dispatch_platform_ui_runtime(
        next,
        RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
    )
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
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
        )
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
    dispatch_platform_ui_runtime(
        next,
        RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
    )
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
    use crate::app::window_registry::PreparedWindowRegistration;
    use flui_runtime::owner_turn::PreparedCloseRequest;

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
                resize: Some(Box::new(move |_| {
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
                    text_sizing: None,
                    registration: PreparedWindowRegistration::new(&window),
                    frame: Some(frame),
                    close: Some(close),
                },
            )
            .unwrap_or_else(|_| panic!("publish complete native window"));
        let failure = catch_unwind(AssertUnwindSafe(move || {
            operation.resize(address, window_metrics(80.0, 60.0, 1.0));
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
                resize: Some(Box::new(move |metrics| {
                    assert_eq!(metrics, window_metrics(320.0, 200.0, 2.0));
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
                RuntimeTask::Event(RuntimeEvent::Resized(window_metrics(320.0, 200.0, 2.0))),
            )
        };
        let failure = catch_unwind(AssertUnwindSafe(resize)).err();
        assert_eq!(
            failure
                .as_ref()
                .and_then(|failure| failure.downcast_ref::<&str>())
                .copied(),
            // A resize's panic is its operation's, contained (ADR-0258 §1),
            // unless the host shut down in it, which ended the loop.
            if fail && close {
                Some("resize failure")
            } else {
                None
            }
        );
        assert_eq!(calls.get(), 1, "resize reached the installed driver");
        if close {
            assert_eq!(retired.load(Ordering::SeqCst), 1);
            assert!(!frame.liveness.is_live());
            let next_installation = prepared_runtime();
            let next = next_installation.dispatcher();
            let next_frame =
                installed_driver(next_installation, counting_sink(Arc::clone(&scenes)), None);
            dispatch_platform_ui_runtime(
                next,
                RuntimeTask::Frame(next_frame.binding, flui_native::frame_request_now()),
            )
            .expect("new binding runs");
        } else {
            resize().expect("resize restored after failure");
            assert_eq!(calls.get(), 2);
            dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
            )
            .expect("driver still produces frames");
        }
        assert_eq!(scenes.load(Ordering::SeqCst), 1);
        teardown_platform_ui_runtime();
        assert_eq!(retired.load(Ordering::SeqCst), 1);
    }
}

fn a_resize_that_finds_the_driver_leased_reaches_it_at_the_next_checkout() {
    let _clear = OwnerHostClearGuard::arm();
    let mut installation = prepared_runtime();
    let dispatcher = installation.dispatcher();
    let sizes = Rc::new(RefCell::new(Vec::new()));
    let seen = Rc::clone(&sizes);
    let frame = installation
        .frame_driver(FrameDriver::Test(TestFrameDriver {
            installed: None,
            sink: counting_sink(Arc::new(AtomicUsize::new(0))),
            prelude: None,
            resize: Some(Box::new(move |metrics| {
                seen.borrow_mut().push(metrics);
                if seen.borrow().len() == 1 {
                    std::panic::panic_any("deferred resize failure");
                }
            })),
        }))
        .expect("prepare resize driver");
    installation
        .submit()
        .outcome()
        .expect("idle host")
        .expect("published resize driver");
    let host = super::super::host::APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    // Hold the published driver out on a lease, as a frame in progress does.
    let drivers = super::FrameDrivers {
        records: vec![frame.liveness.0.upgrade().expect("live record")],
        retirement: host.native().retirement(),
    };
    let lease = drivers
        .checkout_presentation(dispatcher.address)
        .expect("lease the installed driver");
    let size = |width| window_metrics(width, 200.0, 2.0);
    host.native().resize(dispatcher.address, size(300.0));
    host.native().resize(dispatcher.address, size(320.0));
    assert!(
        sizes.borrow().is_empty(),
        "the leased driver is not reached"
    );
    drop(lease);

    let frame_now = || {
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
        )
    };
    // The resize's panic is the frame operation's, contained (ADR-0258 §1).
    frame_now().expect("the resize's panic is contained");
    assert_eq!(
        *sizes.borrow(),
        [size(320.0)],
        "the latest resize reaches the driver before its next frame"
    );
    frame_now().expect("deliver frame");
    frame_now().expect("deliver frame");
    assert_eq!(
        *sizes.borrow(),
        [size(320.0), size(320.0)],
        "a resize that panicked is tried again once, and is gone once applied"
    );
    teardown_platform_ui_runtime();
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
        dispatch_platform_ui_runtime(
            next,
            RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
        )
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
            #[cfg(not(target_os = "ios"))]
            (
                "deadlines_survive_a_sibling_install_and_close",
                deadlines_survive_a_sibling_install_and_close as fn(),
            ),
            #[cfg(all(feature = "testing", not(target_os = "ios")))]
            (
                "a_window_whose_frames_are_off_wakes_the_loop_for_none_of_its_surface",
                a_window_whose_frames_are_off_wakes_the_loop_for_none_of_its_surface as fn(),
            ),
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios"))
            ))]
            (
                "the_loop_wakes_for_no_floor_of_a_window_the_raster_thread_settled",
                the_loop_wakes_for_no_floor_of_a_window_the_raster_thread_settled as fn(),
            ),
            #[cfg(all(
                feature = "testing",
                not(any(target_os = "android", target_os = "ios"))
            ))]
            (
                "a_desktop_window_draws_its_scene_plugins_frame_in_place_of_the_widget_frame",
                a_desktop_window_draws_its_scene_plugins_frame_in_place_of_the_widget_frame
                    as fn(),
            ),
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
                "a_resize_that_finds_the_driver_leased_reaches_it_at_the_next_checkout",
                a_resize_that_finds_the_driver_leased_reaches_it_at_the_next_checkout as fn(),
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
                "a_frame_request_is_answered_by_what_became_of_it",
                a_frame_request_is_answered_by_what_became_of_it as fn(),
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
                "a_quit_after_the_native_close_dispatches_no_second_close",
                a_quit_after_the_native_close_dispatches_no_second_close as fn(),
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

#[cfg(not(target_os = "ios"))]
fn deadlines_survive_a_sibling_install_and_close() {
    use super::super::{
        desktop::DesktopWakeDeadlines, first_reveal::FirstReveal, frame_pacing::FallbackWake,
    };
    let _clear = OwnerHostClearGuard::arm();
    // Both reveal deadlines are already due, so unrelated future geometry
    // retries cannot hide their loss in the owner's earliest-deadline fold.
    let now = web_time::Instant::now()
        .checked_sub(web_time::Duration::from_secs(10))
        .expect("clock supports a recent past deadline");
    let mut installed = Vec::new();
    for offset in [0, 2] {
        let reveal = Rc::new(FirstReveal::new());
        assert!(!reveal.after_frame(false, now + web_time::Duration::from_secs(offset)));
        let deadline = reveal.next_deadline().expect("armed reveal");
        let sources = Rc::new(DesktopWakeDeadlines {
            fallback: Rc::new(FallbackWake::new()),
            first_reveal: reveal,
        });
        let mut installation = super::super::owner_dispatch::prepare_ui_runtime_alongside(
            UiRuntime::for_test(),
            crate::app::window_test_support::headless_test_window(),
        );
        let dispatcher = installation.dispatcher();
        let frame = installation
            .frame_driver(FrameDriver::TestDeadlines(sources))
            .expect("deadline driver");
        installation
            .submit()
            .outcome()
            .expect("settled")
            .expect("installed");
        installed.push((dispatcher, deadline, frame));
    }
    let earliest = || super::super::host::APP_RUNTIME.with(|slot| slot.borrow().next_wake());
    assert_eq!(
        earliest(),
        Some(installed[0].1),
        "installing B retains A's earlier deadline"
    );
    let host = super::super::host::APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    // Use the actual published record; this private view exposes the existing
    // checkout operation without adding a test API to native storage.
    let drivers = super::FrameDrivers {
        records: vec![installed[0].2.liveness.0.upgrade().expect("live record")],
        retirement: host.native().retirement(),
    };
    let lease = drivers
        .checkout_presentation(installed[0].0.address)
        .expect("lease the installed native driver");
    assert_eq!(
        earliest(),
        Some(installed[0].1),
        "leasing A's driver does not hide its pacing metadata"
    );
    installed[0]
        .0
        .runtime()
        .lifecycle(flui_scheduler::AppLifecycleState::Hidden)
        .expect("hide A");
    assert_eq!(
        earliest(),
        Some(installed[1].1),
        "hidden A contributes no visual deadline"
    );
    installed[0]
        .0
        .runtime()
        .lifecycle(flui_scheduler::AppLifecycleState::Resumed)
        .expect("resume A");
    assert_eq!(
        earliest(),
        Some(installed[0].1),
        "resume restores the retained retry deadline"
    );
    close_this_window(installed[1].0);
    assert_eq!(
        earliest(),
        Some(installed[0].1),
        "closing B retains A's deadline"
    );
    drop(lease);
    assert_eq!(earliest(), Some(installed[0].1));
    close_this_window(installed[0].0);
    assert_eq!(
        earliest(),
        None,
        "closed presentations contribute no deadline"
    );
    teardown_platform_ui_runtime();
}

/// The loop's own raster host, put back however a row that swapped in its
/// own ends.
#[cfg(all(feature = "testing", not(target_os = "ios")))]
struct LoopRasterHost(Option<super::super::raster_host::SharedRasterHost>);

#[cfg(all(feature = "testing", not(target_os = "ios")))]
impl LoopRasterHost {
    /// Make `shared` the loop's raster host until the answer drops.
    fn swap_in(shared: &super::super::raster_host::SharedRasterHost) -> Self {
        Self(Some(super::super::host::APP_RUNTIME.with(|slot| {
            std::mem::replace(&mut slot.borrow_mut().raster_host, shared.clone())
        })))
    }
}

#[cfg(all(feature = "testing", not(target_os = "ios")))]
impl Drop for LoopRasterHost {
    fn drop(&mut self) {
        if let Some(host) = self.0.take() {
            super::super::host::APP_RUNTIME.with(|slot| slot.borrow_mut().raster_host = host);
        }
    }
}

/// A window whose frames are off wakes the loop for none of its raster work:
/// the surface it is owed is served only by its own frame, so the loop's
/// wake reporting it would hand the platform a past instant that no wake
/// moves. Its frames back on, the loop wakes for it again.
///
/// Red-check: let `AppRuntime::next_wake` count every window's raster work
/// (`wake_deadline(|_| true)`) and the hidden window's deadline is reported.
#[cfg(all(feature = "testing", not(target_os = "ios")))]
fn a_window_whose_frames_are_off_wakes_the_loop_for_none_of_its_surface() {
    use super::super::{
        desktop::DesktopWakeDeadlines,
        first_reveal::FirstReveal,
        frame_pacing::FallbackWake,
        raster_host::{
            RasterFactory, SharedRasterHost,
            tests::{attach_a_window, owe_a_surface},
        },
    };
    let _clear = OwnerHostClearGuard::arm();
    let mut installed = Vec::new();
    for _ in 0..2 {
        let sources = Rc::new(DesktopWakeDeadlines {
            fallback: Rc::new(FallbackWake::new()),
            first_reveal: Rc::new(FirstReveal::new()),
        });
        let mut installation = super::super::owner_dispatch::prepare_ui_runtime_alongside(
            UiRuntime::for_test(),
            crate::app::window_test_support::headless_test_window(),
        );
        let dispatcher = installation.dispatcher();
        let frame = installation
            .frame_driver(FrameDriver::TestDeadlines(sources))
            .expect("deadline driver");
        installation
            .submit()
            .outcome()
            .expect("settled")
            .expect("installed");
        installed.push((dispatcher, frame));
    }
    let slots = flui_engine::testing::ScriptedSlots::new();
    let shared = SharedRasterHost::new(
        RasterFactory::Scripted(slots.clone()),
        flui_engine::RasterPlacement::Inline,
    );
    let _previous = LoopRasterHost::swap_in(&shared);
    let (hidden, shown) = (installed[1].0.address, installed[0].0.address);
    attach_a_window(&shared, shown, 1);
    let owed = owe_a_surface(&shared, &slots, hidden, 2);
    let earliest = || super::super::host::APP_RUNTIME.with(|slot| slot.borrow().next_wake());
    let lifecycle = |state| {
        installed[1]
            .0
            .runtime()
            .lifecycle(state)
            .expect("the window's lifecycle changes");
    };

    lifecycle(flui_scheduler::AppLifecycleState::Hidden);
    let while_hidden = earliest();
    lifecycle(flui_scheduler::AppLifecycleState::Resumed);
    let once_resumed = earliest();

    // Everything the row installed goes before it asserts, so a failed
    // assertion leaves the table's later rows the loop they expect; the
    // loop's own raster host comes back however the row ends.
    close_this_window(installed[1].0);
    close_this_window(installed[0].0);
    teardown_platform_ui_runtime();

    assert_ne!(
        while_hidden,
        Some(owed),
        "no other window's wake reports the hidden window's surface"
    );
    assert_eq!(once_resumed, Some(owed), "its own wake reports it again");
}

/// The loop's wake waives the in-flight floor of a window whose frame the
/// raster thread settled and whose wake has returned, beside a window whose
/// frame the thread still renders, whose floor it keeps (ADR-0305 §5).
///
/// Red-check: let `AppRuntime::next_wake` read `next_wake()` instead of
/// `next_wake_owing_floors` and the settled window's floor is reported.
#[cfg(all(
    feature = "testing",
    not(any(target_os = "android", target_os = "ios"))
))]
fn the_loop_wakes_for_no_floor_of_a_window_the_raster_thread_settled() {
    use super::super::raster_host::{
        RasterSlot,
        tests::{THREAD_WAIT, threaded_host_for_two},
    };
    let _clear = OwnerHostClearGuard::arm();
    let prepared = || {
        let runtime = UiRuntime::for_test();
        runtime
            .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 20.0))
            .expect("the root attaches");
        super::super::owner_dispatch::prepare_ui_runtime_alongside(
            runtime,
            crate::app::window_test_support::headless_test_window(),
        )
    };
    let (mut settled, mut drawn) = (prepared(), prepared());
    let (settled_at, drawn_at) = (settled.dispatcher().address, drawn.dispatcher().address);
    let (shared, slots, wakes) = threaded_host_for_two(settled_at, drawn_at);
    let _previous = LoopRasterHost::swap_in(&shared);
    let install = |installation: &mut WindowInstall| {
        let dispatcher = installation.dispatcher();
        let frame = installation
            .frame_driver(FrameDriver::TestRaster(RasterSlot {
                host: shared.clone(),
                address: dispatcher.address,
                size: (20, 20),
                device: flui_engine::DeviceHealth::Healthy,
                renderer_dead: false,
            }))
            .expect("the raster driver");
        (dispatcher, frame)
    };
    let (settled_window, settled_frame) = install(&mut settled);
    let (drawn_window, drawn_frame) = install(&mut drawn);
    for installation in [settled, drawn] {
        installation
            .submit()
            .outcome()
            .expect("settled")
            .expect("installed");
    }
    let frame_now = |dispatcher, frame: &FrameRegistration| {
        let _outcome =
            dispatch_platform_frame(dispatcher, frame.binding, flui_native::frame_request_now());
    };
    // The first window's frame settles on the thread, whose wake returns.
    frame_now(settled_window, &settled_frame);
    let started = std::time::Instant::now();
    while wakes.load(Ordering::SeqCst) == 0 && started.elapsed() < THREAD_WAIT {
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let woken = wakes.load(Ordering::SeqCst) > 0;
    // The second window's frame stays on the thread.
    let render = slots.hold_next_render();
    frame_now(drawn_window, &drawn_frame);
    let rendering = render.began_within(THREAD_WAIT);
    // Each window's floor, read with every floor counted.
    let floor_of = |dispatcher| {
        let floor = Rc::new(Cell::new(None));
        let read = Rc::clone(&floor);
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(move |ui_runtime| read.set(ui_runtime.next_wake()))),
        )
        .expect("the runtime answers");
        floor.get()
    };
    // A floor rounds up from now in 50 ms steps, so which window's comes
    // first depends on when it is read. Read at a moment the settled
    // window's comes first, with no step crossed during the reads: the loop
    // reporting the earlier floor then tells the waived one from the kept.
    let loop_wake = || super::super::host::APP_RUNTIME.with(|slot| slot.borrow().next_wake());
    let reads_until = std::time::Instant::now() + THREAD_WAIT;
    let mut read = None;
    while read.is_none() && std::time::Instant::now() < reads_until {
        let (settled_floor, drawn_floor) = (floor_of(settled_window), floor_of(drawn_window));
        let earliest = loop_wake();
        let unmoved =
            (floor_of(settled_window), floor_of(drawn_window)) == (settled_floor, drawn_floor);
        if unmoved && settled_floor.is_some() && settled_floor < drawn_floor {
            read = Some((settled_floor, drawn_floor, earliest));
        } else {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    // Everything the row installed goes before it asserts, so a failed
    // assertion leaves the table's later rows the loop they expect.
    render.release();
    close_this_window(settled_window);
    close_this_window(drawn_window);
    teardown_platform_ui_runtime();

    assert!(
        woken,
        "the thread settled the first frame and woke its window"
    );
    assert!(rendering, "the thread renders the second frame");
    let Some((settled_floor, drawn_floor, earliest)) = read else {
        panic!("the settled window's floor never came first: both windows hold a frame in flight");
    };
    assert_ne!(
        earliest, settled_floor,
        "the loop wakes for no floor of the settled window"
    );
    assert_eq!(
        earliest, drawn_floor,
        "it wakes for the rendering window's floor"
    );
}

/// A desktop window whose development reload hook draws a scene shows that
/// scene, lent to its slot, and runs no widget frame; a hook that draws
/// nothing leaves the window its widget frame (ADR-0391). Either way the
/// hook is asked once for the wake's frame, at the window's physical size,
/// the frame request the wake served is consumed, and the frame reveals the
/// window. A hook that panics is contained and the widget frame runs. A
/// window whose frames are off, or whose executor turns on a raster
/// thread, does not ask the hook.
///
/// Red-check: take the `ScenePlugin::own_frame` call out of
/// `DesktopFrameDriver::turn` and the drawing hook is never asked, nothing is
/// lent and the widget frame renders; drop the `PluginFrame` instead of
/// finishing it and the frame request stays scheduled; skip its
/// `after_presented` and the window is never revealed; take the
/// `frames_enabled` or the `lends_scenes` check out of `own_frame` and the
/// hidden or threaded window asks the hook.
#[cfg(all(
    feature = "testing",
    not(any(target_os = "android", target_os = "ios"))
))]
fn a_desktop_window_draws_its_scene_plugins_frame_in_place_of_the_widget_frame() {
    use flui_engine::testing::{ScriptedSlots, SlotCall};
    use flui_view::dev_reload::{DevReloadHook, ReloadEvent, ReloadWake};

    use super::super::{
        desktop::{DesktopFrameDriver, DesktopWakeDeadlines},
        first_reveal::FirstReveal,
        frame_pacing::FallbackWake,
        raster_host::{RasterFactory, RasterSlot, SharedRasterHost, tests::attach_a_window},
    };
    use crate::app::hot_reload::WorkerReload;

    /// What a scene plugin's hook does when it is asked for a frame.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Answer {
        /// Draws an empty scene.
        Draws,
        /// Has no scene to draw.
        DrawsNothing,
        /// Panics, as user code may.
        Panics,
    }

    /// A scene plugin's hook, recording the size of each frame it was asked
    /// for.
    struct SceneHook {
        answer: Answer,
        asked: Arc<parking_lot::Mutex<Vec<(f64, f64)>>>,
    }

    impl DevReloadHook for SceneHook {
        fn attach(&mut self, _wake: ReloadWake) {}

        fn poll(&mut self) -> ReloadEvent {
            ReloadEvent::Unchanged
        }

        #[expect(unsafe_code, reason = "implements `scene_frame`, an `unsafe fn`")]
        unsafe fn scene_frame(
            &mut self,
            width: f64,
            height: f64,
            render: &mut dyn FnMut(&flui_layer::Scene, bool) -> bool,
        ) -> bool {
            self.asked.lock().push((width, height));
            match self.answer {
                Answer::Draws => render(
                    &flui_layer::Scene::new(flui_layer::LayerTree::default()),
                    true,
                ),
                Answer::DrawsNothing => false,
                Answer::Panics => std::panic::panic_any("a scene plugin's hook panics"),
            }
        }
    }

    /// One wake of the window: what its hook answers, whether its frames
    /// are on, and where its executor turns.
    #[derive(Debug, Clone, Copy)]
    struct Case {
        answer: Answer,
        shown: bool,
        placement: flui_engine::RasterPlacement,
    }
    /// What the wake made of the window's frame.
    #[derive(Debug, PartialEq)]
    struct Seen {
        asked: Vec<(f64, f64)>,
        lent: usize,
        /// Widget frames the slot rendered; not read on a raster thread,
        /// which renders when it turns.
        rendered: Option<usize>,
        scheduled: Option<bool>,
        revealed: bool,
    }

    const SIZE: (u32, u32) = (20, 30);
    let size = (f64::from(SIZE.0), f64::from(SIZE.1));
    let inline = flui_engine::RasterPlacement::Inline;
    for (case, expected) in [
        // The plugin's scene is lent to the window's slot and no widget
        // frame renders beside it.
        (
            Case {
                answer: Answer::Draws,
                shown: true,
                placement: inline,
            },
            Seen {
                asked: vec![size],
                lent: 1,
                rendered: Some(0),
                scheduled: Some(false),
                revealed: true,
            },
        ),
        // A hook that draws nothing lends no scene: the widget frame runs.
        (
            Case {
                answer: Answer::DrawsNothing,
                shown: true,
                placement: inline,
            },
            Seen {
                asked: vec![size],
                lent: 0,
                rendered: Some(1),
                scheduled: Some(false),
                revealed: true,
            },
        ),
        // A hook that panics is contained and dropped: the widget frame runs.
        (
            Case {
                answer: Answer::Panics,
                shown: true,
                placement: inline,
            },
            Seen {
                asked: vec![size],
                lent: 0,
                rendered: Some(1),
                scheduled: Some(false),
                revealed: true,
            },
        ),
        // A window whose frames are off is not drawn: the hook is not asked.
        (
            Case {
                answer: Answer::Draws,
                shown: false,
                placement: inline,
            },
            Seen {
                asked: vec![],
                lent: 0,
                rendered: Some(0),
                scheduled: Some(false),
                revealed: false,
            },
        ),
        // A raster thread cannot draw a lent scene within the call: the hook
        // is not asked, and the widget frame goes to the thread.
        (
            Case {
                answer: Answer::Draws,
                shown: true,
                placement: flui_engine::RasterPlacement::Threaded,
            },
            Seen {
                asked: vec![],
                lent: 0,
                rendered: None,
                scheduled: Some(false),
                revealed: false,
            },
        ),
    ] {
        let Case {
            answer,
            shown,
            placement,
        } = case;
        let _clear = OwnerHostClearGuard::arm();
        let slots = ScriptedSlots::new();
        let shared = SharedRasterHost::new(RasterFactory::Scripted(slots.clone()), placement);
        let _previous = LoopRasterHost::swap_in(&shared);
        let runtime = UiRuntime::for_test();
        runtime
            .attach_root_widget(&flui_widgets::SizedBox::new(20.0, 30.0))
            .expect("the root attaches");
        let window = crate::app::window_test_support::headless_test_window();
        let mut installation =
            super::super::owner_dispatch::prepare_ui_runtime_alongside(runtime, Rc::clone(&window));
        let dispatcher = installation.dispatcher();
        attach_a_window(&shared, dispatcher.address, 1);
        let first_reveal = Rc::new(FirstReveal::new());
        let asked = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let reload =
            WorkerReload::from_config(&crate::app::AppConfig::new().dev_reload(SceneHook {
                answer,
                asked: Arc::clone(&asked),
            }));
        let frame = installation
            .frame_driver(FrameDriver::Desktop(DesktopFrameDriver {
                deadlines: Rc::new(DesktopWakeDeadlines {
                    fallback: Rc::new(FallbackWake::new()),
                    first_reveal: Rc::clone(&first_reveal),
                }),
                raster: RasterSlot {
                    host: shared,
                    address: dispatcher.address,
                    size: SIZE,
                    device: flui_engine::DeviceHealth::Healthy,
                    renderer_dead: false,
                },
                scene_plugin: reload.scene_plugin(),
                worker_reload: reload,
                reveal_window: Rc::downgrade(&window),
                device_retry: None,
            }))
            .expect("the desktop driver");
        installation
            .submit()
            .outcome()
            .expect("settled")
            .expect("installed");
        if !shown {
            dispatcher
                .runtime()
                .lifecycle(flui_scheduler::AppLifecycleState::Hidden)
                .expect("the window's lifecycle changes");
        }
        let _outcome =
            dispatch_platform_frame(dispatcher, frame.binding, flui_native::frame_request_now());
        // The frame request the wake served, still held, would keep the
        // window's next request from waking it.
        let scheduled = Rc::new(Cell::new(None));
        let read = Rc::clone(&scheduled);
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(move |ui_runtime| {
                read.set(Some(ui_runtime.scheduler().is_frame_scheduled()));
            })),
        )
        .expect("the runtime answers");
        let seen = Seen {
            asked: asked.lock().clone(),
            lent: slots
                .calls()
                .iter()
                .filter(|call| matches!(call, SlotCall::Lent(..)))
                .count(),
            rendered: (placement == inline).then(|| slots.renders().len()),
            scheduled: scheduled.get(),
            // A window the wake's frame revealed earns no second reveal.
            revealed: !first_reveal.after_frame(true, web_time::Instant::now()),
        };

        // Everything the row installed goes before it asserts, so a failed
        // assertion leaves the table's later rows the loop they expect.
        close_this_window(dispatcher);
        teardown_platform_ui_runtime();

        assert_eq!(seen, expected, "{case:?}");
    }
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
                // A deferred callback's panic is its operation's, contained
                // (ADR-0258 §1): its accepted FIFO tail, the cancelled
                // proposal's retirement, runs in the same turn.
                if incoming_failure && !deferred {
                    Some("preparation failure")
                } else {
                    driver_failure.or(handler_failure)
                }
            );
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
            dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
            )
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
            Rc::clone(&window),
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
            window.close_now();
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
    use flui_platform::PlatformWindow;
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

    std::thread_local! {
        /// The window to close from the agent's hook. The hook is `Send` and
        /// the window is the owner thread's, so the hook finds it here, on the
        /// owner thread it runs on.
        static CLOSE_FROM_AGENT: std::cell::RefCell<Option<flui_native::testing::HeadlessHostWindow>> =
            const { std::cell::RefCell::new(None) };
    }

    struct Agent {
        opened: Arc<AtomicUsize>,
        live: Arc<AtomicUsize>,
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
            if let Some(native) = CLOSE_FROM_AGENT.with(|slot| slot.borrow_mut().take()) {
                native.close_now();
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
        let host = crate::app::window_test_support::headless_test_host_window();
        let mock = (&*host as &dyn std::any::Any)
            .downcast_ref::<flui_native::testing::HeadlessHostWindow>()
            .expect("a headless window")
            .clone();
        let window: Rc<dyn PlatformWindow> = host;
        let opened = Arc::new(AtomicUsize::new(0));
        let live = Arc::new(AtomicUsize::new(0));
        CLOSE_FROM_AGENT.with(|slot| {
            *slot.borrow_mut() =
                matches!(case, DuringInstall::CloseFromAgent).then(|| mock.clone());
        });
        // The loop's development agents, as a runner starts them.
        super::super::dev_agents::start(&crate::AppConfig::new().dev_agent(Agent {
            opened: Arc::clone(&opened),
            live: Arc::clone(&live),
        }));
        let runtime = UiRuntime::for_test();
        runtime
            .attach_root_widget(&flui_widgets::SizedBox::new(10.0, 10.0))
            .expect("mount root");
        let agent_window = super::super::dev_agents::vend(&runtime, runtime.presentation_id())
            .expect("vend mounted presentation");
        let mut installation = prepare_replacement_ui_runtime(runtime, Rc::clone(&window));
        installation.dev_agent(Some(agent_window));
        let native = Rc::clone(&window);
        installation
            .frame_driver(FrameDriver::Test(TestFrameDriver {
                sink: ScriptedSink::always_presents(),
                installed: Some(Box::new(move || match case {
                    DuringInstall::Keep | DuringInstall::CloseFromAgent => {}
                    DuringInstall::Close => {
                        native.close_now();
                    }
                    DuringInstall::Teardown => teardown_platform_ui_runtime(),
                    DuringInstall::Panic => panic!("native initialization failed"),
                    DuringInstall::CloseAndPanic => {
                        native.close_now();
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
        super::super::dev_agents::end();
        teardown_platform_ui_runtime();
        let next = prepared_runtime();
        let dispatcher = next.dispatcher();
        let scenes = Arc::new(AtomicUsize::new(0));
        let frame = installed_driver(next, counting_sink(Arc::clone(&scenes)), None);
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::Frame(frame.binding, flui_native::frame_request_now()),
        )
        .expect("the next installation can render after recovery");
        assert_eq!(scenes.load(Ordering::SeqCst), 1, "{case:?}");
        teardown_platform_ui_runtime();
    }
}

/// A window's metrics from a logical size and scale, as a platform reports them.
fn window_metrics(width: f64, height: f64, scale: f64) -> flui_foundation::geometry::WindowMetrics {
    flui_foundation::geometry::WindowMetrics::from_logical(
        flui_foundation::geometry::Size::new(width, height),
        flui_foundation::geometry::DevicePixelRatio::try_new(scale).expect("a ratio in range"),
    )
    .expect("valid window metrics")
}
