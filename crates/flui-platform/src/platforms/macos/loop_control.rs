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
    pending_reopens: usize,
    reopen_queued: bool,
    reopen_active: bool,
}

/// Contains only thread-safe Rust state. AppKit objects never cross the queue.
pub(super) struct LoopControl {
    pub(super) owner_signal: Arc<crate::shared::owner_signal::OwnerSignal>,
    state: Mutex<State>,
    windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
    handlers: Arc<Mutex<PlatformHandlers>>,
}

impl LoopControl {
    pub(super) fn new(
        windows: Arc<Mutex<HashMap<u64, Arc<MacOSWindow>>>>,
        handlers: Arc<Mutex<PlatformHandlers>>,
    ) -> Arc<Self> {
        Arc::new_cyclic(|weak: &Weak<Self>| {
            let weak = weak.clone();
            let owner_signal = crate::shared::owner_signal::OwnerSignal::new(Arc::new(move || {
                let weak = weak.clone();
                exec_async_guarded(owner_queue(), move || {
                    cleanup_step(|| {
                        if let Some(control) = weak.upgrade()
                            && control.owner_signal.drive()
                        {
                            control.quit();
                        }
                    });
                });
                Ok(())
            }));
            Self {
                owner_signal,
                state: Mutex::new(State {
                    phase: Phase::Dormant,
                    queued: false,
                    requested: false,
                    explicit_quit: false,
                    pending_reopens: 0,
                    reopen_queued: false,
                    reopen_active: false,
                }),
                windows,
                handlers,
            }
        })
    }

    pub(super) fn accepts_windows(&self) -> bool {
        let state = self.state.lock();
        !state.explicit_quit
            && !matches!(state.phase, Phase::Stopping | Phase::Stopped)
            && self.owner_signal.accepting()
    }

    /// Replacements and rejected registrations are destroyed outside both locks.
    pub(super) fn set_reopen(&self, callback: Box<dyn FnMut() + Send>) {
        let mut callback = Some(callback);
        let previous = {
            let mut handlers = self.handlers.lock();
            if self.accepts_windows() {
                std::mem::replace(&mut handlers.reopen, callback.take())
            } else {
                None
            }
        };
        cleanup_step(|| drop(previous));
        cleanup_step(|| drop(callback));
    }

    fn request_reopen(self: &Arc<Self>) {
        {
            let mut state = self.state.lock();
            if !self.owner_signal.accepting()
                || state.explicit_quit
                || matches!(state.phase, Phase::Stopping | Phase::Stopped)
            {
                return;
            }
            state.pending_reopens = state
                .pending_reopens
                .checked_add(1)
                .expect("BUG: more pending reopen signals than addressable memory");
        }
        self.schedule_reopens();
    }

    fn schedule_reopens(self: &Arc<Self>) {
        let enqueue = {
            let mut state = self.state.lock();
            if state.phase == Phase::Running
                && self.owner_signal.accepting()
                && !state.explicit_quit
                && state.pending_reopens != 0
                && !state.reopen_active
                && !state.reopen_queued
            {
                state.reopen_queued = true;
                true
            } else {
                false
            }
        };
        if enqueue {
            let weak = Arc::downgrade(self);
            exec_async_guarded(owner_queue(), move || {
                // The outer GCD helper predates hostile panic payload handling.
                // Contain the complete delivery here before its trampoline sees it.
                cleanup_step(|| {
                    if let Some(control) = weak.upgrade() {
                        control.drain_reopens();
                    }
                });
            });
        }
    }

    fn drain_reopens(self: &Arc<Self>) {
        {
            let mut state = self.state.lock();
            state.reopen_queued = false;
            if state.phase != Phase::Running
                || state.explicit_quit
                || !self.owner_signal.accepting()
            {
                state.pending_reopens = 0;
                return;
            }
            if state.reopen_active {
                return;
            }
            state.reopen_active = true;
        }
        // Active includes callback destruction: either can pump a nested AppKit
        // loop. Such signals stay pending until this callback lease has ended.
        let _active = ReopenDrain(self);
        loop {
            {
                let mut state = self.state.lock();
                if state.phase != Phase::Running
                    || state.explicit_quit
                    || !self.owner_signal.accepting()
                {
                    state.pending_reopens = 0;
                    return;
                }
                if state.pending_reopens == 0 {
                    return;
                }
                state.pending_reopens -= 1;
            }
            let mut callback = self.handlers.lock().reopen.take();
            cleanup_step(|| {
                if let Some(callback) = callback.as_mut() {
                    callback();
                }
            });
            {
                let mut handlers = self.handlers.lock();
                if self.accepts_windows() && handlers.reopen.is_none() {
                    handlers.reopen = callback.take();
                }
            }
            cleanup_step(|| drop(callback));
        }
    }

