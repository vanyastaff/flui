//! What the desktop window manager draws around and behind a Win32 window:
//! the system backdrop material, the corners, the title bar's colours and
//! theme, and the light or dark chrome that follows the user's app theme.
//!
//! [`WindowsWindowExt`] is how a Windows application reaches these on any
//! window a host hands out (`flui_native::os::windows`):
//! [`WindowsWindowExt::win32`] is `None` for a window of another host, and
//! every command of the [`Win32Window`] it gives otherwise does what it says
//! or answers why not ([`Win32WindowError`]). [`system_appearance`] and
//! [`apply_chrome`] are what the window procedure applies when the theme
//! changes; [`apply_corners`] and [`opening_chrome`] are what a window opens
//! with (`flui_platform::os_options::Win32WindowOptions`).
//!
//! Each desktop window manager attribute is an [`Attribute`] constant that
//! pairs its ordinal with the type of its value, and each mapping between a
//! FLUI value and the system's is one table read both ways.

use std::marker::PhantomData;

use flui_foundation::geometry::Point;
use flui_platform::os_options::{Win32WindowOptions, WindowsCornerPreference, WindowsTheme};
use flui_platform::{Brightness, Unsupported, UnsupportedReason, WindowAppearance, WindowMaterial};
use windows::Win32::Foundation::{FALSE, HWND, TRUE};
use windows::Win32::Graphics::Dwm::{
    DWM_BB_ENABLE, DWM_BLURBEHIND, DWM_SYSTEMBACKDROP_TYPE, DWM_WINDOW_CORNER_PREFERENCE,
    DWMSBT_AUTO, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TABBEDWINDOW, DWMSBT_TRANSIENTWINDOW,
    DWMWA_CAPTION_COLOR, DWMWA_COLOR_DEFAULT, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DEFAULT,
    DWMWCP_DONOTROUND, DWMWCP_ROUND, DWMWCP_ROUNDSMALL, DWMWINDOWATTRIBUTE,
    DwmEnableBlurBehindWindow, DwmGetWindowAttribute, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::HRGN;
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetWindowLongPtrW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SetWindowLongPtrW, SetWindowPos, WS_MAXIMIZEBOX,
};
use windows::core::w;

use super::WindowsWindow;
use super::material::SystemBackdrop;
use crate::host_window::HostWindow;
use crate::owner::hwnd_affinity::UserDataRefusal;
use crate::version::gate::Since;

// ============================================================================
// The extension and its view
// ============================================================================

/// Windows-specific window extensions.
///
/// Implemented for every [`HostWindow`] (ADR-0151 §8). A window of another
/// host (headless, winit) is not a Win32 window: [`win32`](Self::win32) answers
/// `None` for it, and nothing is asked of it.
///
/// ```
/// use flui_native::os::windows::{WindowsBackdrop, WindowsWindowExt};
/// use flui_native::{HostWindow, headless_platform};
/// use flui_platform::WindowOptions;
/// use flui_testing::window_handler::RecordingWindowHandler;
///
/// /// Mica behind the window, if it is a Win32 window that took it.
/// fn mica(window: &dyn HostWindow) -> bool {
///     window
///         .win32()
///         .is_some_and(|win32| win32.set_backdrop(WindowsBackdrop::Mica).is_ok())
/// }
///
/// // A headless window is not a Win32 window: nothing is asked of it.
/// let window = headless_platform().open_window(
///     WindowOptions::default().with_visible(false),
///     RecordingWindowHandler::new(),
/// )?;
/// assert!(!mica(window.as_ref()));
/// # Ok::<(), flui_native::OpenWindowError>(())
/// ```
pub trait WindowsWindowExt {
    /// This window's Win32 extras, or `None` for a window of another host.
    #[must_use]
    fn win32(&self) -> Option<Win32Window<'_>>;
}

impl WindowsWindowExt for dyn HostWindow + '_ {
    fn win32(&self) -> Option<Win32Window<'_>> {
        self.as_any().downcast_ref().map(Win32Window)
    }
}

