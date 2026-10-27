//! Prepare native wiring and resources before admitting joint publication.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use flui_platform::PlatformWindow;
use flui_runtime::owner::{InstallInitialization, PreparedInstall, WindowObservation};

#[cfg(all(
    not(target_os = "android"),
    not(target_arch = "wasm32"),
    any(test, not(target_os = "ios"))
))]
use crate::app::close_request::{CloseRequestHandler, CloseResponse, PreparedCloseRequest};
use crate::app::window_registry::PreparedWindowRegistration;

use super::InstalledHost;
use super::frame_driver::{FrameDriver, FrameInstallError, FrameRegistration, PreparedFrame};
use super::installed_host::Installation;
use super::native_bindings::PreparedNativeWindow;
use super::owner_dispatch::{PresentationDispatcher, RuntimeTask, dispatch_platform_ui_runtime};

/// What a runner sizes a window's surface and root layout from: the
/// drawable area the window's snapshot reports, in device and logical pixels,
/// and the scale between them.
///
/// A window that has reported no drawable area yet (it opened minimized)
/// gives its content size at scale 1; the metrics it reports once it has one
/// reach the presentation as a resize before anything is drawn there.
#[derive(Clone, Copy, Debug)]
pub(super) struct SurfaceGeometry {
    pub(super) physical: flui_foundation::geometry::Size<i32>,
    pub(super) logical: flui_foundation::geometry::Size<f64>,
    pub(super) scale: f64,
}

impl SurfaceGeometry {
    pub(super) fn of(window: &dyn PlatformWindow) -> Self {
        let state = window.state();
        if let Some(metrics) = state.metrics {
            return Self {
                physical: metrics.physical(),
                logical: metrics.logical(),
                scale: metrics.scale().get(),
            };
        }
        let logical = state.content_size;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a window's content size in logical pixels is far below i32::MAX"
        )]
        let physical = flui_foundation::geometry::Size::new(
            logical.width.max(0.0).round() as i32,
            logical.height.max(0.0).round() as i32,
        );
        Self {
            physical,
            logical,
            scale: 1.0,
        }
    }
}

pub(super) struct WindowInstall {
    host: InstalledHost,
    prepared: PreparedInstall,
    native: PreparedNativeWindow,
    initial: InstallInitialization,
    window: InstallationWindow,
}

/// Owns native closure until the complete logical/native installation succeeds.
pub(super) struct InstallationWindow {
    window: Option<Arc<dyn PlatformWindow>>,
    closed: Arc<AtomicBool>,
    published: bool,
    agent: Option<(
        crate::app::dev_agent::DevAgent,
        flui_view::dev_agent::AgentWindow,
    )>,
}

impl InstallationWindow {
    fn new(window: Arc<dyn PlatformWindow>) -> Self {
        Self {
            window: Some(window),
            closed: Arc::new(AtomicBool::new(false)),
            published: false,
            agent: None,
        }
    }

    pub(super) fn native(&self) -> &Arc<dyn PlatformWindow> {
        self.window
            .as_ref()
            .expect("BUG: live installation retains its native window")
    }

    pub(super) fn cancellation(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.closed)
    }
    pub(super) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }
    pub(super) fn publish(&mut self) {
        self.published = true;
    }

    pub(super) fn notify_opened(&mut self) {
        if let Some((agent, window)) = self.agent.take() {
            agent.window_opened(window);
        }
    }
}

impl Drop for InstallationWindow {
    fn drop(&mut self) {
        let Some(window) = self.window.take() else {
            return;
        };
        let mut first = None;
        if !self.published {
            self.closed.store(true, Ordering::Release);
            first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = window.apply(flui_platform::WindowCommand::Close);
            }))
            .err();
        }
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(window))).err();
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first,
            failure,
            "installation window retirement",
        );
        {
            let agent = self.agent.take();
            let failure =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(agent))).err();
            crate::app::lifecycle_state::preserve_first_lifecycle_panic(
                &mut first,
                failure,
                "installation agent retirement",
            );
        }
        if let Some(first) = first {
            if std::thread::panicking() {
                std::mem::forget(first);
            } else {
                std::panic::resume_unwind(first);
            }
        }
    }
}

