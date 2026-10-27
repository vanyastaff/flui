//! Installed owner delivery through a private capture failure seam. No SDK calls.

use super::*;
use crate::app::runner::window_routes::OwnerOpenRouted as _;
use flui_native::{HeadlessPlatform, Platform};
use flui_runtime::ui_runtime::UiRuntime;

fn installed_capture(
    run: impl FnOnce(
        Rc<NativeTextSizing>,
        Rc<Cell<usize>>,
        flui_native::HeadlessOwnerTurns,
        super::super::frame_driver::FrameBinding,
    ),
) {
    let _clear = super::super::host::OwnerHostClearGuard::arm();
    let platform = HeadlessPlatform::new();
    let turns = platform.owner_turns();
    let installed = Rc::new(RefCell::new(None));
    let completed = Rc::clone(&installed);
    Box::new(platform)
        .run(Box::new(move |owner| {
            let owner = Rc::new(owner);
            owner.on_wake(Box::new(refresh_installed_after_wake))?;
            let window = owner
                .open_routed(flui_platform::WindowOptions::default())?
                .try_ready()?;
            let runtime = UiRuntime::new(
                flui_runtime::presentation::PresentationWindow::new(
                    Arc::clone(&window) as Arc<dyn flui_platform::PlatformWindow>,
                    None,
                )
                .with_text_store_host(Some(flui_testing::RecordingTextStoreHost::new())),
                1.0,
                flui_runtime::ui_runtime::RuntimeHostServices::new(
                    Arc::new(|| {}),
                    Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    Arc::new(flui_platform::InMemoryClipboard::new()),
                    &flui_painting::FontCollection::new(),
                    flui_scheduler::ClockSource::Platform,
                ),
            )?;
            let address = PresentationAddress {
                ui_runtime_id: runtime.id(),
                presentation_id: runtime.presentation_id(),
            };
            let record = NativeTextSizing::prepare(address, Arc::clone(&window), Rc::clone(&owner));
            let calls = Rc::new(Cell::new(0));
            let observed = Rc::clone(&calls);
            *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
                observed.set(observed.get() + 1);
                Ok(TextSizingCaptureState::Pending {
                    check_after: Duration::from_secs(1),
                })
            }));
            runtime.install_captured_text_sizing_for(runtime.presentation_id(), record.source());
            let mut installation =
                super::super::owner_dispatch::prepare_replacement_ui_runtime(runtime, window);
            installation.text_sizing(Rc::clone(&record));
            let frame = installation
                .frame_driver(super::super::frame_driver::FrameDriver::Test(
                    super::super::frame_driver::TestFrameDriver {
                        sink: flui_runtime::testing::ScriptedSink::always_presents(),
                        installed: None,
                        prelude: None,
                        resize: None,
                    },
                ))
                .expect("installed product-pump driver");
            installation
                .submit()
                .outcome()
                .expect("idle owner")
                .expect("published native record");
            *completed.borrow_mut() = Some((record, calls, owner, frame.binding));
            Ok(())
        }))
        .expect("headless owner fixture");
    let (record, calls, owner, binding) = installed.borrow_mut().take().expect("published fixture");
    run(record, calls, turns, binding);
    super::super::owner_dispatch::teardown_platform_ui_runtime();
    owner.quit();
}

fn refresh_installed() {
    let host = super::super::host::APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    host.native()
        .refresh_text_sizing(host.logical(), host.effects(), false);
}

fn refresh_installed_after_wake() {
    let host = super::super::host::APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
    host.native()
        .refresh_text_sizing(host.logical(), host.effects(), true);
}

fn expired_deadline_is_consumed_before_accepting_the_next_delay() {
    installed_capture(|record, calls, _, _| {
        let before = calls.get();
        record.state.borrow_mut().deadline = Some(web_time::Instant::now());
        refresh_installed();
        assert_eq!(calls.get(), before + 1, "the due check reaches acquisition");
        assert!(record.deadline().expect("accepted check debt") > web_time::Instant::now());
        refresh_installed();
        assert_eq!(
            calls.get(),
            before + 1,
            "a consumed deadline cannot cause immediate repeated polling"
        );
    });
}

fn stalled_receipt_is_observed_without_hot_polling_and_can_settle_later() {
    installed_capture(|record, calls, _, _| {
        let observed = Rc::clone(&calls);
        *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
            observed.set(observed.get() + 1);
            Ok(TextSizingCaptureState::Stalled)
        }));
        let source = record.source();
        record.state.borrow_mut().deadline = Some(web_time::Instant::now());
        refresh_installed();
        let stalled_calls = calls.get();
        assert!(record.deadline().expect("late receipt watch") > web_time::Instant::now());
        refresh_installed();
        assert_eq!(
            calls.get(),
            stalled_calls,
            "watch waits for its owner deadline"
        );
        assert_eq!(
            record.source(),
            source,
            "a timeout does not replace capture authority or admit a new producer"
        );
        let observed = Rc::clone(&calls);
        *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
            observed.set(observed.get() + 1);
            Ok(TextSizingCaptureState::RetryAfter(Duration::from_secs(1)))
        }));
        record.state.borrow_mut().deadline = Some(web_time::Instant::now());
        refresh_installed();
        assert_eq!(
            calls.get(),
            stalled_calls + 1,
            "the same capture can be observed after its late receipt settles"
        );
        assert_eq!(record.source(), source);
        assert!(record.deadline().expect("paced retry") > web_time::Instant::now());
    });
}