/// What a Win32 window offers beyond the window contract: the desktop window
/// manager's backdrop, corners, title bar and theme, Snap Layouts, and the
/// window's DPI. [`WindowsWindowExt::win32`] gives one only for a Win32
/// window, so none of these can meet another host's window.
///
/// A Win32 window's attributes are changed and read on its owner thread, the
/// thread that created it; every command and query does what it says or
/// answers why not ([`Win32WindowError`]), and none reaches a handle the
/// window no longer owns.
#[derive(Debug, Clone, Copy)]
pub struct Win32Window<'a>(&'a WindowsWindow);

/// Why a [`Win32Window`] command or query did not do what it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Win32WindowError {
    /// Not here, in the contract's vocabulary: the window is closed
    /// ([`UnsupportedReason::NoWindow`]), or the value is one of the contract
    /// newer than this host ([`UnsupportedReason::NotYetOnThisHost`]).
    #[error(transparent)]
    Unsupported(#[from] Unsupported),
    /// The call came from another thread than the window's owner thread, the
    /// one that created it.
    #[error("the call is not on the window's owner thread")]
    NotOnTheOwnerThread,
    /// The system refused the request: this Windows build does not know it
    /// (the corners need Windows 11 build 22000, a system backdrop build
    /// 22621), or it refused the value.
    #[error("the system refused `{request}` (HRESULT {hresult:#010x})")]
    Refused {
        /// The system call or attribute that was refused.
        request: &'static str,
        /// The `HRESULT` it failed with.
        hresult: i32,
    },
    /// The system reported a value this host has no FLUI value for.
    #[error("the system reported {value} for `{request}`, which this host does not know")]
    Unrecognized {
        /// The attribute that was read.
        request: &'static str,
        /// The value it held.
        value: i32,
    },
}

impl Win32WindowError {
    /// The window is closed.
    const CLOSED: Self = Self::Unsupported(Unsupported::new(UnsupportedReason::NoWindow));
    /// A value of the contract newer than this host.
    const NOT_YET_ON_THIS_HOST: Self =
        Self::Unsupported(Unsupported::new(UnsupportedReason::NotYetOnThisHost));
}

// ============================================================================
// Backdrop material
// ============================================================================

/// The material the desktop window manager draws behind a window
/// (`DWM_SYSTEMBACKDROP_TYPE`, Windows 11 build 22621 and later).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum WindowsBackdrop {
    /// As the desktop window manager decides.
    Auto,
    /// None: an opaque window.
    None,
    /// Mica, the long-lived window's material: a tint of the desktop
    /// wallpaper.
    Mica,
    /// Mica Alt, Mica with a stronger tint, behind a window with tabs in its
    /// title bar.
    MicaAlt,
    /// Acrylic, a transient surface's material: a blur of what is behind the
    /// window.
    Acrylic,
}

/// Every backdrop and the system's value for it, read both ways.
const BACKDROPS: [(WindowsBackdrop, DWM_SYSTEMBACKDROP_TYPE); 5] = [
    (WindowsBackdrop::Auto, DWMSBT_AUTO),
    (WindowsBackdrop::None, DWMSBT_NONE),
    (WindowsBackdrop::Mica, DWMSBT_MAINWINDOW),
    (WindowsBackdrop::MicaAlt, DWMSBT_TABBEDWINDOW),
    (WindowsBackdrop::Acrylic, DWMSBT_TRANSIENTWINDOW),
];

/// Every corner preference and the system's value for it, read both ways.
const CORNERS: [(WindowsCornerPreference, DWM_WINDOW_CORNER_PREFERENCE); 4] = [
    (WindowsCornerPreference::Default, DWMWCP_DEFAULT),
    (WindowsCornerPreference::DoNotRound, DWMWCP_DONOTROUND),
    (WindowsCornerPreference::Round, DWMWCP_ROUND),
    (WindowsCornerPreference::RoundSmall, DWMWCP_ROUNDSMALL),
];

/// The system's value for `value`, from its table.
fn to_system<F: PartialEq + Copy, S: Copy>(table: &[(F, S)], value: F) -> Option<S> {
    table
        .iter()
        .find(|(flui, _)| *flui == value)
        .map(|(_, system)| *system)
}

