//! macOS-specific window extensions: [`MacOSWindowExt`], how an application
//! reaches the AppKit features beyond the cross-platform window — tabs,
//! native fullscreen, level, collection behavior, shadow, alpha and the
//! material behind the content — on any window a host hands out
//! (`flui_native::os::macos`), and [`AppKitWindow`], what each asks of the
//! AppKit window.

use std::sync::atomic::Ordering;

use flui_foundation::geometry::Point;
use objc2::msg_send;

use super::{BObjC, MacOSWindow, NIL, NO, YES, route_on_owner};
use crate::host_window::HostWindow;

// ============================================================================
// macOS Window Extension Trait
// ============================================================================

/// macOS-specific window extensions.
///
/// Implemented for every [`HostWindow`] (ADR-0151 §8). A window of another
/// host (headless, winit) is not an AppKit window: [`appkit`](Self::appkit)
/// answers `None` for it, and nothing is asked of it.
///
/// ```
/// use flui_native::os::macos::{MacOSBackdrop, MacOSMaterial, MacOSWindowExt};
/// use flui_native::{HostWindow, headless_platform};
/// use flui_platform::WindowOptions;
///
/// /// The sidebar material behind the window, if it is an AppKit window.
/// fn sidebar(window: &dyn HostWindow) -> bool {
///     window
///         .appkit()
///         .is_some_and(|appkit| {
///             appkit
///                 .set_backdrop(MacOSBackdrop::Material(MacOSMaterial::Sidebar))
///                 .is_ok()
///         })
/// }
///
/// // A headless window is not an AppKit window: nothing is asked of it.
/// let window = headless_platform().open_window(WindowOptions::default().with_visible(false))?;
/// assert!(!sidebar(window.as_ref()));
/// # Ok::<(), flui_native::OpenWindowError>(())
/// ```
///
/// # Platform Availability
///
/// - **Tabbed Windows:** macOS 10.12+ (Sierra)
/// - **Full Screen Transitions:** macOS 10.7+ (Lion)
pub trait MacOSWindowExt {
    /// This window's AppKit extras, or `None` for a window of another host.
    #[must_use]
    fn appkit(&self) -> Option<AppKitWindow<'_>>;
}

impl MacOSWindowExt for dyn HostWindow + '_ {
    fn appkit(&self) -> Option<AppKitWindow<'_>> {
        self.as_any().downcast_ref().map(AppKitWindow)
    }
}

/// What an AppKit window offers beyond the window contract. Every command
/// runs on the window's owner lane, wherever it is called from.
/// [`MacOSWindowExt::appkit`] gives one only for an AppKit window, so none of
/// these can meet another host's window.
#[derive(Debug, Clone, Copy)]
pub struct AppKitWindow<'a>(pub(super) &'a MacOSWindow);

/// Why two windows were not joined as tabs ([`AppKitWindow::add_tab_to_window`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TabJoinError {
    /// The other window is not an AppKit window.
    #[error("the other window is not an AppKit window")]
    NotAppKit,
    /// The two are one window.
    #[error("a window cannot be a tab of itself")]
    SameWindow,
    /// The two windows are not both on the AppKit main thread's owner lane.
    #[error("the windows are not both on the AppKit main thread's owner lane")]
    NotOnTheMainLane,
    /// One of the windows is closed.
    #[error("one of the windows is closed")]
    Closed,
}

// ============================================================================
// macOS Window Level
// ============================================================================

/// macOS window z-ordering level.
///
/// Corresponds to NSWindowLevel values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MacOSWindowLevel {
    /// Normal window level (NSNormalWindowLevel = 0).
    Normal,

    /// Floating window level (NSFloatingWindowLevel = 3).
    ///
    /// Window floats above normal windows.
    Floating,

    /// Torn-off menu level (NSTornOffMenuWindowLevel = 3).
    TornOffMenu,

    /// Modal panel level (NSModalPanelWindowLevel = 8).
    ///
    /// Used for modal dialogs.
    ModalPanel,

    /// Main menu level (NSMainMenuWindowLevel = 24).
    MainMenu,

    /// Status window level (NSStatusWindowLevel = 25).
    ///
    /// Used for menu bar extras and status items.
    Status,

    /// Floating panel level (NSFloatingPanelWindowLevel = INT_MAX - 1).
    FloatingPanel,

    /// Pop-up menu level (NSPopUpMenuWindowLevel = 101).
    PopUpMenu,

    /// Screen saver level (NSScreenSaverWindowLevel = 1000).
    ScreenSaver,
}

