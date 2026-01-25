//! macOS Multi-Window Management
//!
//! This module provides centralized management for multiple windows in a macOS application.
//! It handles window lifecycle, coordination, and inter-window communication.
//!
//! # Features
//!
//! - Window registration and tracking
//! - Window focus management
//! - Window cascade positioning
//! - Window grouping and tabs
//! - Inter-window messaging
//!
//! # Usage
//!
//! ```rust,ignore
//! use flui_platform::macos::{WindowManager, WindowOptions};
//!
//! let mut manager = WindowManager::new();
//!
//! // Create a new window
//! let window_id = manager.create_window(WindowOptions::default())?;
//!
//! // Focus the window
//! manager.focus_window(window_id)?;
//!
//! // List all windows
//! let windows = manager.all_windows();
//! ```

use flui_types::geometry::{Point, Size};
use flui_types::Pixels;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

// ============================================================================
// Window Manager
// ============================================================================

/// Centralized window manager for macOS applications.
///
/// Manages the lifecycle and coordination of multiple windows.
pub struct WindowManager {
    /// Map of window ID to window info.
    windows: HashMap<WindowId, WindowInfo>,

    /// Next window ID to assign.
    next_id: u64,

    /// Currently focused window ID.
    focused_window: Option<WindowId>,

    /// Window groups for tabbed windows.
    groups: HashMap<GroupId, Vec<WindowId>>,

    /// Next group ID to assign.
    next_group_id: u64,
}

impl WindowManager {
    /// Create a new window manager.
    pub fn new() -> Self {
        Self {
            windows: HashMap::new(),
            next_id: 1,
            focused_window: None,
            groups: HashMap::new(),
            next_group_id: 1,
        }
    }

    /// Register a new window.
    ///
    /// Returns the assigned window ID.
    pub fn register_window(&mut self, options: WindowOptions) -> WindowId {
        let id = WindowId(self.next_id);
        self.next_id += 1;

        let info = WindowInfo {
            id,
            title: options.title.clone(),
            position: options.position,
            size: options.size,
            visible: options.visible,
            resizable: options.resizable,
            minimizable: options.minimizable,
            closable: options.closable,
            level: options.level,
            group: None,
        };

        self.windows.insert(id, info);

        // Auto-focus if this is the first window
        if self.focused_window.is_none() {
            self.focused_window = Some(id);
        }

        id
    }

    /// Unregister a window.
    pub fn unregister_window(&mut self, id: WindowId) -> bool {
        if let Some(_info) = self.windows.remove(&id) {
            // Update focused window if needed
            if self.focused_window == Some(id) {
                self.focused_window = self.windows.keys().next().copied();
            }

            // Remove from group if applicable
            for group_windows in self.groups.values_mut() {
                group_windows.retain(|&win_id| win_id != id);
            }

            true
        } else {
            false
        }
    }

    /// Get window info.
    pub fn get_window(&self, id: WindowId) -> Option<&WindowInfo> {
        self.windows.get(&id)
    }

    /// Get mutable window info.
    pub fn get_window_mut(&mut self, id: WindowId) -> Option<&mut WindowInfo> {
        self.windows.get_mut(&id)
    }

    /// Get all window IDs.
    pub fn all_windows(&self) -> Vec<WindowId> {
        self.windows.keys().copied().collect()
    }

    /// Get count of windows.
    pub fn window_count(&self) -> usize {
        self.windows.len()
    }

    /// Focus a window.
    pub fn focus_window(&mut self, id: WindowId) -> bool {
        if self.windows.contains_key(&id) {
            self.focused_window = Some(id);
            true
        } else {
            false
        }
    }

    /// Get currently focused window ID.
    pub fn focused_window(&self) -> Option<WindowId> {
        self.focused_window
    }

    /// Calculate cascade position for a new window.
    ///
    /// Cascades windows in a staggered pattern (like macOS does by default).
    pub fn calculate_cascade_position(&self, window_size: Size<Pixels>) -> Point<Pixels> {
        let count = self.window_count();
        let cascade_offset = 28.0; // Standard macOS cascade offset

        let x = 100.0 + (count as f32 * cascade_offset);
        let y = 100.0 + (count as f32 * cascade_offset);

        Point::new(Pixels(x), Pixels(y))
    }

    /// Create a window group (for tabbed windows).
    pub fn create_group(&mut self) -> GroupId {
        let id = GroupId(self.next_group_id);
        self.next_group_id += 1;
        self.groups.insert(id, Vec::new());
        id
    }