/// The FLUI value for the system's `value`, from the same table.
fn from_system<F: Copy, S: PartialEq + Copy>(table: &[(F, S)], value: S) -> Option<F> {
    table
        .iter()
        .find(|(_, system)| *system == value)
        .map(|(flui, _)| *flui)
}

/// The backdrop each material of the contract is: none for an opaque
/// window, Mica for the system's.
const fn system_backdrop(material: WindowMaterial) -> DWM_SYSTEMBACKDROP_TYPE {
    match material {
        WindowMaterial::Opaque => DWMSBT_NONE,
        WindowMaterial::System => DWMSBT_MAINWINDOW,
    }
}

/// Write `hwnd`'s system backdrop: the one write of
/// `DWMWA_SYSTEMBACKDROP_TYPE`, on a build the gate let through.
fn set_system_backdrop(
    hwnd: HWND,
    _gate: Since<SystemBackdrop>,
    value: DWM_SYSTEMBACKDROP_TYPE,
) -> Result<(), Win32WindowError> {
    set(hwnd, SYSTEM_BACKDROP, value)
}

/// Draw `material` behind `hwnd`, on a build the gate let through; the
/// window keeps the backdrop it had when the desktop window manager refuses.
pub(super) fn apply_material(
    hwnd: HWND,
    gate: Since<SystemBackdrop>,
    material: WindowMaterial,
) -> Result<(), flui_platform::ApplyError> {
    material_applied(set_system_backdrop(hwnd, gate, system_backdrop(material)))
}

/// What the window answers for the desktop window manager's `outcome`: its
/// refusal is a failure of the system, [`ApplyError::Platform`], not a
/// capability the window lacks — admission already held the capability
/// (ADR-0254 §3).
///
/// [`ApplyError::Platform`]: flui_platform::ApplyError::Platform
fn material_applied(
    outcome: Result<(), Win32WindowError>,
) -> Result<(), flui_platform::ApplyError> {
    outcome.map_err(|refusal| flui_platform::ApplyError::failed(refusal.to_string()))
}

// ============================================================================
// The desktop window manager's attributes
// ============================================================================

/// One desktop window manager attribute and the type of its value.
#[derive(Clone, Copy)]
struct Attribute<T> {
    id: DWMWINDOWATTRIBUTE,
    name: &'static str,
    value: PhantomData<fn() -> T>,
}

impl<T> Attribute<T> {
    /// # Safety
    ///
    /// `T` is the layout the desktop window manager reads and writes for
    /// `id`: [`set`] and [`get`] pass `size_of::<T>()` as the value's size.
    const unsafe fn new(id: DWMWINDOWATTRIBUTE, name: &'static str) -> Self {
        Self {
            id,
            name,
            value: PhantomData,
        }
    }
}

// SAFETY: `DWMWA_WINDOW_CORNER_PREFERENCE` holds a `DWM_WINDOW_CORNER_PREFERENCE`
// (a 4-byte enum, dwmapi.h), the type it is paired with.
const CORNER_PREFERENCE: Attribute<DWM_WINDOW_CORNER_PREFERENCE> = unsafe {
    Attribute::new(
        DWMWA_WINDOW_CORNER_PREFERENCE,
        "DWMWA_WINDOW_CORNER_PREFERENCE",
    )
};
// SAFETY: `DWMWA_SYSTEMBACKDROP_TYPE` holds a `DWM_SYSTEMBACKDROP_TYPE` (a
// 4-byte enum), the type it is paired with.
const SYSTEM_BACKDROP: Attribute<DWM_SYSTEMBACKDROP_TYPE> =
    unsafe { Attribute::new(DWMWA_SYSTEMBACKDROP_TYPE, "DWMWA_SYSTEMBACKDROP_TYPE") };
