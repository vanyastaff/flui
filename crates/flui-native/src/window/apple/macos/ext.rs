//! macOS-specific window extensions: [`MacOSWindowExt`], the AppKit features
//! beyond the cross-platform window — tiling, tabs, collection behavior and
//! level — on any window a host hands out, and what each asks of the AppKit
//! window. The window's Liquid Glass backdrop is crate-private
//! (`window::apple::liquid_glass`).
//!
//! ```rust,ignore
//! use flui_native::{HostWindow, MacOSWindowExt, TilingConfiguration};
//!
//! fn arrange(window: &dyn HostWindow) {
//!     // `None`: the window is not an AppKit window, and nothing was asked of it.
//!     let _ = window.enable_tiling(TilingConfiguration::new());
//! }
//! ```

use std::sync::atomic::Ordering;

use flui_foundation::geometry::Point;
use objc2::runtime::Bool;
use objc2::{class, msg_send};
use objc2_app_kit::NSWindowStyleMask;
use objc2_foundation::NSRect;

use super::{BObjC, MacOSWindow, NIL, NO, ObjcId, YES, route_on_owner};
use crate::host_window::HostWindow;
use crate::window::apple::liquid_glass::LiquidGlassConfig;
use tiling::TilingConfiguration;

// ============================================================================
// macOS Window Extension Trait
// ============================================================================

/// macOS-specific window extensions.
///
/// This trait provides access to macOS-specific features that are not part
/// of the cross-platform window contract. It is implemented for every
/// [`HostWindow`]: a window of another host (headless, winit) is not an AppKit
/// window, and every method answers `None` for it and asks nothing of it
/// (ADR-0151 §8). For an AppKit window it answers `Some`.
///
/// # Platform Availability
///
/// - **Window Tiling:** macOS 15.0+ (Sequoia)
/// - **Tabbed Windows:** macOS 10.12+ (Sierra)
/// - **Full Screen Transitions:** macOS 10.7+ (Lion)
pub trait MacOSWindowExt {
    /// Enable window tiling with the specified configuration.
    ///
    /// Window tiling allows the window to suggest tile layouts for multi-window
    /// workflows, similar to Windows Snap Layouts.
    ///
    /// # Platform Requirements
    ///
    /// - macOS 15.0+ (Sequoia)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_native::{TilePosition, TilingConfiguration};
    ///
    /// let config = TilingConfiguration::new()
    ///     .with_primary_position(TilePosition::Left)
    ///     .with_split_ratio(0.5);
    ///
    /// window.enable_tiling(config);
    /// ```
    fn enable_tiling(&self, config: TilingConfiguration) -> Option<()>;

    /// Disable window tiling.
    fn disable_tiling(&self) -> Option<()>;

    /// Check if window tiling is enabled.
    fn is_tiling_enabled(&self) -> Option<bool>;

    /// Enable tabbed window mode.
    ///
    /// When enabled, multiple windows can be grouped into tabs within a single
    /// window frame.
    ///
    /// # Platform Requirements
    ///
    /// - macOS 10.12+ (Sierra)
    fn enable_tabbing(&self) -> Option<()>;

    /// Disable tabbed window mode.
    fn disable_tabbing(&self) -> Option<()>;

    /// Add this window to a tab group with another window.
    ///
    /// Borrows the other window rather than accepting a numeric window ID: both
    /// wrappers keep their native objects alive through the join. Answers
    /// `Some(true)` after AppKit receives the join, and `Some(false)` if either
    /// window is closed, the other is not an AppKit window, the windows are the
    /// same, or their owner lanes are not the same AppKit main lane.
    ///
    /// Numeric IDs cannot establish that ownership:
    ///
    /// ```compile_fail,E0308
    /// use flui_native::{HostWindow, MacOSWindowExt};
    ///
    /// fn join(window: &dyn HostWindow, other: &dyn HostWindow) -> Option<bool> {
    ///     window.add_tab_to_window(1)
    /// }
    /// ```
    ///
    /// ```no_run
    /// use flui_native::{HostWindow, MacOSWindowExt};
    ///
    /// fn join(window: &dyn HostWindow, other: &dyn HostWindow) -> Option<bool> {
    ///     window.add_tab_to_window(other)
    /// }
    /// ```
    fn add_tab_to_window(&self, other: &dyn HostWindow) -> Option<bool>;