impl MacOSWindowLevel {
    /// Convert to NSWindowLevel integer value.
    pub fn to_ns_value(self) -> isize {
        match self {
            MacOSWindowLevel::Normal => 0,
            MacOSWindowLevel::Floating | MacOSWindowLevel::TornOffMenu => 3,
            MacOSWindowLevel::ModalPanel => 8,
            MacOSWindowLevel::MainMenu => 24,
            MacOSWindowLevel::Status => 25,
            MacOSWindowLevel::FloatingPanel => isize::MAX - 1,
            MacOSWindowLevel::PopUpMenu => 101,
            MacOSWindowLevel::ScreenSaver => 1000,
        }
    }
}

// ============================================================================
// macOS Collection Behavior
// ============================================================================

/// macOS window collection behavior flags.
///
/// Corresponds to NSWindowCollectionBehavior values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MacOSCollectionBehavior {
    bits: u32,
}

impl MacOSCollectionBehavior {
    /// Default collection behavior.
    pub const DEFAULT: Self = Self { bits: 0 };

    /// Window can enter fullscreen mode.
    pub const CAN_FULLSCREEN: Self = Self { bits: 1 << 7 };

    /// Window participates in Spaces.
    pub const MANAGED: Self = Self { bits: 1 << 2 };

    /// Window does not participate in Spaces.
    pub const TRANSIENT: Self = Self { bits: 1 << 3 };

    /// Window appears in all Spaces.
    pub const CAN_JOIN_ALL_SPACES: Self = Self { bits: 1 << 0 };

    /// Window moves to active Space when activated.
    pub const MOVE_TO_ACTIVE_SPACE: Self = Self { bits: 1 << 1 };

    /// Primary window for fullscreen mode.
    pub const PRIMARY_FULLSCREEN: Self = Self { bits: 1 << 4 };

    /// Auxiliary window for fullscreen mode.
    pub const AUXILIARY_FULLSCREEN: Self = Self { bits: 1 << 5 };

    /// Fullscreen window uses immersive mode.
    pub const FULLSCREEN_IMMERSIVE: Self = Self { bits: 1 << 8 };

    /// Create a custom combination of behaviors.
    pub fn from_bits(bits: u32) -> Self {
        Self { bits }
    }

    /// Get the raw bits value.
    pub fn bits(self) -> u32 {
        self.bits
    }

    /// Combine with another behavior.
    pub fn with(self, other: Self) -> Self {
        Self {
            bits: self.bits | other.bits,
        }
    }
}

// ============================================================================
// What an AppKit window offers
// ============================================================================