// SAFETY: `DWMWA_USE_IMMERSIVE_DARK_MODE` holds a 4-byte `BOOL`, an `i32`.
const DARK_MODE: Attribute<i32> = unsafe {
    Attribute::new(
        DWMWA_USE_IMMERSIVE_DARK_MODE,
        "DWMWA_USE_IMMERSIVE_DARK_MODE",
    )
};
// SAFETY: `DWMWA_CAPTION_COLOR` holds a 4-byte `COLORREF`, a `u32`.
const CAPTION_COLOR: Attribute<u32> =
    unsafe { Attribute::new(DWMWA_CAPTION_COLOR, "DWMWA_CAPTION_COLOR") };

/// Set `hwnd`'s `attribute` to `value`.
fn set<T>(hwnd: HWND, attribute: Attribute<T>, value: T) -> Result<(), Win32WindowError> {
    // SAFETY: `T` is the layout of `attribute`'s value (the `# Safety` of
    // `Attribute::new`, discharged where each constant is declared); the
    // pointer addresses the stack-local `value` for the call, and the size is
    // its width. A handle that names no window only makes the call fail.
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute.id,
            (&raw const value).cast(),
            size_of::<T>() as u32,
        )
    }
    .map_err(|error| refused(attribute.name, &error))
}

/// `hwnd`'s `attribute`.
fn get<T: Default>(hwnd: HWND, attribute: Attribute<T>) -> Result<T, Win32WindowError> {
    let mut value = T::default();
    // SAFETY: as in `set`: `T` is the layout of `attribute`'s value, so the
    // system writes at most `size_of::<T>()` bytes into the initialized
    // stack-local `value`, which is read only when the call succeeded.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            attribute.id,
            (&raw mut value).cast(),
            size_of::<T>() as u32,
        )
    }
    .map_err(|error| refused(attribute.name, &error))?;
    Ok(value)
}

fn refused(request: &'static str, error: &windows::core::Error) -> Win32WindowError {
    Win32WindowError::Refused {
        request,
        hresult: error.code().0,
    }
}

// ============================================================================
// Corners and theme
// ============================================================================

/// Ask the desktop window manager to round `hwnd`'s corners as `preference`
/// says. Builds before Windows 11 do not know the attribute and keep square
/// corners: they answer [`Win32WindowError::Refused`].
pub(super) fn apply_corners(
    hwnd: HWND,
    preference: WindowsCornerPreference,
) -> Result<(), Win32WindowError> {
    let value = to_system(&CORNERS, preference).ok_or(Win32WindowError::NOT_YET_ON_THIS_HOST)?;
    set(hwnd, CORNER_PREFERENCE, value)
}

/// The appearance `theme` draws the chrome in, `None` for the user's app
/// theme, which the window then follows; `Err` for a theme newer than this
/// host.
fn chosen_appearance(theme: WindowsTheme) -> Result<Option<WindowAppearance>, Win32WindowError> {
    match theme {
        WindowsTheme::Light => Ok(Some(WindowAppearance::Light)),
        WindowsTheme::Dark => Ok(Some(WindowAppearance::Dark)),
        WindowsTheme::System => Ok(None),
        _ => Err(Win32WindowError::NOT_YET_ON_THIS_HOST),
    }
}

/// The chrome a window opened with `options` starts with, given the user's
/// app theme `system`: the appearance it draws and whether it follows `system`
/// from then on. A theme newer than this host opens as the user's.
pub(super) fn opening_chrome(
    options: &Win32WindowOptions,
    system: WindowAppearance,
) -> (WindowAppearance, bool) {
    match chosen_appearance(options.theme) {
        Ok(Some(appearance)) => (appearance, false),
        Ok(None) | Err(_) => (system, true),
    }
}

/// The app theme chosen in Settings › Personalization › Colors.
///
/// Windows exposes it only as the `AppsUseLightTheme` registry value; the
/// window's own DWM dark-mode flag is chrome this backend sets, not an
/// observation. A missing value (before Windows 10 1809, or a profile that
/// never chose) means light.
pub(super) fn system_appearance() -> WindowAppearance {
    let mut light: u32 = 1;
    let mut size = size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated literals from `w!`.
    // `RRF_RT_REG_DWORD` admits only a 4-byte value, `size` states the 4-byte
    // capacity of the stack-local `light` the data pointer addresses, and
    // `light` is read only when the call succeeded.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut light).cast()),
            Some(&raw mut size),
        )
    };
    if status.is_ok() && light == 0 {
        WindowAppearance::Dark
    } else {
        WindowAppearance::Light
    }
}

