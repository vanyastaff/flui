//! Window and scroll event types
//!
//! This module provides types for window-level events and scroll input.

use crate::Offset;

use super::keyboard::KeyModifiers;

/// Scroll delta
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum ScrollDelta {
    /// Scroll by lines (e.g., mouse wheel clicks)
    Lines {
        /// Horizontal lines
        x: f32,
        /// Vertical lines
        y: f32,
    },

    /// Scroll by pixels (e.g., touchpad)
    Pixels {
        /// Horizontal pixels
        x: f32,
        /// Vertical pixels
        y: f32,
    },
}

/// Scroll event data
#[derive(Debug, Clone)]
pub struct ScrollEventData {
    /// Position where scroll occurred
    pub position: Offset,

    /// Scroll delta
    pub delta: ScrollDelta,

    /// Current modifier keys state
    pub modifiers: KeyModifiers,
}

impl ScrollEventData {
    /// Create new scroll event data
    pub fn new(position: Offset, delta: ScrollDelta) -> Self {
        Self {
            position,
            delta,
            modifiers: KeyModifiers::new(),
        }
    }
}

/// System theme
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Theme {
    /// Light theme
    #[default]
    Light,
    /// Dark theme
    Dark,
}

impl Theme {
    /// Check if this is the dark theme
    pub fn is_dark(&self) -> bool {
        matches!(self, Theme::Dark)
    }

    /// Check if this is the light theme
    pub fn is_light(&self) -> bool {
        matches!(self, Theme::Light)
    }
}

/// Window event types
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum WindowEvent {
    /// Window was resized
    Resized {
        /// New width in pixels
        width: u32,
        /// New height in pixels
        height: u32,
    },

    /// Window gained or lost focus
    FocusChanged {
        /// Whether window is focused
        focused: bool,
    },

    /// Window visibility changed (minimized/restored)
    VisibilityChanged {
        /// Whether window is visible
        visible: bool,
    },

    /// Window close was requested
    CloseRequested,

    /// Window scale factor changed (DPI change)
    ScaleChanged {
        /// New scale factor
        scale: f64,
    },

    /// System theme changed (dark/light mode)
    ThemeChanged {
        /// New theme
        theme: Theme,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme() {
        assert!(Theme::Dark.is_dark());
        assert!(!Theme::Dark.is_light());
        assert!(Theme::Light.is_light());
        assert!(!Theme::Light.is_dark());
    }

    #[test]
    fn test_scroll_event_data() {
        let data = ScrollEventData::new(
            Offset::new(100.0, 200.0),
            ScrollDelta::Lines { x: 0.0, y: 1.0 },
        );
        assert_eq!(data.position, Offset::new(100.0, 200.0));
    }
}
