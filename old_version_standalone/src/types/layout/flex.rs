//! Flex layout types
//!
//! This module contains types for flex layout system,
//! similar to Flutter's Flexible and Flex widgets.

/// How a flex child should fit in the available space.
///
/// Similar to Flutter's FlexFit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlexFit {
    /// The child fills the available space (flex: 1).
    Tight,
    /// The child can be smaller than the available space.
    Loose,
}

impl Default for FlexFit {
    fn default() -> Self {
        FlexFit::Loose
    }
}

/// Direction of flex layout.
///
/// Similar to Flutter's Axis but specific to flex layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlexDirection {
    /// Horizontal flex layout (row).
    Row,
    /// Vertical flex layout (column).
    Column,
    /// Horizontal flex layout in reverse order.
    RowReverse,
    /// Vertical flex layout in reverse order.
    ColumnReverse,
}

impl FlexDirection {
    /// Check if this direction is horizontal.
    pub fn is_horizontal(&self) -> bool {
        matches!(self, FlexDirection::Row | FlexDirection::RowReverse)
    }

    /// Check if this direction is vertical.
    pub fn is_vertical(&self) -> bool {
        !self.is_horizontal()
    }

    /// Check if this direction is reversed.
    pub fn is_reversed(&self) -> bool {
        matches!(
            self,
            FlexDirection::RowReverse | FlexDirection::ColumnReverse
        )
    }

    /// Get the opposite direction.
    pub fn opposite(&self) -> Self {
        match self {
            FlexDirection::Row => FlexDirection::Column,
            FlexDirection::Column => FlexDirection::Row,
            FlexDirection::RowReverse => FlexDirection::ColumnReverse,
            FlexDirection::ColumnReverse => FlexDirection::RowReverse,
        }
    }

    /// Get the reversed version of this direction.
    pub fn reversed(&self) -> Self {
        match self {
            FlexDirection::Row => FlexDirection::RowReverse,
            FlexDirection::Column => FlexDirection::ColumnReverse,
            FlexDirection::RowReverse => FlexDirection::Row,
            FlexDirection::ColumnReverse => FlexDirection::Column,
        }
    }
}

impl Default for FlexDirection {
    fn default() -> Self {
        FlexDirection::Row
    }
}

/// Wrap behavior for flex layout.
///
/// Determines how flex items wrap when they exceed container size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlexWrap {
    /// No wrapping - items overflow.
    NoWrap,
    /// Wrap onto multiple lines.
    Wrap,
    /// Wrap in reverse direction.
    WrapReverse,
}

impl Default for FlexWrap {
    fn default() -> Self {
        FlexWrap::NoWrap
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_flex_fit() {
        assert_eq!(FlexFit::default(), FlexFit::Loose);
    }

    #[test]
    fn test_flex_direction_checks() {
        assert!(FlexDirection::Row.is_horizontal());
        assert!(FlexDirection::RowReverse.is_horizontal());
        assert!(!FlexDirection::Row.is_vertical());

        assert!(FlexDirection::Column.is_vertical());
        assert!(FlexDirection::ColumnReverse.is_vertical());
        assert!(!FlexDirection::Column.is_horizontal());

        assert!(!FlexDirection::Row.is_reversed());
        assert!(FlexDirection::RowReverse.is_reversed());
        assert!(!FlexDirection::Column.is_reversed());
        assert!(FlexDirection::ColumnReverse.is_reversed());
    }

    #[test]
    fn test_flex_direction_opposite() {
        assert_eq!(FlexDirection::Row.opposite(), FlexDirection::Column);
        assert_eq!(FlexDirection::Column.opposite(), FlexDirection::Row);
        assert_eq!(
            FlexDirection::RowReverse.opposite(),
            FlexDirection::ColumnReverse
        );
        assert_eq!(
            FlexDirection::ColumnReverse.opposite(),
            FlexDirection::RowReverse
        );
    }

    #[test]
    fn test_flex_direction_reversed() {
        assert_eq!(FlexDirection::Row.reversed(), FlexDirection::RowReverse);
        assert_eq!(FlexDirection::RowReverse.reversed(), FlexDirection::Row);
        assert_eq!(
            FlexDirection::Column.reversed(),
            FlexDirection::ColumnReverse
        );
        assert_eq!(
            FlexDirection::ColumnReverse.reversed(),
            FlexDirection::Column
        );
    }

    #[test]
    fn test_flex_wrap() {
        assert_eq!(FlexWrap::default(), FlexWrap::NoWrap);
    }
}
