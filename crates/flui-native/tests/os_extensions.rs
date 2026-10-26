//! An OS's extension of `HostWindow` reaches that OS's window and no other: a
//! window of another host has no view of it, so none of the view's commands
//! can reach it, and asking does not panic (ADR-0151 §8). What a window is
//! created with on one OS (`WindowOptions::os`) is taken by that OS's host,
//! and another host opens the window without it.
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
            "a_win32_window_opens_with_its_options_and_another_host_without_them",
            a_win32_window_opens_with_its_options_and_another_host_without_them,
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

    /// Runs `check` on a window of each host other than the OS's own, opened
    /// with `options` and naming the host; fails if a host could not open one.
    fn with_foreign_windows(
        options: WindowOptions,
        check: impl Fn(&str, &dyn HostWindow) + 'static,
    ) {
        let headless = flui_native::headless_platform();
        let window = headless
            .open_window(options.clone())
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
                        .open_window(options)
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
        #[cfg(not(feature = "winit-backend"))]
        drop(options);
    }

    #[cfg(target_os = "windows")]
    fn the_windows_extension_has_no_view_of_another_hosts_window() {
        use flui_native::os::windows::WindowsWindowExt;

        with_foreign_windows(hidden(), |host, window| {
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
        assert!(win32.dpi() > 0, "a live Win32 window has a DPI");
        window.close();
    }

    /// The Win32 host opens a window with the corners and theme its options
    /// ask for, both read back from the desktop window manager; a headless or
    /// winit window given the same options opens without them, as a window
    /// the Windows extension has no view of.
    #[cfg(target_os = "windows")]
    fn a_win32_window_opens_with_its_options_and_another_host_without_them() {
        use flui_native::os::windows::WindowsWindowExt;
        use flui_platform::{Win32WindowOptions, WindowsCornerPreference, WindowsTheme};

        let square_dark = Win32WindowOptions::default()
            .with_corner_preference(WindowsCornerPreference::DoNotRound)
            .with_theme(WindowsTheme::Dark);
        with_foreign_windows(hidden().with_os(square_dark.clone()), |host, window| {
            assert!(
                window.win32().is_none(),
                "a {host} window opens without the Win32 options and has no Win32 view"
            );
        });

        let platform: Box<dyn Platform> =
            Box::new(flui_native::WindowsPlatform::new().expect("the Win32 platform"));
        // One of the two themes is not the user's, so a host that ignored the
        // options would draw that window in the user's theme instead.
        let small_light = Win32WindowOptions::default()
            .with_corner_preference(WindowsCornerPreference::RoundSmall)
            .with_theme(WindowsTheme::Light);
        for (options, corners, theme) in [
            (
                square_dark,
                WindowsCornerPreference::DoNotRound,
                WindowsTheme::Dark,
            ),
            (
                small_light,
                WindowsCornerPreference::RoundSmall,
                WindowsTheme::Light,
            ),
        ] {
            let window = platform
                .open_window(hidden().with_os(options))
                .expect("a Win32 window opens");
            let win32 = window.win32().expect("a Win32 window has its view");
            assert_eq!(win32.corner_preference(), corners);
            assert_eq!(win32.theme(), theme);
            window.close();
        }
    }

    #[cfg(target_os = "macos")]
    fn the_macos_extension_has_no_view_of_another_hosts_window() {
        use flui_native::os::macos::MacOSWindowExt;

        with_foreign_windows(hidden(), |host, window| {
            assert!(
                window.appkit().is_none(),
                "a {host} window is not an AppKit window"
            );
        });
    }

    /// The material goes behind an AppKit window's content, changes in place
    /// when set again, and comes away.
    #[cfg(target_os = "macos")]
    fn an_appkit_backdrop_goes_behind_the_content_and_comes_away() {
        use flui_native::os::macos::{MacOSBackdrop, MacOSWindowExt};

        let platform: Box<dyn Platform> =
            Box::new(flui_native::MacOSPlatform::new().expect("the AppKit platform"));
        let window = platform
            .open_window(hidden())
            .expect("an AppKit window opens");
        let appkit = window.appkit().expect("an AppKit window has its view");
        assert_eq!(appkit.set_backdrop(MacOSBackdrop::Sidebar), Ok(()));
        assert_eq!(appkit.set_backdrop(MacOSBackdrop::HudWindow), Ok(()));
        assert_eq!(appkit.clear_backdrop(), Ok(()));
        assert_eq!(appkit.clear_backdrop(), Ok(()));
        window.close();
    }
}
