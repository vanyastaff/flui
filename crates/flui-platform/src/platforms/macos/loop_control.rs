//! Standalone AppKit loop ownership and orderly, policy-aware stopping.
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};

use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send, rc::Retained,
    runtime::ProtocolObject,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate,
    NSApplicationTerminateReply, NSEvent, NSEventModifierFlags, NSEventType,
};
use objc2_foundation::{NSObject, NSObjectProtocol, NSPoint};
use parking_lot::Mutex;

use super::{
    owner_lane::{
        exec_async_guarded, owner_queue, report_contained_panic,
        run_cleanup_guarded as cleanup_step,
    },
    window::MacOSWindow,
};
use crate::{error::PlatformError, shared::PlatformHandlers, traits::PlatformWindow};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Dormant,
    Starting,
    Running,
    Stopping,
    Stopped,
}

struct State {
    phase: Phase,
    queued: bool,
    requested: bool,
    explicit_quit: bool,
}

/// Contains only thread-safe Rust state. AppKit objects never cross the queue.
pub(super) struct LoopControl {
    state: Mutex<State>,
    windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
    handlers: Arc<Mutex<PlatformHandlers>>,
}

impl LoopControl {
    pub(super) fn new(
        windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
        handlers: Arc<Mutex<PlatformHandlers>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                phase: Phase::Dormant,
                queued: false,
                requested: false,
                explicit_quit: false,
            }),
            windows,
            handlers,
        })
    }

    pub(super) fn accepts_windows(&self) -> bool {
        !matches!(self.state.lock().phase, Phase::Stopping | Phase::Stopped)
    }

    /// Always deferred: reentrant requests cannot observe an absent leased hook.
    pub(super) fn request(self: &Arc<Self>, explicit_quit: bool) {
        let enqueue = {
            let mut state = self.state.lock();
            if matches!(state.phase, Phase::Stopping | Phase::Stopped) {
                return;
            }
            state.requested = true;
            state.explicit_quit |= explicit_quit;
            if state.phase == Phase::Running && !state.queued {
                state.queued = true;
                true
            } else {
                false
            }
        };
        if enqueue {
            let weak = Arc::downgrade(self);
            exec_async_guarded(owner_queue(), move || {
                if let Some(control) = weak.upgrade() {
                    control.evaluate();
                }
            });
        }
    }

    pub(super) fn quit(&self) {
        let running = {
            let mut state = self.state.lock();
            if matches!(state.phase, Phase::Stopping | Phase::Stopped) {
                return;
            }
            let running = state.phase == Phase::Running;
            state.phase = Phase::Stopping;
            running
        };
        if running {
            stop_application();
        }
    }

    fn evaluate(self: &Arc<Self>) {
        let explicit = {
            let mut state = self.state.lock();
            state.queued = false;
            if state.phase != Phase::Running {
                return;
            }
            state.requested = false;
            std::mem::take(&mut state.explicit_quit)
        };
        if explicit {
            self.quit();
            return;
        }
        if !self.windows.lock().is_empty() {
            return;
        }
        // The hook may replace itself, open a window, request another turn, or quit.
        // Both invocation AND destruction of the displaced hook are outside locks.
        let hook = self.handlers.lock().exit_policy.take();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            hook.as_ref().is_none_or(|hook| hook())
        }));
        let mut hook = hook;
        {
            let mut handlers = self.handlers.lock();
            if self.accepts_windows() && handlers.exit_policy.is_none() {
                handlers.exit_policy = hook.take();
            }
        }
        cleanup_step(|| drop(hook));
        match result {
            Ok(allow)
                if allow
                    && self.state.lock().phase == Phase::Running
                    && self.windows.lock().is_empty() =>
            {
                self.quit();
            }
            Ok(_) => {}
            Err(payload) => {
                report_contained_panic(payload);
                self.quit();
            }
        }
    }

    fn start_running(self: &Arc<Self>) -> bool {
        let requested = {
            let mut state = self.state.lock();
            if state.phase != Phase::Starting {
                return false;
            }
            state.phase = Phase::Running;
            state.requested
        };
        if requested {
            self.request(false);
        }
        true
    }

    fn finish(&self) {
        {
            let mut state = self.state.lock();
            if state.phase == Phase::Stopped {
                return;
            }
            state.phase = Phase::Stopping;
        }
        // No queued cleanup: these resources must be released while main is alive.
        let windows: Vec<_> = self.windows.lock().values().cloned().collect();
        for window in &windows {
            cleanup_step(|| window.callbacks().clear());
        }
        for window in &windows {
            cleanup_step(|| PlatformWindow::close(window.as_ref()));
        }
        let remaining = std::mem::take(&mut *self.windows.lock());
        cleanup_step(|| drop(remaining));
        cleanup_step(|| drop(windows));
        let callback = self.handlers.lock().quit.take();
        if let Some(callback) = callback {
            cleanup_step(callback);
        }
        let handlers = std::mem::take(&mut *self.handlers.lock());
        cleanup_step(|| drop(handlers));
        self.state.lock().phase = Phase::Stopped;
    }
}

