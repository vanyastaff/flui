//! Spacer widget - creates empty space that expands in a flex layout
//!
//! A widget that takes up space proportional to its flex value in a Row, Column, or Flex.
//! Similar to Flutter's Spacer widget.
//!
//! Spacer is equivalent to an Expanded widget wrapping a zero-sized SizedBox.
//! It's a convenience widget for creating flexible empty space in flex layouts.
//!
//! # Usage Patterns
//!
//! ## 1. Equal Spacing
//! ```rust,ignore
//! Spacer::new()  // flex: 1
//! ```
//!
//! ## 2. Custom Proportions
//! ```rust,ignore
//! Spacer::with_flex(2)  // Takes 2x the space
//! ```

use flui_core::widget::{ParentDataWidget, Widget};
use flui_core::RenderNode;
use flui_rendering::FlexParentData;

/// A widget that creates flexible empty space in a Row, Column, or Flex.
///
/// Spacer is a convenience widget that wraps a zero-sized SizedBox with
/// flex properties. It takes up space proportionally to its flex value
/// but renders nothing.
///
/// ## Key Characteristics
///
/// - **No visual content**: Spacer renders nothing (zero-sized box)
/// - **Flexible sizing**: Expands based on flex factor
/// - **Flex container only**: Must be direct child of Row/Column/Flex
///
/// ## Layout Behavior
///
/// 1. Row/Column lays out inflexible children first
/// 2. Remaining space divided among Spacer/Expanded children by flex
/// 3. Spacer takes its proportional space but renders nothing
///
/// ## Common Use Cases
///
/// ### Push elements apart
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Text::new("Left"),
///         Spacer::new(),              // Pushes Text to edges
///         Text::new("Right"),
///     ])
/// ```
///
/// ### Center with equal margins
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Spacer::new(),
///         Button::new("Center"),
///         Spacer::new(),
///     ])
/// ```
///
/// ### Proportional spacing
/// ```rust,ignore
/// Column::new()
///     .children(vec![
///         Header::new(),
///         Spacer::with_flex(1),       // 1/4 of space
///         Content::new(),
///         Spacer::with_flex(3),       // 3/4 of space
///         Footer::new(),
///     ])
/// ```
///
/// ### Toolbar layout
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         IconButton::new("back"),
///         Spacer::new(),
///         Text::new("Title"),
///         Spacer::new(),
///         IconButton::new("menu"),
///     ])
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Push buttons to edges
/// Row::new()
///     .children(vec![
///         Button::new("Cancel"),
///         Spacer::new(),
///         Button::new("OK"),
///     ])
///
/// // Triple section layout
/// Row::new()
///     .children(vec![
///         Icon::new(),
///         Spacer::with_flex(2),
///         Text::new("Center"),
///         Spacer::with_flex(2),
///         Icon::new(),
///     ])
///
/// // Top and bottom elements
/// Column::new()
///     .children(vec![
///         TopBar::new(),
///         Spacer::new(),              // Fills middle
///         BottomBar::new(),
///     ])
/// ```
///
/// ## Spacer vs SizedBox
///
/// - **Spacer**: Flexible size, proportional to remaining space
/// - **SizedBox**: Fixed size, always the same dimensions
///
/// ## See Also
///
/// - Expanded: For flexible children with content
/// - SizedBox: For fixed-size empty space
/// - Padding: For consistent spacing around widgets
#[derive(Debug, Clone)]
pub struct Spacer {
    /// The flex factor.
    ///
    /// Determines how much space this Spacer gets relative to other
    /// flexible children (Expanded/Spacer) in the flex container.
    ///
    /// Default is 1.
    pub flex: i32,

    /// The child widget (always a zero-sized SizedBox).
    child: Widget,
}

impl Spacer {
    /// Creates a new Spacer with flex factor 1.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let spacer = Spacer::new();
    /// ```
    pub fn new() -> Self {
        use crate::SizedBox;
        Self {
            flex: 1,
            child: Widget::from(SizedBox::builder()
                .width(0.0)
                .height(0.0)
                .build()),
        }
    }

