//! macOS-specific window extensions: [`MacOSWindowExt`], how an application
//! reaches the AppKit features beyond the cross-platform window — tabs, level,
//! collection behavior, shadow, alpha and the material behind the content — on
//! any window a host hands out (`flui_native::os::macos`), and
//! [`AppKitWindow`], what each asks of the AppKit window.
//!
//! What the window contract already does (the mode, full screen included, the
//! frame, the material, the scale) is a `WindowCommand` or a fact of the
//! window's state, not a method here: a second way would leave the snapshot
//! behind.

use std::sync::atomic::Ordering;

use flui_platform::{Unsupported, UnsupportedReason};
use objc2::msg_send;
use objc2_app_kit::{
    NSFloatingWindowLevel, NSMainMenuWindowLevel, NSModalPanelWindowLevel, NSNormalWindowLevel,
    NSPopUpMenuWindowLevel, NSScreenSaverWindowLevel, NSStatusWindowLevel,
    NSWindowCollectionBehavior, NSWindowLevel, NSWindowTabbingMode,
};

use super::{BObjC, MacOSWindow, NO, YES};
use crate::host_window::HostWindow;

// ============================================================================
// macOS Window Extension Trait
// ============================================================================

/// macOS-specific window extensions.
///
/// Implemented for every [`HostWindow`] (ADR-0151 §8). A window of another
/// host (headless, another system's) is not an AppKit window: [`appkit`](Self::appkit)
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
/// let window = headless_platform().open_window(WindowOptions::default().visible(false))?;
/// assert!(!sidebar(window.as_ref()));
/// # Ok::<(), flui_native::OpenWindowError>(())
/// ```
///
/// # Platform Availability
///
/// - **Tabbed Windows:** macOS 10.12+ (Sierra)
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

/// What an AppKit window offers beyond the window contract.
///
/// Every command runs on the window's owner thread, the AppKit main thread:
/// the view borrows a window that is neither `Send` nor `Sync`, so it is used
/// only where the window was made. [`MacOSWindowExt::appkit`] gives one only
/// for an AppKit window, so none of these can meet another host's window.
/// Each command does what it says or answers why not
/// ([`AppKitWindowError`]).
#[derive(Debug, Clone, Copy)]
pub struct AppKitWindow<'a>(pub(super) &'a MacOSWindow);

/// Why an [`AppKitWindow`] command or query did not do what it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AppKitWindowError {
    /// Not here, in the contract's vocabulary: the window is closed
    /// ([`UnsupportedReason::NoWindow`]).
    #[error(transparent)]
    Unsupported(#[from] Unsupported),
    /// AppKit holds a window level that no [`MacOSWindowLevel`] names:
    /// something other than this view set it.
    #[error("the window's level is one no MacOSWindowLevel names")]
    UnnamedLevel,
}

impl AppKitWindowError {
    /// The window is closed.
    const CLOSED: Self = Self::Unsupported(Unsupported::new(UnsupportedReason::NoWindow));
}

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
    /// Not here, in the contract's vocabulary: one of the windows is closed
    /// ([`UnsupportedReason::NoWindow`]).
    #[error(transparent)]
    Unsupported(#[from] Unsupported),
}

impl TabJoinError {
    /// One of the windows is closed.
    const CLOSED: Self = Self::Unsupported(Unsupported::new(UnsupportedReason::NoWindow));
}

// ============================================================================
// Alpha
// ============================================================================

/// How opaque a window is drawn, from 0 (transparent) to 1 (opaque).
///
/// Never NaN and never outside `0..=1`: [`try_new`](Self::try_new) refuses
/// such a value, so none reaches AppKit.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct MacOSWindowAlpha(f64);

/// The value given for a [`MacOSWindowAlpha`] is NaN or outside `0..=1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a window's alpha is a number from 0 to 1")]
pub struct InvalidWindowAlpha;

impl MacOSWindowAlpha {
    /// Opaque: the alpha every window has until it is given another.
    pub const OPAQUE: Self = Self(1.0);

    /// Fully transparent.
    pub const TRANSPARENT: Self = Self(0.0);

    /// `value` as a window's alpha.
    ///
    /// # Errors
    /// [`InvalidWindowAlpha`] when `value` is NaN or outside `0..=1`.
    pub fn try_new(value: f64) -> Result<Self, InvalidWindowAlpha> {
        if (0.0..=1.0).contains(&value) {
            // `-0.0` is in the range; it is drawn as `0.0` and kept as one.
            Ok(Self(value + 0.0))
        } else {
            Err(InvalidWindowAlpha)
        }
    }

    /// The alpha, from 0 to 1.
    #[must_use]
    pub const fn get(self) -> f64 {
        self.0
    }
}

impl Default for MacOSWindowAlpha {
    fn default() -> Self {
        Self::OPAQUE
    }
}

