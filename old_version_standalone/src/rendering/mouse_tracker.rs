//! Mouse tracking and cursor management
//!
//! This module provides utilities for tracking mouse/pointer state,
//! similar to Flutter's MouseTracker.

use crate::types::core::{Offset, Rect};
use std::collections::HashMap;

/// Mouse cursor types.
///
/// Similar to Flutter's `SystemMouseCursors`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseCursor {
    /// Default cursor (usually an arrow)
    Default,
    /// Text selection cursor (I-beam)
    Text,
    /// Hand cursor for clickable items
    Hand,
    /// Pointer/finger cursor
    Pointer,
    /// Wait cursor (spinning wheel/hourglass)
    Wait,
    /// Help cursor (question mark)
    Help,
    /// Crosshair cursor
    Crosshair,
    /// Move cursor (4-way arrows)
    Move,
    /// No-drop cursor (circle with line through it)
    NoDrop,
    /// Not allowed cursor
    NotAllowed,
    /// Grab cursor (open hand)
    Grab,
    /// Grabbing cursor (closed hand)
    Grabbing,
    /// Resize cursor - horizontal
    ResizeHorizontal,
    /// Resize cursor - vertical
    ResizeVertical,
    /// Resize cursor - diagonal (top-left to bottom-right)
    ResizeDiagonal1,
    /// Resize cursor - diagonal (top-right to bottom-left)
    ResizeDiagonal2,
    /// Resize cursor - north
    ResizeNorth,
    /// Resize cursor - south
    ResizeSouth,
    /// Resize cursor - east
    ResizeEast,
    /// Resize cursor - west
    ResizeWest,
    /// Resize cursor - north-east
    ResizeNorthEast,
    /// Resize cursor - north-west
    ResizeNorthWest,
    /// Resize cursor - south-east
    ResizeSouthEast,
    /// Resize cursor - south-west
    ResizeSouthWest,
    /// Hidden/invisible cursor
    None,
}

impl Default for MouseCursor {
    fn default() -> Self {
        MouseCursor::Default
    }
}

impl MouseCursor {
    /// Convert to egui cursor icon.
    pub fn to_egui(&self) -> egui::CursorIcon {
        match self {
            MouseCursor::Default => egui::CursorIcon::Default,
            MouseCursor::Text => egui::CursorIcon::Text,
            MouseCursor::Hand => egui::CursorIcon::PointingHand,
            MouseCursor::Pointer => egui::CursorIcon::PointingHand,
            MouseCursor::Wait => egui::CursorIcon::Wait,
            MouseCursor::Help => egui::CursorIcon::Help,
            MouseCursor::Crosshair => egui::CursorIcon::Crosshair,
            MouseCursor::Move => egui::CursorIcon::Move,
            MouseCursor::NoDrop => egui::CursorIcon::NoDrop,
            MouseCursor::NotAllowed => egui::CursorIcon::NotAllowed,
            MouseCursor::Grab => egui::CursorIcon::Grab,
            MouseCursor::Grabbing => egui::CursorIcon::Grabbing,
            MouseCursor::ResizeHorizontal => egui::CursorIcon::ResizeHorizontal,
            MouseCursor::ResizeVertical => egui::CursorIcon::ResizeVertical,
            MouseCursor::ResizeDiagonal1 => egui::CursorIcon::ResizeNeSw,
            MouseCursor::ResizeDiagonal2 => egui::CursorIcon::ResizeNwSe,
            MouseCursor::ResizeNorth => egui::CursorIcon::ResizeNorth,
            MouseCursor::ResizeSouth => egui::CursorIcon::ResizeSouth,
            MouseCursor::ResizeEast => egui::CursorIcon::ResizeEast,
            MouseCursor::ResizeWest => egui::CursorIcon::ResizeWest,
            MouseCursor::ResizeNorthEast => egui::CursorIcon::ResizeNorthEast,
            MouseCursor::ResizeNorthWest => egui::CursorIcon::ResizeNorthWest,
            MouseCursor::ResizeSouthEast => egui::CursorIcon::ResizeSouthEast,
            MouseCursor::ResizeSouthWest => egui::CursorIcon::ResizeSouthWest,
            MouseCursor::None => egui::CursorIcon::None,
        }
    }
}

