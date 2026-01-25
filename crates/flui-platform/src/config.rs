//! Platform configuration types
//!
//! This module provides configuration structures for customizing platform behavior,
//! including window management, hotkeys, and display selection.

/// Configuration for window behavior across platforms
///
/// Controls various aspects of window management including hotkeys, fullscreen
/// behavior, and resize handling. Each platform can interpret these settings
/// appropriately for its window system.
///
/// # Examples
///
/// ```rust
/// use flui_platform::{WindowConfiguration, FullscreenMonitor};
///
/// // Default configuration (F11 toggles fullscreen on current monitor)
/// let default = WindowConfiguration::default();
///
/// // Disable fullscreen hotkey
/// let no_hotkey = WindowConfiguration {
///     fullscreen_hotkey: None,
///     ..Default::default()
/// };
///
/// // Fullscreen on primary monitor with debounced resize
/// let custom = WindowConfiguration {
///     fullscreen_hotkey: Some(0x7A), // F11
///     resize_debounce_ms: Some(16),  // ~60fps
///     fullscreen_monitor: FullscreenMonitor::Primary,
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowConfiguration {
    /// Virtual key code for fullscreen toggle hotkey.
    ///
    /// Platform-specific virtual key codes:
    /// - Windows: VK_* constants (e.g., 0x7A for F11, 0x73 for F4)
    /// - macOS: kVK_* constants (e.g., 0x67 for F11)
    /// - Linux/X11: XK_* keysyms (e.g., 0xFFC8 for F11)
    ///
    /// Set to `None` to disable fullscreen hotkey.
    pub fullscreen_hotkey: Option<u16>,

    /// Debounce time for resize events in milliseconds.
    ///
    /// When set, the platform will batch multiple rapid resize events and
    /// dispatch a single event after the debounce period. This prevents
    /// excessive redraws during interactive window resizing.
    ///
    /// Recommended values:
    /// - `None` - No debouncing (immediate resize events)
    /// - `Some(16)` - ~60fps update rate
    /// - `Some(33)` - ~30fps update rate
    /// - `Some(50)` - Conservative debouncing
    ///
    /// Set to `None` for immediate resize events (default).
    pub resize_debounce_ms: Option<u64>,

    /// Which monitor to use when entering fullscreen mode.
    ///
    /// Controls the display selection behavior when transitioning to fullscreen.
    /// Different strategies work better for different applications:
    /// - Games typically use `Current` (fullscreen where window is)
    /// - Presentations often use `Primary` (fullscreen on main display)
    /// - Multi-monitor tools may use `Index(n)` (specific display)
    pub fullscreen_monitor: FullscreenMonitor,
}

impl Default for WindowConfiguration {
    fn default() -> Self {
        WindowConfiguration {
            // F11 is conventional fullscreen hotkey across platforms
            fullscreen_hotkey: Some(0x7A),
            // No debouncing by default - immediate resize feedback
            resize_debounce_ms: None,
            // Fullscreen on the monitor containing the window
            fullscreen_monitor: FullscreenMonitor::Current,
        }
    }
}

/// Strategy for selecting which monitor to use for fullscreen mode
///
/// Different applications have different requirements for fullscreen behavior:
///
/// # Strategies
///
/// - **Current**: Use the monitor that currently contains the most of the window.
///   Best for games and immersive applications where users position windows deliberately.
///
/// - **Primary**: Always use the primary/main display (as configured in OS settings).
///   Best for presentations, video players, and single-monitor workflows.
///
/// - **Index**: Use a specific monitor by index (0-based).
///   Best for multi-monitor setups where the application needs control over display selection.
///   Note: If the index is out of range, falls back to `Current`.
///
/// # Examples
///
/// ```rust
/// use flui_platform::FullscreenMonitor;
///
/// // Fullscreen on whatever monitor has the window
/// let current = FullscreenMonitor::Current;
///
/// // Always fullscreen on primary display
/// let primary = FullscreenMonitor::Primary;
///
/// // Fullscreen on second monitor (useful for presentations)
/// let second = FullscreenMonitor::Index(1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FullscreenMonitor {
    /// Use the monitor that currently contains the most of the window
    Current,

    /// Use the primary display (as configured in OS display settings)
    Primary,

    /// Use a specific monitor by index (0-based)
    ///
    /// If the index is out of range, falls back to `Current` behavior.
    Index(usize),
}

