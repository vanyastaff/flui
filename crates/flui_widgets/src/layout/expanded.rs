//! Expanded widget - forces child to fill available space in Row/Column
//!
//! A widget that expands a child of a Row, Column, or Flex to fill the available space.
//! Similar to Flutter's Expanded widget.
//!
//! Expanded is a shorthand for Flexible with FlexFit::Tight.
//!
//! # Usage Patterns
//!
//! ## 1. Direct Construction
//! ```rust,ignore
//! Expanded::new(widget)
//! ```
//!
//! ## 2. With Custom Flex
//! ```rust,ignore
//! Expanded::with_flex(2, widget)
//! ```

use flui_core::view::{AnyView, ChangeFlags, View};
use flui_core::render::RenderNode;
use flui_core::{BuildContext, Element};
use flui_rendering::{FlexItemMetadata, RenderFlexItem};

/// A widget that expands a child of a Row, Column, or Flex to fill available space.
///
/// Expanded is equivalent to Flexible with FlexFit::Tight. It forces the child
/// to expand to fill the available space in the main axis.
///
/// ## Key Differences from Flexible
///
/// - **Flexible (FlexFit::Loose)**: Child can be smaller than allocated space
/// - **Expanded (FlexFit::Tight)**: Child must fill allocated space
///
/// ## Layout Behavior
///
/// 1. Row/Column lays out inflexible children first
/// 2. Remaining space is divided among Expanded/Flexible children based on flex
/// 3. Each Expanded child MUST fill its allocated space
///
/// ## Common Use Cases
///
/// ### Equal Width Columns
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Expanded::new(Container::new().color(Color::RED)),
///         Expanded::new(Container::new().color(Color::GREEN)),
///         Expanded::new(Container::new().color(Color::BLUE)),
///     ])
/// // Each column gets 1/3 of width and fills the full height
/// ```
///
/// ### Sidebar Layout
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Container::new().width(200.0),  // Fixed sidebar
///         Expanded::new(content_area),     // Content fills remaining width
///     ])
/// ```
///
/// ### Responsive Buttons
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Expanded::with_flex(1, Button::new("Cancel")),
///         SizedBox::new().width(8.0),
///         Expanded::with_flex(2, Button::new("Confirm")),  // 2x wider
///     ])
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Three equal columns
/// Row::new()
///     .children(vec![
///         Expanded::new(Text::new("Column 1")),
///         Expanded::new(Text::new("Column 2")),
///         Expanded::new(Text::new("Column 3")),
///     ])
///
/// // Proportional layout
/// Column::new()
///     .children(vec![
///         Expanded::with_flex(2, Header::new()),    // 2/5 of height
///         Expanded::with_flex(3, Content::new()),   // 3/5 of height
///     ])
///
/// // Mixed fixed and flexible
/// Row::new()
///     .children(vec![
///         Icon::new(),                              // Fixed size
///         Expanded::new(Text::new("Title")),        // Fills remaining space
///         Icon::new(),                              // Fixed size
///     ])
/// ```
///
/// ## See Also
///
/// - Flexible: For children that can be smaller than allocated space
/// - Row: Horizontal flex layout
/// - Column: Vertical flex layout
#[derive(Clone)]
pub struct Expanded {
    /// The flex factor.
    ///
    /// Determines how much space this child gets relative to other flexible children.
    /// Default is 1.
    pub flex: i32,

    /// The child widget.
    pub child: Box<dyn AnyView>,
}

// Manual Debug implementation since AnyView doesn't implement Debug
impl std::fmt::Debug for Expanded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expanded")
            .field("flex", &self.flex)
            .field("child", &"<AnyView>")
            .finish()
    }
}

impl Expanded {
    /// Creates a new Expanded widget with flex factor 1.
    ///
    /// # Arguments
    ///
    /// * `child` - The child widget to expand
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Expanded::new(Box::new(Container::new()));
    /// ```
    pub fn new(child: Box<dyn AnyView>) -> Self {
        Self { flex: 1, child }
    }

    /// Creates an Expanded widget with a custom flex factor.
    ///
    /// # Arguments
    ///
    /// * `flex` - The flex factor (must be positive)
    /// * `child` - The child widget
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // This child gets twice as much space as flex: 1
    /// let widget = Expanded::with_flex(2, Box::new(Container::new()));
    /// ```
    pub fn with_flex(flex: i32, child: Box<dyn AnyView>) -> Self {
        Self { flex, child }
    }

    /// Validates Expanded configuration.
    ///
    /// Returns an error if flex is non-positive.
    pub fn validate(&self) -> Result<(), String> {
        if self.flex <= 0 {
            return Err(format!(
                "Invalid flex: {}. Expanded requires flex > 0.",
                self.flex
            ));
        }
        Ok(())
    }
}