fn reentrant_close_during_capture_cannot_restore_delivery() {
    installed_capture(|record, calls, _, _| {
        let observed = Rc::clone(&calls);
        let address = record.address();
        *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
            observed.set(observed.get() + 1);
            let host =
                super::super::host::APP_RUNTIME.with(|slot| slot.borrow().installed_host.clone());
            host.native().fence_presentation(address);
            Ok(TextSizingCaptureState::Pending {
                check_after: Duration::from_secs(1),
            })
        }));
        record.state.borrow_mut().deadline = Some(web_time::Instant::now());
        refresh_installed();
        assert!(
            record.deadline().is_none(),
            "fenced work cannot republish a deadline"
        );
        let after_close = calls.get();
        refresh_installed();
        assert_eq!(
            calls.get(),
            after_close,
            "retired authority cannot query again"
        );
    });
}

fn completion_wake_observes_the_receipt_without_wake_feedback() {
    installed_capture(|record, calls, turns, _| {
        let before = calls.get();
        let deadline = record.deadline().expect("future receipt check");
        turns.drive();
        assert_eq!(
            calls.get(),
            before + 1,
            "actual owner completion wake bypasses a future acquisition deadline"
        );
        assert_eq!(
            record.deadline(),
            Some(deadline),
            "observing an unchanged receipt preserves its original deadline"
        );
        turns.drive();
        assert_eq!(
            calls.get(),
            before + 1,
            "unchanged deadline must not manufacture another owner wake"
        );
    });
}

/// A backgrounded Android or iOS app has no native window: every capture
/// answers `Unavailable`, and no timer changes that. The record parks without
/// a deadline and posts no wake, so the owner is not woken twice a second for as
/// long as the window is gone.
fn an_unavailable_context_parks_without_a_deadline_or_a_wake() {
    installed_capture(|record, calls, turns, _| {
        // The fixture's pending receipt check posted a wake; take it first.
        turns.drive();
        let observed = Rc::clone(&calls);
        *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
            observed.set(observed.get() + 1);
            Err(flui_native::TextSizingCaptureError::Unavailable)
        }));
        record.state.borrow_mut().deadline = Some(web_time::Instant::now());
        // Parking releases the document edits waiting on the geometry, once,
        // which can cost one more owner turn.
        refresh_installed();
        turns.drive();
        let before = calls.get();
        for _ in 0..3 {
            refresh_installed();
            turns.drive();
        }
        assert_eq!(
            calls.get(),
            before + 3,
            "each owner turn tries once; no wake of the record's own runs another"
        );
        assert!(
            record.deadline().is_none(),
            "a window that is gone arms no retry deadline"
        );
        assert!(
            record.state.borrow().parked,
            "document edits are released while the geometry is unavailable"
        );
    });
}

/// A failing native call is retried at a doubling delay up to a ceiling, not
/// twice a second for as long as it fails.
fn native_acquisition_failures_back_off_to_a_ceiling() {
    installed_capture(|record, calls, _, _| {
        let observed = Rc::clone(&calls);
        *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
            observed.set(observed.get() + 1);
            Err(flui_native::TextSizingCaptureError::Native {
                message: "injected acquisition failure".into(),
            })
        }));
        let assert_next_retry_in = |expected: u64| {
            let expected = Duration::from_millis(expected);
            record.state.borrow_mut().deadline = Some(web_time::Instant::now());
            refresh_installed();
            let armed = record
                .deadline()
                .expect("a failed acquisition arms a retry")
                .saturating_duration_since(web_time::Instant::now());
            assert!(
                armed <= expected && armed > expected / 2,
                "armed {armed:?}, expected about {expected:?}"
            );
        };
        assert_next_retry_in(500);
        assert_next_retry_in(1_000);
        // Android answers a retry with `Pending` before the result of its
        // capture; the failure that came before it still counts.
        let observed = Rc::clone(&calls);
        *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
            observed.set(observed.get() + 1);
            Ok(TextSizingCaptureState::Pending {
                check_after: Duration::from_millis(10),
            })
        }));
        record.state.borrow_mut().deadline = Some(web_time::Instant::now());
        refresh_installed();
        let observed = Rc::clone(&calls);
        *record.capture_sampler.borrow_mut() = Some(Box::new(move || {
            observed.set(observed.get() + 1);
            Err(flui_native::TextSizingCaptureError::Native {
                message: "injected acquisition failure".into(),
            })
        }));
        for expected in [2_000, 4_000, 8_000, 8_000] {
            assert_next_retry_in(expected);
        }
    });
}