// ============================================================================
// macOS Window Level
// ============================================================================

/// Where a window is ordered among the windows of every application.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MacOSWindowLevel {
    /// With the documents and other windows of applications.
    #[default]
    Normal,
    /// Above the normal windows: a palette, an inspector.
    Floating,
    /// Above the floating windows: a modal panel.
    ModalPanel,
    /// The menu bar's level.
    MainMenu,
    /// Above the menu bar: a status window.
    Status,
    /// A pop-up menu's level.
    PopUpMenu,
    /// Above everything else: a screen saver.
    ScreenSaver,
}

impl MacOSWindowLevel {
    /// Every level, which [`from_appkit`](Self::from_appkit) searches.
    const ALL: [Self; 7] = [
        Self::Normal,
        Self::Floating,
        Self::ModalPanel,
        Self::MainMenu,
        Self::Status,
        Self::PopUpMenu,
        Self::ScreenSaver,
    ];

    /// AppKit's value for this level.
    fn to_appkit(self) -> NSWindowLevel {
        match self {
            Self::Normal => NSNormalWindowLevel,
            Self::Floating => NSFloatingWindowLevel,
            Self::ModalPanel => NSModalPanelWindowLevel,
            Self::MainMenu => NSMainMenuWindowLevel,
            Self::Status => NSStatusWindowLevel,
            Self::PopUpMenu => NSPopUpMenuWindowLevel,
            Self::ScreenSaver => NSScreenSaverWindowLevel,
        }
    }

    /// The level AppKit's `value` is, if one is.
    fn from_appkit(value: NSWindowLevel) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|level| level.to_appkit() == value)
    }
}

// ============================================================================
// macOS Collection Behavior
// ============================================================================

/// Which Spaces a window is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MacOSSpaces {
    /// The Space it was opened on, as AppKit decides.
    #[default]
    Default,
    /// Every Space.
    All,
    /// The active Space: the window moves there when it is activated.
    FollowsActive,
}

/// What Mission Control does with a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MacOSMissionControl {
    /// As AppKit decides for the window.
    #[default]
    Default,
    /// Shown and moved with the Spaces it is on.
    Managed,
    /// Floats in Spaces and is hidden by Mission Control.
    Transient,
    /// Unaffected by Mission Control: it stays where it is.
    Stationary,
}

/// Whether a window is in the cycle of the Cycle Through Windows shortcut.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum MacOSWindowCycle {
    /// As AppKit decides for the window.
    #[default]
    Default,
    /// In the cycle.
    Participates,
    /// Left out of the cycle.
    Ignored,
}

/// How a window behaves with Spaces, Mission Control and the window cycle.
///
/// Each part is one of the choices AppKit lets a window combine, so a value
/// of this type is always one AppKit accepts. Full screen is the contract's
/// (`WindowMode::Fullscreen`): every window keeps the full-screen behavior
/// that mode needs, whatever this says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct MacOSCollectionBehavior {
    /// Which Spaces the window is on.
    pub spaces: MacOSSpaces,
    /// What Mission Control does with the window.
    pub mission_control: MacOSMissionControl,
    /// Whether the window is in the window cycle.
    pub cycle: MacOSWindowCycle,
}

impl MacOSCollectionBehavior {
    /// This behavior on the Spaces `spaces` names.
    #[must_use]
    pub const fn spaces(mut self, spaces: MacOSSpaces) -> Self {
        self.spaces = spaces;
        self
    }

    /// This behavior with Mission Control doing what `mission_control` says.
    #[must_use]
    pub const fn mission_control(mut self, mission_control: MacOSMissionControl) -> Self {
        self.mission_control = mission_control;
        self
    }

    /// This behavior in or out of the window cycle, as `cycle` says.
    #[must_use]
    pub const fn cycle(mut self, cycle: MacOSWindowCycle) -> Self {
        self.cycle = cycle;
        self
    }

    /// AppKit's flags for this behavior, with the full-screen behavior every
    /// window keeps.
    fn to_appkit(self) -> NSWindowCollectionBehavior {
        let spaces = match self.spaces {
            MacOSSpaces::Default => NSWindowCollectionBehavior::Default,
            MacOSSpaces::All => NSWindowCollectionBehavior::CanJoinAllSpaces,
            MacOSSpaces::FollowsActive => NSWindowCollectionBehavior::MoveToActiveSpace,
        };
        let mission_control = match self.mission_control {
            MacOSMissionControl::Default => NSWindowCollectionBehavior::Default,
            MacOSMissionControl::Managed => NSWindowCollectionBehavior::Managed,
            MacOSMissionControl::Transient => NSWindowCollectionBehavior::Transient,
            MacOSMissionControl::Stationary => NSWindowCollectionBehavior::Stationary,
        };
        let cycle = match self.cycle {
            MacOSWindowCycle::Default => NSWindowCollectionBehavior::Default,
            MacOSWindowCycle::Participates => NSWindowCollectionBehavior::ParticipatesInCycle,
            MacOSWindowCycle::Ignored => NSWindowCollectionBehavior::IgnoresCycle,
        };
        spaces | mission_control | cycle | NSWindowCollectionBehavior::FullScreenPrimary
    }
}

