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
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use flui_foundation::geometry::{Size, WindowMetrics};
    use flui_native::testing::appkit;
    use flui_native::{HostWindow, MacOSPlatform, Platform as _};
    use flui_platform::{
        Availability, PlatformWindow, WindowCommand, WindowMaterial, WindowOptions,
    };
    use flui_testing::window_commands::AdmitAndApply as _;
    use flui_testing::window_conformance::{self, Readback, WindowFixture};
    use flui_testing::window_handler::{HandlerCall, RecordingWindowHandler};
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSView, NSVisualEffectMaterial,
        NSVisualEffectView, NSWindow, NSWindowOcclusionState,
    };

    pub(super) const CASES: &[(&str, fn())] = &[
        ("the_appkit_window_conforms", the_appkit_window_conforms),
        (
            "a_window_zoomed_before_full_screen_returns_to_normal",
            a_window_zoomed_before_full_screen_returns_to_normal,
        ),
        (
            "a_redraw_from_another_thread_is_queued_without_waiting",
            a_redraw_from_another_thread_is_queued_without_waiting,
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

    /// The AppKit window behind an open FLUI window's raw handle.
    #[expect(
        unsafe_code,
        reason = "the FLUI view behind an open AppKit window's raw window handle"
    )]
    fn ns_window_of(window: &dyn HostWindow) -> Retained<NSWindow> {
        use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

        let target = window.surface_target();
        let handle = target.window_handle().expect("a live AppKit window");
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

    /// An AppKit window for the conformance checks. Dropping it closes the
    /// window and settles, so a case that fails part way leaves no window for
    /// the next one.
    struct AppKitFixture {
        app: Retained<NSApplication>,
        _platform: MacOSPlatform,
        window: Rc<dyn HostWindow>,
        ns_window: Retained<NSWindow>,
        handler: Rc<RecordingWindowHandler>,
    }

    impl WindowFixture for AppKitFixture {
        fn window(&self) -> Rc<dyn PlatformWindow> {
            Rc::clone(&self.window) as Rc<dyn PlatformWindow>
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
                material: self.material(),
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
                appkit::run_for(Duration::from_millis(50));
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

        /// The window system's own close, the title bar's close button: the
        /// checks reach it only for a window that does not take
        /// `WindowCommand::Close`. Cleanup is `Drop`'s.
        fn close(&mut self) {
            self.ns_window.performClose(None);
            self.settle();
        }

        /// The AppKit input table: an AppKit window hands the system no input
        /// region.
        fn input_regions(&self) -> Availability {
            flui_native::input::availability::macos().input_regions()
        }

        /// The checks run on the main thread, where an AppKit window's target
        /// hands out its view.
        fn has_native_surface(&self) -> bool {
            true
        }
    }

    impl Drop for AppKitFixture {
        fn drop(&mut self) {
            // A window the checks closed already is closed; any other refusal
            // is logged, so a case already unwinding keeps its first failure.
            self.window.close_for_cleanup();
            self.settle();
        }
    }

    impl AppKitFixture {
        /// The contract's material AppKit draws behind the window: an
        /// `NSVisualEffectView` with the window-background material as the
        /// content view is `System`, another material has no name here, and
        /// FLUI's view as the content view, no effect view, is `Opaque`.
        fn material(&self) -> Option<WindowMaterial> {
            let content = self.ns_window.contentView()?;
            match content.downcast::<NSVisualEffectView>() {
                Ok(effect) => (effect.material() == NSVisualEffectMaterial::WindowBackground)
                    .then_some(WindowMaterial::System),
                Err(_) => Some(WindowMaterial::Opaque),
            }
        }
    }

    /// A shown AppKit window, settled.
    fn open(title: &str) -> AppKitFixture {
        let mtm = MainThreadMarker::new().expect("the cases run on the main thread");
        let app = launched(mtm);
        let platform = MacOSPlatform::new().expect("the AppKit platform");
        let handler = RecordingWindowHandler::new();
        let window = platform
            .open_window(
                WindowOptions::default().with_title(title),
                Rc::clone(&handler) as _,
            )
            .expect("an AppKit window opens");
        let ns_window = ns_window_of(window.as_ref());
        let mut fixture = AppKitFixture {
            app,
            _platform: platform,
            window,
            ns_window,
            handler,
        };
        fixture.settle();
        fixture
    }

    fn the_appkit_window_conforms() {
        let mut fixture = open("window conformance");
        window_conformance::assert_conforms(&mut fixture);
    }

    /// A redraw asked for on another thread is queued for the main thread,
    /// not waited for: the asking thread is done while the main thread is
    /// still joined on it, and the frame arrives once AppKit runs again.
    fn a_redraw_from_another_thread_is_queued_without_waiting() {
        let fixture = open("redraw from another thread");
        let frames = |handler: &RecordingWindowHandler| {
            handler
                .take_calls()
                .iter()
                .filter(|call| matches!(call, HandlerCall::FrameRequested(_)))
                .count()
        };

        fixture.handler.take_calls();
        appkit::run_for(Duration::from_millis(500));
        assert_eq!(
            frames(&fixture.handler),
            0,
            "a settled window asks for no frame of its own"
        );

        let requester = fixture.window.redraw_requester();
        std::thread::spawn(move || requester.request_redraw())
            .join()
            .expect("the asking thread");
        assert_eq!(
            frames(&fixture.handler),
            0,
            "no frame runs before the main thread runs AppKit"
        );
        appkit::run_for(Duration::from_millis(500));
        assert!(
            frames(&fixture.handler) >= 1,
            "the redraw asked for on another thread produced no frame request"
        );
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
    }
}