fn stop_application() {
    let mtm = MainThreadMarker::new().expect("BUG: AppKit stopping runs on main");
    let app = NSApplication::sharedApplication(mtm);
    app.stop(None);
    // Apple stop(_:) checks the flag only after an NSEvent is dispatched; a
    // GCD callback alone does not unblock it. Same shape as winit 0.30.13.
    // SAFETY: application-defined event has no context/window or borrowed data.
    let event = NSEvent::otherEventWithType_location_modifierFlags_timestamp_windowNumber_context_subtype_data1_data2(NSEventType::ApplicationDefined, NSPoint::new(0.0, 0.0), NSEventModifierFlags::empty(), 0.0, 0, None, 0, 0, 0);
    if let Some(event) = event {
        app.postEvent_atStart(&event, true);
    }
}

define_class!(
    // SAFETY: NSObject has no additional subclass invariants; ivars are Rust-owned.
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = Weak<LoopControl>]
    /// Private, loop-scoped delegate routing native quit requests.
    struct LoopDelegate;
    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for LoopDelegate {}
    // SAFETY: this delegate is retained for its entire installation on main.
    unsafe impl NSApplicationDelegate for LoopDelegate {
        #[unsafe(method(applicationShouldTerminate:))]
        fn should_terminate(&self, _app: &NSApplication) -> NSApplicationTerminateReply {
            // Never run user teardown inside the Objective-C callback. Guard the
            // queue request as well: Rust unwinding must not cross this ABI.
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if let Some(control) = self.ivars().upgrade() { control.request(true); }
            })) { report_contained_panic(payload); }
            NSApplicationTerminateReply::TerminateCancel
        }
    }
);

/// Scope guard retains the non-owning delegate until after cleanup and detachment.
pub(super) struct LoopOwnership {
    app: Retained<NSApplication>,
    delegate: Retained<LoopDelegate>,
    control: Arc<LoopControl>,
    previous_policy: NSApplicationActivationPolicy,
}

impl LoopOwnership {
    pub(super) fn acquire(
        app: &NSApplication,
        control: Arc<LoopControl>,
    ) -> Result<Self, PlatformError> {
        if app.isRunning() || app.delegate().is_some() {
            return Err(PlatformError::EventLoop { message: "FLUI requires a standalone NSApplication with no running loop or existing delegate".into() });
        }
        let mtm = MainThreadMarker::new().expect("BUG: AppKit ownership is acquired on main");
        let allocated = LoopDelegate::alloc(mtm).set_ivars(Arc::downgrade(&control));
        // SAFETY: NSObject initialization of our newly allocated subclass.
        let delegate: Retained<LoopDelegate> = unsafe { msg_send![super(allocated), init] };
        let previous_policy = app.activationPolicy();
        {
            let mut state = control.state.lock();
            if state.phase == Phase::Dormant {
                state.phase = Phase::Starting;
            }
        }
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        Ok(Self {
            app: app.retain(),
            delegate,
            control,
            previous_policy,
        })
    }

    pub(super) fn run(&self) {
        // quit during bootstrap must not be lost by starting a fresh run loop.
        if self.control.start_running() {
            self.app.activateIgnoringOtherApps(true);
            self.app.run();
        }
    }
}

impl Drop for LoopOwnership {
    fn drop(&mut self) {
        // A callback destructor can panic as well as a callback body. Keep
        // detachment outside the shield so even bootstrap unwinding cannot
        // leave AppKit pointing at the delegate that this guard will drop.
        let cleanup =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.control.finish()));
        self.control.state.lock().phase = Phase::Stopped;
        if let Err(payload) = cleanup {
            report_contained_panic(payload);
        }
        let ours = ProtocolObject::<dyn NSApplicationDelegate>::from_ref(&*self.delegate);
        if self
            .app
            .delegate()
            .as_deref()
            .is_some_and(|current| std::ptr::eq(current, ours))
        {
            if self.app.activationPolicy() == NSApplicationActivationPolicy::Regular {
                self.app.setActivationPolicy(self.previous_policy);
            }
            self.app.setDelegate(None);
        }
    }
}