    /// Add window to a group.
    pub fn add_to_group(&mut self, window_id: WindowId, group_id: GroupId) -> bool {
        if let Some(group) = self.groups.get_mut(&group_id) {
            if let Some(info) = self.windows.get_mut(&window_id) {
                info.group = Some(group_id);
                group.push(window_id);
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// Get windows in a group.
    pub fn get_group(&self, group_id: GroupId) -> Option<&[WindowId]> {
        self.groups.get(&group_id).map(|v| v.as_slice())
    }

    /// Remove window from its group.
    pub fn remove_from_group(&mut self, window_id: WindowId) -> bool {
        if let Some(info) = self.windows.get_mut(&window_id) {
            if let Some(group_id) = info.group {
                if let Some(group) = self.groups.get_mut(&group_id) {
                    group.retain(|&id| id != window_id);
                    info.group = None;
                    return true;
                }
            }
        }
        false
    }

    /// Find windows by title (partial match).
    pub fn find_by_title(&self, title: &str) -> Vec<WindowId> {
        self.windows
            .iter()
            .filter(|(_, info)| info.title.contains(title))
            .map(|(&id, _)| id)
            .collect()
    }

    /// Get visible windows.
    pub fn visible_windows(&self) -> Vec<WindowId> {
        self.windows
            .iter()
            .filter(|(_, info)| info.visible)
            .map(|(&id, _)| id)
            .collect()
    }
}

impl Default for WindowManager {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Window ID
// ============================================================================

/// Unique identifier for a window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

impl WindowId {
    /// Create a new window ID (for testing).
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw ID value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

// ============================================================================
// Group ID
// ============================================================================

/// Unique identifier for a window group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GroupId(pub u64);

impl GroupId {
    /// Create a new group ID (for testing).
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the raw ID value.
    pub fn as_u64(self) -> u64 {
        self.0
    }
}

// ============================================================================
// Window Info
// ============================================================================

/// Information about a window.
#[derive(Debug, Clone)]
pub struct WindowInfo {
    /// Window ID.
    pub id: WindowId,

    /// Window title.
    pub title: String,

    /// Window position (top-left corner).
    pub position: Point<Pixels>,

    /// Window size.
    pub size: Size<Pixels>,

    /// Whether the window is visible.
    pub visible: bool,

    /// Whether the window can be resized.
    pub resizable: bool,

    /// Whether the window can be minimized.
    pub minimizable: bool,

    /// Whether the window can be closed.
    pub closable: bool,

    /// Window level (for z-ordering).
    pub level: WindowLevel,

    /// Group ID if this window is part of a tabbed group.
    pub group: Option<GroupId>,
}

// ============================================================================
// Window Options
// ============================================================================

/// Options for creating a new window.
#[derive(Debug, Clone)]
pub struct WindowOptions {
    /// Window title.
    pub title: String,

    /// Initial position.
    pub position: Point<Pixels>,

    /// Initial size.
    pub size: Size<Pixels>,

    /// Start visible.
    pub visible: bool,

    /// Allow resizing.
    pub resizable: bool,

    /// Allow minimizing.
    pub minimizable: bool,

    /// Allow closing.
    pub closable: bool,

    /// Window level.
    pub level: WindowLevel,
}

impl WindowOptions {
    /// Create new window options with defaults.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            position: Point::new(Pixels(100.0), Pixels(100.0)),
            size: Size::new(Pixels(800.0), Pixels(600.0)),
            visible: true,
            resizable: true,
            minimizable: true,
            closable: true,
            level: WindowLevel::Normal,
        }
    }

    /// Set window title.
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Set window position.
    pub fn with_position(mut self, position: Point<Pixels>) -> Self {
        self.position = position;
        self
    }

    /// Set window size.
    pub fn with_size(mut self, size: Size<Pixels>) -> Self {
        self.size = size;
        self
    }

    /// Set initial visibility.
    pub fn with_visible(mut self, visible: bool) -> Self {
        self.visible = visible;
        self
    }

    /// Set resizable.
    pub fn with_resizable(mut self, resizable: bool) -> Self {
        self.resizable = resizable;
        self
    }

    /// Set window level.
    pub fn with_level(mut self, level: WindowLevel) -> Self {
        self.level = level;
        self
    }
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self::new("FLUI Window")
    }
}

// ============================================================================
// Window Level
// ============================================================================

/// Window z-ordering level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowLevel {
    /// Normal window level.
    Normal,

    /// Floating window (stays above normal windows).
    Floating,

    /// Modal panel level.
    Modal,

    /// Popover level.
    Popover,

    /// Screen saver level.
    ScreenSaver,
}

impl WindowLevel {
    /// Get macOS NSWindowLevel value.
    #[cfg(target_os = "macos")]
    pub fn to_ns_window_level(self) -> isize {
        match self {
            WindowLevel::Normal => 0,        // NSNormalWindowLevel
            WindowLevel::Floating => 3,      // NSFloatingWindowLevel
            WindowLevel::Modal => 8,         // NSModalPanelWindowLevel
            WindowLevel::Popover => 101,     // NSPopUpMenuWindowLevel
            WindowLevel::ScreenSaver => 1000, // NSScreenSaverWindowLevel
        }
    }
}

// ============================================================================
// Thread-Safe Window Manager
// ============================================================================

/// Thread-safe wrapper around WindowManager.
pub type SharedWindowManager = Arc<Mutex<WindowManager>>;

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_manager_create() {
        let mut manager = WindowManager::new();
        assert_eq!(manager.window_count(), 0);

        let id = manager.register_window(WindowOptions::default());
        assert_eq!(manager.window_count(), 1);
        assert_eq!(manager.focused_window(), Some(id));
    }

