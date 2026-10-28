//! An OS's extension of `HostWindow` reaches that OS's window and no other: a
//! window of another host has no view of it, so none of the view's commands
//! can reach it, and asking does not panic (ADR-0151 §8). A view's command
//! does what it says or answers why not. What a window is created with on one
//! OS (`WindowOptions::os_options`) is taken by that OS's host.
//!
//! The foreign windows are the headless host's and, with `winit-backend`, the
//! winit host's on the same OS. Its own harness: winit builds its event loop
//! only on the process's main thread, which a libtest worker is not. The macOS
//! cases build wherever the macOS target is type-checked; they run only on a
//! Mac.

fn main() {
    #[cfg(any(target_os = "windows", target_os = "macos"))]
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

#[cfg(any(target_os = "windows", target_os = "macos"))]
mod cases {
    use flui_native::{HostWindow, Platform};
    use flui_platform::WindowOptions;
    use flui_testing::window_commands::AdmitAndApply as _;

    /// Every case, by name.
    pub(crate) const CASES: &[(&str, fn())] = &[
        #[cfg(target_os = "windows")]
        (
            "the_windows_extension_has_no_view_of_another_hosts_window",
            the_windows_extension_has_no_view_of_another_hosts_window,
        ),
        #[cfg(target_os = "windows")]
        (
            "a_win32_window_opens_with_its_options",
            a_win32_window_opens_with_its_options,
        ),
        #[cfg(target_os = "windows")]
        (
            "a_win32_view_refuses_another_thread_and_a_closed_window",
            a_win32_view_refuses_another_thread_and_a_closed_window,
        ),
        #[cfg(target_os = "macos")]
        (
            "the_macos_extension_has_no_view_of_another_hosts_window",
            the_macos_extension_has_no_view_of_another_hosts_window,
        ),
        #[cfg(target_os = "macos")]
        (
            "an_appkit_backdrop_goes_behind_the_content_and_comes_away",
            an_appkit_backdrop_goes_behind_the_content_and_comes_away,
        ),
    ];

    fn hidden() -> WindowOptions {
        WindowOptions::default()
            .with_title("os extensions")
            .with_visible(false)
    }

    /// Runs `check` on a window of each host other than the OS's own, naming
    /// the host; fails if a host could not open one.
    fn with_foreign_windows(check: impl Fn(&str, &dyn HostWindow) + 'static) {
        let headless = flui_native::headless_platform();
        let window = headless
            .open_window(
                hidden(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("a headless window opens");
        check("headless", window.as_ref());

        #[cfg(feature = "winit-backend")]
        {
            use std::cell::Cell;
            use std::rc::Rc;

            let checked = Rc::new(Cell::new(false));
            let marked = Rc::clone(&checked);
            let platform: Box<dyn Platform> = Box::new(flui_native::WinitPlatform::new());
            platform
                .run(flui_native::testing::app::on_ready(move |owner| {
                    let window = owner
                        .open_window(
                            hidden(),
                            flui_testing::window_handler::RecordingWindowHandler::new(),
                        )
                        .expect("the winit owner opens a window")
                        .try_ready()
                        .expect("a window opened on the owner is ready");
                    check("winit", window.as_ref());
                    marked.set(true);
                    window.close_now();
                    owner.quit();
                    Ok(())
                }))
                .expect("the winit loop runs");
            assert!(checked.get(), "the winit host opened no window to check");
        }
    }

    #[cfg(target_os = "windows")]
    fn the_windows_extension_has_no_view_of_another_hosts_window() {
        use flui_native::os::windows::WindowsWindowExt;

        with_foreign_windows(|host, window| {
            assert!(
                window.win32().is_none(),
                "a {host} window is not a Win32 window"
            );
        });

        let platform: Box<dyn Platform> =
            Box::new(flui_native::WindowsPlatform::new().expect("the Win32 platform"));
        let window = platform
            .open_window(
                hidden(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("a Win32 window opens");
        let win32 = window.win32().expect("a Win32 window has its view");
        assert!(
            win32.dpi().is_ok_and(|dpi| dpi > 0),
            "a live Win32 window has a DPI"
        );
        window.close_now();
    }

    /// The Win32 host opens a window with the corners and theme its options
    /// ask for, both read back from the desktop window manager. One of the two
    /// themes is not the user's, so a host that ignored the options would draw
    /// that window in the user's theme instead.
    #[cfg(target_os = "windows")]
    fn a_win32_window_opens_with_its_options() {
        use flui_native::os::windows::WindowsWindowExt;
        use flui_platform::os_options::{
            OsWindowOptions, Win32WindowOptions, WindowsCornerPreference, WindowsTheme,
        };

        let platform: Box<dyn Platform> =
            Box::new(flui_native::WindowsPlatform::new().expect("the Win32 platform"));
        for (corners, theme) in [
            (WindowsCornerPreference::DoNotRound, WindowsTheme::Dark),
            (WindowsCornerPreference::RoundSmall, WindowsTheme::Light),
        ] {
            let options = OsWindowOptions::default().with_windows(
                Win32WindowOptions::default()
                    .with_corner_preference(corners)
                    .with_theme(theme),
            );
            let window = platform
                .open_window(
                    hidden().with_os_options(options),
                    flui_testing::window_handler::RecordingWindowHandler::new(),
                )
                .expect("a Win32 window opens");
            let win32 = window.win32().expect("a Win32 window has its view");
            assert_eq!(win32.corner_preference(), Ok(corners));
            assert_eq!(win32.theme(), Ok(theme));
            window.close_now();
        }
    }

    /// A Win32 window's view answers why it did nothing: off the window's
    /// owner thread it touches nothing, and once the window is closed it
    /// reaches no handle, which the system may have given another window.
    #[cfg(target_os = "windows")]
    fn a_win32_view_refuses_another_thread_and_a_closed_window() {
        use flui_native::os::windows::{Win32WindowError, WindowsWindowExt};
        use flui_platform::os_options::WindowsCornerPreference;
        use flui_platform::{Unsupported, UnsupportedReason};

        let platform: Box<dyn Platform> =
            Box::new(flui_native::WindowsPlatform::new().expect("the Win32 platform"));
        let window = platform
            .open_window(
                hidden(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("a Win32 window opens");
        let win32 = window.win32().expect("a Win32 window has its view");
        let elsewhere = std::thread::scope(|scope| {
            scope
                .spawn(|| win32.set_corner_preference(WindowsCornerPreference::DoNotRound))
                .join()
                .expect("the other thread returns")
        });
        assert_eq!(elsewhere, Err(Win32WindowError::NotOnTheOwnerThread));
        assert_eq!(
            win32.corner_preference(),
            Ok(WindowsCornerPreference::Round),
            "the refused call changed nothing"
        );
        window.close_now();
        let closed = Err(Win32WindowError::Unsupported(Unsupported::new(
            UnsupportedReason::NoWindow,
        )));
        assert_eq!(
            win32.set_corner_preference(WindowsCornerPreference::DoNotRound),
            closed
        );
        assert_eq!(win32.dpi().map(drop), closed);
    }

    #[cfg(target_os = "macos")]
    fn the_macos_extension_has_no_view_of_another_hosts_window() {
        use flui_native::os::macos::MacOSWindowExt;

        with_foreign_windows(|host, window| {
            assert!(
                window.appkit().is_none(),
                "a {host} window is not an AppKit window"
            );
        });
    }

    /// Whether `a` and `b` are one view.
    #[cfg(target_os = "macos")]
    fn same_view(a: &objc2_app_kit::NSView, b: &objc2_app_kit::NSView) -> bool {
        std::ptr::eq(std::ptr::from_ref(a), std::ptr::from_ref(b))
    }

    /// What AppKit holds of a window, read through the FLUI view its raw
    /// handle names.
    #[cfg(target_os = "macos")]
    struct AppKitTree {
        window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
        flui_view: objc2::rc::Retained<objc2_app_kit::NSView>,
    }

    #[cfg(target_os = "macos")]
    impl AppKitTree {
        #[expect(
            unsafe_code,
            reason = "the FLUI view behind an open AppKit window's raw window handle"
        )]
        fn of(window: &dyn HostWindow) -> Self {
            use raw_window_handle::RawWindowHandle;

            let handle = window.window_handle().expect("a live AppKit window");
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                panic!("an AppKit window hands out an AppKit handle");
            };
            // SAFETY: an AppKit window's handle names its FLUI view, an
            // `NSView` the open window's view hierarchy keeps alive; retaining
            // it gives this test its own reference on the main thread the
            // cases run on.
            let flui_view = unsafe {
                objc2::rc::Retained::retain(handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>())
            }
            .expect("the handle names a view");
            let window = flui_view.window().expect("the FLUI view is in its window");
            Self { window, flui_view }
        }

        fn content_view(&self) -> objc2::rc::Retained<objc2_app_kit::NSView> {
            self.window
                .contentView()
                .expect("the window has a content view")
        }

        fn content_is(&self, class: &objc2::runtime::AnyClass) -> bool {
            use objc2::runtime::NSObjectProtocol as _;

            self.content_view().isKindOfClass(class)
        }

        /// Whether `view` is the FLUI view.
        fn is_flui_view(&self, view: &objc2_app_kit::NSView) -> bool {
            same_view(view, &self.flui_view)
        }

        #[expect(
            unsafe_code,
            reason = "`-[NSView superview]`, which objc2-app-kit marks unsafe"
        )]
        fn flui_view_superview(&self) -> Option<objc2::rc::Retained<objc2_app_kit::NSView>> {
            // SAFETY: the FLUI view is alive (this tree retains it) and in its
            // window's view hierarchy, which nothing changes while the read
            // runs on the main thread the cases run on.
            unsafe { self.flui_view.superview() }
        }

        fn flui_view_is_first_responder(&self) -> bool {
            self.window.firstResponder().is_some_and(|responder| {
                std::ptr::addr_eq(
                    objc2::rc::Retained::as_ptr(&responder),
                    objc2::rc::Retained::as_ptr(&self.flui_view),
                )
            })
        }
    }

    /// The style and content view of an `NSGlassEffectView`, which
    /// `objc2-app-kit` binds only behind a feature this crate leaves off.
    #[cfg(target_os = "macos")]
    #[expect(
        unsafe_code,
        reason = "NSGlassEffectView's properties, sent by hand on a view of that class"
    )]
    fn glass_style_and_content(
        glass: &objc2_app_kit::NSView,
    ) -> (isize, Option<objc2::rc::Retained<objc2_app_kit::NSView>>) {
        // SAFETY: the caller checked that `glass` is an `NSGlassEffectView`,
        // whose `style` is an `NSInteger` property and `contentView` an
        // `NSView` one; read on the main thread the cases run on.
        unsafe {
            (
                objc2::msg_send![glass, style],
                objc2::msg_send![glass, contentView],
            )
        }
    }

    /// A material goes behind an AppKit window's content: an
    /// `NSVisualEffectView` becomes the content view with FLUI's view inside
    /// it, still the first responder; setting another material changes that
    /// view in place; Liquid Glass replaces it with an `NSGlassEffectView`
    /// where AppKit has one and answers `NotOnThisDevice` where it has not;
    /// clearing makes FLUI's view the content view again.
    #[cfg(target_os = "macos")]
    fn an_appkit_backdrop_goes_behind_the_content_and_comes_away() {
        use flui_native::os::macos::{
            GlassStyle, LiquidGlass, MacOSBackdrop, MacOSBackdropError, MacOSMaterial,
            MacOSWindowExt,
        };
        use flui_platform::{Unsupported, UnsupportedReason};
        use objc2::runtime::AnyClass;
        use objc2_app_kit::{NSVisualEffectMaterial, NSVisualEffectView};

        let platform: Box<dyn Platform> =
            Box::new(flui_native::MacOSPlatform::new().expect("the AppKit platform"));
        let window = platform
            .open_window(
                hidden(),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("an AppKit window opens");
        let appkit = window.appkit().expect("an AppKit window has its view");
        let tree = AppKitTree::of(window.as_ref());
        assert!(tree.is_flui_view(&tree.content_view()));

        let material = |tree: &AppKitTree| {
            tree.content_view()
                .downcast::<NSVisualEffectView>()
                .expect("the content view is an NSVisualEffectView")
                .material()
        };
        assert_eq!(
            appkit.set_backdrop(MacOSBackdrop::Material(MacOSMaterial::Sidebar)),
            Ok(())
        );
        assert!(tree.content_is(
            AnyClass::get(c"NSVisualEffectView").expect("AppKit has NSVisualEffectView")
        ));
        let effect = tree.content_view();
        assert!(
            tree.flui_view_superview()
                .is_some_and(|superview| same_view(&superview, &effect)),
            "FLUI's view is the effect view's subview"
        );
        assert!(tree.flui_view_is_first_responder());
        assert_eq!(material(&tree), NSVisualEffectMaterial::Sidebar);

        assert_eq!(
            appkit.set_backdrop(MacOSBackdrop::Material(MacOSMaterial::HudWindow)),
            Ok(())
        );
        assert!(
            same_view(&tree.content_view(), &effect),
            "the effect view changes in place"
        );
        assert_eq!(material(&tree), NSVisualEffectMaterial::HUDWindow);

        let glass = MacOSBackdrop::Glass(LiquidGlass::default().with_style(GlassStyle::Clear));
        if let Some(glass_class) = AnyClass::get(c"NSGlassEffectView") {
            assert_eq!(appkit.set_backdrop(glass), Ok(()));
            assert!(tree.content_is(glass_class));
            let (style, content) = glass_style_and_content(&tree.content_view());
            assert_eq!(style, 1, "NSGlassEffectViewStyleClear");
            assert!(content.is_some_and(|content| tree.is_flui_view(&content)));
            assert!(tree.flui_view_is_first_responder());
        } else {
            assert_eq!(
                appkit.set_backdrop(glass),
                Err(MacOSBackdropError::Unsupported(Unsupported::new(
                    UnsupportedReason::NotOnThisDevice
                )))
            );
            assert!(
                same_view(&tree.content_view(), &effect),
                "a refusal changes nothing"
            );
        }
        assert_eq!(
            appkit.set_backdrop(MacOSBackdrop::Glass(
                LiquidGlass::default().with_corner_radius(f64::NAN)
            )),
            Err(MacOSBackdropError::InvalidCornerRadius)
        );

        assert_eq!(appkit.clear_backdrop(), Ok(()));
        assert!(tree.is_flui_view(&tree.content_view()));
        assert!(tree.flui_view_is_first_responder());
        assert_eq!(appkit.clear_backdrop(), Ok(()), "nothing to take away");
        window.close_now();
    }
}
