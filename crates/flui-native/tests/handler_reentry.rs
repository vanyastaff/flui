//! The platform calls the application's and a window's handlers with none of
//! its locks or borrows held, on every host: a handler may install another,
//! quit or open a window from inside its call; a replaced handler's captures,
//! whose destructor calls the platform, are dropped after the platform let go
//! of its state; and a handler's panic stays the first failure, its displaced
//! captures retained rather than dropped into a second one.
//!
//! A case that regresses deadlocks rather than fails, so each one runs on a
//! worker thread and is reported after [`TIMEOUT`]: the worker stays parked
//! for the rest of the process.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{RecvTimeoutError, channel};
use std::sync::{Arc, Weak};
use std::time::Duration;

use flui_native::{HeadlessPlatform, HostWindow, Platform};
use flui_platform::{
    DispatchEventResult, SessionEndAnswer, SessionEndPhase, SystemNotice, WindowEvent,
    WindowOptions,
};
use parking_lot::Mutex;

/// How long a case may take before it counts as deadlocked.
const TIMEOUT: Duration = Duration::from_secs(10);

/// Run `case` on a worker thread: its panic is resumed here, and a case that
/// does not finish within [`TIMEOUT`] fails as deadlocked.
fn within(case: &'static str, body: impl FnOnce() + Send + 'static) {
    let (done, finished) = channel();
    let worker = std::thread::spawn(move || {
        body();
        let _ = done.send(());
    });
    match finished.recv_timeout(TIMEOUT) {
        Ok(()) => worker.join().expect("the case finished"),
        Err(RecvTimeoutError::Disconnected) => {
            std::panic::resume_unwind(worker.join().expect_err("the case panicked"));
        }
        Err(RecvTimeoutError::Timeout) => panic!(
            "{case}: deadlocked — a handler ran, or a replaced one was dropped, under a lock the \
             platform holds"
        ),
    }
}

/// Runs its action when dropped: a capture whose destructor calls back into
/// the platform, as a UI runtime's state does.
struct OnDrop(Option<Box<dyn FnOnce() + Send + Sync>>);

impl OnDrop {
    fn new(action: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self(Some(Box::new(action)))
    }

    fn inert() -> Self {
        Self(None)
    }
}

impl Drop for OnDrop {
    fn drop(&mut self) {
        if let Some(action) = self.0.take() {
            action();
        }
    }
}

/// Records whether the capture it sits in was dropped.
struct Witness(Arc<AtomicBool>);