impl WindowInstall {
    pub(super) fn new(
        host: InstalledHost,
        prepared: PreparedInstall,
        window: Arc<dyn PlatformWindow>,
    ) -> Self {
        let address = prepared.address();
        let window = InstallationWindow::new(window);
        let closing = window.cancellation();
        let dispatcher = PresentationDispatcher {
            owner_thread: std::thread::current().id(),
            address,
        };
        // Terminal closure during native preparation must survive even though
        // the logical membership is not yet visible to dispatch.
        window.native().on_close(Box::new(move || {
            closing.store(true, Ordering::Release);
            let _ = dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::ClosePresentation(address.presentation_id),
            );
        }));
        let registration = PreparedWindowRegistration::new(window.native());
        Self {
            host,
            prepared,
            native: PreparedNativeWindow {
                text_sizing: None,
                registration,
                frame: None,
                close: None,
            },
            initial: InstallInitialization {
                address,
                observations: Vec::new(),
                lifecycle: None,
            },
            window,
        }
    }

    pub(super) fn dispatcher(&self) -> PresentationDispatcher {
        PresentationDispatcher {
            owner_thread: std::thread::current().id(),
            address: self.prepared.address(),
        }
    }

    #[cfg(any(
        target_os = "android",
        target_os = "ios",
        all(test, not(target_arch = "wasm32"))
    ))]
    pub(super) fn text_sizing(
        &mut self,
        sizing: std::rc::Rc<super::native_text_sizing::NativeTextSizing>,
    ) {
        self.native.text_sizing = Some(sizing);
    }

    pub(super) fn dev_agent(
        &mut self,
        agent: Option<(
            crate::app::dev_agent::DevAgent,
            flui_view::dev_agent::AgentWindow,
        )>,
    ) {
        self.window.agent = agent;
    }

    pub(super) fn frame_driver(
        &mut self,
        driver: FrameDriver,
    ) -> Result<FrameRegistration, FrameInstallError> {
        if self.native.frame.is_some() {
            return Err(FrameInstallError::AlreadyInstalled);
        }
        let (prepared, registration) = PreparedFrame::new(self.prepared.address(), driver);
        self.native.frame = Some(prepared);
        Ok(registration)
    }

    #[cfg(all(
        not(target_os = "android"),
        not(target_arch = "wasm32"),
        any(test, not(target_os = "ios"))
    ))]
    pub(super) fn close_requests(&mut self, handler: Option<CloseRequestHandler>) {
        let address = self.prepared.address();
        self.native.close = Some(PreparedCloseRequest::new(
            address,
            self.window.native(),
            handler,
        ));
        let router = self.host.native().close_requests();
        self.window.native().on_should_close(Box::new(move || {
            matches!(
                router.consult(address, crate::app::close_request::CloseReason::User),
                CloseResponse::Close
            )
        }));
    }

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
    pub(super) fn on_close(&self, mut run: impl FnMut() + Send + 'static) {
        let closed = self.window.cancellation();
        self.window.native().on_close(Box::new(move || {
            closed.store(true, Ordering::Release);
            run();
        }));
    }

    pub(super) fn observe(&mut self, observation: WindowObservation) {
        self.initial.observations.push(observation);
    }

    /// Follow the window's snapshot into its presentation: the one
    /// subscription a window has for its facts (ADR-0254 §1).
    ///
    /// Each `on_state_changed` reads the snapshot and compares it with the one
    /// seen before (`RuntimeEvent::of_state_change`): the metrics, safe
    /// area, appearance, execution state, focus, whether the content is seen
    /// and the hover reach the presentation as the facts that changed, and
    /// the `WindowHandle` subscribers get the new state. A change of display,
    /// or of the scale its display gives the window, wakes the owner, whose
    /// next turn samples the platform's display snapshot and re-derives every
    /// window's display and refresh period (ADR-0236).
    ///
    /// The subscription is registered before the seed is read, so a change
    /// between the two is compared against the seed rather than lost, and the
    /// presentation starts with the window's real facts, display and period
    /// before its first frame.
    pub(super) fn observe_window(&mut self) {
        use super::owner_dispatch::RuntimeEvent;

        let window = Arc::clone(self.window.native());
        let dispatcher = self.dispatcher();
        // Bound to this host's owner now, so a late change after a replacement
        // wakes the retired owner only (a cheap defence; nothing pins it).
        let proxy = super::host::with_owner_platform(flui_native::OwnerPlatform::proxy);
        let seen = Arc::new(parking_lot::Mutex::new(None::<flui_platform::WindowState>));
        // Weak: the callback lives in the window's own handler table, and a
        // strong capture would keep the window alive past close.
        let observed = Arc::downgrade(&window);
        let last = Arc::clone(&seen);
        window.on_state_changed(Box::new(move || {
            let Some(window) = observed.upgrade() else {
                return;
            };
            // Read with no lock held: the window is platform code.
            let next = window.state();
            let previous = last.lock().replace(next.clone());
            let Some(previous) = previous else {
                // Before the seed: the seed reads this state or a newer one.
                return;
            };
            let scale = |state: &flui_platform::WindowState| {
                state
                    .metrics
                    .map(flui_foundation::geometry::WindowMetrics::scale)
            };
            let rescaled = scale(&previous) != scale(&next);
            if (previous.display != next.display || rescaled)
                && let Some(proxy) = &proxy
            {
                // A failed wake leaves the owner's retry debt; nothing to do.
                let _ = proxy.wake();
            }
            for event in RuntimeEvent::of_state_change(&previous, &next) {
                // A refused delivery means the presentation is gone.
                let _ = dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Event(event));
            }
        }));
        let seed = window.state();
        *seen.lock() = Some(seed.clone());
        self.observe(WindowObservation::Snapshot {
            execution: seed.execution,
            focused: seed.focused,
            visible: seed.is_seen(),
        });
        if let Some(metrics) = seed.metrics {
            self.observe(WindowObservation::Metrics(metrics));
        }
        self.observe(WindowObservation::SafeArea(seed.safe_area));
        self.observe(WindowObservation::Brightness(seed.appearance.into()));
        self.host.displays().follow(dispatcher, &window);
        let display = self.host.displays().derive(window.as_ref());
        self.observe(WindowObservation::Display(display));
    }

    /// Complete Web bootstrap wiring before sampling and publishing native state.
    #[cfg(any(
        target_arch = "wasm32",
        all(test, not(target_os = "android"), not(target_os = "ios"))
    ))]
    pub(super) fn submit_web(
        mut self,
        driver: FrameDriver,
        platform: &flui_native::SharedPlatform,
    ) -> anyhow::Result<()> {
        use super::owner_dispatch::{install_input_wiring, with_owner_callback};

        let window = Arc::clone(self.window.native());
        let dispatcher = self.dispatcher();
        install_input_wiring(dispatcher, window.as_ref());
        let binding = self.frame_driver(driver)?.binding;
        window.on_request_frame(Box::new(move |request| {
            with_owner_callback(|_| {
                let _ =
                    dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(binding, request));
            });
        }));
        self.terminal_callbacks(platform);
        // The subscription precedes the seed. Keep redraw ownership published
        // before lifecycle reconciliation can synchronously request a frame.
        self.observe_window();
        super::host::APP_RUNTIME.with(|slot| slot.borrow().set_redraw_window(window));
        self.submit()
            .outcome()
            .expect("BUG: the fresh web host completes its initial installation synchronously")?;
        Ok(())
    }

    #[cfg(all(not(target_os = "ios"), not(target_arch = "wasm32")))]
    pub(super) fn lifecycle(&mut self, lifecycle: flui_scheduler::AppLifecycleState) {
        self.initial.lifecycle = Some(lifecycle);
    }

    /// Close the window when the platform quits, unless something closed it
    /// first.
    ///
    /// The flag is set by the window's native close (the `on_close` that
    /// [`new`](Self::new) registers) and by a cancelled installation, and each
    /// owns the presentation's retirement. Android's `Destroy` fires that close
    /// before its loop exits and runs this hook, and the close retires the sole
    /// presentation together with its UI runtime, so a second close here would
    /// only be refused by a runtime that no longer exists.
    #[cfg(any(test, target_os = "android", target_arch = "wasm32"))]
    pub(super) fn terminal_callbacks(&self, platform: &flui_native::SharedPlatform) {
        let closed = self.window.cancellation();
        let dispatcher = self.dispatcher();
        platform.on_quit(Box::new(move || {
            if !closed.swap(true, Ordering::AcqRel) {
                super::owner_dispatch::close_this_window(dispatcher);
            }
        }));
    }

    pub(super) fn submit(self) -> Installation {
        self.host
            .submit_window(self.prepared, self.native, self.initial, self.window)
    }
}