/// Draw `hwnd`'s title bar and borders dark exactly when `appearance` is.
/// A build before Windows 10 20H1 refuses the attribute.
pub(super) fn apply_chrome(
    hwnd: HWND,
    appearance: WindowAppearance,
) -> Result<(), Win32WindowError> {
    set(
        hwnd,
        DARK_MODE,
        i32::from(Brightness::from(appearance).is_dark()),
    )
}

// ============================================================================
// What a Win32 window offers
// ============================================================================

impl Win32Window<'_> {
    /// The window's handle, while this view's window owns it and the call is
    /// on its owner thread.
    fn live(self) -> Result<HWND, Win32WindowError> {
        let window = self.0;
        match super::proc::with_window_context_checked(window.hwnd(), "Win32Window", |context| {
            context.identity == window.identity
        }) {
            Ok(true) => Ok(window.hwnd()),
            Err(UserDataRefusal::ForeignThread) => Err(Win32WindowError::NotOnTheOwnerThread),
            Ok(false)
            | Err(
                UserDataRefusal::WindowGone
                | UserDataRefusal::ForeignClass
                | UserDataRefusal::EmptySlot,
            ) => Err(Win32WindowError::CLOSED),
        }
    }

    /// Set the material behind the window, through the same build gate as
    /// the contract's `WindowCommand::SetMaterial`.
    ///
    /// # Errors
    /// [`Win32WindowError`]: the window is closed, the call is off its owner
    /// thread, the build predates the system backdrop (Windows 11 build
    /// 22621: `Unsupported` with `NotOnThisDevice`), or the system refused.
    pub fn set_backdrop(self, backdrop: WindowsBackdrop) -> Result<(), Win32WindowError> {
        let hwnd = self.live()?;
        let gate = self.0.backdrop_gate()?;
        let value =
            to_system(&BACKDROPS, backdrop).ok_or(Win32WindowError::NOT_YET_ON_THIS_HOST)?;
        set_system_backdrop(hwnd, gate, value)
    }

    /// The material behind the window.
    ///
    /// # Errors
    /// As [`set_backdrop`](Self::set_backdrop), and
    /// [`Win32WindowError::Unrecognized`] for a material this host does not
    /// know.
    pub fn backdrop(self) -> Result<WindowsBackdrop, Win32WindowError> {
        let hwnd = self.live()?;
        self.0.backdrop_gate()?;
        let value = get(hwnd, SYSTEM_BACKDROP)?;
        from_system(&BACKDROPS, value).ok_or(Win32WindowError::Unrecognized {
            request: SYSTEM_BACKDROP.name,
            value: value.0,
        })
    }

    /// Show or hide the layouts Windows 11 offers over the maximize button
    /// (Snap Layouts), which it shows for a window with one.
    ///
    /// # Errors
    /// [`Win32WindowError`]: the window is closed, the call is off its owner
    /// thread, or the system refused to redraw the frame.
    pub fn set_snap_layouts(self, enabled: bool) -> Result<(), Win32WindowError> {
        let hwnd = self.live()?;
        // SAFETY: plain style reads and writes on the live handle `live`
        // checked, on its owner thread; `SetWindowPos` with no geometry
        // flags only redraws the frame for the new style.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
            let style = if enabled {
                style | WS_MAXIMIZEBOX.0
            } else {
                style & !WS_MAXIMIZEBOX.0
            };
            SetWindowLongPtrW(hwnd, GWL_STYLE, style as isize);
            SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        }
        .map_err(|error| refused("SetWindowPos", &error))
    }

    /// Whether Windows 11 offers Snap Layouts over the window's maximize
    /// button.
    ///
    /// # Errors
    /// The window is closed, or the call is off its owner thread.
    pub fn snap_layouts(self) -> Result<bool, Win32WindowError> {
        let hwnd = self.live()?;
        // SAFETY: a plain style read on the live handle `live` checked.
        let style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) } as u32;
        Ok(style & WS_MAXIMIZEBOX.0 != 0)
    }

    /// Set how the desktop window manager rounds the window's corners.
    ///
    /// # Errors
    /// [`Win32WindowError`]: the window is closed, the call is off its owner
    /// thread, or the system refused (Windows 10 has no rounded corners).
    pub fn set_corner_preference(
        self,
        preference: WindowsCornerPreference,
    ) -> Result<(), Win32WindowError> {
        apply_corners(self.live()?, preference)
    }

    /// How the desktop window manager rounds the window's corners.
    ///
    /// # Errors
    /// As [`set_corner_preference`](Self::set_corner_preference).
    pub fn corner_preference(self) -> Result<WindowsCornerPreference, Win32WindowError> {
        let value = get(self.live()?, CORNER_PREFERENCE)?;
        from_system(&CORNERS, value).ok_or(Win32WindowError::Unrecognized {
            request: CORNER_PREFERENCE.name,
            value: value.0,
        })
    }

    /// Blur what is behind the window's transparent areas, or stop.
    ///
    /// # Errors
    /// [`Win32WindowError`]: the window is closed, the call is off its owner
    /// thread, or the system refused.
    pub fn set_blur_behind(self, enabled: bool) -> Result<(), Win32WindowError> {
        let hwnd = self.live()?;
        let blur = DWM_BLURBEHIND {
            dwFlags: DWM_BB_ENABLE,
            fEnable: if enabled { TRUE } else { FALSE },
            hRgnBlur: HRGN::default(),
            fTransitionOnMaximized: FALSE,
        };
        // SAFETY: the call reads the stack-local, fully initialized `blur`
        // through the typed pointer for its duration.
        unsafe { DwmEnableBlurBehindWindow(hwnd, &raw const blur) }
            .map_err(|error| refused("DwmEnableBlurBehindWindow", &error))
    }

    /// Draw the title bar and borders dark or light, whatever the user's app
    /// theme does later.
    ///
    /// # Errors
    /// [`Win32WindowError`]: the window is closed, the call is off its owner
    /// thread, or the system refused (before Windows 10 20H1).
    pub fn set_dark_mode(self, dark: bool) -> Result<(), Win32WindowError> {
        let hwnd = self.live()?;
        set(hwnd, DARK_MODE, i32::from(dark))?;
        // An explicit choice outlives later system theme changes.
        super::proc::with_window_context(hwnd, "set_dark_mode", |context| {
            context.chrome_follows_system.set(false);
        });
        Ok(())
    }

    /// Whether the title bar and borders are drawn dark.
    ///
    /// # Errors
    /// As [`set_dark_mode`](Self::set_dark_mode).
    pub fn dark_mode(self) -> Result<bool, Win32WindowError> {
        Ok(get(self.live()?, DARK_MODE)? != 0)
    }

    /// Set the theme of the title bar and borders; [`WindowsTheme::System`]
    /// follows the user's app theme now and with each later change of it.
    ///
    /// # Errors
    /// As [`set_dark_mode`](Self::set_dark_mode).
    pub fn set_theme(self, theme: WindowsTheme) -> Result<(), Win32WindowError> {
        if let Some(appearance) = chosen_appearance(theme)? {
            return self.set_dark_mode(Brightness::from(appearance).is_dark());
        }
        let hwnd = self.live()?;
        apply_chrome(hwnd, system_appearance())?;
        super::proc::with_window_context(hwnd, "set_theme", |context| {
            context.chrome_follows_system.set(true);
        });
        Ok(())
    }

    /// The theme the title bar and borders are drawn in now, light or dark.
    ///
    /// # Errors
    /// As [`set_dark_mode`](Self::set_dark_mode).
    pub fn theme(self) -> Result<WindowsTheme, Win32WindowError> {
        Ok(if self.dark_mode()? {
            WindowsTheme::Dark
        } else {
            WindowsTheme::Light
        })
    }

    /// Set the title bar's colour, or `None` for the system's (Windows 11).
    ///
    /// # Errors
    /// [`Win32WindowError`]: the window is closed, the call is off its owner
    /// thread, or the system refused (Windows 10 has no caption colour).
    pub fn set_title_bar_color(self, color: Option<(u8, u8, u8)>) -> Result<(), Win32WindowError> {
        let hwnd = self.live()?;
        // A `COLORREF` is 0x00BBGGRR.
        let value = color.map_or(DWMWA_COLOR_DEFAULT, |(red, green, blue)| {
            (u32::from(blue) << 16) | (u32::from(green) << 8) | u32::from(red)
        });
        set(hwnd, CAPTION_COLOR, value)
    }

    /// The window's DPI.
    ///
    /// # Errors
    /// The window is closed, or the call is off its owner thread.
    pub fn dpi(self) -> Result<u32, Win32WindowError> {
        let hwnd = self.live()?;
        // SAFETY: takes the live handle by value; `0` means it names no
        // window, which `live` ruled out unless it closed in between.
        match unsafe { GetDpiForWindow(hwnd) } {
            0 => Err(Win32WindowError::CLOSED),
            dpi => Ok(dpi),
        }
    }

    /// A point in device pixels, in logical pixels.
    #[must_use]
    pub fn convert_point_from_device(self, point: Point<i32>) -> Point<f64> {
        let scale = self.0.scale_factor();
        Point::new(f64::from(point.x) / scale, f64::from(point.y) / scale)
    }

    /// A point in logical pixels, in device pixels.
    #[must_use]
    pub fn convert_point_to_device(self, point: Point<f64>) -> Point<i32> {
        let scale = self.0.scale_factor();
        Point::new(
            (point.x * scale).round() as i32,
            (point.y * scale).round() as i32,
        )
    }
}