fn stalled_capture_releases_waiting_document_edits_without_layout() {
    acquisition_without_progress_releases_document_edits(|| Ok(TextSizingCaptureState::Stalled));
}

fn no_flight_retry_releases_waiting_document_edits_without_layout() {
    acquisition_without_progress_releases_document_edits(|| {
        Ok(TextSizingCaptureState::RetryAfter(Duration::from_secs(1)))
    });
}

fn native_acquisition_error_releases_waiting_document_edits_without_layout() {
    acquisition_without_progress_releases_document_edits(|| {
        Err(flui_native::TextSizingCaptureError::Native {
            message: "injected acquisition failure".into(),
        })
    });
}

fn acquisition_without_progress_releases_document_edits(
    result: fn() -> Result<TextSizingCaptureState, flui_native::TextSizingCaptureError>,
) {
    use super::super::owner_dispatch::{
        PresentationDispatcher, RuntimeTask, dispatch_platform_ui_runtime,
    };
    use flui_platform::text_store::{
        InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore,
    };
    installed_capture(move |record, _, _, binding| {
        let store = InMemoryTextStore::new("pending");
        let attached = Rc::clone(&store);
        let dispatcher = PresentationDispatcher {
            owner_thread: std::thread::current().id(),
            address: record.address(),
        };
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(move |runtime| {
                runtime
                    .text_input_handle()
                    .attach(flui_interaction::TextInputClient::new(attached))
                    .expect("actual store attachment");
                runtime
                    .attach_root_widget(&flui_widgets::Text::new("unresolved native size"))
                    .expect("actual text");
            })),
        )
        .expect("actual pending frame owner turn");
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::Frame(binding, flui_native::frame_request_now()),
        )
        .expect("actual installed product pump");
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(|runtime| {
                assert!(
                    runtime
                        .widgets()
                        .pipeline_owner()
                        .expect("pipeline")
                        .try_with_layout(|_| ())
                        .is_none(),
                    "pending text cannot publish geometry"
                );
            })),
        )
        .expect("observe actual pending layout");
        let edited = Rc::new(Cell::new(false));
        let granted = Rc::clone(&edited);
        assert_eq!(
            store.request_lock(
                LockGrant::read_write(move |session| {
                    session
                        .insert_at_selection("x")
                        .expect("document repair is admitted");
                    granted.set(true);
                }),
                LockTiming::Async
            ),
            Ok(LockOutcome::Deferred)
        );
        assert!(
            !edited.get(),
            "Waiting holds document edits before bounded capture observation"
        );
        *record.capture_sampler.borrow_mut() = Some(Box::new(result));
        record.state.borrow_mut().deadline = Some(web_time::Instant::now());
        refresh_installed();
        assert!(
            edited.get(),
            "capture without progress cues Parked and releases the queued document grant"
        );
        dispatch_platform_ui_runtime(
            dispatcher,
            RuntimeTask::TestCallback(Box::new(|runtime| {
                assert!(
                    runtime
                        .widgets()
                        .pipeline_owner()
                        .expect("pipeline")
                        .try_with_layout(|_| ())
                        .is_none(),
                    "Parked permits edits while geometry remains unavailable"
                );
            })),
        )
        .expect("observe withheld geometry");
    });
}

#[test]
fn installed_native_text_sizing_delivery() {
    crate::table_test::run_table(
        "installed_native_text_sizing_delivery",
        &[
            (
                "no_flight_retry_releases_waiting_document_edits_without_layout",
                no_flight_retry_releases_waiting_document_edits_without_layout as fn(),
            ),
            (
                "native_acquisition_error_releases_waiting_document_edits_without_layout",
                native_acquisition_error_releases_waiting_document_edits_without_layout as fn(),
            ),
            (
                "stalled_capture_releases_waiting_document_edits_without_layout",
                stalled_capture_releases_waiting_document_edits_without_layout as fn(),
            ),
            (
                "completion_wake_observes_the_receipt_without_wake_feedback",
                completion_wake_observes_the_receipt_without_wake_feedback as fn(),
            ),
            (
                "expired_deadline_is_consumed_before_accepting_the_next_delay",
                expired_deadline_is_consumed_before_accepting_the_next_delay as fn(),
            ),
            (
                "stalled_receipt_is_observed_without_hot_polling_and_can_settle_later",
                stalled_receipt_is_observed_without_hot_polling_and_can_settle_later as fn(),
            ),
            (
                "reentrant_close_during_capture_cannot_restore_delivery",
                reentrant_close_during_capture_cannot_restore_delivery as fn(),
            ),
            (
                "an_unavailable_context_parks_without_a_deadline_or_a_wake",
                an_unavailable_context_parks_without_a_deadline_or_a_wake as fn(),
            ),
            (
                "native_acquisition_failures_back_off_to_a_ceiling",
                native_acquisition_failures_back_off_to_a_ceiling as fn(),
            ),
        ],
    );
}