// Implement View trait
impl View for Expanded {
    type Element = Element;
    type State = ();

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child
        let (child_elem, _child_state) = self.child.build_any(ctx);
        let child_id = ctx.tree().write().insert(child_elem.into_element());

        // Create RenderFlexItem wrapper with FlexItemMetadata
        let render = RenderFlexItem::new(FlexItemMetadata::expanded_with_flex(self.flex));

        let render_node = RenderNode::Single {
            render: Box::new(render),
            child: Some(child_id),
        };

        let render_element = flui_core::element::RenderElement::new(render_node);
        (Element::Render(render_element), ())
    }

    fn rebuild(
        self,
        prev: &Self,
        _state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // TODO: Implement proper rebuild logic if needed
        // For now, return NONE as View architecture handles rebuilding
        ChangeFlags::NONE
    }
}

/// Macro for creating Expanded with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // With default flex: 1
/// expanded!(Container::new())
///
/// // With custom flex
/// expanded!(2, Container::new())
/// ```
#[macro_export]
macro_rules! expanded {
    ($child:expr) => {
        $crate::Expanded::new($child)
    };
    ($flex:expr, $child:expr) => {
        $crate::Expanded::with_flex($flex, $child)
    };
}

#[cfg(disabled_test)] // TODO: Update tests to new Widget API
mod tests {
    use super::*;

    // Mock widget for testing
    #[derive(Debug, Clone)]
    struct MockWidget {
        #[allow(dead_code)]
        id: String,
    }

    impl MockWidget {
        fn new(id: &str) -> Self {
            Self { id: id.to_string() }
        }
    }

    impl Widget for MockWidget {
        fn create_element(&self) -> Box<dyn flui_core::Element> {
            unimplemented!("MockWidget is for testing only")
        }
    }

    #[test]
    fn test_expanded_new() {
        let widget = Expanded::new(MockWidget::new("child"));
        assert_eq!(widget.flex, 1);
    }

    #[test]
    fn test_expanded_with_flex() {
        let widget = Expanded::with_flex(2, MockWidget::new("child"));
        assert_eq!(widget.flex, 2);
    }

    #[test]
    fn test_expanded_with_flex_3() {
        let widget = Expanded::with_flex(3, MockWidget::new("child"));
        assert_eq!(widget.flex, 3);
    }

    #[test]
    fn test_expanded_validate_ok() {
        let widget = Expanded::new(MockWidget::new("child"));
        assert!(widget.validate().is_ok());

        let widget = Expanded::with_flex(5, MockWidget::new("child"));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_expanded_validate_zero_flex() {
        let widget = Expanded::with_flex(0, MockWidget::new("child"));
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_expanded_validate_negative_flex() {
        let widget = Expanded::with_flex(-1, MockWidget::new("child"));
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_expanded_create_parent_data() {
        let widget = Expanded::new(MockWidget::new("child"));
        let parent_data = widget.create_parent_data();
        assert_eq!(parent_data.flex, Some(1));
        assert_eq!(parent_data.fit, FlexFit::Tight);
    }

    #[test]
    fn test_expanded_create_parent_data_custom_flex() {
        let widget = Expanded::with_flex(4, MockWidget::new("child"));
        let parent_data = widget.create_parent_data();
        assert_eq!(parent_data.flex, Some(4));
        assert_eq!(parent_data.fit, FlexFit::Tight);
    }

    #[test]
    fn test_expanded_macro_default_flex() {
        let widget = expanded!(MockWidget::new("child"));
        assert_eq!(widget.flex, 1);
    }

    #[test]
    fn test_expanded_macro_custom_flex() {
        let widget = expanded!(3, MockWidget::new("child"));
        assert_eq!(widget.flex, 3);
    }

    #[test]
    fn test_expanded_flex_factor_1() {
        let widget = Expanded::new(MockWidget::new("child"));
        assert_eq!(widget.flex, 1);
    }

    #[test]
    fn test_expanded_flex_factor_multiple() {
        let widgets = vec![
            Expanded::new(MockWidget::new("child1")),
            Expanded::with_flex(2, MockWidget::new("child2")),
            Expanded::with_flex(3, MockWidget::new("child3")),
        ];

        assert_eq!(widgets[0].flex, 1);
        assert_eq!(widgets[1].flex, 2);
        assert_eq!(widgets[2].flex, 3);
    }

    #[test]
    fn test_expanded_always_tight_fit() {
        // Expanded always uses FlexFit::Tight
        let widget = Expanded::new(MockWidget::new("child"));
        let parent_data = widget.create_parent_data();
        assert_eq!(parent_data.fit, FlexFit::Tight);

        let widget = Expanded::with_flex(5, MockWidget::new("child"));
        let parent_data = widget.create_parent_data();
        assert_eq!(parent_data.fit, FlexFit::Tight);
    }
}