#[cfg(test)]
mod material_tests {
    //! The contract's material on a real Win32 window, on the build this
    //! machine runs and on a build below the gate the platform is told it
    //! runs. The desktop window manager's own attribute is the readback.

    use std::rc::Rc;

    use flui_platform::{
        AdmissionError, ApplyError, Availability, CommandRefusal, PlatformFailure, Unsupported,
        UnsupportedReason, WindowCommand, WindowMaterial, WindowOptions,
    };
    use flui_testing::window_commands::AdmitAndApply as _;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DWMSBT_MAINWINDOW, DWMSBT_NONE};

    use super::super::WindowsWindow;
    use super::{
        SYSTEM_BACKDROP, Win32WindowError, WindowsBackdrop, WindowsWindowExt as _, get,
        material_applied,
    };
    use crate::host_window::HostWindow;
    use crate::platform::Platform as _;
    use crate::version::gate::windows::WindowsBuild;

    const NOT_ON_THIS_DEVICE: Unsupported = Unsupported::new(UnsupportedReason::NotOnThisDevice);

    fn open(platform: &crate::WindowsPlatform) -> (Rc<dyn HostWindow>, HWND) {
        let window = platform
            .open_window(
                WindowOptions::default().with_title("flui material"),
                flui_testing::window_handler::RecordingWindowHandler::new(),
            )
            .expect("a Win32 window opens");
        let hwnd = window
            .as_any()
            .downcast_ref::<WindowsWindow>()
            .expect("a Win32 platform opens Win32 windows")
            .hwnd();
        (window, hwnd)
    }

    /// The material the desktop window manager holds for `hwnd`.
    fn held(hwnd: HWND) -> i32 {
        get(hwnd, SYSTEM_BACKDROP)
            .expect("the attribute reads on Windows 11")
            .0
    }

    /// On a build below 22621 the window says it draws no material, on this
    /// device; it opens without one, admission refuses `SetMaterial` with
    /// `NotOnThisDevice`, and the Win32 view's own backdrop is refused the
    /// same way: no write reaches the attribute.
    fn below_the_gate_a_material_is_not_on_this_device() {
        let platform =
            crate::WindowsPlatform::at_build(WindowsBuild::from_number(22000)).expect("platform");
        let (window, hwnd) = open(&platform);
        assert_eq!(
            platform.window_capabilities().material,
            Availability::Unavailable
        );
        assert_eq!(
            window.state().capabilities.material,
            Availability::Unavailable
        );
        let before = held(hwnd);
        assert_ne!(before, DWMSBT_MAINWINDOW.0, "it opens without a material");
        assert_eq!(
            window.admit_and_apply(WindowCommand::SetMaterial(WindowMaterial::System)),
            Err(CommandRefusal::Admission(AdmissionError::Unsupported(
                NOT_ON_THIS_DEVICE
            )))
        );
        let view = window.win32().expect("a Win32 window");
        assert_eq!(
            view.set_backdrop(WindowsBackdrop::Mica),
            Err(Win32WindowError::Unsupported(NOT_ON_THIS_DEVICE))
        );
        assert_eq!(held(hwnd), before, "the attribute was left alone");
        window.close_now();
    }

    /// On this machine's build, each material is the backdrop the desktop
    /// window manager holds for the window once the command applied; a
    /// command admitted before the window closed answers `Closed`.
    fn from_the_gate_on_each_material_reaches_the_desktop_window_manager() {
        let platform = crate::WindowsPlatform::try_new().expect("platform");
        if super::super::material::gate(WindowsBuild::read()).is_err() {
            eprintln!("skipped: this machine's build is below the system backdrop's");
            return;
        }
        let (window, hwnd) = open(&platform);
        assert_eq!(window.state().capabilities.material, Availability::Native);
        assert_eq!(
            held(hwnd),
            DWMSBT_MAINWINDOW.0,
            "it opens with the system's"
        );
        for (material, backdrop) in [
            (WindowMaterial::Opaque, DWMSBT_NONE),
            (WindowMaterial::System, DWMSBT_MAINWINDOW),
            (WindowMaterial::Opaque, DWMSBT_NONE),
        ] {
            assert_eq!(
                window.admit_and_apply(WindowCommand::SetMaterial(material)),
                Ok(()),
                "{material:?}"
            );
            assert_eq!(held(hwnd), backdrop.0, "{material:?}");
        }
        let late = WindowCommand::SetMaterial(WindowMaterial::System)
            .admit(&window.state().capabilities)
            .expect("admitted while the window is open");
        window
            .admit_and_apply(WindowCommand::Close)
            .expect("the window closes");
        assert_eq!(window.apply(late), Err(ApplyError::Closed));
    }

    /// The desktop window manager's refusal is a failure of the system,
    /// `ApplyError::Platform`, not a log line and not a missing capability.
    fn a_refusal_of_the_desktop_window_manager_is_a_platform_failure() {
        let refusal = Win32WindowError::Refused {
            request: "DWMWA_SYSTEMBACKDROP_TYPE",
            hresult: windows::Win32::Foundation::E_INVALIDARG.0,
        };
        assert!(matches!(
            material_applied(Err(refusal)),
            Err(ApplyError::Platform(PlatformFailure::Failed { .. }))
        ));
        assert_eq!(material_applied(Ok(())), Ok(()));
    }

    #[test]
    fn window_material_on_win32() {
        crate::table_test::run_table(
            "window_material_on_win32",
            &[
                (
                    "below_the_gate_a_material_is_not_on_this_device",
                    below_the_gate_a_material_is_not_on_this_device as fn(),
                ),
                (
                    "from_the_gate_on_each_material_reaches_the_desktop_window_manager",
                    from_the_gate_on_each_material_reaches_the_desktop_window_manager as fn(),
                ),
                (
                    "a_refusal_of_the_desktop_window_manager_is_a_platform_failure",
                    a_refusal_of_the_desktop_window_manager_is_a_platform_failure as fn(),
                ),
            ],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{BACKDROPS, CORNERS, from_system, to_system};

    /// Each table maps every FLUI value to a distinct system value and back.
    #[test]
    fn each_mapping_is_one_table_read_both_ways() {
        for (flui, system) in CORNERS {
            assert_eq!(to_system(&CORNERS, flui), Some(system));
            assert_eq!(from_system(&CORNERS, system), Some(flui));
        }
        for (flui, system) in BACKDROPS {
            assert_eq!(to_system(&BACKDROPS, flui), Some(system));
            assert_eq!(from_system(&BACKDROPS, system), Some(flui));
        }
    }
}