impl Drop for Witness {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

fn payload_text(payload: &(dyn Any + Send)) -> Option<&str> {
    flui_foundation::panic::payload_text(payload)
}

// --- application handlers -------------------------------------------------

/// Installs an application handler of one kind, its closure holding `capture`.
type InstallHandler = fn(&dyn Platform, OnDrop);

/// Every application handler a platform takes.
const APPLICATION_HANDLERS: &[(&str, InstallHandler)] = &[
    ("on_quit", |platform, capture| {
        platform.on_quit(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_window_event", |platform, capture| {
        platform.on_window_event(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
    ("set_exit_policy_hook", |platform, capture| {
        platform.set_exit_policy_hook(Box::new(move || {
            let _ = &capture;
            false
        }));
    }),
    ("set_wake_deadline_hook", |platform, capture| {
        platform.set_wake_deadline_hook(Box::new(move || {
            let _ = &capture;
            None
        }));
    }),
    ("on_session_end", |platform, capture| {
        platform.on_session_end(Box::new(move |_| {
            let _ = &capture;
            SessionEndAnswer::Proceed
        }));
    }),
    ("on_keyboard_layout_change", |platform, capture| {
        platform.on_keyboard_layout_change(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_reopen", |platform, capture| {
        platform.on_reopen(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_host_notice", |platform, capture| {
        platform.on_host_notice(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
];

/// For each kind of application handler: a handler whose captures call the
/// platform when dropped is replaced, and its destructor reaches the platform.
fn replaced_application_handlers_reach_the_platform(platform: &Arc<dyn Platform>) {
    for &(name, install) in APPLICATION_HANDLERS {
        let reached = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reached);
        let weak: Weak<dyn Platform> = Arc::downgrade(platform);
        install(
            platform.as_ref(),
            OnDrop::new(move || {
                if let Some(platform) = weak.upgrade() {
                    let _ = platform.active_window();
                    observed.store(true, Ordering::SeqCst);
                }
            }),
        );
        install(platform.as_ref(), OnDrop::inert());
        assert!(
            reached.load(Ordering::SeqCst),
            "{name}: the replaced handler's destructor did not reach the platform"
        );
    }
}

// --- window callbacks -----------------------------------------------------

/// Installs a window callback of one kind, its closure holding `capture`.
type InstallCallback = fn(&dyn HostWindow, OnDrop);

/// Every callback a window takes.
const WINDOW_CALLBACKS: &[(&str, InstallCallback)] = &[
    ("on_input", |window, capture| {
        window.on_input(Box::new(move |_| {
            let _ = &capture;
            DispatchEventResult::default()
        }));
    }),
    ("on_request_frame", |window, capture| {
        window.on_request_frame(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_resize", |window, capture| {
        window.on_resize(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
    ("on_state_changed", |window, capture| {
        window.on_state_changed(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_close", |window, capture| {
        window.on_close(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_should_close", |window, capture| {
        window.on_should_close(Box::new(move || {
            let _ = &capture;
            true
        }));
    }),
    ("on_safe_area_change", |window, capture| {
        window.on_safe_area_change(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
    ("on_execution_state_change", |window, capture| {
        window.on_execution_state_change(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
    ("on_active_status_change", |window, capture| {
        window.on_active_status_change(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
    ("on_visibility_status_change", |window, capture| {
        window.on_visibility_status_change(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
    ("on_hover_status_change", |window, capture| {
        window.on_hover_status_change(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
    ("on_appearance_changed", |window, capture| {
        window.on_appearance_changed(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_display_changed", |window, capture| {
        window.on_display_changed(Box::new(move || {
            let _ = &capture;
        }));
    }),
    ("on_surface_status_change", |window, capture| {
        window.on_surface_status_change(Box::new(move |_| {
            let _ = &capture;
        }));
    }),
];

/// For each kind of window callback: a callback whose captures register the
/// same kind again when dropped is replaced, and that registration lands.
fn replaced_window_callbacks_reach_their_window(window: &Arc<dyn HostWindow>) {
    for &(name, install) in WINDOW_CALLBACKS {
        let reached = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reached);
        let weak = Arc::downgrade(window);
        install(
            window.as_ref(),
            OnDrop::new(move || {
                if let Some(window) = weak.upgrade() {
                    install(window.as_ref(), OnDrop::inert());
                    observed.store(true, Ordering::SeqCst);
                }
            }),
        );
        install(window.as_ref(), OnDrop::inert());
        assert!(
            reached.load(Ordering::SeqCst),
            "{name}: the replaced callback's destructor could not register again"
        );
    }
}

// --- headless -------------------------------------------------------------

fn headless() -> Arc<HeadlessPlatform> {
    Arc::new(HeadlessPlatform::new())
}

/// A window-event handler that, told a window was created, installs its
/// successor, opens another window and quits: each reaches the platform, the
/// successor hears the nested window, and the handler that ran is retired.
fn headless_window_event_handler_reenters() {
    within("headless window-event handler", || {
        let platform = headless();
        let quits = Arc::new(AtomicUsize::new(0));
        let counted = Arc::clone(&quits);
        platform.on_quit(Box::new(move || {
            counted.fetch_add(1, Ordering::SeqCst);
        }));
        let successor_heard = Arc::new(Mutex::new(Vec::new()));
        let retired = Arc::new(AtomicBool::new(false));
        let witness = Witness(Arc::clone(&retired));
        let nested_window = Arc::new(Mutex::new(None));
        let (weak, heard, nested) = (
            Arc::downgrade(&platform),
            Arc::clone(&successor_heard),
            Arc::clone(&nested_window),
        );
        let mut first = true;
        platform.on_window_event(Box::new(move |event| {
            let _ = &witness;
            if !std::mem::take(&mut first) {
                return;
            }
            let platform = weak.upgrade().expect("the platform");
            let heard = Arc::clone(&heard);
            platform.on_window_event(Box::new(move |event| heard.lock().push(event)));
            let window = platform
                .open_window(WindowOptions::default())
                .expect("a window opens from inside the handler");
            *nested.lock() = Some(window.id());
            platform.quit();
            assert!(matches!(event, WindowEvent::Created(_)));
        }));

        let _window = platform
            .open_window(WindowOptions::default())
            .expect("a window");

        let nested = nested_window.lock().expect("the handler opened a window");
        assert!(
            matches!(successor_heard.lock().as_slice(), [WindowEvent::Created(id)] if *id == nested),
            "the successor hears the window opened from inside the first handler"
        );
        assert_eq!(quits.load(Ordering::SeqCst), 1, "the quit reached the host");
        assert!(
            retired.load(Ordering::SeqCst),
            "the handler that ran is retired once it returns"
        );
    });
}

/// A session-end hook that installs its successor from inside its call, its
/// captures calling the platform when dropped.
fn headless_session_end_hook_replaces_itself() {
    within("headless session-end hook", || {
        let platform = headless();
        let reached = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reached);
        let weak = Arc::downgrade(&platform);
        let calls_back = OnDrop::new(move || {
            if let Some(platform) = weak.upgrade() {
                let _ = platform.active_window();
                observed.store(true, Ordering::SeqCst);
            }
        });
        let weak = Arc::downgrade(&platform);
        platform.on_session_end(Box::new(move |_| {
            let _ = &calls_back;
            let platform = weak.upgrade().expect("the platform");
            platform.on_session_end(Box::new(|_| SessionEndAnswer::Block));
            platform.quit();
            SessionEndAnswer::Proceed
        }));

        assert_eq!(
            platform.simulate_session_end(SessionEndPhase::Query),
            SessionEndAnswer::Proceed
        );
        assert!(
            reached.load(Ordering::SeqCst),
            "the replaced hook's destructor reached the platform"
        );
        assert_eq!(
            platform.simulate_session_end(SessionEndPhase::Query),
            SessionEndAnswer::Block,
            "the successor answers the next phase"
        );
    });
}

/// A system-notice hook that installs its successor from inside its call, its
/// captures reporting another notice when dropped: that notice is heard after
/// the one being delivered, by the successor.
fn headless_notice_hook_replaces_itself() {
    within("headless system-notice hook", || {
        let platform = headless();
        let reached = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&reached);
        let weak = Arc::downgrade(&platform);
        let calls_back = OnDrop::new(move || {
            if let Some(platform) = weak.upgrade() {
                platform.simulate_host_notice(SystemNotice::SessionLocked);
                observed.store(true, Ordering::SeqCst);
            }
        });
        let successor = Arc::new(Mutex::new(Vec::new()));
        let (weak, heard) = (Arc::downgrade(&platform), Arc::clone(&successor));
        platform.on_host_notice(Box::new(move |_| {
            let _ = &calls_back;
            let heard = Arc::clone(&heard);
            weak.upgrade()
                .expect("the platform")
                .on_host_notice(Box::new(move |notice| heard.lock().push(notice)));
        }));

        platform.simulate_host_notice(SystemNotice::Sleep);
        assert!(
            reached.load(Ordering::SeqCst),
            "the replaced hook's destructor reached the platform"
        );
        platform.simulate_host_notice(SystemNotice::Wake);
        assert_eq!(
            *successor.lock(),
            [SystemNotice::SessionLocked, SystemNotice::Wake]
        );
    });
}

fn headless_replaced_application_handlers_reach_the_platform() {
    within("headless application handlers", || {
        let platform: Arc<dyn Platform> = headless();
        replaced_application_handlers_reach_the_platform(&platform);
    });
}

fn headless_replaced_window_callbacks_reach_their_window() {
    within("headless window callbacks", || {
        let platform = headless();
        let window = platform
            .open_window(WindowOptions::default())
            .expect("a window");
        replaced_window_callbacks_reach_their_window(&window);
    });
}

/// A session-end hook that installs its successor and then panics: the
/// caller gets that panic, the hook's captures are retained rather than
/// dropped after the failure, and the successor answers the next phase.
fn headless_failing_hook_keeps_the_first_failure() {
    within("headless failing session-end hook", || {
        let platform = headless();
        let dropped = Arc::new(AtomicBool::new(false));
        let witness = Witness(Arc::clone(&dropped));
        let weak = Arc::downgrade(&platform);
        platform.on_session_end(Box::new(move |_| {
            let _ = &witness;
            weak.upgrade()
                .expect("the platform")
                .on_session_end(Box::new(|_| SessionEndAnswer::Block));
            panic!("first failure");
        }));

        let failure = catch_unwind(AssertUnwindSafe(|| {
            platform.simulate_session_end(SessionEndPhase::Query)
        }))
        .expect_err("the hook's panic reaches the caller");
        assert_eq!(payload_text(failure.as_ref()), Some("first failure"));
        assert!(
            !dropped.load(Ordering::SeqCst),
            "the failed hook's captures are retained, not dropped"
        );
        assert_eq!(
            platform.simulate_session_end(SessionEndPhase::Query),
            SessionEndAnswer::Block
        );
    });
}

#[test]
fn handlers_run_outside_every_lock_of_the_headless_platform() {
    crate::run_table(
        "handlers_run_outside_every_lock_of_the_headless_platform",
        &[
            (
                "a window-event handler installs, opens and quits",
                headless_window_event_handler_reenters,
            ),
            (
                "a session-end hook replaces itself",
                headless_session_end_hook_replaces_itself,
            ),
            (
                "a system-notice hook replaces itself",
                headless_notice_hook_replaces_itself,
            ),
            (
                "replaced application handlers reach the platform",
                headless_replaced_application_handlers_reach_the_platform,
            ),
            (
                "replaced window callbacks reach their window",
                headless_replaced_window_callbacks_reach_their_window,
            ),
            (
                "a failing hook keeps the first failure",
                headless_failing_hook_keeps_the_first_failure,
            ),
        ],
    );
}

// --- winit ----------------------------------------------------------------

/// The winit platform's registrations, before its loop runs: a replaced
/// handler's destructor reaches the platform.
#[cfg(all(
    feature = "winit-backend",
    any(target_os = "windows", target_os = "linux")
))]
#[test]
fn replaced_winit_handlers_reach_the_platform() {
    within("winit application handlers", || {
        let platform: Arc<dyn Platform> = Arc::new(flui_native::WinitPlatform::new());
        replaced_application_handlers_reach_the_platform(&platform);
    });
}

// --- Win32 ----------------------------------------------------------------

#[cfg(target_os = "windows")]
mod win32 {
    #![expect(
        unsafe_code,
        reason = "pumps the owner thread's own Win32 message queue"
    )]

    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use flui_native::{Platform, WindowsPlatform};
    use flui_platform::{WindowEvent, WindowOptions};
    use parking_lot::Mutex;
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, MSG, PM_REMOVE, PeekMessageW, TranslateMessage, WM_QUIT,
    };

    use super::{
        Witness, replaced_application_handlers_reach_the_platform,
        replaced_window_callbacks_reach_their_window, within,
    };

    fn platform() -> Arc<WindowsPlatform> {
        Arc::new(WindowsPlatform::new().expect("native Windows platform"))
    }

    fn hidden() -> WindowOptions {
        WindowOptions::default()
            .with_title("handler reentry")
            .with_visible(false)
    }

    /// Dispatch this thread's messages until `done`, for at most two seconds;
    /// a posted quit is consumed, not acted on.
    fn pump_until(mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(2) {
            let mut message = MSG::default();
            // SAFETY: `message` is a stack-local out-parameter; the messages
            // are this thread's own.
            unsafe {
                while PeekMessageW(&raw mut message, None, 0, 0, PM_REMOVE).as_bool() {
                    if message.message == WM_QUIT {
                        continue;
                    }
                    let _ = TranslateMessage(&raw const message);
                    DispatchMessageW(&raw const message);
                }
            }
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    /// A window-event handler that, told a window closed, installs its
    /// successor, opens another window and quits.
    fn window_event_handler_reenters() {
        within("Win32 window-event handler", || {
            let platform = platform();
            let successor_heard = Arc::new(AtomicUsize::new(0));
            let nested_window = Arc::new(Mutex::new(None));
            let (weak, heard, nested) = (
                Arc::downgrade(&platform),
                Arc::clone(&successor_heard),
                Arc::clone(&nested_window),
            );
            let mut first = true;
            platform.on_window_event(Box::new(move |event| {
                if !matches!(event, WindowEvent::Closed(_)) || !std::mem::take(&mut first) {
                    return;
                }
                let platform = weak.upgrade().expect("the platform");
                let heard = Arc::clone(&heard);
                platform.on_window_event(Box::new(move |event| {
                    if matches!(event, WindowEvent::Closed(_)) {
                        heard.fetch_add(1, Ordering::SeqCst);
                    }
                }));
                *nested.lock() = Some(
                    platform
                        .open_window(hidden())
                        .expect("a window opens from inside the handler"),
                );
                platform.quit();
            }));

            platform.open_window(hidden()).expect("a window").close();
            assert!(pump_until(|| nested_window.lock().is_some()));
            let nested = nested_window.lock().take().expect("the nested window");
            nested.close();
            assert!(pump_until(|| successor_heard.load(Ordering::SeqCst) == 1));
        });
    }

    fn replaced_application_handlers() {
        within("Win32 application handlers", || {
            let platform: Arc<dyn Platform> = platform();
            replaced_application_handlers_reach_the_platform(&platform);
        });
    }

    fn replaced_window_callbacks() {
        within("Win32 window callbacks", || {
            let platform = platform();
            let window = platform.open_window(hidden()).expect("a window");
            replaced_window_callbacks_reach_their_window(&window);
            window.close();
        });
    }

    /// An exit-policy hook that installs its successor and then panics inside
    /// the window procedure: the panic is contained as a veto, the hook's
    /// captures are retained rather than dropped after the failure, and the
    /// successor decides the next request.
    fn failing_exit_policy_keeps_the_first_failure() {
        within("Win32 failing exit-policy hook", || {
            let platform = platform();
            let ran = Arc::new(AtomicBool::new(false));
            let dropped = Arc::new(AtomicBool::new(false));
            let successor_calls = Arc::new(AtomicUsize::new(0));
            let witness = Witness(Arc::clone(&dropped));
            let (weak, failed, successor) = (
                Arc::downgrade(&platform),
                Arc::clone(&ran),
                Arc::clone(&successor_calls),
            );
            platform.set_exit_policy_hook(Box::new(move || {
                let _ = &witness;
                failed.store(true, Ordering::SeqCst);
                let successor = Arc::clone(&successor);
                weak.upgrade()
                    .expect("the platform")
                    .set_exit_policy_hook(Box::new(move || {
                        successor.fetch_add(1, Ordering::SeqCst);
                        false
                    }));
                panic!("first failure");
            }));

            platform.request_exit_policy_reevaluation();
            assert!(pump_until(|| ran.load(Ordering::SeqCst)), "the hook ran");
            assert!(
                !dropped.load(Ordering::SeqCst),
                "the failed hook's captures are retained, not dropped"
            );
            platform.request_exit_policy_reevaluation();
            assert!(
                pump_until(|| successor_calls.load(Ordering::SeqCst) == 1),
                "the successor decides the next request"
            );
        });
    }

    #[test]
    fn handlers_run_outside_every_borrow_of_the_win32_platform() {
        crate::run_table(
            "handlers_run_outside_every_borrow_of_the_win32_platform",
            &[
                (
                    "a window-event handler installs, opens and quits",
                    window_event_handler_reenters,
                ),
                (
                    "replaced application handlers reach the platform",
                    replaced_application_handlers,
                ),
                (
                    "replaced window callbacks reach their window",
                    replaced_window_callbacks,
                ),
                (
                    "a failing exit-policy hook keeps the first failure",
                    failing_exit_policy_keeps_the_first_failure,
                ),
            ],
        );
    }
}
