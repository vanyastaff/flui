//! macOS Window Tiling API (Sequoia 15+)
//!
//! This module provides support for macOS's native window tiling feature introduced
//! in macOS Sequoia 15. Window tiling allows apps to suggest tile layouts for
//! multi-window workflows, similar to Windows Snap Layouts.
//!
//! # Platform Requirements
//!
//! - macOS 15.0 (Sequoia) or later
//! - NSWindow with `allowsTiling` enabled
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::macos::{WindowTiling, TilingConfiguration, TilePosition};
//!
//! // Enable tiling for a window
//! let config = TilingConfiguration::new()
//!     .with_primary_position(TilePosition::Left)
//!     .with_split_ratio(0.5);
//!
//! window.enable_tiling(config)?;
//! ```

use flui_types::geometry::{Rect, Size};
use flui_types::Pixels;

// ============================================================================
// Tiling Configuration
// ============================================================================

/// Window tiling configuration for macOS Sequoia 15+.
///
/// Configures how windows should be tiled when using macOS's native tiling feature.
#[derive(Debug, Clone, PartialEq)]
pub struct TilingConfiguration {
    /// Primary window tile position.
    pub primary_position: TilePosition,

    /// Split ratio between primary and secondary windows (0.0-1.0).
    ///
    /// - 0.5 = Equal split
    /// - 0.33 = Primary takes 1/3, secondary takes 2/3
    /// - 0.67 = Primary takes 2/3, secondary takes 1/3
    pub split_ratio: f32,

    /// Tiling layout mode.
    pub layout: TilingLayout,

    /// Whether to show resize handle between tiles.
    pub show_resize_handle: bool,

