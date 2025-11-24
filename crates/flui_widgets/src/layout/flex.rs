//! Flex widget - generic flex layout container
//!
//! A widget that displays its children in a one-dimensional array.
//! Similar to Flutter's Flex widget.
//!
//! This is the base widget for Row (horizontal) and Column (vertical).
//!
//! # Usage Patterns
//!
//! ## 1. Chainable Builder Pattern
//! ```rust,ignore
//! // Chainable child() method (recommended for multiple children)
//! Flex::builder()
//!     .direction(Axis::Horizontal)
//!     .child(child1)
//!     .child(child2)
//!     .child(child3)
//!     .main_axis_alignment(MainAxisAlignment::Center)
//!     .build()
//!
//! // All children at once
//! Flex::builder()
//!     .direction(Axis::Horizontal)
//!     .children(vec![child1, child2, child3])
//!     .main_axis_alignment(MainAxisAlignment::Center)
//!     .build()
//! ```
//!
//! ## 2. Convenience Methods
//! ```rust,ignore
//! // Centered alignment
//! Flex::centered(Axis::Horizontal, vec![child1, child2])
//!
//! // Spaced with padding between items
//! Flex::spaced(Axis::Vertical, 16.0, vec![child1, child2])
//!
//! // Start aligned
//! Flex::start(Axis::Horizontal, vec![child1, child2])
//! ```

use bon::Builder;
use flui_core::element::Element;
use flui_core::render::RenderBoxExt;
use flui_core::view::children::Children;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::RenderFlex;
use flui_types::layout::{Axis, CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

use crate::SizedBox;

/// A widget that displays its children in a one-dimensional array.
///
/// Flex is the generic flex container that can be oriented horizontally or vertically.
/// For convenience, use Row (horizontal) or Column (vertical) instead of Flex directly.
///
/// ## Layout Behavior
///
/// - **Main axis**: The primary direction (horizontal or vertical)
/// - **Cross axis**: The perpendicular direction
/// - **Main axis size**: Can be `Max` (fill available space) or `Min` (shrink to children)
///
/// ## Examples
///
/// ```rust,ignore
/// // Horizontal flex (same as Row)
/// Flex::builder()
///     .direction(Axis::Horizontal)
///     .main_axis_alignment(MainAxisAlignment::SpaceBetween)
///     .children(vec![
///         Box::new(Text::new("Left")),
///         Box::new(Text::new("Right")),
///     ])
///     .build()
///
/// // Vertical flex (same as Column)
/// Flex::builder()
///     .direction(Axis::Vertical)
///     .main_axis_alignment(MainAxisAlignment::Center)
///     .children(vec![
///         Box::new(Text::new("Top")),
///         Box::new(Text::new("Bottom")),
///     ])
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Axis, into),
    on(MainAxisAlignment, into),
    on(CrossAxisAlignment, into),
    on(MainAxisSize, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Flex {
    /// The children widgets.
    ///
    /// Can be set via:
    /// - `.children(vec![...])` to set all at once
    /// - `.child(widget)` repeatedly to add one at a time (chainable)
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Children,

    /// Optional key for widget identification
    pub key: Option<String>,

    /// The direction to use as the main axis.
    ///
    /// - Axis::Horizontal: Children laid out left-to-right (Row)
    /// - Axis::Vertical: Children laid out top-to-bottom (Column)
    #[builder(default = Axis::Horizontal)]
    pub direction: Axis,

    /// How children should be placed along the main axis.
    ///
    /// Defaults to MainAxisAlignment::Start if not specified.
    #[builder(default = MainAxisAlignment::Start)]
    pub main_axis_alignment: MainAxisAlignment,

    /// How children should be aligned along the cross axis.
    ///
    /// Defaults to CrossAxisAlignment::Center if not specified.
    #[builder(default = CrossAxisAlignment::Center)]
    pub cross_axis_alignment: CrossAxisAlignment,

    /// How much space should be occupied in the main axis.
    ///
    /// - MainAxisSize::Max: Fills all available space (default)
    /// - MainAxisSize::Min: Shrinks to fit children
    #[builder(default = MainAxisSize::Max)]
    pub main_axis_size: MainAxisSize,
}

impl std::fmt::Debug for Flex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flex")
            .field("children", &format!("[{} children]", self.children.len()))
            .field("key", &self.key)
            .field("direction", &self.direction)
            .field("main_axis_alignment", &self.main_axis_alignment)
            .field("cross_axis_alignment", &self.cross_axis_alignment)
            .field("main_axis_size", &self.main_axis_size)
            .finish()
    }
}

// bon Builder Extensions - Custom builder methods for FlexBuilder
use flex_builder::{IsUnset, SetChildren, State};

impl<S: State> FlexBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets all children at once.
    ///
    /// # Example
    /// ```rust,ignore
    /// Flex::builder()
    ///     .direction(Axis::Horizontal)
    ///     .children(vec![child1, child2, child3])
    ///     .build()
    /// ```
    pub fn children(self, children: impl Into<Children>) -> FlexBuilder<SetChildren<S>> {
        self.children_internal(children.into())
    }
}