impl AppKitWindow<'_> {
    /// Let AppKit group this window with others into tabs (macOS 10.12+).
    pub fn enable_tabbing(self) {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            // Enable automatic tabbing (macOS 10.12+)
            let tabbing_mode: isize = 1; // NSWindowTabbingModeAutomatic
            let _: () = msg_send![this.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing enabled");
        });
    }

    /// Keep AppKit from grouping this window into tabs.
    pub fn disable_tabbing(self) {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let tabbing_mode: isize = 2; // NSWindowTabbingModeDisallowed
            let _: () = msg_send![this.ns_window, setTabbingMode: tabbing_mode];

            tracing::debug!("Window tabbing disabled");
        });
    }

    /// Add `other` to this window's tab group.
    ///
    /// Borrows the other window rather than accepting a numeric window ID: both
    /// wrappers keep their native objects alive through the join. Numeric IDs
    /// cannot establish that ownership:
    ///
    /// ```compile_fail,E0308
    /// use flui_native::os::macos::{AppKitWindow, TabJoinError};
    ///
    /// fn join(window: AppKitWindow<'_>) -> Result<(), TabJoinError> {
    ///     window.add_tab_to_window(1)
    /// }
    /// ```
    ///
    /// ```no_run
    /// use flui_native::HostWindow;
    /// use flui_native::os::macos::{MacOSWindowExt, TabJoinError};
    ///
    /// fn join(window: &dyn HostWindow, other: &dyn HostWindow) -> Option<Result<(), TabJoinError>> {
    ///     window.appkit().map(|appkit| appkit.add_tab_to_window(other))
    /// }
    /// ```
    ///
    /// # Errors
    /// [`TabJoinError`]: the other window is not an AppKit window, is this
    /// one, is not on the same AppKit main-thread owner lane, or either is
    /// closed.
    pub fn add_tab_to_window(self, other: &dyn HostWindow) -> Result<(), TabJoinError> {
        let this = self.0;
        let other = other
            .as_any()
            .downcast_ref::<MacOSWindow>()
            .ok_or(TabJoinError::NotAppKit)?;
        if this.ns_window == other.ns_window {
            return Err(TabJoinError::SameWindow);
        }
        if !this.owner_is_main || !other.owner_is_main || !std::ptr::eq(this.owner, other.owner) {
            return Err(TabJoinError::NotOnTheMainLane);
        }
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: both borrowed wrappers own live NSWindows, constructed
            // with releasedWhenClosed:NO. The checks above establish one main
            // owner lane; this route executes there. Extra retains keep both
            // receivers alive through synchronous AppKit callback reentry.
            if this.closed.load(Ordering::Acquire) || other.closed.load(Ordering::Acquire) {
                return Err(TabJoinError::Closed);
            }
            let Some(_this_pin) = objc2::rc::Retained::retain(this.ns_window) else {
                return Err(TabJoinError::Closed);
            };
            let Some(_other_pin) = objc2::rc::Retained::retain(other.ns_window) else {
                return Err(TabJoinError::Closed);
            };
            let _: () = msg_send![this.ns_window, addTabbedWindow:other.ns_window ordered:0]; // NSWindowAbove
            Ok(())
        })
    }

    /// Toggle native fullscreen, with AppKit's animation and a Space of its
    /// own (macOS 10.7+).
    pub fn toggle_native_fullscreen(self) {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let _: () = msg_send![this.ns_window, toggleFullScreen: NIL];
            tracing::debug!("Toggled native fullscreen");
        });
    }

    /// Set the window's level (its z-ordering).
    pub fn set_window_level(self, level: MacOSWindowLevel) {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let level_value = level.to_ns_value();
            let _: () = msg_send![this.ns_window, setLevel: level_value];
            tracing::debug!("Set window level to {:?} ({})", level, level_value);
        });
    }

    /// The window's level; a level AppKit reports that no
    /// [`MacOSWindowLevel`] names reads as `Normal`.
    #[must_use]
    pub fn window_level(self) -> MacOSWindowLevel {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let level_value: isize = msg_send![this.ns_window, level];
            match level_value {
                0 => MacOSWindowLevel::Normal,
                3 => MacOSWindowLevel::Floating,
                8 => MacOSWindowLevel::ModalPanel,
                24 => MacOSWindowLevel::MainMenu,
                25 => MacOSWindowLevel::Status,
                101 => MacOSWindowLevel::PopUpMenu,
                1000 => MacOSWindowLevel::ScreenSaver,
                _ if level_value == isize::MAX - 1 => MacOSWindowLevel::FloatingPanel,
                _ => MacOSWindowLevel::Normal, // Default to normal for unknown values
            }
        })
    }

    /// Set how the window behaves with Spaces, Exposé and fullscreen.
    pub fn set_collection_behavior(self, behavior: MacOSCollectionBehavior) {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let _: () = msg_send![this.ns_window, setCollectionBehavior: behavior.bits() as usize];
            tracing::debug!("Set collection behavior: {:?}", behavior);
        });
    }

    /// Give the window a shadow, or take it away.
    pub fn set_has_shadow(self, has_shadow: bool) {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let value: BObjC = if has_shadow { YES } else { NO };
            let _: () = msg_send![this.ns_window, setHasShadow: value];
            tracing::debug!("Set window shadow: {}", has_shadow);
        });
    }

    /// Set the window's alpha, from 0.0 (transparent) to 1.0 (opaque); a
    /// value outside is clamped.
    pub fn set_alpha(self, alpha: f64) {
        let this = self.0;
        route_on_owner(this.owner, this.owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and the body runs on the owner thread — inline on
            // the OS main thread for a main-lane owner, or dispatched onto the
            // lane under the reentrancy guard — before the message is sent.
            // Capturing `this` (a `&MacOSWindow`)
            // rather than the raw-pointer field is what keeps the closure
            // `Send`: `&MacOSWindow: Send` via the `unsafe impl Sync`.
            let clamped_alpha = alpha.clamp(0.0, 1.0);
            let _: () = msg_send![this.ns_window, setAlphaValue: clamped_alpha];
            tracing::debug!("Set window alpha: {}", clamped_alpha);
        });
    }

    /// The window's backing scale factor (2.0 on a Retina display).
    #[must_use]
    pub fn backing_scale_factor(self) -> f64 {
        self.0.state.lock().scale_factor
    }

    /// A point in backing (device) pixels, in window coordinates.
    #[must_use]
    pub fn convert_point_from_backing(self, point: Point<f64>) -> Point<f64> {
        let scale = self.backing_scale_factor();
        Point::new(point.x / scale, point.y / scale)
    }

    /// A point in window coordinates, in backing (device) pixels.
    #[must_use]
    pub fn convert_point_to_backing(self, point: Point<f64>) -> Point<f64> {
        let scale = self.backing_scale_factor();
        Point::new(point.x * scale, point.y * scale)
    }
}
