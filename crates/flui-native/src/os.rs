//! What one operating system's windows offer beyond the window contract
//! (ADR-0151 §8, ADR-0254 §8), one module per system, which the facade
//! publishes as `flui::native::<os>`.
//!
//! Each module's extension trait is implemented for [`dyn HostWindow`](crate::HostWindow):
//! its one method narrows a window to that system's view of it, and answers
//! `None` for a window of any other host (headless, winit, another system),
//! so nothing is asked of a window that cannot take it and nothing panics.
//! The views take and return FLUI types only; a raw window or display handle
//! leaves only through `raw-window-handle`'s traits, which every window
//! implements (ADR-0089 §2). What a window is created with on one system is
//! not here but in the contract's `WindowOptions::os`
//! (`flui_platform::OsWindowOptions`).

#[cfg(windows)]
pub mod windows {
    //! What a Win32 window offers beyond the window contract: the desktop
    //! window manager's backdrop, corners, title bar and theme, Snap Layouts,
    //! and the window's DPI. [`WindowsWindowExt::win32`] narrows a window to
    //! its [`Win32Window`] view.
    //!
    //! The corners and theme a Win32 window opens with are
    //! `flui_platform::Win32WindowOptions`, given in `WindowOptions::os`.

    pub use crate::window::windows::backdrop::{Win32Window, WindowsBackdrop, WindowsWindowExt};
}

#[cfg(target_os = "macos")]
pub mod macos {
    //! What an AppKit window offers beyond the window contract: tabs, native
    //! fullscreen, level, collection behavior, shadow, alpha and the material
    //! AppKit draws behind the window's content. [`MacOSWindowExt::appkit`]
    //! narrows a window to its [`AppKitWindow`] view.

    pub use crate::window::apple::macos::{
        AppKitWindow, MacOSBackdrop, MacOSBackdropError, MacOSCollectionBehavior, MacOSWindowExt,
        MacOSWindowLevel, TabJoinError,
    };
}
