//! A real winit loop for tests: the owner signal a hand-built `WinitApp`
//! needs and the wait for the loop to run, and, for the tests of the windows
//! the loop drives, a window opened through the loop's control lane with a
//! scenario run against it on a worker thread while the loop runs on the
//! test's own, over a host whose loop hooks report nothing ([`QuietHost`]).

use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

use winit::event_loop::EventLoop;

use super::{WinitLoop, WinitRunState};

#[cfg(target_os = "linux")]
pub(crate) use window_scenario::{QuietHost, run_window_scenario};

/// The owner signal `run_event_loop` installs, for a hand-built `WinitApp`:
/// a wake posts a user event to `event_loop`.
pub(crate) fn install_test_owner_signal(platform: &WinitLoop, event_loop: &EventLoop<()>) {
    let proxy = event_loop.create_proxy();
    *platform.owner_signal.lock() = Some(crate::owner::signal::OwnerSignal::new(Arc::new(
        move || {
            proxy
                .send_event(())
                .map_err(|error| crate::PlatformError::EventLoop {
                    message: error.to_string(),
                })
        },
    )));
}

/// Polls (bounded, 2s) until the owner has reached `Running` — the point
/// at which `resumed()` has returned and the owner lane accepts
/// deferred, post-bootstrap requests. Real wall-clock, not simulated:
/// these tests drive an actual winit event loop.
pub(crate) fn wait_for_running(platform: &WinitLoop) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !platform.with_state(|state| matches!(state.run_state, WinitRunState::Running { .. })) {
        assert!(
            Instant::now() < deadline,
            "owner never reached Running within 2s"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(target_os = "linux")]
mod window_scenario {
    use std::{
        collections::HashMap,
        panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
        path::PathBuf,
        sync::{Arc, Weak},
        thread,
    };

    use flui_foundation::geometry::Point;
    use flui_foundation::{ClaimOutcome, DataTransferId};
    use flui_platform::data_transfer::{
        DropFeedback, RepresentationIndex, TransferLimits, TransferRequest,
    };
    use flui_platform::{
        Clipboard, DataTransferOffer, DataTransferSource, DisplayId, DisplaySnapshot,
        DragDropEvent, InMemoryClipboard, NullDataTransferSource, SystemNotice, SystemPreferences,
        WindowAppearance, WindowId, WindowOptions,
    };
    use winit::event_loop::{ActiveEventLoop, EventLoop};

    use super::{install_test_owner_signal, wait_for_running};
    use crate::error::PlatformError;
    use crate::executor::BackgroundExecutor;
    use crate::host_window::HostWindow;
    use crate::owner::notices::{NoticeChannel, StaleOnWake, SystemNoticeHook};
    use crate::owner::signal::OwnerSignal;
    use crate::owner::sources::SystemPreferenceSource;
    use crate::owner::watch::Watch;
    use crate::owner::winit::control::control_lane;
    use crate::owner::winit::{
        FileDrops, HostDisplays, HostNoticeStart, HostNotices, PortalPreferences, PreferenceStart,
        SelfCloseRoute, WinitApp, WinitHost, WinitLoop,
    };
    use crate::platform::Platform;

    /// Run `event_loop` for `platform`, the loop of `owner_platform`, open a
    /// window with `opening` once the loop runs, and run `scenario` against
    /// it on a worker thread; the loop quits when the scenario returns, and a
    /// panic in it fails the test.
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

    /// A winit host whose loop hooks report nothing: no displays, notices,
    /// preferences or host notices, an in-memory clipboard and no file drags.
    /// The tests of a window run a real loop over it without the host table.
    pub(crate) struct QuietHost;

    impl WinitHost for QuietHost {
        fn displays() -> Arc<dyn HostDisplays> {
            Arc::new(NoDisplays)
        }

        fn notices() -> Arc<dyn NoticeChannel> {
            Arc::new(NoNotices)
        }

        fn preferences() -> Box<dyn PreferenceStart> {
            Box::new(NoPreferences)
        }

        fn host_notices() -> Box<dyn HostNoticeStart> {
            Box::new(NoHostNotices)
        }

        fn clipboard() -> Arc<dyn Clipboard> {
            Arc::new(InMemoryClipboard::new())
        }

        fn drag() -> Arc<dyn FileDrops> {
            Arc::new(NoFileDrops)
        }
    }

    /// Displays the source holds none of.
    struct NoDisplays;

    impl StaleOnWake for NoDisplays {
        fn mark_stale(&self) {}
    }

    impl HostDisplays for NoDisplays {
        fn snapshot(&self) -> Result<DisplaySnapshot, PlatformError> {
            Ok(DisplaySnapshot::empty())
        }

        fn id_of_window(&self, _: &winit::window::Window) -> Option<DisplayId> {
            None
        }

        fn resample(&self, _: &ActiveEventLoop, _: Option<&Arc<OwnerSignal>>) -> bool {
            false
        }

        fn observe(self: Arc<Self>, _: &Arc<OwnerSignal>) -> Watch {
            Watch::new(())
        }

        fn is_wayland(&self) -> bool {
            false
        }
    }

    /// A notice channel that keeps no hook and marks nothing stale.
    struct NoNotices;

    impl NoticeChannel for NoNotices {
        fn set_hook(&self, _: SystemNoticeHook) {}

        fn mark_held_stale_on_wake(&self, _: Weak<dyn StaleOnWake>) {}

        fn dispatch(&self, _: SystemNotice) {}
    }

    /// Preferences that stay the defaults.
    struct NoPreferences;

    impl PreferenceStart for NoPreferences {
        fn start(
            &self,
            _: &BackgroundExecutor,
            _: &Arc<OwnerSignal>,
        ) -> Arc<dyn PortalPreferences> {
            Arc::new(NoPreferences)
        }
    }

    impl SystemPreferenceSource for NoPreferences {
        fn sample(&self) -> Result<SystemPreferences, PlatformError> {
            Ok(SystemPreferences::default())
        }
    }

    impl PortalPreferences for NoPreferences {
        fn appearance(&self) -> WindowAppearance {
            WindowAppearance::default()
        }
    }

    /// Host notices that never queue anything.
    struct NoHostNotices;

    impl HostNoticeStart for NoHostNotices {
        fn start(&self, _: &BackgroundExecutor, _: &Arc<OwnerSignal>) -> Box<dyn HostNotices> {
            Box::new(NoHostNotices)
        }
    }

    impl HostNotices for NoHostNotices {
        fn deliver(&self, _: &WinitLoop) {}
    }

    /// A drag transport that hears of no files: every report ends nothing
    /// and offers nothing.
    struct NoFileDrops;

    impl DataTransferSource for NoFileDrops {
        fn clipboard_offer(&self) -> Option<DataTransferOffer> {
            NullDataTransferSource.clipboard_offer()
        }

        fn request(
            &self,
            id: DataTransferId,
            representation: RepresentationIndex,
            limits: TransferLimits,
        ) -> TransferRequest {
            NullDataTransferSource.request(id, representation, limits)
        }

        fn update_drop_feedback(&self, id: DataTransferId, feedback: DropFeedback) {
            NullDataTransferSource.update_drop_feedback(id, feedback);
        }

        fn conclude_drop(&self, id: DataTransferId) {
            NullDataTransferSource.conclude_drop(id);
        }
    }

    impl FileDrops for NoFileDrops {
        fn note_hovered_file(
            &self,
            _: WindowId,
            _: PathBuf,
            _: Option<Point<f64>>,
        ) -> Option<DragDropEvent> {
            None
        }

        fn note_dropped_file(
            &self,
            _: WindowId,
            _: PathBuf,
            _: Option<Point<f64>>,
        ) -> Option<DragDropEvent> {
            None
        }

        fn note_hover_cancelled(&self, _: WindowId) -> Option<DragDropEvent> {
            None
        }

        fn windows_awaiting_freeze(&self) -> Vec<WindowId> {
            Vec::new()
        }

        fn freeze_completed(
            &self,
            _: &HashMap<WindowId, Point<f64>>,
        ) -> Vec<(WindowId, DragDropEvent)> {
            Vec::new()
        }

        fn forget_window(&self, _: WindowId) {}
    }
}