    /// Toggle native fullscreen mode with macOS animation.
    ///
    /// This uses the native macOS fullscreen API with the slide-in animation
    /// and creates a new Space (virtual desktop).
    ///
    /// # Platform Requirements
    ///
    /// - macOS 10.7+ (Lion)
    fn toggle_native_fullscreen(&self) -> Option<()>;

    /// Set the window level (z-ordering).
    ///
    /// Controls whether the window floats above other windows, appears as a
    /// modal, etc.
    fn set_window_level(&self, level: MacOSWindowLevel) -> Option<()>;

    /// Get the window level.
    fn window_level(&self) -> Option<MacOSWindowLevel>;

    /// Set the window's collection behavior.
    ///
    /// Controls how the window behaves with Spaces, Exposé, and fullscreen
    /// modes.
    fn set_collection_behavior(&self, behavior: MacOSCollectionBehavior) -> Option<()>;

    /// Enable/disable window shadow.
    fn set_has_shadow(&self, has_shadow: bool) -> Option<()>;

    /// Set window alpha (transparency).
    ///
    /// # Parameters
    ///
    /// - `alpha`: 0.0 (fully transparent) to 1.0 (fully opaque)
    fn set_alpha(&self, alpha: f64) -> Option<()>;

    /// Get the window's backing scale factor (1.0 or 2.0 for Retina).
    fn backing_scale_factor(&self) -> Option<f64>;

    /// Convert point from backing (pixel) coordinates to window coordinates.
    fn convert_point_from_backing(&self, point: Point<f64>) -> Option<Point<f64>>;

    /// Convert point from window coordinates to backing (pixel) coordinates.
    fn convert_point_to_backing(&self, point: Point<f64>) -> Option<Point<f64>>;
}

/// `window` as an AppKit window: `None` for a window of another host.
fn appkit(window: &dyn HostWindow) -> Option<&MacOSWindow> {
    window.as_any().downcast_ref()
}

impl MacOSWindowExt for dyn HostWindow + '_ {
    fn enable_tiling(&self, config: TilingConfiguration) -> Option<()> {
        appkit(self).map(|_| MacOSWindow::enable_tiling(&config))
    }

    fn disable_tiling(&self) -> Option<()> {
        appkit(self).map(|_| MacOSWindow::disable_tiling())
    }

    fn is_tiling_enabled(&self) -> Option<bool> {
        appkit(self).map(|_| MacOSWindow::is_tiling_enabled())
    }

    fn enable_tabbing(&self) -> Option<()> {
        appkit(self).map(MacOSWindow::enable_tabbing)
    }

    fn disable_tabbing(&self) -> Option<()> {
        appkit(self).map(MacOSWindow::disable_tabbing)
    }

    fn add_tab_to_window(&self, other: &dyn HostWindow) -> Option<bool> {
        let window = appkit(self)?;
        Some(appkit(other).is_some_and(|other| window.add_tab_to_window(other)))
    }

    fn toggle_native_fullscreen(&self) -> Option<()> {
        appkit(self).map(MacOSWindow::toggle_native_fullscreen)
    }

    fn set_window_level(&self, level: MacOSWindowLevel) -> Option<()> {
        appkit(self).map(|window| window.set_window_level(level))
    }

    fn window_level(&self) -> Option<MacOSWindowLevel> {
        appkit(self).map(MacOSWindow::window_level)
    }

    fn set_collection_behavior(&self, behavior: MacOSCollectionBehavior) -> Option<()> {
        appkit(self).map(|window| window.set_collection_behavior(behavior))
    }

    fn set_has_shadow(&self, has_shadow: bool) -> Option<()> {
        appkit(self).map(|window| window.set_has_shadow(has_shadow))
    }

    fn set_alpha(&self, alpha: f64) -> Option<()> {
        appkit(self).map(|window| window.set_alpha(alpha))
    }

    fn backing_scale_factor(&self) -> Option<f64> {
        appkit(self).map(MacOSWindow::backing_scale_factor)
    }

    fn convert_point_from_backing(&self, point: Point<f64>) -> Option<Point<f64>> {
        appkit(self).map(|window| window.convert_point_from_backing(point))
    }

    fn convert_point_to_backing(&self, point: Point<f64>) -> Option<Point<f64>> {
        appkit(self).map(|window| window.convert_point_to_backing(point))
    }
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
// What the extension asks of an AppKit window
// ============================================================================