    /// Always deferred: reentrant requests cannot observe an absent leased hook.
    pub(super) fn request(self: &Arc<Self>, explicit_quit: bool) {
        if explicit_quit {
            self.owner_signal.fence();
        }
        let enqueue = {
            let mut state = self.state.lock();
            if matches!(state.phase, Phase::Stopping | Phase::Stopped) {
                return;
            }
            state.requested = true;
            state.explicit_quit |= explicit_quit;
            if explicit_quit {
                state.pending_reopens = 0;
            }
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
            state.pending_reopens = 0;
            running
        };
        self.owner_signal.close();
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
            state.explicit_quit
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
        self.schedule_reopens();
        if let Err(error) = self.owner_signal.start() {
            tracing::error!(%error, "could not start owner signals");
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
            state.pending_reopens = 0;
        }
        self.owner_signal.close();
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
        let reopen = self.handlers.lock().reopen.take();
        cleanup_step(|| drop(reopen));
        let handlers = std::mem::take(&mut *self.handlers.lock());
        cleanup_step(|| drop(handlers));
        self.state.lock().phase = Phase::Stopped;
    }
}

struct ReopenDrain<'a>(&'a Arc<LoopControl>);

impl Drop for ReopenDrain<'_> {
    fn drop(&mut self) {
        self.0.state.lock().reopen_active = false;
        cleanup_step(|| self.0.schedule_reopens());
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
    /// Private, loop-scoped delegate routing native quit and reopen requests.
    struct LoopDelegate;
    // SAFETY: NSObjectProtocol has no additional requirements.
    unsafe impl NSObjectProtocol for LoopDelegate {}
    // SAFETY: this delegate is retained for its entire installation on main.
    unsafe impl NSApplicationDelegate for LoopDelegate {
        #[unsafe(method(applicationShouldHandleReopen:hasVisibleWindows:))]
        fn should_reopen(&self, _app: &NSApplication, _has_visible_windows: bool) -> bool {
            cleanup_step(|| {
                if let Some(control) = self.ivars().upgrade() { control.request_reopen(); }
            });
            // FLUI owns window policy. Suppress AppKit's default untitled
            // document creation, regardless of its visible-window classification.
            false
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn control() -> Arc<LoopControl> {
        LoopControl::new(
            Arc::new(Mutex::new(HashMap::new())),
            Arc::new(Mutex::new(PlatformHandlers::default())),
        )
    }

    #[test]
    fn reopen_nested_delivery_preserves_pending_and_uses_replacement_after_lease() {
        let control = control();
        let calls = Arc::new(AtomicUsize::new(0));
        let owner = Arc::clone(&control);
        let observed = Arc::clone(&calls);
        control.set_reopen(Box::new(move || {
            assert_eq!(observed.fetch_add(1, Ordering::SeqCst), 0);
            let next = Arc::clone(&observed);
            owner.set_reopen(Box::new(move || {
                assert_eq!(next.fetch_add(1, Ordering::SeqCst), 1);
            }));
            owner.request_reopen();
            owner.drain_reopens();
            assert_eq!(observed.load(Ordering::SeqCst), 1);
            assert!(owner.state.lock().reopen_active);
        }));
        control.request_reopen(); // Starting/dormant does not enqueue native work.
        control.state.lock().phase = Phase::Running;
        control.drain_reopens();
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(control.state.lock().pending_reopens, 0);
        assert!(!control.state.lock().reopen_active);
        control.finish();
    }

    #[test]
    fn reopen_explicit_quit_fences_delivery_registration_and_window_admission() {
        let control = control();
        control.set_reopen(Box::new(|| panic!("quit-fenced event delivered")));
        control.request_reopen();
        control.request(true); // Dormant request fences without scheduling AppKit.
        assert!(!control.accepts_windows());
        control.state.lock().phase = Phase::Running;
        control.drain_reopens();
        let dropped = Arc::new(AtomicUsize::new(0));
        struct Marker(Arc<AtomicUsize>);
        impl Drop for Marker {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let marker = Marker(Arc::clone(&dropped));
        control.set_reopen(Box::new(move || {
            let _ = &marker;
        }));
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert_eq!(control.state.lock().pending_reopens, 0);
        control.finish();
    }
}
