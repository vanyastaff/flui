//! Native show-mode probe. Run each case in a child with an external timeout.
#[cfg(target_os = "macos")]
mod native {
    use flui_platform::{OwnerPlatform, PlatformWindow, WindowOptions, WindowShowError};
    use objc2_app_kit::{NSWindow, NSWindowStyleMask};
    use std::{
        cell::RefCell,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
        time::Duration,
    };
    thread_local! {
        static OWNER: RefCell<Option<OwnerPlatform>> = const { RefCell::new(None) };
    }
    fn quit() {
        OWNER.with(|slot| slot.borrow().as_ref().expect("owner").quit());
    }
    #[expect(
        unsafe_code,
        reason = "native probe queries the retained AppKit window on its owner thread"
    )]
    fn native_window(window: &dyn PlatformWindow) -> &NSWindow {
        let mac = window
            .as_any()
            .downcast_ref::<flui_platform::platforms::macos::MacOSWindow>()
            .expect("native AppKit backend");
        // SAFETY: the caller holds the Rust window on main. Its native object is
        // an NSWindow retained until the Rust wrapper drops, including after close.
        unsafe { &*mac.ns_window().cast::<NSWindow>() }
    }
    fn later(failed: Arc<AtomicBool>, delay: Duration, body: impl FnOnce() + Send + 'static) {
        std::thread::spawn(move || {
            std::thread::sleep(delay);
            dispatch2::DispatchQueue::main().exec_async(move || {
                if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
                    std::mem::forget(payload);
                    failed.store(true, Ordering::SeqCst);
                    quit();
                }
            });
        });
    }
    pub(super) fn run() {
        let mode = std::env::args().nth(1).unwrap_or_else(|| "normal".into());
        assert!(matches!(
            mode.as_str(),
            "normal" | "hidden" | "minimized" | "maximized" | "maximized-minimized" | "fullscreen"
        ));
        let legacy = std::env::args().any(|arg| arg == "--legacy-restore");
        let failed = Arc::new(AtomicBool::new(false));
        let failure = Arc::clone(&failed);
        flui_platform::current_platform()
            .expect("platform")
            .run(Box::new(move |owner| {
                let window = owner.open_window(WindowOptions::default())?.try_ready()?;
                owner.shared().set_exit_policy_hook(Box::new(|| false));
                OWNER.with(|slot| slot.replace(Some(owner)));
                let failure_next = Arc::clone(&failure);
                later(failure, Duration::from_millis(300), move || {
                    match mode.as_str() {
                        "hidden" => native_window(window.as_ref()).orderOut(None),
                        "minimized" => window.minimize(),
                        "maximized" | "maximized-minimized" => window.maximize(),
                        "fullscreen" => window.toggle_fullscreen(),
                        _ => {}
                    }
                    let failure_last = Arc::clone(&failure_next);
                    later(failure_next, Duration::from_secs(2), move || {
                        if mode == "maximized-minimized" {
                            assert!(
                                native_window(window.as_ref()).isZoomed(),
                                "maximize actually observed"
                            );
                            window.minimize();
                        }
                        let failure_check = Arc::clone(&failure_last);
                        later(failure_last, Duration::from_millis(800), move || {
                            let native = native_window(window.as_ref());
                            let maximized = native.isZoomed();
                            let fullscreen =
                                native.styleMask().contains(NSWindowStyleMask::FullScreen);
                            if mode == "maximized" || mode == "maximized-minimized" {
                                assert!(maximized, "maximized mode established");
                            }
                            if mode == "fullscreen" {
                                assert!(fullscreen, "fullscreen transition observed");
                            }
                            if mode == "minimized" || mode == "maximized-minimized" {
                                assert!(native.isMiniaturized(), "minimized mode established");
                            }
                            if mode == "hidden" {
                                assert!(!native.isVisible(), "hidden state established");
                            }
                            if legacy {
                                window.restore();
                                window.activate();
                            } else {
                                window.show().expect("show");
                                window.show().expect("repeat");
                            }
                            later(failure_check, Duration::from_millis(800), move || {
                                let native = native_window(window.as_ref());
                                assert!(native.isVisible());
                                assert!(!native.isMiniaturized());
                                assert_eq!(native.isZoomed(), maximized, "show preserves zoom");
                                assert_eq!(
                                    native.styleMask().contains(NSWindowStyleMask::FullScreen),
                                    fullscreen,
                                    "show preserves fullscreen"
                                );
                                println!("SHOW_MODE_VERIFIED={mode}");
                                window.close();
                                assert_eq!(window.show(), Err(WindowShowError::Closed));
                                quit();
                            });
                        });
                    });
                });
                Ok(())
            }))
            .expect("normal native return");
        OWNER.with(|slot| slot.borrow_mut().take());
        assert!(
            !failed.load(Ordering::SeqCst),
            "native show invariant failed"
        );
        println!("SHOW_NORMAL_RETURN");
    }
}
#[cfg(target_os = "macos")]
fn main() {
    native::run();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    panic!("native show probe requires macOS");
}
