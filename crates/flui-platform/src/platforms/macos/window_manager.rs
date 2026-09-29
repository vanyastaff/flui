//! macOS Multi-Window Management
//!
//! This module provides centralized management for multiple windows in a macOS
//! application. It handles window lifecycle, coordination, and inter-window
//! communication.
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

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use flui_foundation::geometry::{Point, Size};

// The platform-wide window identity: `crate::traits::WindowId` is the single
// canonical definition; backends never
// mint their own WindowId type. Re-exported because `macos::mod` surfaces
// the window-manager API (including this ID type) as a group.
pub use crate::traits::WindowId;

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

impl std::fmt::Debug for WindowManager {
    // Hand-written: `WindowInfo` holds raw Objective-C handles.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WindowManager")
            .field("windows", &self.windows.len())
            .field("focused_window", &self.focused_window)
            .finish_non_exhaustive()
    }
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
    pub fn calculate_cascade_position(&self, _window_size: Size<f64>) -> Point<f64> {
        let count = self.window_count();
        let cascade_offset = 28.0; // Standard macOS cascade offset

        let x = 100.0 + (count as f64 * cascade_offset);
        let y = 100.0 + (count as f64 * cascade_offset);

        Point::new(x, y)
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
        self.groups.get(&group_id).map(std::vec::Vec::as_slice)
    }

    /// Remove window from its group.
    pub fn remove_from_group(&mut self, window_id: WindowId) -> bool {
        if let Some(info) = self.windows.get_mut(&window_id)
            && let Some(group_id) = info.group
            && let Some(group) = self.groups.get_mut(&group_id)
        {
            group.retain(|&id| id != window_id);
            info.group = None;
            return true;
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
    pub position: Point<f64>,

    /// Window size.
    pub size: Size<f64>,

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
    pub position: Point<f64>,

    /// Initial size.
    pub size: Size<f64>,

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
            position: Point::new(100.0, 100.0),
            size: Size::new(800.0, 600.0),
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
    pub fn with_position(mut self, position: Point<f64>) -> Self {
        self.position = position;
        self
    }

    /// Set window size.
    pub fn with_size(mut self, size: Size<f64>) -> Self {
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
            WindowLevel::Normal => 0,         // NSNormalWindowLevel
            WindowLevel::Floating => 3,       // NSFloatingWindowLevel
            WindowLevel::Modal => 8,          // NSModalPanelWindowLevel
            WindowLevel::Popover => 101,      // NSPopUpMenuWindowLevel
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
