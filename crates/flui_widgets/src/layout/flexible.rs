//! Flexible widget - controls how a child flexes in Row/Column
//!
//! A widget that controls how a child of a Row, Column, or Flex flexes.
//! Similar to Flutter's Flexible widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Flexible {
//!     flex: 1,
//!     child: Some(Box::new(widget)),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Flexible::builder()
//!     .flex(2)
//!     .child(widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! flexible! {
//!     flex: 1,
//! }
//! ```

use bon::Builder;
use flui_core::widget::{ParentDataWidget, Widget};
use flui_core::RenderNode;
use flui_rendering::{FlexFit, FlexParentData};

/// A widget that controls how a child of a Row, Column, or Flex flexes.
///
/// Flexible allows a child of Row, Column, or Flex to expand to fill the available
/// space in the main axis. The flex factor determines how much space the child gets
/// relative to other flexible children.
///
/// ## Flex Factor
///
/// The flex factor determines the ratio of space this child gets compared to other
/// flexible children:
///
/// - `flex: 1` - Gets 1 unit of remaining space
/// - `flex: 2` - Gets 2 units (twice as much as flex: 1)
/// - `flex: 0` - Treated as inflexible (same as not wrapping in Flexible)
///
/// ## Fit Modes
///
/// - `FlexFit::Loose` (default) - Child can be smaller than allocated space
/// - `FlexFit::Tight` - Child must fill allocated space (used by Expanded)
///
/// ## Layout Behavior
///
/// 1. Row/Column lays out inflexible children first
/// 2. Remaining space is divided among flexible children based on flex factors
/// 3. Each flexible child gets: `(remaining_space * flex) / total_flex`
///
/// ## Common Use Cases
///
/// ### Equal Distribution
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Flexible::new(1, Container::new()),  // 1/3 of space
///         Flexible::new(1, Container::new()),  // 1/3 of space
///         Flexible::new(1, Container::new()),  // 1/3 of space
///     ])
/// ```
///
/// ### Proportional Distribution
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Flexible::new(1, Container::new()),  // 1/4 of space
///         Flexible::new(3, Container::new()),  // 3/4 of space
///     ])
/// ```
///
/// ### Mixed Flexible and Fixed
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         Container::new().width(50.0),        // Fixed 50px
///         Flexible::new(1, Container::new()),  // Gets remaining space
///         Container::new().width(100.0),       // Fixed 100px
///     ])
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Sidebar layout
/// Row::new()
///     .children(vec![
///         // Fixed sidebar
///         Container::new().width(200.0).color(Color::GREY),
///
///         // Flexible content area
///         Flexible::new(1, Container::new().color(Color::WHITE)),
///     ])
///
/// // Responsive buttons
/// Row::new()
///     .children(vec![
///         Flexible::new(1, Button::new("Cancel")),
///         SizedBox::new().width(8.0),  // Spacing
///         Flexible::new(2, Button::new("Confirm")),  // Twice as wide
///     ])
/// ```
///
/// ## See Also
///
/// - Expanded: A Flexible with FlexFit::Tight (forces child to fill space)
/// - Row: Horizontal flex layout
/// - Column: Vertical flex layout
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(i32, into),
    finish_fn = build_flexible
)]
pub struct Flexible {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The flex factor.
    ///
    /// Determines how much space this child gets relative to other flexible children.
    /// Must be non-negative. A flex factor of 0 is treated as inflexible.
    #[builder(default = 1)]
    pub flex: i32,

