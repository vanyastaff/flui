//! Position types for absolute and relative positioning
//!
//! This module contains types for representing positions,
//! similar to Flutter's Positioned widget and positioning system.

use egui::{Pos2, Rect, Vec2};

/// Represents an absolute position with optional constraints.
///
/// Similar to Flutter's Positioned widget properties.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Position {
    /// Distance from the left edge
    pub left: Option<f32>,
    /// Distance from the top edge
    pub top: Option<f32>,
    /// Distance from the right edge
    pub right: Option<f32>,
    /// Distance from the bottom edge
    pub bottom: Option<f32>,
    /// Explicit width (overrides left/right if specified)
    pub width: Option<f32>,
    /// Explicit height (overrides top/bottom if specified)
    pub height: Option<f32>,
}

impl Position {
    /// Create a new position with all fields set to None.
    pub const fn new() -> Self {
        Self {
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Position from the left edge.
    pub const fn from_left(left: f32) -> Self {
        Self {
            left: Some(left),
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Position from the top edge.
    pub const fn from_top(top: f32) -> Self {
        Self {
            left: None,
            top: Some(top),
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Position from the right edge.
    pub const fn from_right(right: f32) -> Self {
        Self {
            left: None,
            top: None,
            right: Some(right),
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Position from the bottom edge.
    pub const fn from_bottom(bottom: f32) -> Self {
        Self {
            left: None,
            top: None,
            right: None,
            bottom: Some(bottom),
            width: None,
            height: None,
        }
    }

    /// Fill the entire area.
    pub const fn fill() -> Self {
        Self {
            left: Some(0.0),
            top: Some(0.0),
            right: Some(0.0),
            bottom: Some(0.0),
            width: None,
            height: None,
        }
    }

    /// Position at specific coordinates.
    pub const fn at(left: f32, top: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Position with specific dimensions.
    pub const fn with_size(left: f32, top: f32, width: f32, height: f32) -> Self {
        Self {
            left: Some(left),
            top: Some(top),
            right: None,
            bottom: None,
            width: Some(width),
            height: Some(height),
        }
    }

    /// Builder: set left edge distance.
    pub const fn left(mut self, left: f32) -> Self {
        self.left = Some(left);
        self
    }

    /// Builder: set top edge distance.
    pub const fn top(mut self, top: f32) -> Self {
        self.top = Some(top);
        self
    }

    /// Builder: set right edge distance.
    pub const fn right(mut self, right: f32) -> Self {
        self.right = Some(right);
        self
    }

    /// Builder: set bottom edge distance.
    pub const fn bottom(mut self, bottom: f32) -> Self {
        self.bottom = Some(bottom);
        self
    }

    /// Builder: set width.
    pub const fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Builder: set height.
    pub const fn height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Resolve this position to a Rect within the given container.
    pub fn resolve(&self, container: Rect) -> Rect {
        let container_width = container.width();
        let container_height = container.height();

        let x = if let Some(left) = self.left {
            container.left() + left
        } else if let Some(right) = self.right {
            if let Some(width) = self.width {
                container.right() - right - width
            } else {
                container.right() - right
            }
        } else {
            container.left()
        };

        let y = if let Some(top) = self.top {
            container.top() + top
        } else if let Some(bottom) = self.bottom {
            if let Some(height) = self.height {
                container.bottom() - bottom - height
            } else {
                container.bottom() - bottom
            }
        } else {
            container.top()
        };

        let width = if let Some(w) = self.width {
            w
        } else if let Some(left) = self.left {
            if let Some(right) = self.right {
                container_width - left - right
            } else {
                container_width - left
            }
        } else if let Some(right) = self.right {
            container_width - right
        } else {
            container_width
        };

        let height = if let Some(h) = self.height {
            h
        } else if let Some(top) = self.top {
            if let Some(bottom) = self.bottom {
                container_height - top - bottom
            } else {
                container_height - top
            }
        } else if let Some(bottom) = self.bottom {
            container_height - bottom
        } else {
            container_height
        };

        Rect::from_min_size(Pos2::new(x, y), Vec2::new(width, height))
    }
}

impl Default for Position {
    fn default() -> Self {
        Self::new()
    }
}

/// A rectangle with an absolute position.
///
/// Similar to Flutter's positioned rect concept.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedRect {
    /// The rectangle
    pub rect: Rect,
    /// The position data
    pub position: Position,
}

impl PositionedRect {
    /// Create a new positioned rect.
    pub const fn new(rect: Rect, position: Position) -> Self {
        Self { rect, position }
    }

    /// Create from a rect at a specific position.
    pub fn at(rect: Rect, left: f32, top: f32) -> Self {
        Self {
            rect,
            position: Position::at(left, top),
        }
    }

    /// Get the final rect within a container.
    pub fn resolve(&self, container: Rect) -> Rect {
        self.position.resolve(container)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_creation() {
        let pos = Position::new();
        assert_eq!(pos.left, None);
        assert_eq!(pos.top, None);

        let from_left = Position::from_left(10.0);
        assert_eq!(from_left.left, Some(10.0));
        assert_eq!(from_left.top, None);

        let at_pos = Position::at(10.0, 20.0);
        assert_eq!(at_pos.left, Some(10.0));
        assert_eq!(at_pos.top, Some(20.0));
    }

    #[test]
    fn test_position_builder() {
        let pos = Position::new()
            .left(10.0)
            .top(20.0)
            .width(100.0)
            .height(50.0);

        assert_eq!(pos.left, Some(10.0));
        assert_eq!(pos.top, Some(20.0));
        assert_eq!(pos.width, Some(100.0));
        assert_eq!(pos.height, Some(50.0));
    }

    #[test]
    fn test_position_fill() {
        let fill = Position::fill();
        assert_eq!(fill.left, Some(0.0));
        assert_eq!(fill.top, Some(0.0));
        assert_eq!(fill.right, Some(0.0));
        assert_eq!(fill.bottom, Some(0.0));
    }

    #[test]
    fn test_position_resolve_simple() {
        let container = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 100.0));

        let pos = Position::at(10.0, 20.0).width(50.0).height(30.0);
        let resolved = pos.resolve(container);

        assert_eq!(resolved.left(), 10.0);
        assert_eq!(resolved.top(), 20.0);
        assert_eq!(resolved.width(), 50.0);
        assert_eq!(resolved.height(), 30.0);
    }

    #[test]
    fn test_position_resolve_fill() {
        let container = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 100.0));

        let fill = Position::fill();
        let resolved = fill.resolve(container);

        assert_eq!(resolved, container);
    }

    #[test]
    fn test_position_resolve_from_right() {
        let container = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 100.0));

        let pos = Position::from_right(10.0).width(50.0);
        let resolved = pos.resolve(container);

        assert_eq!(resolved.right(), 190.0); // 200 - 10
        assert_eq!(resolved.width(), 50.0);
    }

    #[test]
    fn test_position_resolve_stretch() {
        let container = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(200.0, 100.0));

        // Stretch horizontally with margins
        let pos = Position::new().left(10.0).right(10.0).top(20.0).height(50.0);
        let resolved = pos.resolve(container);

        assert_eq!(resolved.left(), 10.0);
        assert_eq!(resolved.width(), 180.0); // 200 - 10 - 10
        assert_eq!(resolved.top(), 20.0);
        assert_eq!(resolved.height(), 50.0);
    }

    #[test]
    fn test_positioned_rect() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 50.0));
        let pos = PositionedRect::at(rect, 10.0, 20.0);

        assert_eq!(pos.rect, rect);
        assert_eq!(pos.position.left, Some(10.0));
        assert_eq!(pos.position.top, Some(20.0));
    }
}
