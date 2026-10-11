//! AppKit preferences through the public winit fallback, without a user window.
//!
//! On macOS: `cargo run -p flui-platform --features winit-backend --example winit_preferences_probe`.
//! This checks native reads and owner retirement, not an external OS-setting change.

#[cfg(target_os = "macos")]
fn main() {
    use flui_platform::{Platform, PlatformError, WinitPlatform};
    use flui_platform_api::MotionPreference;
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    struct RetireCheck {
        platform: Arc<WinitPlatform>,
        retired: Arc<AtomicBool>,
    }
    impl Drop for RetireCheck {
        fn drop(&mut self) {
            self.retired
                .store(self.platform.preferences().is_err(), Ordering::SeqCst);
        }
    }

    let platform = Arc::new(WinitPlatform::new());
    let initial = Arc::clone(&platform);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&calls);
    let retired = Arc::new(AtomicBool::new(false));
    let capture = RetireCheck {
        platform: Arc::clone(&platform),
        retired: Arc::clone(&retired),
    };
    Arc::clone(&platform)
        .run_event_loop(Box::new(move |owner| {
            let observation = owner.preferences().expect("initial owner observation");
            let expected = if objc2_app_kit::NSWorkspace::sharedWorkspace()
                .accessibilityDisplayShouldReduceMotion()
            {
                MotionPreference::Reduce
            } else {
                MotionPreference::NoPreference
            };
            assert_eq!(observation.motion(), Some(expected));
            assert!(observation.gestures().double_click_interval().is_some());
            let foreign = std::thread::scope(|scope| {
                scope
                    .spawn(|| initial.preferences())
                    .join()
                    .expect("foreign read returned")
            });
            assert!(matches!(foreign, Err(PlatformError::Preferences { .. })));
            let proxy = owner.proxy();
            let next = proxy.clone();
            owner.on_wake(Box::new(move || {
                let _ = &capture;
                assert_eq!(
                    capture
                        .platform
                        .preferences()
                        .expect("owner-turn read")
                        .motion(),
                    Some(expected)
                );
                observed_calls.fetch_add(1, Ordering::SeqCst);
                next.request_quit().expect("owner quit");
            }))?;
            proxy.wake().expect("owner turn");
            Ok(())
        }))
        .expect("native owner returned");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(retired.load(Ordering::SeqCst));
    assert!(platform.preferences().is_err());
    println!("WINIT_PREFERENCES_PASS");
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("winit_preferences_probe requires macOS");
    std::process::exit(1);
}