/// `NSViewWidthSizable | NSViewHeightSizable`: a subview that tracks its
/// superview's size.
const AUTORESIZE_FILL: usize = (1 << 1) | (1 << 4);

impl MacOSWindow {
    /// Puts a Liquid Glass backdrop behind the window's content.
    #[expect(
        dead_code,
        reason = "the window's backdrop material is set by the background material command (ADR-0254 §3), which applies this on macOS; no public path reaches it before then"
    )]
    pub(crate) fn set_liquid_glass_config(&self, config: LiquidGlassConfig) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self` (which
            // `this` reborrows), and so is `this.view`, which its view
            // hierarchy holds; the swap below retains the view while it is
            // detached. The NSVisualEffectView is alloc-init'ed, its +1 is
            // released once the window retains it as the content view. The
            // body runs on the owner thread — inline on the OS main thread for
            // a main-lane owner, or dispatched onto the lane under the
            // reentrancy guard — before the messages are sent.
            // Capturing `this` (a `&MacOSWindow`) rather than the raw-pointer
            // field is what keeps the closure `Send`: `&MacOSWindow: Send` via
            // the `unsafe impl Sync`.
            let view = this.view;
            let current: ObjcId = msg_send![this.ns_window, contentView];

            // The effect view goes behind the FLUI view rather than in its
            // place: the FLUI view carries input, IME, frame requests,
            // appearance changes and the surface, so it is re-parented into
            // the effect view, which becomes the content view. A repeated
            // call reconfigures the effect view already installed instead of
            // nesting another.
            let effect_view = if current == view {
                let effect_view: ObjcId = msg_send![class!(NSVisualEffectView), alloc];
                let effect_view: ObjcId = msg_send![effect_view, init];
                let frame: NSRect = msg_send![view, frame];
                let _: () = msg_send![effect_view, setFrame: frame];
                let _: () = msg_send![effect_view, setAutoresizingMask: AUTORESIZE_FILL];

                // Replacing the content view releases the window's reference
                // to the FLUI view; hold one until the effect view owns it.
                let _: ObjcId = msg_send![view, retain];
                let _: () = msg_send![this.ns_window, setContentView: effect_view];
                let _: () = msg_send![effect_view, release];
                let bounds: NSRect = msg_send![effect_view, bounds];
                let _: () = msg_send![view, setFrame: bounds];
                let _: () = msg_send![view, setAutoresizingMask: AUTORESIZE_FILL];
                let _: () = msg_send![effect_view, addSubview: view];
                let _: () = msg_send![view, release];

                // Leaving the window resigned the view's first-responder
                // status; keyboard and IME input must keep reaching it.
                let _: Bool = msg_send![this.ns_window, makeFirstResponder: view];
                effect_view
            } else {
                current
            };

            // Set material (NSVisualEffectMaterial)
            let material_value: usize = config.material.to_ns_visual_effect_material();
            let _: () = msg_send![effect_view, setMaterial: material_value];

            // Set blending mode (NSVisualEffectBlendingMode)
            let blending_mode: usize = config.blending_mode.to_ns_blending_mode();
            let _: () = msg_send![effect_view, setBlendingMode: blending_mode];

            // Set state (NSVisualEffectState)
            let state: usize = 1; // NSVisualEffectStateActive
            let _: () = msg_send![effect_view, setState: state];

            // Make window titlebar transparent if requested
            if config.transparent_titlebar {
                let style_mask: NSWindowStyleMask = msg_send![this.ns_window, styleMask];
                let new_style_mask = style_mask | NSWindowStyleMask::FullSizeContentView;
                let _: () = msg_send![this.ns_window, setStyleMask: new_style_mask];
                let _: () = msg_send![this.ns_window, setTitlebarAppearsTransparent: YES];
            }

            tracing::debug!("Applied Liquid Glass material: {:?}", config.material);
        });
    }

    /// Takes the Liquid Glass backdrop away, putting the content back as the
    /// window's content view.
    #[expect(
        dead_code,
        reason = "the window's backdrop material is set by the background material command (ADR-0254 §3), which applies this on macOS; no public path reaches it before then"
    )]
    pub(crate) fn clear_liquid_glass(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and so
            // is `this.view`, which its view hierarchy holds; the view is
            // retained while detached from the effect view. The body runs on
            // the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard —
            // before the messages are sent.
            let view = this.view;
            let current: ObjcId = msg_send![this.ns_window, contentView];

            // Put the original FLUI view back as the content view, keeping its
            // state; the window releases the effect view.
            if current != view {
                let _: ObjcId = msg_send![view, retain];
                let _: () = msg_send![view, removeFromSuperview];
                let _: () = msg_send![this.ns_window, setContentView: view];
                let _: () = msg_send![view, release];
                let _: Bool = msg_send![this.ns_window, makeFirstResponder: view];
            }

            // Restore titlebar appearance
            let _: () = msg_send![this.ns_window, setTitlebarAppearsTransparent: NO];

            tracing::debug!("Cleared Liquid Glass effect");
        });
    }

    pub(crate) fn enable_tiling(config: &TilingConfiguration) {
        // Native tiling API requires macOS 15+; until adopted, the
        // configuration is recorded for observability only.
        tracing::info!(
            "Window tiling enabled: position={:?}, ratio={}, layout={:?}",
            config.primary_position,
            config.split_ratio,
            config.layout
        );
    }

    pub(crate) fn disable_tiling() {
        tracing::info!("Window tiling disabled");
    }

    pub(crate) fn is_tiling_enabled() -> bool {
        // Native tiling API requires macOS 15+; not yet adopted.
        false
    }

    pub(crate) fn enable_tabbing(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
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

    pub(crate) fn disable_tabbing(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
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

    pub(crate) fn add_tab_to_window(&self, other: &MacOSWindow) -> bool {
        if !self.owner_is_main
            || !other.owner_is_main
            || !std::ptr::eq(self.owner, other.owner)
            || self.ns_window == other.ns_window
        {
            return false;
        }
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: both borrowed wrappers own live NSWindows, constructed
            // with releasedWhenClosed:NO. The checks above establish one main
            // owner lane; this route executes there. Extra retains keep both
            // receivers alive through synchronous AppKit callback reentry.
            if this.closed.load(Ordering::Acquire) || other.closed.load(Ordering::Acquire) {
                return false;
            }
            let Some(_this_pin) = objc2::rc::Retained::retain(this.ns_window) else {
                return false;
            };
            let Some(_other_pin) = objc2::rc::Retained::retain(other.ns_window) else {
                return false;
            };
            let _: () = msg_send![this.ns_window, addTabbedWindow:other.ns_window ordered:0]; // NSWindowAbove
            true
        })
    }

    pub(crate) fn toggle_native_fullscreen(&self) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
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

    pub(crate) fn set_window_level(&self, level: MacOSWindowLevel) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
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

    pub(crate) fn window_level(&self) -> MacOSWindowLevel {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        route_on_owner(owner, owner_is_main, || unsafe {
            // SAFETY: `ns_window` is alive for the lifetime of `self`, and the
            // body runs on the owner thread — inline on the OS main thread for a main-lane
            // owner, or dispatched onto the lane under the reentrancy guard — before the
            // message is sent.
            let level_value: isize = msg_send![self.ns_window, level];
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

    pub(crate) fn set_collection_behavior(&self, behavior: MacOSCollectionBehavior) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
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

    pub(crate) fn set_has_shadow(&self, has_shadow: bool) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
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

    pub(crate) fn set_alpha(&self, alpha: f64) {
        let owner = self.owner;
        let owner_is_main = self.owner_is_main;
        let this = self;
        route_on_owner(owner, owner_is_main, || unsafe {
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

    pub(crate) fn backing_scale_factor(&self) -> f64 {
        let state = self.state.lock();
        state.scale_factor
    }

    pub(crate) fn convert_point_from_backing(&self, point: Point<f64>) -> Point<f64> {
        let scale = self.backing_scale_factor();
        Point::new(point.x / scale, point.y / scale)
    }

    pub(crate) fn convert_point_to_backing(&self, point: Point<f64>) -> Point<f64> {
        let scale = self.backing_scale_factor();
        Point::new(point.x * scale, point.y * scale)
    }
}

pub(crate) mod tiling {
    //! macOS Window Tiling API (Sequoia 15+)
    //!
    //! This module provides support for macOS's native window tiling feature
    //! introduced in macOS Sequoia 15. Window tiling allows apps to suggest tile
    //! layouts for multi-window workflows, similar to Windows Snap Layouts.
    //!
    //! # Platform Requirements
    //!
    //! - macOS 15.0 (Sequoia) or later
    //! - NSWindow with `allowsTiling` enabled
    //!
    //! # Usage
    //!
    //! ```rust,ignore
    //! use flui_native::macos::{WindowTiling, TilingConfiguration, TilePosition};
    //!
    //! // Enable tiling for a window
    //! let config = TilingConfiguration::new()
    //!     .with_primary_position(TilePosition::Left)
    //!     .with_split_ratio(0.5);
    //!
    //! window.enable_tiling(config)?;
    //! ```

    use flui_foundation::geometry::{Rect, Size};

    /// A [`TilingConfiguration`] combines a [`TilePosition`] and [`TilingLayout`]
    /// that are not compatible with each other.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
    #[error("{position:?} is not a valid primary position for {layout:?}")]
    pub struct TilingError {
        /// The incompatible position that was set.
        pub position: TilePosition,
        /// The layout it was paired with.
        pub layout: TilingLayout,
    }

    // ============================================================================
    // Tiling Configuration
    // ============================================================================

    /// Window tiling configuration for macOS Sequoia 15+.
    ///
    /// Configures how windows should be tiled when using macOS's native tiling
    /// feature.
    #[derive(Debug, Clone, PartialEq)]
    pub struct TilingConfiguration {
        /// Primary window tile position.
        pub primary_position: TilePosition,

        /// Split ratio between primary and secondary windows (0.0-1.0).
        ///
        /// - 0.5 = Equal split
        /// - 0.33 = Primary takes 1/3, secondary takes 2/3
        /// - 0.67 = Primary takes 2/3, secondary takes 1/3
        pub split_ratio: f64,

        /// Tiling layout mode.
        pub layout: TilingLayout,

        /// Whether to show resize handle between tiles.
        pub show_resize_handle: bool,

        /// Minimum size for tiled windows.
        pub min_tile_size: Size<f64>,
    }

    impl TilingConfiguration {
        /// Create a new tiling configuration with default settings.
        ///
        /// Defaults:
        /// - Primary position: Left
        /// - Split ratio: 0.5 (equal split)
        /// - Layout: SideBySide
        /// - Show resize handle: true
        /// - Min tile size: 400x300 pixels
        pub fn new() -> Self {
            Self {
                primary_position: TilePosition::Left,
                split_ratio: 0.5,
                layout: TilingLayout::SideBySide,
                show_resize_handle: true,
                min_tile_size: Size::new(400.0, 300.0),
            }
        }

        /// Set the primary window position.
        pub fn with_primary_position(mut self, position: TilePosition) -> Self {
            self.primary_position = position;
            self
        }

        /// Set the split ratio (clamped to 0.2-0.8).
        pub fn with_split_ratio(mut self, ratio: f64) -> Self {
            self.split_ratio = ratio.clamp(0.2, 0.8);
            self
        }

        /// Set the tiling layout mode.
        pub fn with_layout(mut self, layout: TilingLayout) -> Self {
            self.layout = layout;
            self
        }

        /// Enable or disable the resize handle.
        pub fn with_resize_handle(mut self, show: bool) -> Self {
            self.show_resize_handle = show;
            self
        }

        /// Set minimum tile size.
        pub fn with_min_size(mut self, size: Size<f64>) -> Self {
            self.min_tile_size = size;
            self
        }

        /// Calculate tile rectangles for the given screen size.
        ///
        /// Returns `(primary_rect, secondary_rect)`.
        ///
        /// # Errors
        ///
        /// Returns [`TilingError`] if `self.primary_position` is not one of the
        /// positions [`TilePosition::is_valid_for_layout`] accepts for
        /// `self.layout` (e.g. `TopLeft` with a `SideBySide` layout) — the
        /// builder methods that set `layout`/`primary_position` independently do
        /// not cross-validate them, so this can only be caught here.
        pub fn calculate_tiles(
            &self,
            screen_size: Size<f64>,
        ) -> Result<(Rect<f64>, Rect<f64>), TilingError> {
            if !self.primary_position.is_valid_for_layout(self.layout) {
                return Err(TilingError {
                    position: self.primary_position,
                    layout: self.layout,
                });
            }

            let width = screen_size.width;
            let height = screen_size.height;

            let tiles = match self.layout {
                TilingLayout::SideBySide => {
                    let split_x = width * self.split_ratio;

                    match self.primary_position {
                        TilePosition::Left => (
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(0.0, 0.0),
                                Size::new(split_x, height),
                            ),
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(split_x, 0.0),
                                Size::new(width - split_x, height),
                            ),
                        ),
                        TilePosition::Right => (
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(width - split_x, 0.0),
                                Size::new(split_x, height),
                            ),
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(0.0, 0.0),
                                Size::new(width - split_x, height),
                            ),
                        ),
                        // Every other `TilePosition` was already rejected by the
                        // `is_valid_for_layout` guard above.
                        _ => unreachable!(
                            "BUG: is_valid_for_layout guard above did not match calculate_tiles's own SideBySide arm"
                        ),
                    }
                }

                TilingLayout::TopBottom => {
                    let split_y = height * self.split_ratio;

                    match self.primary_position {
                        TilePosition::Top => (
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(0.0, 0.0),
                                Size::new(width, split_y),
                            ),
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(0.0, split_y),
                                Size::new(width, height - split_y),
                            ),
                        ),
                        TilePosition::Bottom => (
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(0.0, height - split_y),
                                Size::new(width, split_y),
                            ),
                            Rect::from_origin_size(
                                flui_foundation::geometry::Point::new(0.0, 0.0),
                                Size::new(width, height - split_y),
                            ),
                        ),
                        _ => unreachable!(
                            "BUG: is_valid_for_layout guard above did not match calculate_tiles's own TopBottom arm"
                        ),
                    }
                }

                TilingLayout::Quarters => {
                    // Split screen into 4 equal quadrants
                    let half_width = width * 0.5;
                    let half_height = height * 0.5;

                    let primary = match self.primary_position {
                        TilePosition::TopLeft => Rect::from_origin_size(
                            flui_foundation::geometry::Point::new(0.0, 0.0),
                            Size::new(half_width, half_height),
                        ),
                        TilePosition::TopRight => Rect::from_origin_size(
                            flui_foundation::geometry::Point::new(half_width, 0.0),
                            Size::new(half_width, half_height),
                        ),
                        TilePosition::BottomLeft => Rect::from_origin_size(
                            flui_foundation::geometry::Point::new(0.0, half_height),
                            Size::new(half_width, half_height),
                        ),
                        TilePosition::BottomRight => Rect::from_origin_size(
                            flui_foundation::geometry::Point::new(half_width, half_height),
                            Size::new(half_width, half_height),
                        ),
                        _ => unreachable!(
                            "BUG: is_valid_for_layout guard above did not match calculate_tiles's own Quarters arm"
                        ),
                    };

                    // Secondary takes the rest (3 quadrants)
                    let secondary = Rect::from_origin_size(
                        flui_foundation::geometry::Point::new(0.0, 0.0),
                        screen_size,
                    );

                    (primary, secondary)
                }
            };

            Ok(tiles)
        }

        /// Check if tiling is available on this macOS version.
        pub fn is_available() -> bool {
            #[cfg(target_os = "macos")]
            {
                // TODO: Check macOS version >= 15.0 (Sequoia)
                // For now, return false as it requires macOS 15+
                false
            }

            #[cfg(not(target_os = "macos"))]
            {
                false
            }
        }
    }

    impl Default for TilingConfiguration {
        fn default() -> Self {
            Self::new()
        }
    }

    // ============================================================================
    // Tile Position
    // ============================================================================

    /// Position of a tiled window.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum TilePosition {
        /// Left half of screen (SideBySide layout).
        Left,
        /// Right half of screen (SideBySide layout).
        Right,
        /// Top half of screen (TopBottom layout).
        Top,
        /// Bottom half of screen (TopBottom layout).
        Bottom,
        /// Top-left quadrant (Quarters layout).
        TopLeft,
        /// Top-right quadrant (Quarters layout).
        TopRight,
        /// Bottom-left quadrant (Quarters layout).
        BottomLeft,
        /// Bottom-right quadrant (Quarters layout).
        BottomRight,
    }

    impl TilePosition {
        /// Get a human-readable description of this position.
        pub fn description(&self) -> &str {
            match self {
                TilePosition::Left => "Left half",
                TilePosition::Right => "Right half",
                TilePosition::Top => "Top half",
                TilePosition::Bottom => "Bottom half",
                TilePosition::TopLeft => "Top-left quadrant",
                TilePosition::TopRight => "Top-right quadrant",
                TilePosition::BottomLeft => "Bottom-left quadrant",
                TilePosition::BottomRight => "Bottom-right quadrant",
            }
        }

        /// Check if this position is valid for the given layout.
        pub fn is_valid_for_layout(&self, layout: TilingLayout) -> bool {
            match layout {
                TilingLayout::SideBySide => {
                    matches!(self, TilePosition::Left | TilePosition::Right)
                }
                TilingLayout::TopBottom => matches!(self, TilePosition::Top | TilePosition::Bottom),
                TilingLayout::Quarters => matches!(
                    self,
                    TilePosition::TopLeft
                        | TilePosition::TopRight
                        | TilePosition::BottomLeft
                        | TilePosition::BottomRight
                ),
            }
        }
    }

    // ============================================================================
    // Tiling Layout
    // ============================================================================

    /// Window tiling layout mode.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub enum TilingLayout {
        /// Side-by-side layout (left/right split).
        SideBySide,
        /// Top/bottom layout (horizontal split).
        TopBottom,
        /// Quarters layout (4 quadrants).
        Quarters,
    }

    impl TilingLayout {
        /// Get a human-readable description of this layout.
        pub fn description(&self) -> &str {
            match self {
                TilingLayout::SideBySide => "Side by side",
                TilingLayout::TopBottom => "Top and bottom",
                TilingLayout::Quarters => "Four quadrants",
            }
        }

        /// Get valid tile positions for this layout.
        pub fn valid_positions(&self) -> &[TilePosition] {
            match self {
                TilingLayout::SideBySide => &[TilePosition::Left, TilePosition::Right],
                TilingLayout::TopBottom => &[TilePosition::Top, TilePosition::Bottom],
                TilingLayout::Quarters => &[
                    TilePosition::TopLeft,
                    TilePosition::TopRight,
                    TilePosition::BottomLeft,
                    TilePosition::BottomRight,
                ],
            }
        }
    }

    // ============================================================================
    // Window Tiling State
    // ============================================================================

    /// Current tiling state of a window.
    #[derive(Debug, Clone, PartialEq)]
    pub struct TilingState {
        /// Whether tiling is enabled for this window.
        pub enabled: bool,

        /// Current tiling configuration.
        pub configuration: Option<TilingConfiguration>,

        /// Current tile position (if tiled).
        pub current_position: Option<TilePosition>,

        /// Companion window ID (the other tiled window).
        pub companion_window: Option<u64>,
    }

    impl TilingState {
        /// Create a new tiling state (disabled).
        pub fn new() -> Self {
            Self {
                enabled: false,
                configuration: None,
                current_position: None,
                companion_window: None,
            }
        }

        /// Check if the window is currently tiled.
        pub fn is_tiled(&self) -> bool {
            self.enabled && self.current_position.is_some()
        }

        /// Check if the window has a companion (is tiled with another window).
        pub fn has_companion(&self) -> bool {
            self.companion_window.is_some()
        }
    }

    impl Default for TilingState {
        fn default() -> Self {
            Self::new()
        }
    }

    // ============================================================================
    // Tests
    // ============================================================================
}
