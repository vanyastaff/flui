//! macOS-specific window extensions
//!
//! This module provides macOS-specific features that extend the core `Window` trait.
//! These features are only available on macOS and use Apple's AppKit APIs.
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::Window;
//! use flui_platform::macos::MacOSWindowExt;
//!
//! // Use cross-platform API
//! window.set_title("My App");
//!
//! // Use macOS-specific extensions
//! window.set_liquid_glass(LiquidGlassMaterial::Standard);
//! window.enable_tiling(TilingConfiguration::new());
//! ```

use super::liquid_glass::{LiquidGlassConfig, LiquidGlassMaterial};
use super::window_tiling::TilingConfiguration;

// ============================================================================
// macOS Window Extension Trait
// ============================================================================

/// macOS-specific window extensions.
///
/// This trait provides access to macOS-specific features that are not part
/// of the cross-platform `Window` trait.
///
/// # Platform Availability
///
/// - **Liquid Glass Materials:** macOS 14.0+ (Sonoma), full support in macOS 26+ (Tahoe)
/// - **Window Tiling:** macOS 15.0+ (Sequoia)
/// - **Tabbed Windows:** macOS 10.12+ (Sierra)
/// - **Full Screen Transitions:** macOS 10.7+ (Lion)
#[cfg(target_os = "macos")]
pub trait MacOSWindowExt {
    /// Apply Liquid Glass material effect to the window.
    ///
    /// Liquid Glass is macOS Tahoe 26's new translucent material system.
    ///
    /// # Platform Requirements
    ///
    /// - macOS 14.0+ for basic vibrancy effects
    /// - macOS 26.0+ (Tahoe) for full Liquid Glass materials
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// window.set_liquid_glass(LiquidGlassMaterial::Standard);
    /// ```
    fn set_liquid_glass(&mut self, material: LiquidGlassMaterial);

    /// Apply custom Liquid Glass configuration.
    ///
    /// Allows fine-tuning blur radius, tint color, and blending mode.
    fn set_liquid_glass_config(&mut self, config: LiquidGlassConfig);

    /// Remove Liquid Glass effect and restore opaque window.
    fn clear_liquid_glass(&mut self);

    /// Enable window tiling with the specified configuration.
    ///
    /// Window tiling allows the window to suggest tile layouts for multi-window workflows,
    /// similar to Windows Snap Layouts.
    ///
    /// # Platform Requirements
    ///
    /// - macOS 15.0+ (Sequoia)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use flui_platform::macos::{TilingConfiguration, TilePosition};
    ///
    /// let config = TilingConfiguration::new()
    ///     .with_primary_position(TilePosition::Left)
    ///     .with_split_ratio(0.5);
    ///
    /// window.enable_tiling(config);
    /// ```
    fn enable_tiling(&mut self, config: TilingConfiguration);

    /// Disable window tiling.
    fn disable_tiling(&mut self);

    /// Check if window tiling is enabled.
    fn is_tiling_enabled(&self) -> bool;

    /// Enable tabbed window mode.
    ///
    /// When enabled, multiple windows can be grouped into tabs within a single window frame.
    ///
    /// # Platform Requirements
    ///
    /// - macOS 10.12+ (Sierra)
    fn enable_tabbing(&mut self);

    /// Disable tabbed window mode.
    fn disable_tabbing(&mut self);

    /// Add this window to a tab group with another window.
    ///
    /// # Parameters
    ///
    /// - `other_window_id`: ID of the window to tab with
    fn add_tab_to_window(&mut self, other_window_id: u64);

    /// Toggle native fullscreen mode with macOS animation.
    ///
    /// This uses the native macOS fullscreen API with the slide-in animation
    /// and creates a new Space (virtual desktop).
    ///
    /// # Platform Requirements
    ///
    /// - macOS 10.7+ (Lion)
    fn toggle_native_fullscreen(&mut self);

    /// Set the window level (z-ordering).
    ///
    /// Controls whether the window floats above other windows, appears as a modal, etc.
    fn set_window_level(&mut self, level: MacOSWindowLevel);

    /// Get the window level.
    fn window_level(&self) -> MacOSWindowLevel;

    /// Set the window's collection behavior.
    ///
    /// Controls how the window behaves with Spaces, Exposé, and fullscreen modes.
    fn set_collection_behavior(&mut self, behavior: MacOSCollectionBehavior);

    /// Enable/disable window shadow.
    fn set_has_shadow(&mut self, has_shadow: bool);

    /// Set window alpha (transparency).
    ///
    /// # Parameters
    ///
    /// - `alpha`: 0.0 (fully transparent) to 1.0 (fully opaque)
    fn set_alpha(&mut self, alpha: f32);

    /// Get the window's backing scale factor (1.0 or 2.0 for Retina).
    fn backing_scale_factor(&self) -> f32;

    /// Convert point from backing (pixel) coordinates to window coordinates.
    fn convert_point_from_backing(&self, point: flui_types::geometry::Point<flui_types::Pixels>)
        -> flui_types::geometry::Point<flui_types::Pixels>;

    /// Convert point from window coordinates to backing (pixel) coordinates.
    fn convert_point_to_backing(&self, point: flui_types::geometry::Point<flui_types::Pixels>)
        -> flui_types::geometry::Point<flui_types::Pixels>;
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
            MacOSWindowLevel::Floating => 3,
            MacOSWindowLevel::TornOffMenu => 3,
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
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_level_values() {
        assert_eq!(MacOSWindowLevel::Normal.to_ns_value(), 0);
        assert_eq!(MacOSWindowLevel::Floating.to_ns_value(), 3);
        assert_eq!(MacOSWindowLevel::ModalPanel.to_ns_value(), 8);
        assert_eq!(MacOSWindowLevel::Status.to_ns_value(), 25);
    }

    #[test]
    fn test_collection_behavior_bits() {
        let behavior = MacOSCollectionBehavior::CAN_FULLSCREEN
            .with(MacOSCollectionBehavior::MANAGED);

        assert_eq!(behavior.bits(), (1 << 7) | (1 << 2));
    }

    #[test]
    fn test_collection_behavior_default() {
        let default = MacOSCollectionBehavior::DEFAULT;
        assert_eq!(default.bits(), 0);
    }
}
