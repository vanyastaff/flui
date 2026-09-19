//! Main-thread AppKit lifecycle cases; scripts/check-macos-exit.py bounds each process.
#[cfg(target_os = "macos")]
mod native {
    // This native executable verifies AppKit ownership through typed Objective-C handles.
    #![expect(unsafe_code)]
    use flui_platform::{OwnerPlatform, PlatformWindow, WindowOptions};
    use objc2::{
        MainThreadMarker, MainThreadOnly, define_class, msg_send,
        rc::{Retained, Weak, autoreleasepool},
        runtime::ProtocolObject,
    };
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSApplicationDelegate, NSWindow,
    };
    use objc2_foundation::{NSObject, NSObjectProtocol};
    use std::{
        cell::RefCell,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        time::Duration,
    };

    thread_local! { static NATIVE_WINDOWS: RefCell<Vec<Weak<NSWindow>>> = const { RefCell::new(Vec::new()) }; }

    thread_local! { static OWNER: RefCell<Option<OwnerPlatform>> = const { RefCell::new(None) }; }
    fn quit() {
        OWNER.with(|owner| owner.borrow().as_ref().expect("owner").quit());
    }
    fn open() -> Arc<dyn PlatformWindow> {
        OWNER.with(|owner| {
            owner
                .borrow()
                .as_ref()
                .expect("owner")
                .open_window(WindowOptions {
                    visible: false,
                    ..WindowOptions::default()
                })
                .expect("open")
                .try_ready()
                .expect("ready")
        })
    }

    thread_local! { static REPLACEMENT_DELEGATE: RefCell<Option<Retained<ForeignDelegate>>> = const { RefCell::new(None) }; }
    struct PanickingDrop;
    impl Drop for PanickingDrop {
        fn drop(&mut self) {
            panic!("intentional callback destructor panic");
        }
    }

    struct DropMarker(&'static str);
    impl Drop for DropMarker {
        fn drop(&mut self) {
            println!("{}", self.0);
        }
    }

    fn on_main(body: impl FnOnce() + Send + 'static) {
        dispatch2::DispatchQueue::main().exec_async(move || {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).is_err() {
                // Failure only: never unwind through GCD's C trampoline.
                std::process::abort();
            }
        });
    }

    fn remember(window: &Arc<dyn PlatformWindow>) {
        // SAFETY: this probe selected native AppKit; its WindowId is the live
        // NSWindow address. Main owns the wrapper throughout this extra retain.
        let native =
            unsafe { Retained::retain(window.id().0 as *mut NSWindow) }.expect("native window");
        NATIVE_WINDOWS.with(|windows| windows.borrow_mut().push(Weak::new(&native)));
    }

    define_class!(
        // SAFETY: NSObject imposes no additional subclass requirements.
        #[unsafe(super = NSObject)]
        #[thread_kind = MainThreadOnly]
        struct ForeignDelegate;
        // SAFETY: neither protocol imposes additional implementation requirements.
        unsafe impl NSObjectProtocol for ForeignDelegate {}
        unsafe impl NSApplicationDelegate for ForeignDelegate {}
    );

    fn foreign_delegate(mtm: MainThreadMarker) -> Retained<ForeignDelegate> {
        // SAFETY: initializing a fresh NSObject subclass on main.
        unsafe { msg_send![ForeignDelegate::alloc(mtm), init] }
    }

    pub(super) fn run() {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::ERROR)
            .init();
        let _stack = DropMarker("EXIT_POLICY_STACK_DROPPED");
        let case = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "last-window".into());
        assert!(
            matches!(
                case.as_str(),
                "last-window"
                    | "two-windows"
                    | "veto"
                    | "service-release"
                    | "hook-reentrant"
                    | "close-reopens"
                    | "hook-opens"
                    | "explicit-quit"
                    | "native-quit"
                    | "bootstrap-quit"
                    | "pre-run-quit"
                    | "bootstrap-error"
                    | "foreign-owner"
                    | "running-owner"
                    | "delegate-replaced"
                    | "policy-replaced"
                    | "bootstrap-panic"
                    | "quit-panic"
                    | "coalesced"
                    | "hook-drop-panic"
            ),
            "unknown probe case: {case}"
        );
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        let original_policy = app.activationPolicy();
        if case == "foreign-owner" {
            let delegate = foreign_delegate(mtm);
            app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            let platform = flui_platform::current_platform().expect("construct without mutation");
            assert_eq!(
                app.activationPolicy(),
                NSApplicationActivationPolicy::Accessory
            );
            let result = platform.run(Box::new(|_| panic!("foreign bootstrap must not run")));
            assert!(result.is_err());
            assert!(app.delegate().is_some_and(|current| std::ptr::eq(
                std::ptr::from_ref(&*current),
                ProtocolObject::from_ref(&*delegate)
            )));
            assert_eq!(
                app.activationPolicy(),
                NSApplicationActivationPolicy::Accessory
            );
            app.setDelegate(None);
            app.setActivationPolicy(original_policy);
            println!("EXIT_POLICY_RETURNED");
            println!("EXIT_POLICY_CASE=foreign-owner");
            return;
        }
        let quit_calls = Arc::new(AtomicUsize::new(0));
        let case_for_ready = case.clone();
        autoreleasepool(|_| {
            let platform = flui_platform::current_platform().expect("native platform");
            if case == "pre-run-quit" {
                platform.quit();
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                platform.run(Box::new(move |owner| {
                    let shared = owner.shared();
                    let quit_calls = Arc::clone(&quit_calls);
                    OWNER.with(|slot| *slot.borrow_mut() = Some(owner));
                    let dropped = DropMarker("EXIT_POLICY_CALLBACK_DROPPED");
                    let panic_on_quit = case_for_ready == "quit-panic";
                    shared.on_quit(Box::new(move || {
                        let _ = &dropped;
                        assert_eq!(quit_calls.fetch_add(1, Ordering::SeqCst), 0);
                        quit();
                        println!("EXIT_POLICY_QUIT_NOTIFIED");
                        assert!(!panic_on_quit, "intentional quit callback panic");
                    }));
                    if case_for_ready == "pre-run-quit" {
                        return Ok(());
                    }
                    let window = open();
                    remember(&window);
                    window.on_close(Box::new(|| println!("EXIT_POLICY_WINDOW_CLOSED")));
                    if case_for_ready == "bootstrap-panic" {
                        let panic_on_drop = PanickingDrop;
                        window.on_close(Box::new(move || {
                            let _ = &panic_on_drop;
                        }));
                        panic!("intentional bootstrap panic");
                    }
                    if matches!(
                        case_for_ready.as_str(),
                        "delegate-replaced" | "policy-replaced"
                    ) {
                        let mtm = MainThreadMarker::new().expect("main");
                        let app = NSApplication::sharedApplication(mtm);
                        if case_for_ready == "delegate-replaced" {
                            let delegate = foreign_delegate(mtm);
                            app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
                            REPLACEMENT_DELEGATE.with(|slot| *slot.borrow_mut() = Some(delegate));
                        }
                        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
                        quit();
                        return Ok(());
                    }
                    if case_for_ready == "bootstrap-error" {
                        return Err(Box::new(std::io::Error::other(
                            "intentional bootstrap failure",
                        )));
                    }
                    if case_for_ready == "bootstrap-quit" {
                        quit();
                        return Ok(());
                    }
                    let case = case_for_ready;
                    let hook_calls = Arc::new(AtomicUsize::new(0));
                    let allow = Arc::new(AtomicBool::new(false));
                    if matches!(
                        case.as_str(),
                        "veto" | "service-release" | "coalesced" | "two-windows"
                    ) {
                        if case == "two-windows" {
                            allow.store(true, Ordering::SeqCst);
                        }
                        let allow = Arc::clone(&allow);
                        let calls = Arc::clone(&hook_calls);
                        shared.set_exit_policy_hook(Box::new(move || {
                            assert!(MainThreadMarker::new().is_some());
                            calls.fetch_add(1, Ordering::SeqCst);
                            allow.load(Ordering::SeqCst)
                        }));
                    }
                    if case == "hook-drop-panic" {
                        let shared_hook = shared.clone();
                        let panic_on_drop = PanickingDrop;
                        shared.set_exit_policy_hook(Box::new(move || {
                            let _ = &panic_on_drop;
                            shared_hook.set_exit_policy_hook(Box::new(|| true));
                            println!("EXIT_POLICY_HOOK_DROP_ARMED");
                            true
                        }));
                    }
                    if case == "hook-reentrant" {
                        let shared_hook = shared.clone();
                        shared.set_exit_policy_hook(Box::new(move || {
                            shared_hook.request_exit_policy_reevaluation();
                            shared_hook.set_exit_policy_hook(Box::new(|| {
                                println!("EXIT_POLICY_REPLACEMENT_HOOK_INVOKED");
                                on_main(quit);
                                false
                            }));
                            println!("EXIT_POLICY_HOOK_REPLACED");
                            false
                        }));
                    }
                    if case == "close-reopens" || case == "hook-opens" {
                        // OwnerPlatform is owner-affine; use the existing platform
                        // proxy's ready request on main rather than send it to a worker.
                        let reopen = move || {
                            let replacement = open();
                            remember(&replacement);
                            println!("EXIT_POLICY_REPLACEMENT_OPENED");
                            std::thread::spawn(move || {
                                std::thread::sleep(Duration::from_millis(150));
                                on_main(move || {
                                    assert!(
                                        replacement.window_handle().is_ok(),
                                        "replacement must survive a later owner turn"
                                    );
                                    println!("EXIT_POLICY_REPLACEMENT_SURVIVED");
                                    replacement.close();
                                });
                            });
                        };
                        if case == "close-reopens" {
                            window.on_close(Box::new(move || {
                                println!("EXIT_POLICY_WINDOW_CLOSED");
                                reopen();
                            }));
                        } else {
                            let once = std::sync::Mutex::new(Some(reopen));
                            shared.set_exit_policy_hook(Box::new(move || {
                                if let Some(reopen) = once.lock().expect("reopen lock").take() {
                                    reopen();
                                }
                                true
                            }));
                        }
                    }
                    let second = if case == "two-windows" {
                        let second = open();
                        remember(&second);
                        Some(second)
                    } else {
                        None
                    };
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(100));
                        on_main(move || {
                            if !matches!(
                                case.as_str(),
                                "explicit-quit" | "native-quit" | "running-owner" | "quit-panic"
                            ) {
                                window.close();
                            }
                            if case == "last-window"
                                || case == "close-reopens"
                                || case == "hook-opens"
                                || case == "hook-drop-panic"
                            {
                                return;
                            }
                            // The close request has returned, so its queued re-evaluation
                            // precedes this barrier on the same serial owner queue.
                            on_main(move || {
                                if let Some(second) = second {
                                    assert_eq!(
                                        hook_calls.load(Ordering::SeqCst),
                                        0,
                                        "policy must not run with an open window"
                                    );
                                    assert!(
                                        second.window_handle().is_ok(),
                                        "second window still live"
                                    );
                                    println!("EXIT_POLICY_SECOND_SURVIVED");
                                    second.close();
                                    return;
                                }
                                match case.as_str() {
                                    "coalesced" => {
                                        assert_eq!(hook_calls.load(Ordering::SeqCst), 1);
                                        for _ in 0..100 {
                                            shared.request_exit_policy_reevaluation();
                                        }
                                        on_main(move || {
                                            assert_eq!(
                                                hook_calls.load(Ordering::SeqCst),
                                                2,
                                                "one owner recheck for the burst"
                                            );
                                            println!("EXIT_POLICY_BURST_COALESCED");
                                            quit();
                                        });
                                    }
                                    "service-release" => {
                                        assert!(hook_calls.load(Ordering::SeqCst) > 0);
                                        println!("EXIT_POLICY_SERVICE_VETOED");
                                        std::thread::spawn(move || {
                                            allow.store(true, Ordering::SeqCst);
                                            shared.request_exit_policy_reevaluation();
                                        });
                                    }
                                    "native-quit" => NSApplication::sharedApplication(
                                        MainThreadMarker::new().expect("main"),
                                    )
                                    .terminate(None),
                                    "running-owner" => {
                                        let nested = flui_platform::current_platform()
                                            .expect("nested constructor");
                                        assert!(
                                            nested
                                                .run(Box::new(|_| panic!("nested bootstrap")))
                                                .is_err()
                                        );
                                        println!("EXIT_POLICY_RUNNING_REJECTED");
                                        quit();
                                    }
                                    "veto" => {
                                        assert!(hook_calls.load(Ordering::SeqCst) > 0);
                                        println!("EXIT_POLICY_VETO_SURVIVED");
                                        quit();
                                    }
                                    "hook-reentrant" => {
                                        println!("EXIT_POLICY_REENTRANT_SURVIVED");
                                    }
                                    _ => quit(),
                                }
                            });
                        });
                    });
                    Ok(())
                }))
            }));
            if case == "bootstrap-panic" {
                assert!(
                    result.is_err(),
                    "original bootstrap panic must propagate through Rust"
                );
            } else if case == "bootstrap-error" {
                let result = result.expect("no bootstrap panic");
                match result.expect_err("bootstrap failure") {
                    flui_platform::PlatformError::Bootstrap { source, loop_error } => {
                        assert_eq!(source.to_string(), "intentional bootstrap failure");
                        assert!(loop_error.is_none());
                    }
                    other => panic!("expected original bootstrap cause, got {other}"),
                }
            } else {
                result
                    .expect("no lifecycle panic escapes")
                    .expect("normal return");
            }
            OWNER.with(|slot| {
                slot.borrow_mut().take();
            });
            println!("EXIT_POLICY_RETURNED");
        });
        if case == "delegate-replaced" {
            REPLACEMENT_DELEGATE.with(|slot| {
                let slot = slot.borrow();
                let expected =
                    ProtocolObject::from_ref(&**slot.as_ref().expect("replacement retained"));
                assert!(
                    app.delegate().is_some_and(|current| std::ptr::eq(
                        std::ptr::from_ref(&*current),
                        expected
                    )),
                    "external replacement delegate preserved"
                );
            });
            assert_eq!(
                app.activationPolicy(),
                NSApplicationActivationPolicy::Accessory
            );
            app.setDelegate(None);
            REPLACEMENT_DELEGATE.with(|slot| {
                slot.borrow_mut().take();
            });
            app.setActivationPolicy(original_policy);
            println!("EXIT_POLICY_EXTERNAL_OWNERSHIP_PRESERVED");
        } else {
            assert!(app.delegate().is_none(), "owned delegate detached");
            if case == "policy-replaced" {
                assert_eq!(
                    app.activationPolicy(),
                    NSApplicationActivationPolicy::Accessory
                );
                app.setActivationPolicy(original_policy);
                println!("EXIT_POLICY_EXTERNAL_POLICY_PRESERVED");
            } else {
                assert_eq!(app.activationPolicy(), original_policy);
            }
        }
        NATIVE_WINDOWS.with(|windows| {
            assert!(
                windows
                    .borrow()
                    .iter()
                    .all(|window| window.load().is_none()),
                "native windows released after autorelease pool drained"
            );
        });
        println!("EXIT_POLICY_NATIVE_RELEASED");
        println!("EXIT_POLICY_CASE={case}");
    }
}
#[cfg(target_os = "macos")]
fn main() {
    native::run();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("AppKit exit probe requires macOS");
}