impl WindowConfiguration {
    /// Create a new configuration with fullscreen hotkey disabled
    ///
    /// Useful for applications that implement custom fullscreen controls
    /// or want to prevent accidental fullscreen toggling.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_platform::WindowConfiguration;
    ///
    /// let config = WindowConfiguration::no_hotkey();
    /// assert_eq!(config.fullscreen_hotkey, None);
    /// ```
    pub fn no_hotkey() -> Self {
        WindowConfiguration {
            fullscreen_hotkey: None,
            ..Default::default()
        }
    }

    /// Create a new configuration with debounced resize events
    ///
    /// # Arguments
    ///
    /// * `debounce_ms` - Debounce time in milliseconds
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_platform::WindowConfiguration;
    ///
    /// // 60fps resize updates
    /// let config = WindowConfiguration::with_resize_debounce(16);
    /// ```
    pub fn with_resize_debounce(debounce_ms: u64) -> Self {
        WindowConfiguration {
            resize_debounce_ms: Some(debounce_ms),
            ..Default::default()
        }
    }

    /// Create a new configuration targeting a specific monitor for fullscreen
    ///
    /// # Arguments
    ///
    /// * `monitor` - The fullscreen monitor strategy
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_platform::{WindowConfiguration, FullscreenMonitor};
    ///
    /// // Always fullscreen on primary display
    /// let config = WindowConfiguration::with_fullscreen_monitor(FullscreenMonitor::Primary);
    /// ```
    pub fn with_fullscreen_monitor(monitor: FullscreenMonitor) -> Self {
        WindowConfiguration {
            fullscreen_monitor: monitor,
            ..Default::default()
        }
    }

    /// Check if fullscreen hotkey is enabled
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_platform::WindowConfiguration;
    ///
    /// let default = WindowConfiguration::default();
    /// assert!(default.has_fullscreen_hotkey());
    ///
    /// let disabled = WindowConfiguration::no_hotkey();
    /// assert!(!disabled.has_fullscreen_hotkey());
    /// ```
    pub fn has_fullscreen_hotkey(&self) -> bool {
        self.fullscreen_hotkey.is_some()
    }

    /// Get the fullscreen hotkey virtual key code, if enabled
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_platform::WindowConfiguration;
    ///
    /// let config = WindowConfiguration::default();
    /// assert_eq!(config.get_fullscreen_hotkey(), Some(0x7A)); // F11
    /// ```
    pub fn get_fullscreen_hotkey(&self) -> Option<u16> {
        self.fullscreen_hotkey
    }

    /// Check if resize debouncing is enabled
    ///
    /// # Examples
    ///
    /// ```rust
    /// use flui_platform::WindowConfiguration;
    ///
    /// let default = WindowConfiguration::default();
    /// assert!(!default.has_resize_debounce());
    ///
    /// let debounced = WindowConfiguration::with_resize_debounce(16);
    /// assert!(debounced.has_resize_debounce());
    /// ```
    pub fn has_resize_debounce(&self) -> bool {
        self.resize_debounce_ms.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_configuration() {
        let config = WindowConfiguration::default();
        assert_eq!(config.fullscreen_hotkey, Some(0x7A)); // F11
        assert_eq!(config.resize_debounce_ms, None);
        assert_eq!(config.fullscreen_monitor, FullscreenMonitor::Current);
    }

    #[test]
    fn test_no_hotkey() {
        let config = WindowConfiguration::no_hotkey();
        assert_eq!(config.fullscreen_hotkey, None);
        assert!(!config.has_fullscreen_hotkey());
    }

    #[test]
    fn test_with_resize_debounce() {
        let config = WindowConfiguration::with_resize_debounce(16);
        assert_eq!(config.resize_debounce_ms, Some(16));
        assert!(config.has_resize_debounce());
    }

    #[test]
    fn test_with_fullscreen_monitor() {
        let config = WindowConfiguration::with_fullscreen_monitor(FullscreenMonitor::Primary);
        assert_eq!(config.fullscreen_monitor, FullscreenMonitor::Primary);
    }

    #[test]
    fn test_fullscreen_monitor_strategies() {
        assert_eq!(FullscreenMonitor::Current, FullscreenMonitor::Current);
        assert_eq!(FullscreenMonitor::Primary, FullscreenMonitor::Primary);
        assert_eq!(FullscreenMonitor::Index(0), FullscreenMonitor::Index(0));
        assert_ne!(FullscreenMonitor::Current, FullscreenMonitor::Primary);
    }
}