    /// How the child is inscribed into the available space.
    ///
    /// - `FlexFit::Loose` - Child can be smaller than allocated space (default)
    /// - `FlexFit::Tight` - Child must fill allocated space
    #[builder(default = FlexFit::Loose)]
    pub fit: FlexFit,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl Flexible {
    /// Creates a new Flexible widget.
    ///
    /// # Arguments
    ///
    /// * `flex` - The flex factor (must be non-negative)
    /// * `child` - The child widget
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Flexible::new(1, Container::new());
    /// ```
    pub fn new(flex: i32, child: Widget) -> Self {
        Self {
            key: None,
            flex,
            fit: FlexFit::Loose,
            child: Some(child),
        }
    }

    /// Creates a Flexible with FlexFit::Tight.
    ///
    /// This is equivalent to using Expanded widget.
    ///
    /// # Arguments
    ///
    /// * `flex` - The flex factor
    /// * `child` - The child widget
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Flexible::tight(2, Container::new());
    /// ```
    pub fn tight(flex: i32, child: Widget) -> Self {
        Self {
            key: None,
            flex,
            fit: FlexFit::Tight,
            child: Some(child),
        }
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = Flexible::builder().flex(1).build();
    /// widget.set_child(Container::new());
    /// ```
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
    }

    /// Validates Flexible configuration.
    ///
    /// Returns an error if:
    /// - flex is negative
    /// - child is None
    pub fn validate(&self) -> Result<(), String> {
        if self.flex < 0 {
            return Err(format!(
                "Invalid flex: {}. Flex factor must be non-negative.",
                self.flex
            ));
        }

        if self.child.is_none() {
            return Err("Flexible requires a child widget.".to_string());
        }

        Ok(())
    }

    /// Creates FlexParentData for this Flexible.
    ///
    /// This is used internally to communicate flex information to the parent
    /// Row/Column/Flex layout.
    pub fn create_parent_data(&self) -> FlexParentData {
        if self.flex > 0 {
            FlexParentData::new(self.flex, self.fit)
        } else {
            // flex: 0 is treated as inflexible
            FlexParentData::new(0, FlexFit::Loose)
        }
    }
}

impl Default for Flexible {
    fn default() -> Self {
        Self {
            key: None,
            flex: 1,
            fit: FlexFit::Loose,
            child: None,
        }
    }
}

// ========== ParentDataWidget Implementation ==========

impl ParentDataWidget for Flexible {
    fn apply_parent_data(&self, _render_object: &mut RenderNode) {
        // TODO: apply_parent_data needs DynRenderObject trait
        // This will be implemented when the render object trait is ready
    }

