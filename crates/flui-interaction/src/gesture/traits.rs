//! Drag constraints shared by gesture recognizers.

/// Drag axis constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum DragAxis {
    /// Vertical drag only (up/down).
    Vertical,
    /// Horizontal drag only (left/right).
    Horizontal,
    /// Free drag (any direction).
    #[default]
    Free,
}
