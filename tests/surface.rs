//! The escape hatch's surface (ADR-0254 §8): each `flui::native` item through
//! its facade path, and a window of each host narrowed through it.
//!
//! The windows come from the core host, which a test may name: a headless
//! window has no view of any operating system's, and on Windows a Win32
//! window opened with its own options narrows to the view that reads them
//! back. The macOS cases build wherever the macOS target is type-checked.
//! Which paths exist without the feature or off their system is pinned by the
//! compile-fail fixtures under `tests/ui/native/` (`thread_boundary_ui`).

/// Every item through its facade path: removing or moving one fails to build
/// this test.
#[expect(
    unused_imports,
    reason = "each import only has to resolve; nothing here is used"
)]
mod items {
    #[cfg(target_os = "macos")]
    use flui::native::macos::{
        AppKitWindow as _, GlassStyle as _, GlassTint as _, LiquidGlass as _, MacOSBackdrop as _,
        MacOSBackdropError as _, MacOSCollectionBehavior as _, MacOSMaterial as _,
        MacOSWindowExt as _, MacOSWindowLevel as _, TabJoinError as _,
    };
    #[cfg(windows)]
    use flui::native::windows::{
        Win32Window as _, Win32WindowError as _, WindowsBackdrop as _, WindowsWindowExt as _,
    };
    use flui::native::{HostWindow as _, with_window as _};
    use flui::platform::os_options::{
        OsWindowOptions as _, Win32WindowOptions as _, WindowsCornerPreference as _,
        WindowsTheme as _,
    };
}

/// Nothing is presented at an address no runtime issued, so `with_window`
/// answers `None` and does not run the closure.
#[test]
fn with_window_answers_none_where_nothing_is_presented() {
    let address = flui_foundation::PresentationAddress {
        ui_runtime_id: flui_foundation::UiRuntimeId::new(1),
        presentation_id: flui_foundation::PresentationId::new(1),
    };
    assert_eq!(flui::native::with_window(address, |_| ()), None);
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
        use flui::platform::os_options::{
            OsWindowOptions, Win32WindowOptions, WindowsCornerPreference, WindowsTheme,
        };

        let platform = flui_native::WindowsPlatform::new().expect("the Win32 platform");
        let window = flui_native::Platform::open_window(
            &platform,
            hidden().with_os_options(
                OsWindowOptions::default().with_windows(
                    Win32WindowOptions::default()
                        .with_corner_preference(WindowsCornerPreference::DoNotRound)
                        .with_theme(WindowsTheme::Dark),
                ),
            ),
        )
        .expect("a Win32 window opens");
        let win32 = window.win32().expect("a Win32 window narrows to its view");
        assert!(
            win32.dpi().is_ok_and(|dpi| dpi > 0),
            "a live Win32 window has a DPI"
        );
        assert_eq!(
            win32.corner_preference(),
            Ok(WindowsCornerPreference::DoNotRound)
        );
        assert_eq!(win32.theme(), Ok(WindowsTheme::Dark));

        assert_eq!(
            win32.set_corner_preference(WindowsCornerPreference::RoundSmall),
            Ok(())
        );
        assert_eq!(win32.set_theme(WindowsTheme::Light), Ok(()));
        assert_eq!(win32.set_backdrop(WindowsBackdrop::MicaAlt), Ok(()));
        assert_eq!(
            win32.corner_preference(),
            Ok(WindowsCornerPreference::RoundSmall)
        );
        assert_eq!(win32.theme(), Ok(WindowsTheme::Light));
        assert_eq!(win32.backdrop(), Ok(WindowsBackdrop::MicaAlt));
        let close = flui_platform::WindowCommand::Close
            .admit(&flui_platform::PlatformWindow::state(window.as_ref()).capabilities)
            .expect("a Win32 window takes a close");
        flui_platform::PlatformWindow::apply(window.as_ref(), close)
            .expect("a Win32 window closes");
    }
}
