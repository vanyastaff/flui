//! Stack parent data - stores alignment for positioned children

use flui_types::layout::Alignment;

/// Parent data for children of RenderStack
///
/// This data is attached to children of stack containers to control
/// how they are positioned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StackParentData {
    /// Alignment within the stack (for non-positioned children)
    pub alignment: Option<Alignment>,

    /// Position from left edge (for positioned children)
    pub left: Option<f32>,

    /// Position from top edge (for positioned children)
    pub top: Option<f32>,

    /// Position from right edge (for positioned children)
    pub right: Option<f32>,

    /// Position from bottom edge (for positioned children)
    pub bottom: Option<f32>,

    /// Width override (for positioned children)
    pub width: Option<f32>,

    /// Height override (for positioned children)
    pub height: Option<f32>,
}

impl StackParentData {
    /// Create new stack parent data
    pub fn new() -> Self {
        Self::default()
    }

    /// Create positioned stack parent data
    pub fn positioned(
        left: Option<f32>,
        top: Option<f32>,
        right: Option<f32>,
        bottom: Option<f32>,
        width: Option<f32>,
        height: Option<f32>,
    ) -> Self {
        Self {
            alignment: None,
            left,
            top,
            right,
            bottom,
            width,
            height,
        }
    }

    /// Check if this child is positioned
    pub fn is_positioned(&self) -> bool {
        self.left.is_some()
            || self.top.is_some()
            || self.right.is_some()
            || self.bottom.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }
}

impl Default for StackParentData {
    fn default() -> Self {
        Self {
            alignment: None,
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
        }
    }
}