impl<S: State> FlexBuilder<S> {
    /// Builds the Flex with optional validation.
    pub fn build(self) -> Flex {
        let flex = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = flex.validate() {
                tracing::warn!("Flex validation failed: {}", e);
            }
        }

        flex
    }
}

impl Flex {
    /// Validates the Flex configuration
    fn validate(&self) -> Result<(), String> {
        // Add validation logic if needed
        Ok(())
    }

    /// Creates a new Flex widget with the specified direction.
    ///
    /// # Parameters
    ///
    /// - `direction`: The main axis direction (Horizontal or Vertical)
    pub fn new(direction: Axis) -> Self {
        Self {
            children: Children::default(),
            key: None,
            direction,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
        }
    }

    /// Creates a horizontal Flex (same as Row).
    pub fn horizontal() -> Self {
        Self::new(Axis::Horizontal)
    }

    /// Creates a vertical Flex (same as Column).
    pub fn vertical() -> Self {
        Self::new(Axis::Vertical)
    }

    // ========================================================================
    // Convenience Methods
    // ========================================================================

    /// Creates a Flex with centered alignment.
    ///
    /// Both main axis and cross axis are centered.
    ///
    /// # Example
    /// ```rust,ignore
    /// let flex = Flex::centered(Axis::Horizontal, vec![child1, child2]);
    /// ```
    pub fn centered(direction: Axis, children: impl Into<Children>) -> Self {
        Self::builder()
            .direction(direction)
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(children)
            .build()
    }

    /// Creates a Flex with spacing between children.
    ///
    /// Automatically inserts SizedBox spacers between children.
    ///
    /// # Example
    /// ```rust,ignore
    /// // Vertical layout with 16px spacing
    /// let flex = Flex::spaced(Axis::Vertical, 16.0, vec![child1, child2]);
    /// ```
    pub fn spaced(direction: Axis, spacing: f32, children: impl Into<Children>) -> Self {
        let children: Children = children.into();
        let children_vec = children.into_inner();

        if children_vec.is_empty() {
            return Self::builder()
                .direction(direction)
                .children(Vec::<Element>::new())
                .build();
        }

        let mut spaced_children = Vec::with_capacity(children_vec.len() * 2 - 1);

        for (i, child) in children_vec.into_iter().enumerate() {
            if i > 0 {
                // Add spacer between children
                let spacer: Element = match direction {
                    Axis::Horizontal => SizedBox::h_space(spacing).into_element(),
                    Axis::Vertical => SizedBox::v_space(spacing).into_element(),
                };
                spaced_children.push(spacer);
            }
            spaced_children.push(child);
        }

        Self::builder()
            .direction(direction)
            .children(spaced_children)
            .build()
    }

    /// Creates a Flex with start alignment.
    ///
    /// Children are aligned at the start of the main axis.
    ///
    /// # Example
    /// ```rust,ignore
    /// let flex = Flex::start(Axis::Horizontal, vec![child1, child2]);
    /// ```
    pub fn start(direction: Axis, children: impl Into<Children>) -> Self {
        Self::builder()
            .direction(direction)
            .main_axis_alignment(MainAxisAlignment::Start)
            .children(children)
            .build()
    }

    /// Creates a Flex with end alignment.
    ///
    /// Children are aligned at the end of the main axis.
    ///
    /// # Example
    /// ```rust,ignore
    /// let flex = Flex::end(Axis::Horizontal, vec![child1, child2]);
    /// ```
    pub fn end(direction: Axis, children: impl Into<Children>) -> Self {
        Self::builder()
            .direction(direction)
            .main_axis_alignment(MainAxisAlignment::End)
            .children(children)
            .build()
    }

    /// Creates a Flex with space-between alignment.
    ///
    /// Children are evenly distributed with space between them.
    ///
    /// # Example
    /// ```rust,ignore
    /// let flex = Flex::space_between(Axis::Horizontal, vec![child1, child2, child3]);
    /// ```
    pub fn space_between(direction: Axis, children: impl Into<Children>) -> Self {
        Self::builder()
            .direction(direction)
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .children(children)
            .build()
    }

    /// Creates a Flex with space-around alignment.
    ///
    /// Children are evenly distributed with space around them.
    ///
    /// # Example
    /// ```rust,ignore
    /// let flex = Flex::space_around(Axis::Horizontal, vec![child1, child2, child3]);
    /// ```
    pub fn space_around(direction: Axis, children: impl Into<Children>) -> Self {
        Self::builder()
            .direction(direction)
            .main_axis_alignment(MainAxisAlignment::SpaceAround)
            .children(children)
            .build()
    }

    /// Creates a Flex with space-evenly alignment.
    ///
    /// Children are evenly distributed with equal space between and around them.
    ///
    /// # Example
    /// ```rust,ignore
    /// let flex = Flex::space_evenly(Axis::Horizontal, vec![child1, child2, child3]);
    /// ```
    pub fn space_evenly(direction: Axis, children: impl Into<Children>) -> Self {
        Self::builder()
            .direction(direction)
            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .children(children)
            .build()
    }
}

