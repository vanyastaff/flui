//! Overlay entry - a single overlay instance.

use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for an overlay entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OverlayId(u64);

impl OverlayId {
    /// Generate a new unique overlay ID.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the raw ID value.
    pub fn get(&self) -> u64 {
        self.0
    }
}

impl Default for OverlayId {
    fn default() -> Self {
        Self::new()
    }
}

/// Position of an overlay relative to the screen or anchor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OverlayPosition {
    /// Centered on screen.
    Center,
    /// At specific screen coordinates.
    Absolute {
        /// X coordinate.
        x: f32,
        /// Y coordinate.
        y: f32,
    },
    /// Relative to top-left corner with offset.
    TopLeft {
        /// X offset from left.
        x: f32,
        /// Y offset from top.
        y: f32,
    },
    /// Relative to top-right corner with offset.
    TopRight {
        /// X offset from right.
        x: f32,
        /// Y offset from top.
        y: f32,
    },
    /// Relative to bottom-left corner with offset.
    BottomLeft {
        /// X offset from left.
        x: f32,
        /// Y offset from bottom.
        y: f32,
    },
    /// Relative to bottom-right corner with offset.
    BottomRight {
        /// X offset from right.
        x: f32,
        /// Y offset from bottom.
        y: f32,
    },
    /// Fill the entire screen.
    Fill,
}

impl Default for OverlayPosition {
    fn default() -> Self {
        Self::Center
    }
}

/// Priority level for overlay stacking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum OverlayPriority {
    /// Background overlays (below normal content).
    Background = 0,
    /// Normal priority (default).
    Normal = 100,
    /// Above normal content.
    High = 200,
    /// Modal dialogs.
    Modal = 300,
    /// Tooltips and transient UI.
    Tooltip = 400,
    /// Debug overlays (always on top).
    Debug = 500,
}

impl Default for OverlayPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// A single overlay entry that can be added to the overlay manager.
#[derive(Debug)]
pub struct OverlayEntry {
    id: OverlayId,
    position: OverlayPosition,
    priority: OverlayPriority,
    /// Whether this overlay blocks interaction with content below.
    modal: bool,
    /// Whether to show a scrim (darkened background) behind the overlay.
    show_scrim: bool,
    /// Scrim opacity (0.0 to 1.0).
    scrim_opacity: f32,
    /// Whether the overlay is currently visible.
    visible: bool,
    /// Whether clicking the scrim dismisses the overlay.
    dismiss_on_scrim_tap: bool,
    /// Custom tag for identification.
    tag: Option<String>,
    // In a real implementation, this would hold a View/Widget
    // For now, we just track the metadata
}

impl OverlayEntry {
    /// Create a new overlay entry with default settings.
    pub fn new() -> Self {
        Self {
            id: OverlayId::new(),
            position: OverlayPosition::default(),
            priority: OverlayPriority::default(),
            modal: false,
            show_scrim: false,
            scrim_opacity: 0.5,
            visible: true,
            dismiss_on_scrim_tap: true,
            tag: None,
        }
    }

    /// Create a builder for configuring an overlay entry.
    pub fn builder() -> OverlayEntryBuilder {
        OverlayEntryBuilder::new()
    }

    /// Get the overlay ID.
    pub fn id(&self) -> OverlayId {
        self.id
    }

    /// Get the overlay position.
    pub fn position(&self) -> OverlayPosition {
        self.position
    }

    /// Get the overlay priority.
    pub fn priority(&self) -> OverlayPriority {
        self.priority
    }

    /// Check if the overlay is modal.
    pub fn is_modal(&self) -> bool {
        self.modal
    }

    /// Check if the overlay should show a scrim.
    pub fn show_scrim(&self) -> bool {
        self.show_scrim
    }

    /// Get the scrim opacity.
    pub fn scrim_opacity(&self) -> f32 {
        self.scrim_opacity
    }

    /// Check if the overlay is visible.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Set visibility.
    pub fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    /// Check if tapping the scrim dismisses the overlay.
    pub fn dismiss_on_scrim_tap(&self) -> bool {
        self.dismiss_on_scrim_tap
    }

    /// Get the custom tag.
    pub fn tag(&self) -> Option<&str> {
        self.tag.as_deref()
    }
}

impl Default for OverlayEntry {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for creating overlay entries.
#[derive(Debug, Default)]
pub struct OverlayEntryBuilder {
    position: OverlayPosition,
    priority: OverlayPriority,
    modal: bool,
    show_scrim: bool,
    scrim_opacity: f32,
    dismiss_on_scrim_tap: bool,
    tag: Option<String>,
}

impl OverlayEntryBuilder {
    /// Create a new builder with default values.
    pub fn new() -> Self {
        Self {
            position: OverlayPosition::default(),
            priority: OverlayPriority::default(),
            modal: false,
            show_scrim: false,
            scrim_opacity: 0.5,
            dismiss_on_scrim_tap: true,
            tag: None,
        }
    }

    /// Set the overlay position.
    pub fn position(mut self, position: OverlayPosition) -> Self {
        self.position = position;
        self
    }

    /// Set the overlay priority.
    pub fn priority(mut self, priority: OverlayPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Make the overlay modal (blocks interaction below).
    pub fn modal(mut self, modal: bool) -> Self {
        self.modal = modal;
        self
    }

    /// Show a scrim behind the overlay.
    pub fn with_scrim(mut self, opacity: f32) -> Self {
        self.show_scrim = true;
        self.scrim_opacity = opacity.clamp(0.0, 1.0);
        self
    }

    /// Set whether tapping the scrim dismisses the overlay.
    pub fn dismiss_on_scrim_tap(mut self, dismiss: bool) -> Self {
        self.dismiss_on_scrim_tap = dismiss;
        self
    }

    /// Set a custom tag for identification.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }

    /// Build the overlay entry.
    pub fn build(self) -> OverlayEntry {
        OverlayEntry {
            id: OverlayId::new(),
            position: self.position,
            priority: self.priority,
            modal: self.modal,
            show_scrim: self.show_scrim,
            scrim_opacity: self.scrim_opacity,
            visible: true,
            dismiss_on_scrim_tap: self.dismiss_on_scrim_tap,
            tag: self.tag,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_id_unique() {
        let id1 = OverlayId::new();
        let id2 = OverlayId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_overlay_entry_builder() {
        let entry = OverlayEntry::builder()
            .position(OverlayPosition::TopRight { x: 10.0, y: 10.0 })
            .priority(OverlayPriority::Modal)
            .modal(true)
            .with_scrim(0.7)
            .tag("my-dialog")
            .build();

        assert_eq!(
            entry.position(),
            OverlayPosition::TopRight { x: 10.0, y: 10.0 }
        );
        assert_eq!(entry.priority(), OverlayPriority::Modal);
        assert!(entry.is_modal());
        assert!(entry.show_scrim());
        assert!((entry.scrim_opacity() - 0.7).abs() < f32::EPSILON);
        assert_eq!(entry.tag(), Some("my-dialog"));
    }

    #[test]
    fn test_overlay_priority_ordering() {
        assert!(OverlayPriority::Background < OverlayPriority::Normal);
        assert!(OverlayPriority::Normal < OverlayPriority::High);
        assert!(OverlayPriority::High < OverlayPriority::Modal);
        assert!(OverlayPriority::Modal < OverlayPriority::Tooltip);
        assert!(OverlayPriority::Tooltip < OverlayPriority::Debug);
    }
}
