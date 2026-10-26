//! The escape hatch's surface (ADR-0254 §8): each `flui::native` item through
//! its facade path, and a window of each host narrowed through it.
//!
//! The windows come from the core host, which a test may name: a headless
//! window has no view of any operating system's, and on Windows a Win32
//! window opened with its own options narrows to the view that reads them
//! back. The macOS cases build wherever the macOS target is type-checked.

/// Every item through its facade path: removing or moving one fails to build
/// this test.
#[expect(
    unused_imports,
    reason = "each import only has to resolve; nothing here is used"
)]
mod items {
    use flui::native::HostWindow as _;
    #[cfg(target_os = "macos")]
    use flui::native::macos::{
        AppKitWindow as _, MacOSBackdrop as _, MacOSBackdropError as _,
        MacOSCollectionBehavior as _, MacOSWindowExt as _, MacOSWindowLevel as _,
        TabJoinError as _,
    };
    #[cfg(windows)]
    use flui::native::windows::{Win32Window as _, WindowsBackdrop as _, WindowsWindowExt as _};
    use flui::platform::{
        OsWindowOptions as _, Win32WindowOptions as _, WindowsCornerPreference as _,
        WindowsTheme as _,
    };
}

#[cfg(any(windows, target_os = "macos"))]
mod narrowing {
    use flui::native::HostWindow;
    use flui::platform::WindowOptions;

    fn hidden() -> WindowOptions {
        WindowOptions::default()
            .with_title("flui::native")
            .with_visible(false)
    }

    /// Every extension `flui::native` publishes answers `None` for a window
    /// of the headless host, and asking does not panic.
    #[test]
    fn native_extensions_answer_none_on_a_foreign_host() {
        let window = flui_native::headless_platform()
            .open_window(hidden())
            .expect("a headless window opens");
        let window: &dyn HostWindow = window.as_ref();
        #[cfg(windows)]
        {
            use flui::native::windows::WindowsWindowExt;
            assert!(window.win32().is_none(), "a headless window is not Win32's");
        }
        #[cfg(target_os = "macos")]
        {
            use flui::native::macos::MacOSWindowExt;
            assert!(
                window.appkit().is_none(),
                "a headless window is not AppKit's"
            );
        }
    }

    /// A Win32 window opened with its own options narrows to its view through
    /// `flui::native::windows`, which reads back what the options asked for
    /// and changes it.
    #[cfg(windows)]
    #[test]
    fn a_win32_window_narrows_through_the_facade() {
        use flui::native::windows::{WindowsBackdrop, WindowsWindowExt};
        use flui::platform::{Win32WindowOptions, WindowsCornerPreference, WindowsTheme};

        let platform = flui_native::WindowsPlatform::new().expect("the Win32 platform");
        let window = flui_native::Platform::open_window(
            &platform,
            hidden().with_os(
                Win32WindowOptions::default()
                    .with_corner_preference(WindowsCornerPreference::DoNotRound)
                    .with_theme(WindowsTheme::Dark),
            ),
        )
        .expect("a Win32 window opens");
        let win32 = window.win32().expect("a Win32 window narrows to its view");
        assert!(win32.dpi() > 0, "a live Win32 window has a DPI");
        assert_eq!(
            win32.corner_preference(),
            WindowsCornerPreference::DoNotRound
        );
        assert_eq!(win32.theme(), WindowsTheme::Dark);

        win32.set_corner_preference(WindowsCornerPreference::RoundSmall);
        win32.set_theme(WindowsTheme::Light);
        win32.set_backdrop(WindowsBackdrop::MicaAlt);
        assert_eq!(
            win32.corner_preference(),
            WindowsCornerPreference::RoundSmall
        );
        assert_eq!(win32.theme(), WindowsTheme::Light);
        assert_eq!(win32.backdrop(), WindowsBackdrop::MicaAlt);
        window.close();
    }
}
