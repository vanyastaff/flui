//! Native reopen transport probe, bounded by scripts/check-macos-reopen.py.
#[cfg(target_os = "macos")]
mod native {
    use flui_platform::{OwnerPlatform, SharedPlatform, WindowOptions};
    use objc2::{MainThreadMarker, rc::Retained, runtime::ProtocolObject, sel};
    use objc2_app_kit::{NSApplication, NSApplicationDelegate};
    use objc2_foundation::{NSDate, NSObjectProtocol, NSRunLoop};
    use std::{
        cell::RefCell,
        io::Write,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    thread_local! {
        static OWNER: RefCell<Option<OwnerPlatform>> = const { RefCell::new(None) };
        static OLD_DELEGATE: RefCell<Option<Retained<ProtocolObject<dyn NSApplicationDelegate>>>> = const { RefCell::new(None) };
    }
    fn app() -> Retained<NSApplication> {
        NSApplication::sharedApplication(MainThreadMarker::new().expect("main"))
    }
    fn quit() {
        OWNER.with(|owner| owner.borrow().as_ref().expect("owner").quit());
    }
    fn enqueue(body: impl FnOnce() + Send + 'static) {
        dispatch2::DispatchQueue::main().exec_async(move || {
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
                std::mem::forget(payload);
                eprintln!("REOPEN_PROBE_FAILURE");
                std::process::abort();
            }
        });
    }
    fn signal(visible: bool) {
        let app = app();
        let delegate = app.delegate().expect("owned delegate");
        if !delegate.respondsToSelector(sel!(applicationShouldHandleReopen:hasVisibleWindows:)) {
            eprintln!("REOPEN_MISSING_SELECTOR");
            std::process::abort();
        }
        assert!(!delegate.applicationShouldHandleReopen_hasVisibleWindows(&app, visible));
    }
    fn nested_pump() {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.03));
        println!("REOPEN_NESTED_PUMP_RETURNED");
    }
    struct DropMarker(Arc<AtomicUsize>);
    impl Drop for DropMarker {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    struct HostilePayload;
    impl Drop for HostilePayload {
        fn drop(&mut self) {
            panic!("hostile panic payload drop");
        }
    }
    #[derive(Default)]
    struct Witness {
        depth: AtomicUsize,
        max_depth: AtomicUsize,
        forbidden: AtomicUsize,
        capture_drops: AtomicUsize,
        next_calls: AtomicUsize,
        next_drops: Arc<AtomicUsize>,
    }
    impl Witness {
        fn enter(&self) -> Depth<'_> {
            let depth = self.depth.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_depth.fetch_max(depth, Ordering::SeqCst);
            Depth(self)
        }
    }
    struct Depth<'a>(&'a Witness);
    impl Drop for Depth<'_> {
        fn drop(&mut self) {
            self.0.depth.fetch_sub(1, Ordering::SeqCst);
        }
    }
    struct ReplacingDrop {
        platform: SharedPlatform,
        witness: Arc<Witness>,
        panic_on_drop: bool,
    }
    impl Drop for ReplacingDrop {
        fn drop(&mut self) {
            let _depth = self.witness.enter();
            self.witness.capture_drops.fetch_add(1, Ordering::SeqCst);
            let witness = Arc::clone(&self.witness);
            let released = DropMarker(Arc::clone(&witness.next_drops));
            self.platform.on_reopen(Box::new(move || {
                let _depth = witness.enter();
                let _ = &released;
                witness.next_calls.fetch_add(1, Ordering::SeqCst);
                println!("REOPEN_DROP_REPLACEMENT");
                quit();
            }));
            signal(false);
            nested_pump();
            if self.panic_on_drop {
                std::panic::panic_any(HostilePayload);
            }
        }
    }

    pub(super) fn run() {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::ERROR)
            .init();
        let case = std::env::args().nth(1).unwrap_or_else(|| "visible".into());
        assert!(
            matches!(
                case.as_str(),
                "visible"
                    | "empty"
                    | "starting"
                    | "nested"
                    | "replacement"
                    | "before-delivery"
                    | "quit-inside"
                    | "quit-pending"
                    | "proxy-quit-pending"
                    | "panic"
                    | "drop-reentry"
                    | "drop-panic"
                    | "stale"
                    | "os-visible"
                    | "os-empty"
            ),
            "unknown case {case}"
        );
        let calls = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let witness = Arc::new(Witness::default());
        let run_witness = Arc::clone(&witness);
        let observed = Arc::clone(&calls);
        let dropped = Arc::clone(&drops);
        let mode = case.clone();
        let retained_shared = Arc::new(std::sync::Mutex::new(None));
        let saved_shared = Arc::clone(&retained_shared);
        flui_platform::current_platform()
            .expect("platform")
            .run(Box::new(move |owner| {
                let shared = owner.shared();
                *saved_shared.lock().expect("shared") = Some(shared.clone());
                OWNER.with(|slot| *slot.borrow_mut() = Some(owner));
                shared.set_exit_policy_hook(Box::new(|| false));
                if mode != "empty" && mode != "os-empty" {
                    OWNER.with(|owner| {
                        let window = owner
                            .borrow()
                            .as_ref()
                            .expect("owner")
                            .open_window(WindowOptions::default())
                            .expect("window")
                            .try_ready()
                            .expect("ready");
                        assert!(window.is_visible());
                    });
                }
                if mode == "stale" {
                    OLD_DELEGATE.with(|slot| *slot.borrow_mut() = app().delegate());
                }
                let marker = DropMarker(dropped);
                let shared_callback = shared.clone();
                let callback_mode = mode.clone();
                let callback_witness = Arc::clone(&run_witness);
                shared.on_reopen(Box::new(move || {
                    let _depth = callback_witness.enter();
                    let _ = &marker;
                    let index = observed.fetch_add(1, Ordering::SeqCst);
                    println!("REOPEN_DELIVERED={index}");
                    match callback_mode.as_str() {
                        "nested" if index == 0 => {
                            signal(false);
                            nested_pump();
                            // Depth is checked after run, outside panic containment.
                        }
                        "replacement" if index == 0 => {
                            let observed = Arc::clone(&observed);
                            let witness = Arc::clone(&callback_witness);
                            shared_callback.on_reopen(Box::new(move || {
                                let _depth = witness.enter();
                                observed.fetch_add(1, Ordering::SeqCst);
                                println!("REOPEN_REPLACEMENT");
                                quit();
                            }));
                            signal(false);
                            nested_pump();
                        }
                        "quit-inside" => {
                            signal(false);
                            quit();
                            signal(true);
                            nested_pump();
                        }
                        "panic" if index == 0 => {
                            signal(false);
                            std::panic::panic_any(HostilePayload);
                        }
                        _ => quit(),
                    }
                }));
                if mode == "drop-reentry" || mode == "drop-panic" {
                    let capture = ReplacingDrop {
                        platform: shared.clone(),
                        witness: Arc::clone(&run_witness),
                        panic_on_drop: mode == "drop-panic",
                    };
                    let replace = shared.clone();
                    let witness = Arc::clone(&run_witness);
                    shared.on_reopen(Box::new(move || {
                        let _depth = witness.enter();
                        let _ = &capture;
                        let forbidden = Arc::clone(&witness);
                        replace.on_reopen(Box::new(move || {
                            forbidden.forbidden.fetch_add(1, Ordering::SeqCst);
                            quit();
                        }));
                    }));
                }
                if mode == "starting" {
                    signal(false);
                    return Ok(());
                }
                enqueue(move || {
                    if mode.starts_with("os-") {
                        println!("REOPEN_READY_PID={}", std::process::id());
                        std::io::stdout().flush().expect("flush ready");
                    } else if mode == "before-delivery" {
                        signal(true);
                        shared.on_reopen(Box::new(|| {
                            println!("REOPEN_LATEST_REGISTRATION");
                            quit();
                        }));
                    } else if mode == "proxy-quit-pending" {
                        signal(true);
                        OWNER.with(|slot| {
                            slot.borrow()
                                .as_ref()
                                .expect("owner")
                                .proxy()
                                .request_quit()
                                .expect("proxy quit");
                        });
                        signal(false);
                    } else if mode == "quit-pending" {
                        signal(true);
                        app().terminate(None);
                        signal(false);
                        shared.on_reopen(Box::new(move || {
                            run_witness.forbidden.fetch_add(1, Ordering::SeqCst);
                        }));
                    } else {
                        signal(mode != "empty");
                    }
                });
                Ok(())
            }))
            .expect("normal return");
        OWNER.with(|slot| {
            slot.borrow_mut().take();
        });
        assert_eq!(
            drops.load(Ordering::SeqCst),
            1,
            "run releases callback captures"
        );
        let expected = match case.as_str() {
            "nested" | "replacement" | "panic" => 2,
            "quit-pending" | "proxy-quit-pending" | "drop-reentry" | "drop-panic"
            | "before-delivery" => 0,
            _ => 1,
        };
        assert_eq!(calls.load(Ordering::SeqCst), expected);
        let shared = retained_shared
            .lock()
            .expect("shared")
            .take()
            .expect("saved");
        let late_drop = Arc::new(AtomicUsize::new(0));
        let marker = DropMarker(Arc::clone(&late_drop));
        let forbidden = Arc::clone(&witness);
        shared.on_reopen(Box::new(move || {
            let _ = &marker;
            forbidden.forbidden.fetch_add(1, Ordering::SeqCst);
        }));
        assert_eq!(
            late_drop.load(Ordering::SeqCst),
            1,
            "late registration rejected"
        );
        if case == "stale" {
            let stale_calls = Arc::new(AtomicUsize::new(0));
            let observed_stale = Arc::clone(&stale_calls);
            flui_platform::current_platform()
                .expect("second platform")
                .run(Box::new(move |owner| {
                    OWNER.with(|slot| *slot.borrow_mut() = Some(owner));
                    OWNER.with(|slot| {
                        slot.borrow()
                            .as_ref()
                            .expect("owner")
                            .shared()
                            .on_reopen(Box::new(move || {
                                observed_stale.fetch_add(1, Ordering::SeqCst);
                            }));
                    });
                    enqueue(|| {
                        OLD_DELEGATE.with(|slot| {
                            assert!(
                                !slot
                                    .borrow()
                                    .as_ref()
                                    .expect("old")
                                    .applicationShouldHandleReopen_hasVisibleWindows(&app(), false)
                            );
                        });
                        enqueue(quit);
                    });
                    Ok(())
                }))
                .expect("second return");
            assert_eq!(
                stale_calls.load(Ordering::SeqCst),
                0,
                "old delegate must not invoke new registration"
            );
            OWNER.with(|slot| {
                slot.borrow_mut().take();
            });
            OLD_DELEGATE.with(|slot| {
                slot.borrow_mut().take();
            });
            println!("REOPEN_STALE_INERT");
        }
        assert_eq!(
            witness.forbidden.load(Ordering::SeqCst),
            0,
            "forbidden callbacks never ran"
        );
        assert_eq!(
            witness.depth.load(Ordering::SeqCst),
            0,
            "all callback/cleanup guards released"
        );
        if matches!(
            case.as_str(),
            "nested" | "replacement" | "drop-reentry" | "drop-panic"
        ) {
            assert_eq!(
                witness.max_depth.load(Ordering::SeqCst),
                1,
                "callbacks must not overlap callbacks or capture cleanup"
            );
            println!("REOPEN_SERIAL_DEPTH_VERIFIED");
        }
        if case == "drop-reentry" || case == "drop-panic" {
            assert_eq!(witness.capture_drops.load(Ordering::SeqCst), 1);
            assert_eq!(
                witness.next_calls.load(Ordering::SeqCst),
                1,
                "pending signal delivered after capture cleanup"
            );
            assert_eq!(
                witness.next_drops.load(Ordering::SeqCst),
                1,
                "replacement lease released on quit"
            );
            println!("REOPEN_CAPTURE_CLEANUP_VERIFIED");
        }
        println!("REOPEN_RETURNED_PID={}", std::process::id());
        println!("REOPEN_CASE={case}");
    }
}
#[cfg(target_os = "macos")]
fn main() {
    native::run();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("AppKit reopen probe requires macOS");
}
