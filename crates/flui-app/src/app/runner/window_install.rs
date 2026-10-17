//! Prepare native wiring and resources before admitting joint publication.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use flui_platform::traits::PlatformWindow;
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
    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
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
            #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
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

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
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
            first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| window.close())).err();
        }
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(window))).err();
        crate::app::lifecycle_state::preserve_first_lifecycle_panic(
            &mut first,
            failure,
            "installation window retirement",
        );
        #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
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

    #[cfg(all(not(target_os = "android"), not(target_arch = "wasm32")))]
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

    /// Follow the window's system light/dark appearance into
    /// `MediaQueryData::platform_brightness`.
    ///
    /// The observer is registered before the seed is sampled, so a theme change
    /// between the two is delivered rather than lost, and a user on a dark
    /// desktop never starts light.
    pub(super) fn observe_appearance(&mut self) {
        use super::owner_dispatch::RuntimeEvent;

        let window = Arc::clone(self.window.native());
        let dispatcher = self.dispatcher();
        // Weak: the callback lives in the window's own handler table, and a
        // strong capture would keep the window alive past close.
        let observed = Arc::downgrade(&window);
        window.on_appearance_changed(Box::new(move || {
            if let Some(window) = observed.upgrade() {
                let _ = dispatch_platform_ui_runtime(
                    dispatcher,
                    RuntimeTask::Event(RuntimeEvent::AppearanceChanged(window.appearance().into())),
                );
            }
        }));
        self.observe(WindowObservation::Brightness(window.appearance().into()));
    }

    /// Complete Web bootstrap wiring before sampling and publishing native state.
    #[cfg(any(
        target_arch = "wasm32",
        all(test, not(target_os = "android"), not(target_os = "ios"))
    ))]
    pub(super) fn submit_web(
        mut self,
        driver: FrameDriver,
        platform: &flui_platform::SharedPlatform,
    ) -> anyhow::Result<()> {
        use super::owner_dispatch::{RuntimeEvent, install_input_wiring, with_owner_callback};

        let window = Arc::clone(self.window.native());
        let dispatcher = self.dispatcher();
        install_input_wiring(dispatcher, window.as_ref());
        let binding = self.frame_driver(driver)?.binding;
        window.on_request_frame(Box::new(move || {
            with_owner_callback(|_| {
                let _ = dispatch_platform_ui_runtime(dispatcher, RuntimeTask::Frame(binding));
            });
        }));
        window.on_resize(Box::new(move |size, scale_factor| {
            let _ = dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Event(RuntimeEvent::Resized { size, scale_factor }),
            );
        }));
        self.terminal_callbacks(platform);
        window.on_active_status_change(Box::new(move |focused| {
            let _ = dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Event(RuntimeEvent::WindowFocus(focused)),
            );
        }));
        window.on_visibility_status_change(Box::new(move |visible| {
            let _ = dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Event(RuntimeEvent::WindowVisibility(visible)),
            );
        }));
        window.on_hover_status_change(Box::new(move |is_hovered| {
            let _ = dispatch_platform_ui_runtime(
                dispatcher,
                RuntimeTask::Event(RuntimeEvent::WindowHover(is_hovered)),
            );
        }));
        self.observe_appearance();
        // Observers precede the snapshot. Keep redraw ownership published
        // before lifecycle reconciliation can synchronously request a frame.
        let metrics = WindowObservation::Metrics {
            size: window.logical_size(),
            scale_factor: window.scale_factor(),
        };
        let snapshot = WindowObservation::Snapshot {
            execution: window.execution_state(),
            focused: window.is_focused(),
            visible: window.is_visible(),
        };
        self.observe(snapshot);
        self.observe(metrics);
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

    #[cfg(any(test, target_os = "android", target_arch = "wasm32"))]
    pub(super) fn terminal_callbacks(&self, platform: &flui_platform::SharedPlatform) {
        let closed = self.window.cancellation();
        let dispatcher = self.dispatcher();
        platform.on_quit(Box::new(move || {
            closed.store(true, Ordering::Release);
            super::owner_dispatch::close_this_window(dispatcher);
        }));
    }

    pub(super) fn submit(self) -> Installation {
        self.host
            .submit_window(self.prepared, self.native, self.initial, self.window)
    }
}