impl Default for Flex {
    fn default() -> Self {
        Self::new(Axis::Horizontal)
    }
}

// Implement View trait
impl StatelessView for Flex {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderFlex::new(self.direction)
            .with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_main_axis_size(self.main_axis_size)
            .children(self.children)
    }
}

/// Macro for creating Flex with declarative syntax.
#[macro_export]
macro_rules! flex {
    (direction: $direction:expr) => {
        $crate::Flex::new($direction)
    };
    (horizontal) => {
        $crate::Flex::horizontal()
    };
    (vertical) => {
        $crate::Flex::vertical()
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::testing::test_build_context;
    use flui_core::view::build_context::with_build_context;

    #[test]
    fn test_flex_new() {
        let widget = Flex::new(Axis::Horizontal);
        assert!(widget.key.is_none());
        assert_eq!(widget.direction, Axis::Horizontal);
        assert_eq!(widget.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(widget.cross_axis_alignment, CrossAxisAlignment::Center);
        assert_eq!(widget.main_axis_size, MainAxisSize::Max);
        assert!(widget.children.is_empty());
    }

    #[test]
    fn test_flex_horizontal() {
        let widget = Flex::horizontal();
        assert_eq!(widget.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_vertical() {
        let widget = Flex::vertical();
        assert_eq!(widget.direction, Axis::Vertical);
    }

    #[test]
    fn test_flex_default() {
        let widget = Flex::default();
        assert_eq!(widget.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_builder() {
        let widget = Flex::builder()
            .direction(Axis::Vertical)
            .main_axis_alignment(MainAxisAlignment::Center)
            .build();
        assert_eq!(widget.direction, Axis::Vertical);
        assert_eq!(widget.main_axis_alignment, MainAxisAlignment::Center);
    }

    #[test]
    fn test_flex_chainable_child() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::builder()
                .direction(Axis::Horizontal)
                .children(vec![
                    crate::SizedBox::new(),
                    crate::SizedBox::new(),
                    crate::SizedBox::new(),
                ])
                .build();
            assert_eq!(widget.direction, Axis::Horizontal);
            assert_eq!(widget.children.len(), 3);
        });
    }

    #[test]
    fn test_flex_centered() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::centered(
                Axis::Horizontal,
                vec![crate::SizedBox::new(), crate::SizedBox::new()],
            );
            assert_eq!(widget.direction, Axis::Horizontal);
            assert_eq!(widget.main_axis_alignment, MainAxisAlignment::Center);
            assert_eq!(widget.cross_axis_alignment, CrossAxisAlignment::Center);
            assert_eq!(widget.children.len(), 2);
        });
    }

    #[test]
    fn test_flex_spaced() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::spaced(
                Axis::Vertical,
                16.0,
                vec![
                    crate::SizedBox::new(),
                    crate::SizedBox::new(),
                    crate::SizedBox::new(),
                ],
            );
            assert_eq!(widget.direction, Axis::Vertical);
            // 3 children + 2 spacers = 5 total
            assert_eq!(widget.children.len(), 5);
        });
    }

    #[test]
    fn test_flex_spaced_empty() {
        let widget = Flex::spaced(Axis::Horizontal, 16.0, Vec::<Element>::new());
        assert_eq!(widget.children.len(), 0);
    }

    #[test]
    fn test_flex_start() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::start(Axis::Horizontal, vec![crate::SizedBox::new()]);
            assert_eq!(widget.main_axis_alignment, MainAxisAlignment::Start);
        });
    }

    #[test]
    fn test_flex_end() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::end(Axis::Horizontal, vec![crate::SizedBox::new()]);
            assert_eq!(widget.main_axis_alignment, MainAxisAlignment::End);
        });
    }

    #[test]
    fn test_flex_space_between() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::space_between(Axis::Horizontal, vec![crate::SizedBox::new()]);
            assert_eq!(widget.main_axis_alignment, MainAxisAlignment::SpaceBetween);
        });
    }

    #[test]
    fn test_flex_space_around() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::space_around(Axis::Horizontal, vec![crate::SizedBox::new()]);
            assert_eq!(widget.main_axis_alignment, MainAxisAlignment::SpaceAround);
        });
    }

    #[test]
    fn test_flex_space_evenly() {
        let ctx = test_build_context();
        with_build_context(&ctx, || {
            let widget = Flex::space_evenly(Axis::Horizontal, vec![crate::SizedBox::new()]);
            assert_eq!(widget.main_axis_alignment, MainAxisAlignment::SpaceEvenly);
        });
    }

    #[test]
    fn test_flex_macro_horizontal() {
        let widget = flex!(horizontal);
        assert_eq!(widget.direction, Axis::Horizontal);
    }

    #[test]
    fn test_flex_macro_vertical() {
        let widget = flex!(vertical);
        assert_eq!(widget.direction, Axis::Vertical);
    }

    #[test]
    fn test_flex_macro_direction() {
        let widget = flex!(direction: Axis::Horizontal);
        assert_eq!(widget.direction, Axis::Horizontal);
    }
}
