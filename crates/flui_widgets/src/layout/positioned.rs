//! Positioned widget - positions a child within a Stack
//!
//! A widget that controls where a child of a Stack is positioned.
//! Similar to Flutter's Positioned widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Positioned {
//!     left: Some(10.0),
//!     top: Some(20.0),
//!     child: Some(Box::new(widget)),
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Positioned::builder()
//!     .left(10.0)
//!     .top(20.0)
//!     .child(widget)
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! positioned! {
//!     left: 10.0,
//!     top: 20.0,
//! }
//! ```

use bon::Builder;
use flui_core::{BoxedWidget, Widget, ParentDataWidget};
use flui_rendering::StackParentData;

/// A widget that controls where a child of a Stack is positioned.
///
/// Positioned must be a descendant of a Stack, and the path from the Positioned
/// to its enclosing Stack must contain only StatelessWidgets or StatefulWidgets
/// (not other kinds of widgets, like RenderObjectWidgets).
///
/// ## Positioning Rules
///
/// At least one of left, right, or width must be specified (or none for centered).
/// At least one of top, bottom, or height must be specified (or none for centered).
///
/// If both left and right are specified, width must be null.
/// If both top and bottom are specified, height must be null.
///
/// ## Layout Behavior
///
/// The positioned child is laid out with constraints based on its positioning:
///
/// - **left + right specified**: Width is determined by available space
/// - **left or right + width**: Child has fixed width, positioned from edge
/// - **Neither left nor right**: Child positioned using Stack's alignment
///
/// Same logic applies for top/bottom/height.
///
/// ## Common Use Cases
///
/// ### Top-Left Corner
/// ```rust,ignore
/// Positioned::builder()
///     .left(0.0)
///     .top(0.0)
///     .child(widget)
///     .build()
/// ```
///
/// ### Bottom-Right Corner
/// ```rust,ignore
/// Positioned::builder()
///     .right(0.0)
///     .bottom(0.0)
///     .child(widget)
///     .build()
/// ```
///
/// ### Centered with Size
/// ```rust,ignore
/// Positioned::builder()
///     .width(100.0)
///     .height(50.0)
///     .child(widget)
///     .build()
/// ```
///
/// ### Fill Available Space
/// ```rust,ignore
/// Positioned::fill(widget)
/// // Equivalent to:
/// Positioned::builder()
///     .left(0.0)
///     .top(0.0)
///     .right(0.0)
///     .bottom(0.0)
///     .child(widget)
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// Stack::new()
///     .children(vec![
///         // Background
///         Container::new().width(300.0).height(300.0),
///
///         // Top-left badge
///         Positioned::builder()
///             .left(10.0)
///             .top(10.0)
///             .child(Badge::new("New"))
///             .build(),
///
///         // Bottom-right FAB
///         Positioned::builder()
///             .right(16.0)
///             .bottom(16.0)
///             .child(FloatingActionButton::new())
///             .build(),
///
///         // Centered overlay
///         Positioned::builder()
///             .width(200.0)
///             .height(100.0)
///             .child(Card::new())
///             .build(),
///     ])
/// ```
///
/// ## See Also
///
/// - Stack: For overlaying multiple children
/// - Align: For simple alignment without absolute positioning
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(f32, into),
    finish_fn = build_positioned
)]
pub struct Positioned {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Distance from the left edge of the Stack.
    ///
    /// If both left and right are set, width must be null.
    pub left: Option<f32>,

    /// Distance from the top edge of the Stack.
    ///
    /// If both top and bottom are set, height must be null.
    pub top: Option<f32>,

    /// Distance from the right edge of the Stack.
    ///
    /// If both left and right are set, width must be null.
    pub right: Option<f32>,

    /// Distance from the bottom edge of the Stack.
    ///
    /// If both top and bottom are set, height must be null.
    pub bottom: Option<f32>,

    /// The width of the child.
    ///
    /// If both left and right are set, width must be null.
    pub width: Option<f32>,

    /// The height of the child.
    ///
    /// If both top and bottom are set, height must be null.
    pub height: Option<f32>,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<BoxedWidget>,
}

impl Positioned {
    /// Creates a new Positioned widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Positioned::new();
    /// ```
    pub fn new() -> Self {
        Self {
            key: None,
            left: None,
            top: None,
            right: None,
            bottom: None,
            width: None,
            height: None,
            child: None,
        }
    }

