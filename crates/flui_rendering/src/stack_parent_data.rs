// ! StackParentData - stores positioning information for Stack children
//!
//! This parent data is used by RenderStack to store positioning information
//! for positioned children (left, top, right, bottom, width, height).

use std::fmt::Debug;

/// Parent data for children in a Stack layout
///
/// Stores positioning information for each child. Non-positioned children
/// use None for all fields and are laid out normally. Positioned children
/// can specify any combination of left/top/right/bottom/width/height.
///
/// # Example
///
/// ```rust
/// use flui_rendering::StackParentData;
///
/// // Non-positioned child
/// let non_positioned = StackParentData::new();
/// assert!(!non_positioned.is_positioned());
///
/// // Positioned at top-left
/// let positioned = StackParentData::new()
///     .with_left(10.0)
///     .with_top(20.0);
/// assert!(positioned.is_positioned());
///
/// // Positioned with width and height
/// let sized = StackParentData::new()
///     .with_left(0.0)
///     .with_top(0.0)
///     .with_width(100.0)
///     .with_height(50.0);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct StackParentData {
    /// Distance from left edge of stack
    pub left: Option<f32>,

    /// Distance from top edge of stack
    pub top: Option<f32>,

    /// Distance from right edge of stack
    pub right: Option<f32>,

    /// Distance from bottom edge of stack
    pub bottom: Option<f32>,

    /// Explicit width of child (conflicts with left + right)
    pub width: Option<f32>,

    /// Explicit height of child (conflicts with top + bottom)
    pub height: Option<f32>,
}

impl StackParentData {
    /// Create new StackParentData with no positioning (non-positioned child)
    pub fn new() -> Self {
        Self {
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }

    /// Create positioned StackParentData
    pub fn positioned(
        left: Option<f32>,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
        width: Option<f32>,
        height: Option<f32>,
    ) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
            width,
            height,
        }
    }

    /// Set left position
    pub fn with_left(mut self, left: f32) -> Self {
        self.left = Some(left);
        self
    }

    /// Set top position
    pub fn with_top(mut self, top: f32) -> Self {
        self.top = Some(top);
        self
    }

    /// Set right position
    pub fn with_right(mut self, right: f32) -> Self {
        self.right = Some(right);
        self
    }

    /// Set bottom position
    pub fn with_bottom(mut self, bottom: f32) -> Self {
        self.bottom = Some(bottom);
        self
    }

    /// Set width
    pub fn with_width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// Set height
    pub fn with_height(mut self, height: f32) -> Self {
        self.height = Some(height);
        self
    }

    /// Returns true if this child is positioned
    ///
    /// A child is positioned if any of left, top, right, bottom, width, or height
    /// is specified.
    pub fn is_positioned(&self) -> bool {
        self.left.is_some()
            || self.top.is_some()
            || self.right.is_some()
            || self.bottom.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }

    /// Returns true if this child is non-positioned
    pub fn is_non_positioned(&self) -> bool {
        !self.is_positioned()
    }
}

impl Default for StackParentData {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_parent_data_new() {
        let data = StackParentData::new();
        assert!(data.is_non_positioned());
        assert!(!data.is_positioned());
        assert_eq!(data.left, None);
        assert_eq!(data.top, None);
        assert_eq!(data.right, None);
        assert_eq!(data.bottom, None);
        assert_eq!(data.width, None);
        assert_eq!(data.height, None);
    }

    #[test]
    fn test_stack_parent_data_positioned() {
        let data = StackParentData::positioned(
            Some(10.0),
            Some(20.0),
            Some(30.0),
            Some(40.0),
            Some(100.0),
            Some(200.0),
        );
        assert!(data.is_positioned());
        assert!(!data.is_non_positioned());
        assert_eq!(data.left, Some(10.0));
        assert_eq!(data.top, Some(20.0));
        assert_eq!(data.right, Some(30.0));
        assert_eq!(data.bottom, Some(40.0));
        assert_eq!(data.width, Some(100.0));
        assert_eq!(data.height, Some(200.0));
    }

    #[test]
    fn test_stack_parent_data_with_left() {
        let data = StackParentData::new().with_left(15.0);
        assert!(data.is_positioned());
        assert_eq!(data.left, Some(15.0));
    }

    #[test]
    fn test_stack_parent_data_with_top() {
        let data = StackParentData::new().with_top(25.0);
        assert!(data.is_positioned());
        assert_eq!(data.top, Some(25.0));
    }

    #[test]
    fn test_stack_parent_data_chaining() {
        let data = StackParentData::new()
            .with_left(10.0)
            .with_top(20.0)
            .with_width(100.0)
            .with_height(50.0);

        assert!(data.is_positioned());
        assert_eq!(data.left, Some(10.0));
        assert_eq!(data.top, Some(20.0));
        assert_eq!(data.width, Some(100.0));
        assert_eq!(data.height, Some(50.0));
        assert_eq!(data.right, None);
        assert_eq!(data.bottom, None);
    }

    #[test]
    fn test_stack_parent_data_default() {
        let data = StackParentData::default();
        assert!(data.is_non_positioned());
    }
}