// ============================================================================
// What an AppKit window offers
// ============================================================================

impl<'a> AppKitWindow<'a> {
    /// The window, while it is open.
    fn open(self) -> Result<&'a MacOSWindow, AppKitWindowError> {
        if self.0.closed.load(Ordering::Acquire) {
            Err(AppKitWindowError::CLOSED)
        } else {
            Ok(self.0)
        }
    }

    /// Let AppKit group this window with others into tabs, as the user's
    /// preference for tabs says (macOS 10.12+).
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed.
    pub fn enable_tabbing(self) -> Result<(), AppKitWindowError> {
        self.set_tabbing_mode(NSWindowTabbingMode::Automatic)
    }

    /// Keep AppKit from grouping this window into tabs.
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed.
    pub fn disable_tabbing(self) -> Result<(), AppKitWindowError> {
        self.set_tabbing_mode(NSWindowTabbingMode::Disallowed)
    }

    fn set_tabbing_mode(self, mode: NSWindowTabbingMode) -> Result<(), AppKitWindowError> {
        let window = self.open()?;
        // SAFETY: `ns_window` is alive for the lifetime of `self` (which
        // `window` reborrows), and the window is used on its owner thread
        // only (it is neither `Send` nor `Sync`).
        unsafe {
            let _: () = msg_send![window.ns_window, setTabbingMode: mode];
        }
        Ok(())
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
    /// one, or either is closed.
    pub fn add_tab_to_window(self, other: &dyn HostWindow) -> Result<(), TabJoinError> {
        let this = self.0;
        let other = other
            .as_any()
            .downcast_ref::<MacOSWindow>()
            .ok_or(TabJoinError::NotAppKit)?;
        if this.ns_window == other.ns_window {
            return Err(TabJoinError::SameWindow);
        }
        unsafe {
            // SAFETY: both borrowed wrappers own live NSWindows, constructed
            // with releasedWhenClosed:NO. Neither window is `Send` nor `Sync`,
            // so both are on the AppKit main thread that made them. Extra
            // retains keep both receivers alive through synchronous AppKit
            // callback reentry.
            if this.closed.load(Ordering::Acquire) || other.closed.load(Ordering::Acquire) {
                return Err(TabJoinError::CLOSED);
            }
            let Some(_this_pin) = objc2::rc::Retained::retain(this.ns_window) else {
                return Err(TabJoinError::CLOSED);
            };
            let Some(_other_pin) = objc2::rc::Retained::retain(other.ns_window) else {
                return Err(TabJoinError::CLOSED);
            };
            let _: () = msg_send![this.ns_window, addTabbedWindow:other.ns_window ordered:0]; // NSWindowAbove
            Ok(())
        }
    }

    /// Order the window at `level` among the windows of every application.
    /// The window's state says where that stands it
    /// ([`WindowState::level`](flui_platform::WindowState::level)): above the
    /// normal level is kept on top. A full-screen window stands at the normal
    /// level while it lasts, and takes this one when it leaves.
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed.
    pub fn set_window_level(self, level: MacOSWindowLevel) -> Result<(), AppKitWindowError> {
        let window = self.open()?;
        window.keep_level(level.to_appkit());
        window.publish_state();
        Ok(())
    }

    /// The level the window is ordered at outside full screen, which a
    /// full-screen window keeps for when it leaves.
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed, or it stands at a level
    /// no [`MacOSWindowLevel`] names (the one under the normal level that
    /// `WindowLevel::AlwaysOnBottom` sets).
    pub fn window_level(self) -> Result<MacOSWindowLevel, AppKitWindowError> {
        let window = self.open()?;
        let value: NSWindowLevel = window.state.borrow().level;
        MacOSWindowLevel::from_appkit(value).ok_or(AppKitWindowError::UnnamedLevel)
    }

    /// Set how the window behaves with Spaces, Mission Control and the window
    /// cycle; the full-screen behavior the contract's full-screen mode needs
    /// is kept.
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed.
    pub fn set_collection_behavior(
        self,
        behavior: MacOSCollectionBehavior,
    ) -> Result<(), AppKitWindowError> {
        let window = self.open()?;
        let flags = behavior.to_appkit();
        // SAFETY: as in `set_tabbing_mode`; `flags` is one combination AppKit
        // accepts, since each part of the behavior is one of its exclusive
        // choices.
        unsafe {
            let _: () = msg_send![window.ns_window, setCollectionBehavior: flags];
        }
        Ok(())
    }

    /// Give the window a shadow, or take it away.
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed.
    pub fn set_has_shadow(self, has_shadow: bool) -> Result<(), AppKitWindowError> {
        let window = self.open()?;
        let value: BObjC = if has_shadow { YES } else { NO };
        // SAFETY: as in `set_tabbing_mode`.
        unsafe {
            let _: () = msg_send![window.ns_window, setHasShadow: value];
        }
        Ok(())
    }

    /// Draw the window at `alpha`.
    ///
    /// The window keeps it: a window opened to be revealed after its first
    /// frame stays transparent until then, and is revealed at this alpha.
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed.
    pub fn set_alpha(self, alpha: MacOSWindowAlpha) -> Result<(), AppKitWindowError> {
        let window = self.open()?;
        if window.keep_alpha(alpha) {
            // SAFETY: as in `set_tabbing_mode`.
            unsafe {
                let _: () = msg_send![window.ns_window, setAlphaValue: alpha.get()];
            }
        }
        Ok(())
    }

    /// The alpha the window is drawn at once it is revealed: the last one
    /// [`set_alpha`](Self::set_alpha) gave it, opaque until then.
    ///
    /// # Errors
    /// [`AppKitWindowError`]: the window is closed.
    pub fn alpha(self) -> Result<MacOSWindowAlpha, AppKitWindowError> {
        Ok(self.open()?.state.borrow().alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        InvalidWindowAlpha, MacOSCollectionBehavior, MacOSMissionControl, MacOSSpaces,
        MacOSWindowAlpha, MacOSWindowCycle, MacOSWindowLevel, NSWindowCollectionBehavior,
    };

    /// An alpha is a number from 0 to 1; NaN, an infinity and a value outside
    /// are refused, and `-0.0` is kept as `0.0`.
    #[test]
    fn a_window_alpha_is_a_number_from_zero_to_one() {
        for (value, alpha) in [
            (0.0, Ok(0.0)),
            (-0.0, Ok(0.0)),
            (0.5, Ok(0.5)),
            (1.0, Ok(1.0)),
            (1.000_001, Err(InvalidWindowAlpha)),
            (-0.1, Err(InvalidWindowAlpha)),
            (f64::NAN, Err(InvalidWindowAlpha)),
            (f64::INFINITY, Err(InvalidWindowAlpha)),
            (f64::NEG_INFINITY, Err(InvalidWindowAlpha)),
        ] {
            let made = MacOSWindowAlpha::try_new(value).map(MacOSWindowAlpha::get);
            assert_eq!(made, alpha, "{value}");
            if let Ok(made) = made {
                assert!(made.is_sign_positive(), "{value} is kept positive");
            }
        }
        assert_eq!(MacOSWindowAlpha::default(), MacOSWindowAlpha::OPAQUE);
    }

    /// Each level is one AppKit value and back, and no two levels share one.
    #[test]
    fn each_level_is_one_appkit_value_read_both_ways() {
        for level in MacOSWindowLevel::ALL {
            assert_eq!(
                MacOSWindowLevel::from_appkit(level.to_appkit()),
                Some(level),
                "{level:?}"
            );
        }
        assert_eq!(MacOSWindowLevel::from_appkit(-1), None);
    }

    /// Every behavior keeps the full-screen behavior the contract's mode
    /// needs, and takes one flag of each exclusive group at most.
    #[test]
    fn a_collection_behavior_is_one_appkit_accepts() {
        let groups = [
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::MoveToActiveSpace,
            NSWindowCollectionBehavior::Managed
                | NSWindowCollectionBehavior::Transient
                | NSWindowCollectionBehavior::Stationary,
            NSWindowCollectionBehavior::ParticipatesInCycle
                | NSWindowCollectionBehavior::IgnoresCycle,
        ];
        for spaces in [
            MacOSSpaces::Default,
            MacOSSpaces::All,
            MacOSSpaces::FollowsActive,
        ] {
            for mission_control in [
                MacOSMissionControl::Default,
                MacOSMissionControl::Managed,
                MacOSMissionControl::Transient,
                MacOSMissionControl::Stationary,
            ] {
                for cycle in [
                    MacOSWindowCycle::Default,
                    MacOSWindowCycle::Participates,
                    MacOSWindowCycle::Ignored,
                ] {
                    let flags = MacOSCollectionBehavior::default()
                        .spaces(spaces)
                        .mission_control(mission_control)
                        .cycle(cycle)
                        .to_appkit();
                    assert!(flags.contains(NSWindowCollectionBehavior::FullScreenPrimary));
                    for group in groups {
                        assert!((flags & group).0.count_ones() <= 1, "{flags:?}");
                    }
                }
            }
        }
    }
}