    /// Minimum size for tiled windows.
    pub min_tile_size: Size<Pixels>,
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
            min_tile_size: Size::new(Pixels(400.0), Pixels(300.0)),
        }
    }

    /// Set the primary window position.
    pub fn with_primary_position(mut self, position: TilePosition) -> Self {
        self.primary_position = position;
        self
    }

    /// Set the split ratio (clamped to 0.2-0.8).
    pub fn with_split_ratio(mut self, ratio: f32) -> Self {
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
    pub fn with_min_size(mut self, size: Size<Pixels>) -> Self {
        self.min_tile_size = size;
        self
    }

    /// Calculate tile rectangles for the given screen size.
    ///
    /// Returns (primary_rect, secondary_rect).
    pub fn calculate_tiles(&self, screen_size: Size<Pixels>) -> (Rect<Pixels>, Rect<Pixels>) {
        let width = screen_size.width;
        let height = screen_size.height;

        match self.layout {
            TilingLayout::SideBySide => {
                let split_x = width * Pixels(self.split_ratio);

                let (primary, secondary) = match self.primary_position {
                    TilePosition::Left => (
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(Pixels(0.0), Pixels(0.0)),
                            Size::new(split_x, height),
                        ),
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(split_x, Pixels(0.0)),
                            Size::new(width - split_x, height),
                        ),
                    ),
                    TilePosition::Right => (
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(width - split_x, Pixels(0.0)),
                            Size::new(split_x, height),
                        ),
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(Pixels(0.0), Pixels(0.0)),
                            Size::new(width - split_x, height),
                        ),
                    ),
                    _ => panic!("Invalid position for SideBySide layout"),
                };

                (primary, secondary)
            }

            TilingLayout::TopBottom => {
                let split_y = height * Pixels(self.split_ratio);

                let (primary, secondary) = match self.primary_position {
                    TilePosition::Top => (
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(Pixels(0.0), Pixels(0.0)),
                            Size::new(width, split_y),
                        ),
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(Pixels(0.0), split_y),
                            Size::new(width, height - split_y),
                        ),
                    ),
                    TilePosition::Bottom => (
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(Pixels(0.0), height - split_y),
                            Size::new(width, split_y),
                        ),
                        Rect::from_origin_and_size(
                            flui_types::geometry::Point::new(Pixels(0.0), Pixels(0.0)),
                            Size::new(width, height - split_y),
                        ),
                    ),
                    _ => panic!("Invalid position for TopBottom layout"),
                };

                (primary, secondary)
            }

            TilingLayout::Quarters => {
                // Split screen into 4 equal quadrants
                let half_width = width * Pixels(0.5);
                let half_height = height * Pixels(0.5);

                let primary = match self.primary_position {
                    TilePosition::TopLeft => Rect::from_origin_and_size(
                        flui_types::geometry::Point::new(Pixels(0.0), Pixels(0.0)),
                        Size::new(half_width, half_height),
                    ),
                    TilePosition::TopRight => Rect::from_origin_and_size(
                        flui_types::geometry::Point::new(half_width, Pixels(0.0)),
                        Size::new(half_width, half_height),
                    ),
                    TilePosition::BottomLeft => Rect::from_origin_and_size(
                        flui_types::geometry::Point::new(Pixels(0.0), half_height),
                        Size::new(half_width, half_height),
                    ),
                    TilePosition::BottomRight => Rect::from_origin_and_size(
                        flui_types::geometry::Point::new(half_width, half_height),
                        Size::new(half_width, half_height),
                    ),
                    _ => panic!("Invalid position for Quarters layout"),
                };

                // Secondary takes the rest (3 quadrants)
                let secondary = Rect::from_origin_and_size(
                    flui_types::geometry::Point::new(Pixels(0.0), Pixels(0.0)),
                    screen_size,
                );

                (primary, secondary)
            }
        }
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
            TilingLayout::SideBySide => matches!(self, TilePosition::Left | TilePosition::Right),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tiling_configuration_default() {
        let config = TilingConfiguration::new();
        assert_eq!(config.primary_position, TilePosition::Left);
        assert_eq!(config.split_ratio, 0.5);
        assert_eq!(config.layout, TilingLayout::SideBySide);
        assert!(config.show_resize_handle);
    }

    #[test]
    fn test_split_ratio_clamping() {
        let config = TilingConfiguration::new()
            .with_split_ratio(0.1) // Too small
            .with_split_ratio(0.9); // Too large

        assert_eq!(config.split_ratio, 0.8); // Clamped to max
    }

    #[test]
    fn test_calculate_tiles_side_by_side() {
        let config = TilingConfiguration::new()
            .with_primary_position(TilePosition::Left)
            .with_split_ratio(0.5);

        let screen = Size::new(Pixels(1920.0), Pixels(1080.0));
        let (primary, secondary) = config.calculate_tiles(screen);

        assert_eq!(primary.width(), Pixels(960.0));
        assert_eq!(secondary.width(), Pixels(960.0));
        assert_eq!(primary.height(), Pixels(1080.0));
        assert_eq!(secondary.height(), Pixels(1080.0));
    }

    #[test]
    fn test_calculate_tiles_top_bottom() {
        let config = TilingConfiguration::new()
            .with_layout(TilingLayout::TopBottom)
            .with_primary_position(TilePosition::Top)
            .with_split_ratio(0.6);

        let screen = Size::new(Pixels(1920.0), Pixels(1080.0));
        let (primary, secondary) = config.calculate_tiles(screen);

        assert_eq!(primary.height(), Pixels(648.0)); // 60% of 1080
        assert_eq!(secondary.height(), Pixels(432.0)); // 40% of 1080
    }

    #[test]
    fn test_tile_position_validation() {
        assert!(TilePosition::Left.is_valid_for_layout(TilingLayout::SideBySide));
        assert!(!TilePosition::Left.is_valid_for_layout(TilingLayout::TopBottom));
        assert!(TilePosition::TopLeft.is_valid_for_layout(TilingLayout::Quarters));
    }

    #[test]
    fn test_tiling_state() {
        let mut state = TilingState::new();
        assert!(!state.is_tiled());
        assert!(!state.has_companion());

        state.enabled = true;
        state.current_position = Some(TilePosition::Left);
        assert!(state.is_tiled());

        state.companion_window = Some(12345);
        assert!(state.has_companion());
    }

    #[test]
    fn test_tile_position_description() {
        assert_eq!(TilePosition::Left.description(), "Left half");
        assert_eq!(TilePosition::TopRight.description(), "Top-right quadrant");
    }

    #[test]
    fn test_tiling_layout_valid_positions() {
        let positions = TilingLayout::SideBySide.valid_positions();
        assert_eq!(positions.len(), 2);
        assert!(positions.contains(&TilePosition::Left));
        assert!(positions.contains(&TilePosition::Right));

        let quarters = TilingLayout::Quarters.valid_positions();
        assert_eq!(quarters.len(), 4);
    }
}