    /// Creates a Spacer with a custom flex factor.
    ///
    /// # Arguments
    ///
    /// * `flex` - The flex factor (must be positive)
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // This spacer takes 3x the space of flex: 1
    /// let spacer = Spacer::with_flex(3);
    /// ```
    pub fn with_flex(flex: i32) -> Self {
        use crate::SizedBox;
        Self {
            flex,
            child: Widget::from(SizedBox::builder()
                .width(0.0)
                .height(0.0)
                .build()),
        }
    }

    /// Validates Spacer configuration.
    ///
    /// Returns an error if flex is non-positive.
    pub fn validate(&self) -> Result<(), String> {
        if self.flex <= 0 {
            return Err(format!(
                "Invalid flex: {}. Spacer requires flex > 0.",
                self.flex
            ));
        }
        Ok(())
    }

    /// Creates FlexParentData for this Spacer.
    ///
    /// Always creates FlexParentData with FlexFit::Tight.
    pub fn create_parent_data(&self) -> FlexParentData {
        FlexParentData::expanded_with_flex(self.flex)
    }
}

impl Default for Spacer {
    fn default() -> Self {
        Self::new()
    }
}

// ========== ParentDataWidget Implementation ==========

impl ParentDataWidget for Spacer {
    fn apply_parent_data(&self, _render_object: &mut RenderNode) {
        // TODO: apply_parent_data needs DynRenderObject trait
        // This will be implemented when the render object trait is ready
    }

    fn child(&self) -> &Widget {
        &self.child
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(Spacer, parent_data);

/// Macro for creating Spacer with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // With default flex: 1
/// spacer!()
///
/// // With custom flex
/// spacer!(2)
/// ```
#[macro_export]
macro_rules! spacer {
    () => {
        $crate::Spacer::new()
    };
    ($flex:expr) => {
        $crate::Spacer::with_flex($flex)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spacer_new() {
        let spacer = Spacer::new();
        assert_eq!(spacer.flex, 1);
    }

    #[test]
    fn test_spacer_with_flex() {
        let spacer = Spacer::with_flex(2);
        assert_eq!(spacer.flex, 2);
    }

    #[test]
    fn test_spacer_with_flex_5() {
        let spacer = Spacer::with_flex(5);
        assert_eq!(spacer.flex, 5);
    }

    #[test]
    fn test_spacer_default() {
        let spacer = Spacer::default();
        assert_eq!(spacer.flex, 1);
    }

    #[test]
    fn test_spacer_validate_ok() {
        let spacer = Spacer::new();
        assert!(spacer.validate().is_ok());

        let spacer = Spacer::with_flex(5);
        assert!(spacer.validate().is_ok());
    }

    #[test]
    fn test_spacer_validate_zero_flex() {
        let spacer = Spacer::with_flex(0);
        assert!(spacer.validate().is_err());
    }

    #[test]
    fn test_spacer_validate_negative_flex() {
        let spacer = Spacer::with_flex(-1);
        assert!(spacer.validate().is_err());
    }

    #[test]
    fn test_spacer_create_parent_data() {
        let spacer = Spacer::new();
        let parent_data = spacer.create_parent_data();
        assert_eq!(parent_data.flex, Some(1));
    }

    #[test]
    fn test_spacer_create_parent_data_custom_flex() {
        let spacer = Spacer::with_flex(4);
        let parent_data = spacer.create_parent_data();
        assert_eq!(parent_data.flex, Some(4));
    }

    #[test]
    fn test_spacer_flex_factors() {
        let spacers = vec![
            Spacer::new(),
            Spacer::with_flex(2),
            Spacer::with_flex(3),
        ];

        assert_eq!(spacers[0].flex, 1);
        assert_eq!(spacers[1].flex, 2);
        assert_eq!(spacers[2].flex, 3);
    }
}
