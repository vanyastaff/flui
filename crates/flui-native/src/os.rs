//! What one operating system's windows offer beyond the window contract
//! (ADR-0151 §8, ADR-0254 §8), one module per system, which the facade
//! publishes as `flui::native::<os>`.
//!
//! Each module's extension trait is implemented for [`dyn HostWindow`](crate::HostWindow):
//! its one method narrows a window to that system's view of it, and answers
//! `None` for a window of any other host (headless, winit, another system),
//! so nothing is asked of a window that cannot take it and nothing panics.
//! The views take and return FLUI types only, and each of their commands does
//! what it says or answers why not; a raw window or display handle leaves only
//! through `raw-window-handle`'s traits, which every window implements
//! (ADR-0089 §2). What a window is created with on one system is not here but
//! in the contract's `WindowOptions::os_options`
//! (`flui_platform::os_options`).

#[cfg(windows)]
pub mod windows {
    //! What a Win32 window offers beyond the window contract: the desktop
    //! window manager's backdrop, corners, title bar and theme, Snap Layouts,
    //! and the window's DPI. [`WindowsWindowExt::win32`] narrows a window to
    //! its [`Win32Window`] view, whose commands answer [`Win32WindowError`]
    //! when they did nothing.
    //!
    //! The corners and theme a Win32 window opens with are
    //! `flui_platform::os_options::Win32WindowOptions`.

    pub use crate::window::windows::backdrop::{
        Win32Window, Win32WindowError, WindowsBackdrop, WindowsWindowExt,
    };
}

#[cfg(target_os = "macos")]
pub mod macos {
    //! What an AppKit window offers beyond the window contract: tabs, native
    //! fullscreen, level, collection behavior, shadow, alpha, and what AppKit
    //! draws behind the window's content, a material or Liquid Glass.
    //! [`MacOSWindowExt::appkit`] narrows a window to its [`AppKitWindow`]
    //! view.

    pub use crate::window::apple::macos::{
        AppKitWindow, GlassStyle, GlassTint, LiquidGlass, MacOSBackdrop, MacOSBackdropError,
        MacOSCollectionBehavior, MacOSMaterial, MacOSWindowExt, MacOSWindowLevel, TabJoinError,
    };
}