/// Information about a mouse event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseEvent {
    /// The position of the mouse pointer.
    pub position: Offset,

    /// The button that was pressed/released (if any).
    pub button: Option<MouseButton>,

    /// The type of event.
    pub event_type: MouseEventType,

    /// Timestamp of the event.
    pub timestamp: std::time::Instant,
}

/// Mouse button types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Primary button (usually left)
    Primary,
    /// Secondary button (usually right)
    Secondary,
    /// Middle button
    Middle,
    /// Back button
    Back,
    /// Forward button
    Forward,
}

/// Types of mouse events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseEventType {
    /// Mouse entered a region
    Enter,
    /// Mouse exited a region
    Exit,
    /// Mouse moved within a region
    Hover,
    /// Mouse button pressed
    Down,
    /// Mouse button released
    Up,
    /// Mouse wheel scrolled
    Scroll,
}

/// Annotation for mouse tracking regions.
///
/// Similar to Flutter's `MouseTrackerAnnotation`.
#[derive(Debug, Clone)]
pub struct MouseTrackerAnnotation {
    /// Unique identifier for this region
    pub id: usize,

    /// The cursor to display when hovering over this region
    pub cursor: MouseCursor,

    /// The rectangle for this region
    pub rect: Rect,

    /// Callback for mouse enter events
    pub on_enter: Option<fn(&MouseEvent)>,

    /// Callback for mouse exit events
    pub on_exit: Option<fn(&MouseEvent)>,

    /// Callback for mouse hover events
    pub on_hover: Option<fn(&MouseEvent)>,
}

impl MouseTrackerAnnotation {
    /// Create a new mouse tracker annotation.
    pub fn new(id: usize, rect: Rect) -> Self {
        Self {
            id,
            cursor: MouseCursor::default(),
            rect,
            on_enter: None,
            on_exit: None,
            on_hover: None,
        }
    }

    /// Set the cursor for this region.
    pub fn with_cursor(mut self, cursor: MouseCursor) -> Self {
        self.cursor = cursor;
        self
    }

    /// Check if a point is inside this region.
    pub fn contains(&self, point: Offset) -> bool {
        let pos = egui::pos2(point.dx, point.dy);
        self.rect.contains(pos)
    }
}

/// Tracks mouse state and regions.
///
/// Similar to Flutter's `MouseTracker`.
pub struct MouseTracker {
    /// Current mouse position
    current_position: Option<Offset>,

    /// Currently hovered regions
    hovered_regions: Vec<usize>,

    /// All registered regions
    regions: HashMap<usize, MouseTrackerAnnotation>,

    /// Next available region ID
    next_id: usize,

    /// Current cursor
    current_cursor: MouseCursor,
}

impl Default for MouseTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseTracker {
    /// Create a new mouse tracker.
    pub fn new() -> Self {
        Self {
            current_position: None,
            hovered_regions: Vec::new(),
            regions: HashMap::new(),
            next_id: 1,
            current_cursor: MouseCursor::default(),
        }
    }

    /// Generate a new unique region ID.
    pub fn next_id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Register a new mouse tracking region.
    pub fn register_region(&mut self, annotation: MouseTrackerAnnotation) {
        self.regions.insert(annotation.id, annotation);
    }

    /// Unregister a mouse tracking region.
    pub fn unregister_region(&mut self, id: usize) {
        self.regions.remove(&id);
        self.hovered_regions.retain(|&region_id| region_id != id);
    }