    /// Creates a Positioned that fills the entire Stack.
    ///
    /// Equivalent to setting left: 0, top: 0, right: 0, bottom: 0.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Positioned::fill(Container::new());
    /// ```
    pub fn fill(child: impl Widget + 'static) -> Self {
        Self {
            key: None,
            left: Some(0.0),
            top: Some(0.0),
            right: Some(0.0),
            bottom: Some(0.0),
            width: None,
            height: None,
            child: Some(BoxedWidget::new(child)),
        }
    }

    /// Creates a Positioned from rect coordinates.
    ///
    /// # Arguments
    ///
    /// * `left` - Distance from left edge
    /// * `top` - Distance from top edge
    /// * `width` - Width of child
    /// * `height` - Height of child
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = Positioned::from_rect(10.0, 20.0, 100.0, 50.0, Container::new());
    /// ```
    pub fn from_rect(
        left: f32,
        top: f32,
        width: f32,
        height: f32,
        child: impl Widget + 'static,
    ) -> Self {
        Self {
            key: None,
            left: Some(left),
            top: Some(top),
            right: None,
            bottom: None,
            width: Some(width),
            height: Some(height),
            child: Some(BoxedWidget::new(child)),
        }
    }

    /// Creates a Positioned with directional positioning.
    ///
    /// # Arguments
    ///
    /// * `start` - Distance from the start edge (left in LTR, right in RTL)
    /// * `top` - Distance from top edge
    /// * `end` - Distance from the end edge (right in LTR, left in RTL)
    /// * `bottom` - Distance from bottom edge
    ///
    /// Note: For now, this assumes LTR direction and uses left/right.
    /// TODO: Implement proper TextDirection support.
    pub fn directional(
        start: Option<f32>,
        top: Option<f32>,
        end: Option<f32>,
        bottom: Option<f32>,
        child: impl Widget + 'static,
    ) -> Self {
        Self {
            key: None,
            left: start,
            top,
            right: end,
            bottom,
            width: None,
            height: None,
            child: Some(BoxedWidget::new(child)),
        }
    }

    /// Sets the child widget.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let mut widget = Positioned::new();
    /// widget.set_child(Container::new());
    /// ```
    pub fn set_child<W>(&mut self, child: W)
    where
        W: Widget + std::fmt::Debug + Send + Sync + Clone + 'static,
    {
        self.child = Some(BoxedWidget::new(child));
    }

    /// Validates Positioned configuration.
    ///
    /// Returns an error if:
    /// - Both left and right are set but width is also set
    /// - Both top and bottom are set but height is also set
    /// - Any positioning value is NaN or infinite
    pub fn validate(&self) -> Result<(), String> {
        // Check for conflicting horizontal constraints
        if self.left.is_some() && self.right.is_some() && self.width.is_some() {
            return Err(
                "Cannot specify all of left, right, and width. Choose two at most.".to_string()
            );
        }

        // Check for conflicting vertical constraints
        if self.top.is_some() && self.bottom.is_some() && self.height.is_some() {
            return Err(
                "Cannot specify all of top, bottom, and height. Choose two at most.".to_string()
            );
        }

        // Check for NaN or infinity
        let values = [
            ("left", self.left),
            ("top", self.top),
            ("right", self.right),
            ("bottom", self.bottom),
            ("width", self.width),
            ("height", self.height),
        ];

        for (name, value) in values {
            if let Some(v) = value {
                if v.is_nan() {
                    return Err(format!("Invalid {}: NaN is not allowed", name));
                }
                if v.is_infinite() {
                    return Err(format!("Invalid {}: infinity is not allowed", name));
                }
            }
        }

        Ok(())
    }

    /// Returns true if this Positioned has any positioning data.
    pub fn is_positioned(&self) -> bool {
        self.left.is_some()
            || self.top.is_some()
            || self.right.is_some()
            || self.bottom.is_some()
            || self.width.is_some()
            || self.height.is_some()
    }

    /// Creates StackParentData for this Positioned.
    ///
    /// Converts Positioned positioning values into StackParentData.
    pub fn create_parent_data(&self) -> StackParentData {
        StackParentData::positioned(
            self.left,
            self.top,
            self.right,
            self.bottom,
            self.width,
            self.height,
        )
    }
}

