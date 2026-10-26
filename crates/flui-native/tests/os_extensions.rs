//! An OS's extension of `HostWindow` reaches that OS's window and no other: a
//! window of another host has no view of it, so none of the view's commands
//! can reach it, and asking does not panic (ADR-0151 §8).
//!
//! The foreign windows are the headless host's and, with `winit-backend`, the
//! winit host's on the same OS. Its own harness: winit builds its event loop
//! only on the process's main thread, which a libtest worker is not. The macOS
//! case builds wherever the macOS target is type-checked; it runs only on a
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
        #[cfg(target_os = "macos")]
        (
            "the_macos_extension_has_no_view_of_another_hosts_window",
            the_macos_extension_has_no_view_of_another_hosts_window,
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
        use flui_native::WindowsWindowExt;

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
        assert!(win32.dpi() > 0, "a live Win32 window has a DPI");
        window.close();
    }

    #[cfg(target_os = "macos")]
    fn the_macos_extension_has_no_view_of_another_hosts_window() {
        use flui_native::MacOSWindowExt;

        with_foreign_windows(|host, window| {
            assert!(
                window.appkit().is_none(),
                "a {host} window is not an AppKit window"
            );
        });
    }
}