    /// Update mouse position and trigger appropriate callbacks.
    pub fn update_position(&mut self, position: Offset) {
        let prev_position = self.current_position;
        self.current_position = Some(position);

        // Find regions under the cursor
        let mut new_hovered: Vec<usize> = self
            .regions
            .iter()
            .filter(|(_, annotation)| annotation.contains(position))
            .map(|(id, _)| *id)
            .collect();

        // Sort by z-order (smaller IDs are on top for now)
        new_hovered.sort();

        // Find regions that were entered/exited
        let entered: Vec<usize> = new_hovered
            .iter()
            .filter(|id| !self.hovered_regions.contains(id))
            .copied()
            .collect();

        let exited: Vec<usize> = self
            .hovered_regions
            .iter()
            .filter(|id| !new_hovered.contains(id))
            .copied()
            .collect();

        // Trigger exit callbacks
        for id in exited {
            if let Some(annotation) = self.regions.get(&id) {
                if let Some(on_exit) = annotation.on_exit {
                    let event = MouseEvent {
                        position,
                        button: None,
                        event_type: MouseEventType::Exit,
                        timestamp: std::time::Instant::now(),
                    };
                    on_exit(&event);
                }
            }
        }

        // Trigger enter callbacks
        for id in entered {
            if let Some(annotation) = self.regions.get(&id) {
                if let Some(on_enter) = annotation.on_enter {
                    let event = MouseEvent {
                        position,
                        button: None,
                        event_type: MouseEventType::Enter,
                        timestamp: std::time::Instant::now(),
                    };
                    on_enter(&event);
                }
            }
        }

        // Trigger hover callbacks for currently hovered regions
        if prev_position.is_some() {
            for id in &new_hovered {
                if let Some(annotation) = self.regions.get(id) {
                    if let Some(on_hover) = annotation.on_hover {
                        let event = MouseEvent {
                            position,
                            button: None,
                            event_type: MouseEventType::Hover,
                            timestamp: std::time::Instant::now(),
                        };
                        on_hover(&event);
                    }
                }
            }
        }

        // Update cursor based on topmost hovered region
        self.current_cursor = new_hovered
            .first()
            .and_then(|id| self.regions.get(id))
            .map(|annotation| annotation.cursor)
            .unwrap_or(MouseCursor::Default);

        self.hovered_regions = new_hovered;
    }

    /// Get the current cursor.
    pub fn current_cursor(&self) -> MouseCursor {
        self.current_cursor
    }

    /// Get the current mouse position.
    pub fn current_position(&self) -> Option<Offset> {
        self.current_position
    }

    /// Clear all registered regions.
    pub fn clear(&mut self) {
        self.regions.clear();
        self.hovered_regions.clear();
        self.current_cursor = MouseCursor::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mouse_cursor_to_egui() {
        assert_eq!(MouseCursor::Default.to_egui(), egui::CursorIcon::Default);
        assert_eq!(MouseCursor::Text.to_egui(), egui::CursorIcon::Text);
        assert_eq!(MouseCursor::Hand.to_egui(), egui::CursorIcon::PointingHand);
    }

    #[test]
    fn test_mouse_tracker_annotation() {
        let rect = Rect::from_min_max(
            egui::pos2(10.0, 10.0),
            egui::pos2(50.0, 50.0),
        );

        let annotation = MouseTrackerAnnotation::new(1, rect)
            .with_cursor(MouseCursor::Hand);

        assert_eq!(annotation.cursor, MouseCursor::Hand);
        assert!(annotation.contains(Offset::new(25.0, 25.0)));
        assert!(!annotation.contains(Offset::new(5.0, 5.0)));
    }

    #[test]
    fn test_mouse_tracker_registration() {
        let mut tracker = MouseTracker::new();

        let rect = Rect::from_min_max(
            egui::pos2(10.0, 10.0),
            egui::pos2(50.0, 50.0),
        );

        let id = tracker.next_id();
        let annotation = MouseTrackerAnnotation::new(id, rect);

        tracker.register_region(annotation);
        assert_eq!(tracker.regions.len(), 1);

        tracker.unregister_region(id);
        assert_eq!(tracker.regions.len(), 0);
    }

    #[test]
    fn test_mouse_tracker_hover() {
        let mut tracker = MouseTracker::new();

        let rect = Rect::from_min_max(
            egui::pos2(10.0, 10.0),
            egui::pos2(50.0, 50.0),
        );

        let id = tracker.next_id();
        let annotation = MouseTrackerAnnotation::new(id, rect)
            .with_cursor(MouseCursor::Hand);

        tracker.register_region(annotation);

        // Move mouse into region
        tracker.update_position(Offset::new(25.0, 25.0));
        assert_eq!(tracker.current_cursor(), MouseCursor::Hand);
        assert_eq!(tracker.hovered_regions.len(), 1);

        // Move mouse outside region
        tracker.update_position(Offset::new(5.0, 5.0));
        assert_eq!(tracker.current_cursor(), MouseCursor::Default);
        assert_eq!(tracker.hovered_regions.len(), 0);
    }
}