    #[test]
    fn test_window_manager_unregister() {
        let mut manager = WindowManager::new();
        let id = manager.register_window(WindowOptions::default());

        assert!(manager.unregister_window(id));
        assert_eq!(manager.window_count(), 0);
        assert_eq!(manager.focused_window(), None);

        // Unregister non-existent window
        assert!(!manager.unregister_window(id));
    }

    #[test]
    fn test_window_manager_focus() {
        let mut manager = WindowManager::new();
        let id1 = manager.register_window(WindowOptions::new("Window 1"));
        let id2 = manager.register_window(WindowOptions::new("Window 2"));

        assert_eq!(manager.focused_window(), Some(id1));

        manager.focus_window(id2);
        assert_eq!(manager.focused_window(), Some(id2));
    }

    #[test]
    fn test_cascade_position() {
        let mut manager = WindowManager::new();
        let size = Size::new(Pixels(800.0), Pixels(600.0));

        let pos1 = manager.calculate_cascade_position(size);
        assert_eq!(pos1.x, Pixels(100.0));
        assert_eq!(pos1.y, Pixels(100.0));

        manager.register_window(WindowOptions::default());

        let pos2 = manager.calculate_cascade_position(size);
        assert_eq!(pos2.x, Pixels(128.0)); // 100 + 28
        assert_eq!(pos2.y, Pixels(128.0));
    }

    #[test]
    fn test_window_groups() {
        let mut manager = WindowManager::new();
        let id1 = manager.register_window(WindowOptions::new("Window 1"));
        let id2 = manager.register_window(WindowOptions::new("Window 2"));

        let group_id = manager.create_group();
        assert!(manager.add_to_group(id1, group_id));
        assert!(manager.add_to_group(id2, group_id));

        let group = manager.get_group(group_id).unwrap();
        assert_eq!(group.len(), 2);
        assert!(group.contains(&id1));
        assert!(group.contains(&id2));
    }

    #[test]
    fn test_remove_from_group() {
        let mut manager = WindowManager::new();
        let id = manager.register_window(WindowOptions::default());
        let group_id = manager.create_group();

        manager.add_to_group(id, group_id);
        assert!(manager.remove_from_group(id));

        let group = manager.get_group(group_id).unwrap();
        assert_eq!(group.len(), 0);
    }

    #[test]
    fn test_find_by_title() {
        let mut manager = WindowManager::new();
        manager.register_window(WindowOptions::new("Main Window"));
        manager.register_window(WindowOptions::new("Settings Window"));
        manager.register_window(WindowOptions::new("About"));

        let results = manager.find_by_title("Window");
        assert_eq!(results.len(), 2);

        let results = manager.find_by_title("Settings");
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_visible_windows() {
        let mut manager = WindowManager::new();

        let id1 = manager.register_window(WindowOptions::new("Visible").with_visible(true));
        let id2 = manager.register_window(WindowOptions::new("Hidden").with_visible(false));

        let visible = manager.visible_windows();
        assert_eq!(visible.len(), 1);
        assert!(visible.contains(&id1));
        assert!(!visible.contains(&id2));
    }

    #[test]
    fn test_window_options_builder() {
        let options = WindowOptions::new("Test")
            .with_size(Size::new(Pixels(1024.0), Pixels(768.0)))
            .with_position(Point::new(Pixels(50.0), Pixels(50.0)))
            .with_resizable(false)
            .with_level(WindowLevel::Floating);

        assert_eq!(options.title, "Test");
        assert_eq!(options.size.width, Pixels(1024.0));
        assert!(!options.resizable);
        assert_eq!(options.level, WindowLevel::Floating);
    }

    #[test]
    fn test_window_level_conversion() {
        #[cfg(target_os = "macos")]
        {
            assert_eq!(WindowLevel::Normal.to_ns_window_level(), 0);
            assert_eq!(WindowLevel::Floating.to_ns_window_level(), 3);
            assert_eq!(WindowLevel::Modal.to_ns_window_level(), 8);
        }
    }
}