impl Default for Positioned {
    fn default() -> Self {
        Self::new()
    }
}

// ========== ParentDataWidget Implementation ==========

impl ParentDataWidget for Positioned {
    type ParentDataType = StackParentData;

    fn apply_parent_data(&self, _render_object: &mut ()) {
        // TODO: apply_parent_data needs DynRenderObject trait
        // This will be implemented when the render object trait is ready
    }

    fn child(&self) -> &BoxedWidget {
        self.child.as_ref()
            .expect("Positioned must have a child")
    }
}

// bon Builder Extensions
use positioned_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> PositionedBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Positioned::builder()
    ///     .left(10.0)
    ///     .top(20.0)
    ///     .child(Container::new())
    ///     .build()
    /// ```
    pub fn child(self, child: impl Widget + 'static) -> PositionedBuilder<SetChild<S>> {
        self.child_internal(BoxedWidget::new(child))
    }
}

// Public build() wrapper
impl<S: State> PositionedBuilder<S> {
    /// Builds the Positioned widget.
    ///
    /// Equivalent to calling the generated `build_positioned()` finishing function.
    pub fn build(self) -> Positioned {
        self.build_positioned()
    }
}

/// Macro for creating Positioned with declarative syntax.
///
/// # Examples
///
/// ```rust,ignore
/// // Top-left corner
/// positioned! {
///     left: 0.0,
///     top: 0.0,
/// }
///
/// // Bottom-right with size
/// positioned! {
///     right: 10.0,
///     bottom: 10.0,
///     width: 100.0,
///     height: 50.0,
/// }
/// ```
#[macro_export]
macro_rules! positioned {
    () => {
        $crate::Positioned::default()
    };
    ($($field:ident : $value:expr),* $(,)?) => {
        $crate::Positioned {
            $($field: Some($value.into()),)*
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
    fn test_positioned_new() {
        let widget = Positioned::new();
        assert!(widget.key.is_none());
        assert!(widget.left.is_none());
        assert!(widget.top.is_none());
        assert!(widget.right.is_none());
        assert!(widget.bottom.is_none());
        assert!(widget.width.is_none());
        assert!(widget.height.is_none());
        assert!(widget.child.is_none());
        assert!(!widget.is_positioned());
    }

    #[test]
    fn test_positioned_default() {
        let widget = Positioned::default();
        assert!(!widget.is_positioned());
    }

    #[test]
    fn test_positioned_fill() {
        let widget = Positioned::fill(MockWidget::new("child"));
        assert_eq!(widget.left, Some(0.0));
        assert_eq!(widget.top, Some(0.0));
        assert_eq!(widget.right, Some(0.0));
        assert_eq!(widget.bottom, Some(0.0));
        assert!(widget.width.is_none());
        assert!(widget.height.is_none());
        assert!(widget.child.is_some());
        assert!(widget.is_positioned());
    }

    #[test]
    fn test_positioned_from_rect() {
        let widget = Positioned::from_rect(10.0, 20.0, 100.0, 50.0, MockWidget::new("child"));
        assert_eq!(widget.left, Some(10.0));
        assert_eq!(widget.top, Some(20.0));
        assert_eq!(widget.width, Some(100.0));
        assert_eq!(widget.height, Some(50.0));
        assert!(widget.right.is_none());
        assert!(widget.bottom.is_none());
        assert!(widget.is_positioned());
    }

    #[test]
    fn test_positioned_directional() {
        let widget = Positioned::directional(
            Some(10.0),
            Some(20.0),
            Some(30.0),
            Some(40.0),
            MockWidget::new("child"),
        );
        assert_eq!(widget.left, Some(10.0));
        assert_eq!(widget.top, Some(20.0));
        assert_eq!(widget.right, Some(30.0));
        assert_eq!(widget.bottom, Some(40.0));
        assert!(widget.is_positioned());
    }

    #[test]
    fn test_positioned_builder() {
        let widget = Positioned::builder()
            .left(10.0)
            .top(20.0)
            .width(100.0)
            .build();

        assert_eq!(widget.left, Some(10.0));
        assert_eq!(widget.top, Some(20.0));
        assert_eq!(widget.width, Some(100.0));
        assert!(widget.is_positioned());
    }

    #[test]
    fn test_positioned_builder_with_child() {
        let widget = Positioned::builder()
            .left(10.0)
            .top(20.0)
            .child(MockWidget::new("child"))
            .build();

        assert!(widget.child.is_some());
    }

    #[test]
    fn test_positioned_struct_literal() {
        let widget = Positioned {
            left: Some(15.0),
            top: Some(25.0),
            right: Some(35.0),
            bottom: Some(45.0),
            ..Default::default()
        };

        assert_eq!(widget.left, Some(15.0));
        assert_eq!(widget.top, Some(25.0));
        assert_eq!(widget.right, Some(35.0));
        assert_eq!(widget.bottom, Some(45.0));
    }

    #[test]
    fn test_positioned_validate_ok() {
        // Just left and top
        let widget = Positioned::builder().left(10.0).top(20.0).build();
        assert!(widget.validate().is_ok());

        // Left, top, width, height
        let widget = Positioned::builder()
            .left(10.0)
            .top(20.0)
            .width(100.0)
            .height(50.0)
            .build();
        assert!(widget.validate().is_ok());

        // Left, right (width determined by space)
        let widget = Positioned::builder()
            .left(10.0)
            .right(10.0)
            .build();
        assert!(widget.validate().is_ok());

        // Fill
        let widget = Positioned::fill(MockWidget::new("child"));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_positioned_validate_conflicting_horizontal() {
        let widget = Positioned {
            left: Some(10.0),
            right: Some(10.0),
            width: Some(100.0),
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_positioned_validate_conflicting_vertical() {
        let widget = Positioned {
            top: Some(10.0),
            bottom: Some(10.0),
            height: Some(50.0),
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_positioned_validate_nan() {
        let widget = Positioned {
            left: Some(f32::NAN),
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_positioned_validate_infinite() {
        let widget = Positioned {
            width: Some(f32::INFINITY),
            ..Default::default()
        };
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_positioned_macro_empty() {
        let widget = positioned!();
        assert!(!widget.is_positioned());
    }

    #[test]
    fn test_positioned_macro_with_fields() {
        let widget = positioned! {
            left: 10.0,
            top: 20.0,
        };
        assert_eq!(widget.left, Some(10.0));
        assert_eq!(widget.top, Some(20.0));
    }

    #[test]
    fn test_positioned_is_positioned() {
        let widget = Positioned::new();
        assert!(!widget.is_positioned());

        let widget = Positioned::builder().left(10.0).build();
        assert!(widget.is_positioned());

        let widget = Positioned::builder().width(100.0).build();
        assert!(widget.is_positioned());
    }

    #[test]
    fn test_positioned_set_child() {
        let mut widget = Positioned::new();
        assert!(widget.child.is_none());

        widget.set_child(MockWidget::new("child"));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_positioned_all_edges() {
        let widget = Positioned::builder()
            .left(5.0)
            .top(10.0)
            .right(15.0)
            .bottom(20.0)
            .build();

        assert_eq!(widget.left, Some(5.0));
        assert_eq!(widget.top, Some(10.0));
        assert_eq!(widget.right, Some(15.0));
        assert_eq!(widget.bottom, Some(20.0));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_positioned_with_size() {
        let widget = Positioned::builder()
            .width(200.0)
            .height(100.0)
            .build();

        assert_eq!(widget.width, Some(200.0));
        assert_eq!(widget.height, Some(100.0));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_positioned_only_left() {
        let widget = Positioned::builder().left(50.0).build();
        assert_eq!(widget.left, Some(50.0));
        assert!(widget.top.is_none());
        assert!(widget.is_positioned());
    }

    #[test]
    fn test_positioned_only_right() {
        let widget = Positioned::builder().right(50.0).build();
        assert_eq!(widget.right, Some(50.0));
        assert!(widget.is_positioned());
    }

    #[test]
    fn test_positioned_negative_values() {
        // Negative values are allowed for overflow effects
        let widget = Positioned::builder()
            .left(-10.0)
            .top(-20.0)
            .build();

        assert_eq!(widget.left, Some(-10.0));
        assert_eq!(widget.top, Some(-20.0));
        assert!(widget.validate().is_ok());
    }
}
