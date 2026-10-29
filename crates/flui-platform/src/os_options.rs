//! What a window is created with on one operating system (ADR-0254 §8), in
//! [`WindowOptions::os_options`](crate::WindowOptions::os_options).
//!
//! [`OsWindowOptions`] has one optional value per operating system that has
//! options of its own, each of that system's type: Windows and Android today. A
//! cross-platform application sets every system's options it cares about,
//! without a `cfg` of its own; each host reads only its own system's value,
//! and another host (headless, winit, another system's) opens the window
//! without it. An application sees that its options were not taken where
//! that system's extension in `flui::native` has no view of the window.
//!
//! # Stability
//!
//! These types are Stable, like every item reachable from this crate's API:
//! ADR-0089 §1 makes the promise follow the item, and gives no module a tier
//! of its own. They take the least committal shape that allows: every struct
//! and enum here is `#[non_exhaustive]`, so a system's options, a field or a
//! variant arrives in a minor release; a struct is built from [`Default`] and
//! its `with_*` methods; and nothing here converts to the operating system's
//! own values, which stay private to the host that reads them.
//!
//! ```
//! use flui_platform::WindowOptions;
//! use flui_platform::os_options::{OsWindowOptions, Win32WindowOptions, WindowsCornerPreference};
//!
//! let options = WindowOptions::default().with_os_options(
//!     OsWindowOptions::default().with_windows(
//!         Win32WindowOptions::default().with_corner_preference(WindowsCornerPreference::DoNotRound),
//!     ),
//! );
//! assert!(options.os_options.windows.is_some());
//! ```

/// What a window is created with on each operating system that has options
/// of its own, one optional value per system.
///
/// `None` for a system opens the window there as it opens without options.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct OsWindowOptions {
    /// What a Win32 window is created with; the Win32 host alone reads it.
    pub windows: Option<Win32WindowOptions>,
    /// What the Android Activity's window is created with; the Android host
    /// alone reads it.
    pub android: Option<AndroidWindowOptions>,
}

impl OsWindowOptions {
    /// Set what the Android Activity's window is created with.
    #[must_use]
    pub fn with_android(mut self, android: AndroidWindowOptions) -> Self {
        self.android = Some(android);
        self
    }

    /// Set what a Win32 window is created with.
    #[must_use]
    pub fn with_windows(mut self, windows: Win32WindowOptions) -> Self {
        self.windows = Some(windows);
        self
    }
}

/// What a Win32 window is created with beyond the common options.
///
/// The defaults are what a Win32 window gets without them: rounded corners,
/// and a title bar and borders drawn in the user's app theme.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Win32WindowOptions {
    /// How the desktop window manager rounds the window's corners (Windows 11
    /// build 22000 and later; earlier builds draw square corners).
    pub corner_preference: WindowsCornerPreference,
    /// Whether the title bar and borders are drawn light, dark, or in the
    /// user's app theme and with each later change of it.
    pub theme: WindowsTheme,
}

impl Win32WindowOptions {
    /// Set how the window's corners are rounded.
    #[must_use]
    pub const fn with_corner_preference(
        mut self,
        corner_preference: WindowsCornerPreference,
    ) -> Self {
        self.corner_preference = corner_preference;
        self
    }

    /// Set the theme the title bar and borders are drawn in.
    #[must_use]
    pub const fn with_theme(mut self, theme: WindowsTheme) -> Self {
        self.theme = theme;
        self
    }
}

impl Default for Win32WindowOptions {
    fn default() -> Self {
        Self {
            corner_preference: WindowsCornerPreference::Round,
            theme: WindowsTheme::System,
        }
    }
}

/// How the Windows 11 desktop window manager rounds a window's corners
/// (`DWM_WINDOW_CORNER_PREFERENCE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowsCornerPreference {
    /// As the system decides: rounded for a window with a title bar, square
    /// for one without.
    Default,
    /// Square corners.
    DoNotRound,
    /// Rounded corners.
    Round,
    /// Corners rounded with a smaller radius.
    RoundSmall,
}

/// The theme a Win32 window's title bar and borders are drawn in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowsTheme {
    /// Light.
    Light,
    /// Dark.
    Dark,
    /// The user's app theme (Settings › Personalization › Colors), now and
    /// with each later change of it.
    System,
}

/// What an Android window is created with beyond the common options.
///
/// The default is what the Activity's window gets without them: the
/// display's default colour mode.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct AndroidWindowOptions {
    /// The colour mode the window asks the display for
    /// (`Window.setColorMode`, API 26 and later; earlier systems keep the
    /// default).
    pub color_mode: AndroidColorMode,
}

impl AndroidWindowOptions {
    /// Set the colour mode the window asks the display for.
    #[must_use]
    pub const fn with_color_mode(mut self, color_mode: AndroidColorMode) -> Self {
        self.color_mode = color_mode;
        self
    }
}

/// The colour mode an Android window asks its display for
/// (`ActivityInfo.COLOR_MODE_*`): the system composes the window in a wide
/// gamut or in HDR only when the window asks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AndroidColorMode {
    /// sRGB, the display's default.
    #[default]
    Default,
    /// The display's wide colour gamut, where it has one.
    WideColorGamut,
    /// HDR, where the display can show it.
    Hdr,
}