    fn child(&self) -> &Widget {
        self.child.as_ref().expect("Flexible must have a child")
    }
}

// bon Builder Extensions
use flexible_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> FlexibleBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Flexible::builder()
    ///     .flex(2)
    ///     .child(Container::new())
    ///     .build()
    /// ```
    pub fn child(self, child: Widget) -> FlexibleBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

// Public build() wrapper
impl<S: State> FlexibleBuilder<S> {
    /// Builds the Flexible widget.
    ///
    /// Equivalent to calling the generated `build_flexible()` finishing function.
    pub fn build(self) -> Flexible {
        self.build_flexible()
    }
}

/// Macro for creating Flexible with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Default flex: 1
/// flexible! {}
///
/// // Custom flex
/// flexible! {
///     flex: 2,
/// }
///
/// // With tight fit
/// flexible! {
///     flex: 1,
///     fit: FlexFit::Tight,
/// }
/// ```
#[macro_export]
macro_rules! flexible {
    () => {
        $crate::Flexible::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Flexible {
            $($field: $value.into(),)*
            ..Default::default()
        }
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
    fn test_flexible_new() {
        let widget = Flexible::new(1, MockWidget::new("child"));
        assert!(widget.key.is_none());
        assert_eq!(widget.flex, 1);
        assert_eq!(widget.fit, FlexFit::Loose);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_flexible_tight() {
        let widget = Flexible::tight(2, MockWidget::new("child"));
        assert_eq!(widget.flex, 2);
        assert_eq!(widget.fit, FlexFit::Tight);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_flexible_default() {
        let widget = Flexible::default();
        assert_eq!(widget.flex, 1);
        assert_eq!(widget.fit, FlexFit::Loose);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_flexible_builder() {
        let widget = Flexible::builder().flex(3).fit(FlexFit::Tight).build();

        assert_eq!(widget.flex, 3);
        assert_eq!(widget.fit, FlexFit::Tight);
    }

    #[test]
    fn test_flexible_builder_with_child() {
        let widget = Flexible::builder()
            .flex(2)
            .child(MockWidget::new("child"))
            .build();

        assert_eq!(widget.flex, 2);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_flexible_struct_literal() {
        let widget = Flexible {
            flex: 5,
            fit: FlexFit::Tight,
            child: Some(Box::new(MockWidget::new("child"))),
            ..Default::default()
        };

        assert_eq!(widget.flex, 5);
        assert_eq!(widget.fit, FlexFit::Tight);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_flexible_validate_ok() {
        let widget = Flexible::new(1, MockWidget::new("child"));
        assert!(widget.validate().is_ok());

        let widget = Flexible::new(0, MockWidget::new("child"));
        assert!(widget.validate().is_ok());

        let widget = Flexible::new(100, MockWidget::new("child"));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_flexible_validate_negative_flex() {
        let widget = Flexible::new(-1, MockWidget::new("child"));
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_flexible_validate_no_child() {
        let widget = Flexible::default();
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_flexible_macro_empty() {
        let widget = flexible!();
        assert_eq!(widget.flex, 1);
        assert_eq!(widget.fit, FlexFit::Loose);
    }

    #[test]
    fn test_flexible_macro_with_flex() {
        let widget = flexible! {
            flex: 4,
        };
        assert_eq!(widget.flex, 4);
    }

    #[test]
    fn test_flexible_macro_with_fit() {
        let widget = flexible! {
            flex: 2,
            fit: FlexFit::Tight,
        };
        assert_eq!(widget.flex, 2);
        assert_eq!(widget.fit, FlexFit::Tight);
    }

    #[test]
    fn test_flexible_set_child() {
        let mut widget = Flexible::default();
        assert!(widget.child.is_none());

        widget.set_child(MockWidget::new("child"));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_flexible_create_parent_data() {
        let widget = Flexible::new(2, MockWidget::new("child"));
        let parent_data = widget.create_parent_data();
        assert_eq!(parent_data.flex, Some(2));
        assert_eq!(parent_data.fit, FlexFit::Loose);
    }

    #[test]
    fn test_flexible_create_parent_data_tight() {
        let widget = Flexible::tight(3, MockWidget::new("child"));
        let parent_data = widget.create_parent_data();
        assert_eq!(parent_data.flex, Some(3));
        assert_eq!(parent_data.fit, FlexFit::Tight);
    }

    #[test]
    fn test_flexible_create_parent_data_zero_flex() {
        let widget = Flexible::new(0, MockWidget::new("child"));
        let parent_data = widget.create_parent_data();
        assert_eq!(parent_data.flex, None);
    }

    #[test]
    fn test_flexible_flex_factor_1() {
        let widget = Flexible::new(1, MockWidget::new("child"));
        assert_eq!(widget.flex, 1);
    }

    #[test]
    fn test_flexible_flex_factor_multiple() {
        let widgets = vec![
            Flexible::new(1, MockWidget::new("child1")),
            Flexible::new(2, MockWidget::new("child2")),
            Flexible::new(3, MockWidget::new("child3")),
        ];

        assert_eq!(widgets[0].flex, 1);
        assert_eq!(widgets[1].flex, 2);
        assert_eq!(widgets[2].flex, 3);
    }

    #[test]
    fn test_flexible_both_fit_types() {
        let loose = Flexible::new(1, MockWidget::new("child"));
        let tight = Flexible::tight(1, MockWidget::new("child"));

        assert_eq!(loose.fit, FlexFit::Loose);
        assert_eq!(tight.fit, FlexFit::Tight);
    }
}
