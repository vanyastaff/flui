//! The AppKit window against the window conformance checks of `flui-testing`,
//! with AppKit's own answers as the readback.
//!
//! Its own harness: AppKit windows are built on the main thread, which a
//! libtest worker is not. The cases build wherever the macOS target is
//! type-checked; they run only on a Mac.

fn main() {
    #[cfg(target_os = "macos")]
    {
        let mut arguments = libtest_mimic::Arguments::from_args();
        // One thread: the cases run on the main thread.
        arguments.test_threads = Some(1);
        let trials = cases::CASES
            .iter()
            .map(|&(name, case)| {
                libtest_mimic::Trial::test(name, move || {
                    case();
                    Ok(())
                })
            })
            .collect();
        libtest_mimic::run(&arguments, trials).exit();
    }
}

#[cfg(target_os = "macos")]
mod cases {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::{Size, WindowMetrics};
    use flui_native::{HostWindow, MacOSPlatform, Platform as _};
    use flui_platform::{PlatformWindow, WindowCommand, WindowOptions};
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_conformance::{self, Readback, WindowFixture};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSView, NSWindow, NSWindowOcclusionState,
    };

    pub(super) const CASES: &[(&str, fn())] = &[
        ("the_appkit_window_conforms", the_appkit_window_conforms),
        (
            "a_window_zoomed_before_full_screen_returns_to_normal",
            a_window_zoomed_before_full_screen_returns_to_normal,
        ),
        (
            "each_display_is_named_as_appkit_names_its_screen",
            each_display_is_named_as_appkit_names_its_screen,
        ),
    ];

    /// The snapshot names a display the way System Settings does, from
    /// `NSScreen.localizedName`, screen by screen in AppKit's order.
    fn each_display_is_named_as_appkit_names_its_screen() {
        use objc2_app_kit::NSScreen;

        let mtm = MainThreadMarker::new().expect("the cases run on the main thread");
        let platform = MacOSPlatform::new().expect("the AppKit platform");
        let snapshot = platform.display_snapshot().expect("a display snapshot");
        let names: Vec<String> = NSScreen::screens(mtm)
            .iter()
            .map(|screen| screen.localizedName().to_string())
            .collect();
        let published: Vec<&str> = snapshot
            .displays()
            .iter()
            .map(|display| display.name.as_str())
            .collect();
        assert_eq!(published, names, "the snapshot's display names");
    }

    /// The application as `MacOSPlatform::run` makes it before its loop: a
    /// regular application that has finished launching and is active, so its
    /// window can become key and enter full screen.
    fn launched(mtm: MainThreadMarker) -> Retained<NSApplication> {
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.finishLaunching();
        #[expect(deprecated, reason = "the call `MacOSPlatform::run` makes")]
        app.activateIgnoringOtherApps(true);
        app
    }

    /// Run AppKit for `span`: its events, the main queue the owner lane posts
    /// to, and the animations a mode change plays.
    #[expect(
        unsafe_code,
        reason = "`nextEventMatchingMask:untilDate:inMode:dequeue:` with an `NSDate`, which \
                  this crate's objc2-foundation features leave unbound"
    )]
    fn run_appkit(app: &NSApplication, span: Duration) {
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};
        use objc2_app_kit::{NSEvent, NSEventMask};
        use objc2_foundation::NSDefaultRunLoopMode;

        let deadline = Instant::now() + span;
        while Instant::now() < deadline {
            // SAFETY: plain class and instance messages on the main thread;
            // the date and the event are retained for the calls that use them.
            unsafe {
                let until: Retained<AnyObject> =
                    msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: 0.02_f64];
                let event: Option<Retained<NSEvent>> = msg_send![
                    app,
                    nextEventMatchingMask: NSEventMask::Any,
                    untilDate: &*until,
                    inMode: NSDefaultRunLoopMode,
                    dequeue: true
                ];
                if let Some(event) = event {
                    app.sendEvent(&event);
                }
            }
        }
    }

    /// The AppKit window behind an open FLUI window's raw handle.
    #[expect(
        unsafe_code,
        reason = "the FLUI view behind an open AppKit window's raw window handle"
    )]
    fn ns_window_of(window: &dyn HostWindow) -> Retained<NSWindow> {
        use raw_window_handle::RawWindowHandle;

        let handle = window.window_handle().expect("a live AppKit window");
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            panic!("an AppKit window hands out an AppKit handle");
        };
        // SAFETY: an AppKit window's handle names its FLUI view, which the
        // open window's view hierarchy keeps alive; retaining it gives this
        // test its own reference on the main thread the cases run on.
        let view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
            .expect("the handle names a view");
        view.window().expect("the FLUI view is in its window")
    }

    /// An AppKit window for the conformance checks.
    struct AppKitFixture {
        app: Retained<NSApplication>,
        _platform: MacOSPlatform,
        window: Arc<dyn HostWindow>,
        ns_window: Retained<NSWindow>,
    }

    impl WindowFixture for AppKitFixture {
        fn window(&self) -> Arc<dyn PlatformWindow> {
            Arc::clone(&self.window) as Arc<dyn PlatformWindow>
        }

        fn readback(&self) -> Readback {
            let ns_window = &self.ns_window;
            let miniaturized = ns_window.isMiniaturized();
            let content = ns_window.contentRectForFrameRect(ns_window.frame());
            let metrics = (!miniaturized)
                .then(|| {
                    WindowMetrics::from_logical(
                        Size::new(content.size.width, content.size.height),
                        ns_window.backingScaleFactor(),
                    )
                    .ok()
                })
                .flatten();
            let focused = ns_window.isKeyWindow();
            Readback {
                metrics,
                focused,
                active: focused && self.app.isActive(),
                visible: ns_window.isVisible()
                    && !miniaturized
                    && ns_window
                        .occlusionState()
                        .contains(NSWindowOcclusionState::Visible),
            }
        }

        fn settle(&mut self) {
            // Until the snapshot stops moving: a mode change animates for
            // most of a second, and a full-screen transition can go most of
            // one without publishing.
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut last = self.window.state().revision;
            let mut quiet_since = Instant::now();
            while Instant::now() < deadline {
                run_appkit(&self.app, Duration::from_millis(50));
                let now = self.window.state().revision;
                if now == last {
                    if quiet_since.elapsed() >= Duration::from_secs(1) {
                        return;
                    }
                } else {
                    last = now;
                    quiet_since = Instant::now();
                }
            }
        }

        fn close(&mut self) {
            self.ns_window.performClose(None);
            self.settle();
        }
    }

    /// A shown AppKit window, settled.
    fn open(title: &str) -> AppKitFixture {
        let mtm = MainThreadMarker::new().expect("the cases run on the main thread");
        let app = launched(mtm);
        let platform = MacOSPlatform::new().expect("the AppKit platform");
        let window = platform
            .open_window(WindowOptions::default().with_title(title))
            .expect("an AppKit window opens");
        let ns_window = ns_window_of(window.as_ref());
        let mut fixture = AppKitFixture {
            app,
            _platform: platform,
            window,
            ns_window,
        };
        fixture.settle();
        fixture
    }

    fn the_appkit_window_conforms() {
        let mut fixture = open("window conformance");
        window_conformance::assert_conforms(&mut fixture);
    }

    /// AppKit keeps a window's zoom under full screen and puts it back only
    /// after `windowDidExitFullScreen:`, so leaving for normal mode takes an
    /// un-zoom planned from the frame it comes back with.
    fn a_window_zoomed_before_full_screen_returns_to_normal() {
        use flui_platform::WindowMode::{Fullscreen, Maximized, Normal};

        let mut fixture = open("zoomed under full screen");
        for mode in [Maximized, Fullscreen, Normal] {
            fixture
                .window
                .admit_and_apply(WindowCommand::SetMode(mode))
                .unwrap_or_else(|refusal| panic!("SetMode({mode:?}) was refused: {refusal:?}"));
            fixture.settle();
            assert_eq!(fixture.window.state().mode, mode, "after SetMode({mode:?})");
        }
        assert!(
            !fixture.ns_window.isZoomed(),
            "AppKit kept the zoom of a window asked for normal mode"
        );
        let _ = fixture.window.admit_and_apply(WindowCommand::Close);
        fixture.settle();
    }
}
