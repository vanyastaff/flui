//! Common enum types
//!
//! This module contains various common enum types used across the UI system.

/// Pointer/cursor type.
///
/// Similar to CSS cursor property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Cursor {
    /// Default cursor
    Default,
    /// Pointer/hand cursor (for clickable elements)
    Pointer,
    /// Text selection cursor
    Text,
    /// Move cursor
    Move,
    /// Not allowed cursor
    NotAllowed,
    /// Wait/busy cursor
    Wait,
    /// Crosshair cursor
    Crosshair,
    /// Resize horizontal cursor
    ResizeHorizontal,
    /// Resize vertical cursor
    ResizeVertical,
    /// Resize all directions cursor
    ResizeAll,
    /// Help cursor
    Help,
    /// Grab cursor (for drag & drop)
    Grab,
    /// Grabbing cursor (while dragging)
    Grabbing,
}

impl Default for Cursor {
    fn default() -> Self {
        Cursor::Default
    }
}

/// Visibility state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Visibility {
    /// Element is visible
    Visible,
    /// Element is hidden but still takes up space
    Hidden,
    /// Element is completely removed from layout
    Collapsed,
}

impl Visibility {
    /// Check if element is visible.
    pub fn is_visible(&self) -> bool {
        matches!(self, Visibility::Visible)
    }

    /// Check if element takes up space in layout.
    pub fn takes_space(&self) -> bool {
        !matches!(self, Visibility::Collapsed)
    }
}

impl Default for Visibility {
    fn default() -> Self {
        Visibility::Visible
    }
}

/// Overflow behavior.
///
/// Similar to CSS overflow property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Overflow {
    /// Content is visible outside bounds
    Visible,
    /// Content is clipped to bounds
    Hidden,
    /// Scrollbars appear when needed
    Auto,
    /// Scrollbars always visible
    Scroll,
}

impl Default for Overflow {
    fn default() -> Self {
        Overflow::Visible
    }
}

/// Pointer events handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerEvents {
    /// Element receives pointer events
    Auto,
    /// Element does not receive pointer events
    None,
}

impl PointerEvents {
    /// Check if pointer events are enabled.
    pub fn is_enabled(&self) -> bool {
        matches!(self, PointerEvents::Auto)
    }
}

impl Default for PointerEvents {
    fn default() -> Self {
        PointerEvents::Auto
    }
}

/// Text transform.
///
/// Similar to CSS text-transform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextTransform {
    /// No transformation
    None,
    /// Transform to uppercase
    Uppercase,
    /// Transform to lowercase
    Lowercase,
    /// Capitalize first letter of each word
    Capitalize,
}

impl Default for TextTransform {
    fn default() -> Self {
        TextTransform::None
    }
}

/// White space handling.
///
/// Similar to CSS white-space property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WhiteSpace {
    /// Normal white space handling
    Normal,
    /// Collapse white space, no wrapping
    NoWrap,
    /// Preserve white space and line breaks
    Pre,
    /// Preserve white space, wrap at boundaries
    PreWrap,
    /// Collapse white space, wrap at boundaries
    PreLine,
}

impl Default for WhiteSpace {
    fn default() -> Self {
        WhiteSpace::Normal
    }
}

/// User select behavior.
///
/// Similar to CSS user-select.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UserSelect {
    /// Text can be selected
    Auto,
    /// Text can be selected
    Text,
    /// Text cannot be selected
    None,
    /// Select all on click
    All,
}

impl UserSelect {
    /// Check if selection is allowed.
    pub fn is_selectable(&self) -> bool {
        !matches!(self, UserSelect::None)
    }
}

impl Default for UserSelect {
    fn default() -> Self {
        UserSelect::Auto
    }
}

/// Resize behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Resize {
    /// Not resizable
    None,
    /// Resizable horizontally
    Horizontal,
    /// Resizable vertically
    Vertical,
    /// Resizable in both directions
    Both,
}

impl Resize {
    /// Check if horizontal resize is allowed.
    pub fn allows_horizontal(&self) -> bool {
        matches!(self, Resize::Horizontal | Resize::Both)
    }

    /// Check if vertical resize is allowed.
    pub fn allows_vertical(&self) -> bool {
        matches!(self, Resize::Vertical | Resize::Both)
    }
}

impl Default for Resize {
    fn default() -> Self {
        Resize::None
    }
}

/// Box sizing model.
///
/// Similar to CSS box-sizing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BoxSizing {
    /// Width/height includes only content
    ContentBox,
    /// Width/height includes padding and border
    BorderBox,
}

impl Default for BoxSizing {
    fn default() -> Self {
        BoxSizing::ContentBox
    }
}

/// Display type.
///
/// Simplified version of CSS display property.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Display {
    /// Block-level element
    Block,
    /// Inline element
    Inline,
    /// Inline-block element
    InlineBlock,
    /// Flex container
    Flex,
    /// Grid container
    Grid,
    /// Not displayed
    None,
}

impl Display {
    /// Check if element is displayed.
    pub fn is_displayed(&self) -> bool {
        !matches!(self, Display::None)
    }

    /// Check if element is a flex container.
    pub fn is_flex(&self) -> bool {
        matches!(self, Display::Flex)
    }

    /// Check if element is a grid container.
    pub fn is_grid(&self) -> bool {
        matches!(self, Display::Grid)
    }
}

impl Default for Display {
    fn default() -> Self {
        Display::Block
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visibility() {
        assert!(Visibility::Visible.is_visible());
        assert!(!Visibility::Hidden.is_visible());

        assert!(Visibility::Visible.takes_space());
        assert!(Visibility::Hidden.takes_space());
        assert!(!Visibility::Collapsed.takes_space());
    }

    #[test]
    fn test_pointer_events() {
        assert!(PointerEvents::Auto.is_enabled());
        assert!(!PointerEvents::None.is_enabled());
    }

    #[test]
    fn test_user_select() {
        assert!(UserSelect::Auto.is_selectable());
        assert!(UserSelect::Text.is_selectable());
        assert!(!UserSelect::None.is_selectable());
    }

    #[test]
    fn test_resize() {
        assert!(Resize::Horizontal.allows_horizontal());
        assert!(!Resize::Horizontal.allows_vertical());

        assert!(Resize::Vertical.allows_vertical());
        assert!(!Resize::Vertical.allows_horizontal());

        assert!(Resize::Both.allows_horizontal());
        assert!(Resize::Both.allows_vertical());

        assert!(!Resize::None.allows_horizontal());
        assert!(!Resize::None.allows_vertical());
    }

    #[test]
    fn test_display() {
        assert!(Display::Block.is_displayed());
        assert!(!Display::None.is_displayed());

        assert!(Display::Flex.is_flex());
        assert!(!Display::Block.is_flex());

        assert!(Display::Grid.is_grid());
        assert!(!Display::Block.is_grid());
    }

    #[test]
    fn test_defaults() {
        assert_eq!(Cursor::default(), Cursor::Default);
        assert_eq!(Visibility::default(), Visibility::Visible);
        assert_eq!(Overflow::default(), Overflow::Visible);
        assert_eq!(PointerEvents::default(), PointerEvents::Auto);
        assert_eq!(TextTransform::default(), TextTransform::None);
        assert_eq!(WhiteSpace::default(), WhiteSpace::Normal);
        assert_eq!(UserSelect::default(), UserSelect::Auto);
        assert_eq!(Resize::default(), Resize::None);
        assert_eq!(BoxSizing::default(), BoxSizing::ContentBox);
        assert_eq!(Display::default(), Display::Block);
    }
}
