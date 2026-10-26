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
            .open_window(hidden())
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
                .run(Box::new(move |owner| {
                    let window = owner
                        .open_window(hidden())
                        .expect("the winit owner opens a window")
                        .try_ready()
                        .expect("a window opened on the owner is ready");
                    check("winit", window.as_ref());
                    marked.set(true);
                    window.close();
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
            .open_window(hidden())
            .expect("a Win32 window opens");
        let win32 = window.win32().expect("a Win32 window has its view");
        assert!(
            win32.dpi().is_ok_and(|dpi| dpi > 0),
            "a live Win32 window has a DPI"
        );
        window.close();
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
                .open_window(hidden().with_os_options(options))
                .expect("a Win32 window opens");
            let win32 = window.win32().expect("a Win32 window has its view");
            assert_eq!(win32.corner_preference(), Ok(corners));
            assert_eq!(win32.theme(), Ok(theme));
            window.close();
        }
    }

    /// A Win32 window's view answers why it did nothing: off the window's
    /// owner thread it touches nothing, and once the window is closed it
    /// reaches no handle, which the system may have given another window.
    #[cfg(target_os = "windows")]
    fn a_win32_view_refuses_another_thread_and_a_closed_window() {
        use flui_native::os::windows::{Win32WindowError, WindowsWindowExt};
        use flui_platform::os_options::WindowsCornerPreference;

        let platform: Box<dyn Platform> =
            Box::new(flui_native::WindowsPlatform::new().expect("the Win32 platform"));
        let window = platform
            .open_window(hidden())
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
        window.close();
        assert_eq!(
            win32.set_corner_preference(WindowsCornerPreference::DoNotRound),
            Err(Win32WindowError::Closed)
        );
        assert_eq!(win32.dpi(), Err(Win32WindowError::Closed));
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

    /// What AppKit holds of a window, read through its content view's
    /// handle: the window's content view, and FLUI's view.
    #[cfg(target_os = "macos")]
    struct AppKitTree {
        ns_window: *mut objc2::runtime::AnyObject,
        flui_view: *mut objc2::runtime::AnyObject,
    }

    #[cfg(target_os = "macos")]
    impl AppKitTree {
        fn of(window: &dyn HostWindow) -> Self {
            use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};

            let handle = window.window_handle().expect("a live AppKit window");
            let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
                panic!("an AppKit window hands out an AppKit handle");
            };
            let flui_view = handle.ns_view.as_ptr().cast::<objc2::runtime::AnyObject>();
            // SAFETY: the handle names the live FLUI view of an open window,
            // on the main thread these cases run on.
            let ns_window = unsafe { objc2::msg_send![flui_view, window] };
            Self {
                ns_window,
                flui_view,
            }
        }

        fn content_view(&self) -> *mut objc2::runtime::AnyObject {
            // SAFETY: a live NSWindow, on the main thread.
            unsafe { objc2::msg_send![self.ns_window, contentView] }
        }

        fn content_is(&self, class: &objc2::runtime::AnyClass) -> bool {
            let content = self.content_view();
            // SAFETY: a live NSView, on the main thread.
            let kind: objc2::runtime::Bool =
                unsafe { objc2::msg_send![content, isKindOfClass: class] };
            kind.as_bool()
        }

        fn flui_view_superview(&self) -> *mut objc2::runtime::AnyObject {
            // SAFETY: the live FLUI view, on the main thread.
            unsafe { objc2::msg_send![self.flui_view, superview] }
        }

        fn first_responder(&self) -> *mut objc2::runtime::AnyObject {
            // SAFETY: a live NSWindow, on the main thread.
            unsafe { objc2::msg_send![self.ns_window, firstResponder] }
        }

        fn flui_view_is_first_responder(&self) -> bool {
            self.first_responder() == self.flui_view
        }
    }

    /// A material goes behind an AppKit window's content: an
    /// `NSVisualEffectView` becomes the content view with FLUI's view inside
    /// it, still the first responder; setting another material changes that
    /// view in place; Liquid Glass replaces it with an `NSGlassEffectView`
    /// where AppKit has one and answers `NotOnThisSystem` where it has not;
    /// clearing makes FLUI's view the content view again.
    #[cfg(target_os = "macos")]
    fn an_appkit_backdrop_goes_behind_the_content_and_comes_away() {
        use flui_native::os::macos::{
            GlassStyle, LiquidGlass, MacOSBackdrop, MacOSBackdropError, MacOSMaterial,
            MacOSWindowExt,
        };
        use objc2::runtime::AnyClass;

        let platform: Box<dyn Platform> =
            Box::new(flui_native::MacOSPlatform::new().expect("the AppKit platform"));
        let window = platform
            .open_window(hidden())
            .expect("an AppKit window opens");
        let appkit = window.appkit().expect("an AppKit window has its view");
        let tree = AppKitTree::of(window.as_ref());
        assert_eq!(tree.content_view(), tree.flui_view);

        let effect_class = AnyClass::get(c"NSVisualEffectView").expect("AppKit's effect view");
        assert_eq!(
            appkit.set_backdrop(MacOSBackdrop::Material(MacOSMaterial::Sidebar)),
            Ok(())
        );
        assert!(tree.content_is(effect_class));
        let effect = tree.content_view();
        assert_eq!(tree.flui_view_superview(), effect);
        assert!(tree.flui_view_is_first_responder());
        // SAFETY: a live NSVisualEffectView, on the main thread.
        let material: isize = unsafe { objc2::msg_send![effect, material] };
        assert_eq!(material, 7, "NSVisualEffectMaterialSidebar");

        assert_eq!(
            appkit.set_backdrop(MacOSBackdrop::Material(MacOSMaterial::HudWindow)),
            Ok(())
        );
        assert_eq!(
            tree.content_view(),
            effect,
            "the effect view changes in place"
        );
        // SAFETY: as above.
        let material: isize = unsafe { objc2::msg_send![effect, material] };
        assert_eq!(material, 13, "NSVisualEffectMaterialHUDWindow");

        let glass = MacOSBackdrop::Glass(LiquidGlass::default().with_style(GlassStyle::Clear));
        match AnyClass::get(c"NSGlassEffectView") {
            Some(glass_class) => {
                assert_eq!(appkit.set_backdrop(glass), Ok(()));
                assert!(tree.content_is(glass_class));
                let glass_view = tree.content_view();
                // SAFETY: a live NSGlassEffectView, on the main thread.
                let (style, content): (isize, *mut objc2::runtime::AnyObject) = unsafe {
                    (
                        objc2::msg_send![glass_view, style],
                        objc2::msg_send![glass_view, contentView],
                    )
                };
                assert_eq!(style, 1, "NSGlassEffectViewStyleClear");
                assert_eq!(content, tree.flui_view);
                assert!(tree.flui_view_is_first_responder());
            }
            None => {
                assert_eq!(
                    appkit.set_backdrop(glass),
                    Err(MacOSBackdropError::NotOnThisSystem)
                );
                assert_eq!(tree.content_view(), effect, "a refusal changes nothing");
            }
        }
        assert_eq!(
            appkit.set_backdrop(MacOSBackdrop::Glass(
                LiquidGlass::default().with_corner_radius(f64::NAN)
            )),
            Err(MacOSBackdropError::InvalidCornerRadius)
        );

        assert_eq!(appkit.clear_backdrop(), Ok(()));
        assert_eq!(tree.content_view(), tree.flui_view);
        assert!(tree.flui_view_is_first_responder());
        assert_eq!(appkit.clear_backdrop(), Ok(()), "nothing to take away");
        window.close();
    }
}
