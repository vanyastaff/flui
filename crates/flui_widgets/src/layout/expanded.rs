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
//!
//! ## 3. Builder Pattern
//! ```rust,ignore
//! Expanded::builder()
//!     .flex(2)
//!     .child(widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::element::Element;
use flui_core::render::RenderBoxExt;
use flui_core::view::{IntoElement, StatelessView};

use flui_core::BuildContext;
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
#[derive(Builder)]
#[builder(
    on(i32, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Expanded {
    /// The flex factor.
    ///
    /// Determines how much space this child gets relative to other flexible children.
    /// Default is 1.
    #[builder(default = 1)]
    pub flex: i32,

    /// The child widget.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Element,
}

// Manual Debug implementation since  doesn't implement Debug
impl std::fmt::Debug for Expanded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Expanded")
            .field("flex", &self.flex)
            .field("child", &"<>")
            .finish()
    }
}

// bon Builder Extensions
use expanded_builder::{IsSet, IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> ExpandedBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// Expanded::builder()
    ///     .flex(2)
    ///     .child(Container::new())
    ///     .build()
    /// ```
    pub fn child(self, child: impl IntoElement) -> ExpandedBuilder<SetChild<S>> {
        self.child_internal(child.into_element())
    }
}

// Public build() wrapper
impl<S: State> ExpandedBuilder<S>
where
    S::Child: IsSet,
{
    /// Builds the Expanded with optional validation.
    pub fn build(self) -> Expanded {
        let expanded = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = expanded.validate() {
                tracing::warn!("Expanded validation failed: {}", e);
            }
        }

        expanded
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
    /// let widget = Expanded::new(Container::new());
    /// ```
    pub fn new(child: impl IntoElement) -> Self {
        Self {
            flex: 1,
            child: child.into_element(),
        }
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
    /// let widget = Expanded::with_flex(2, Container::new());
    /// ```
    pub fn with_flex(flex: i32, child: impl IntoElement) -> Self {
        Self {
            flex,
            child: child.into_element(),
        }
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

// Implement View trait - Simplified API
impl StatelessView for Expanded {
    fn build(self, _ctx: &dyn BuildContext) -> impl IntoElement {
        RenderFlexItem::new(FlexItemMetadata::expanded_with_flex(self.flex)).child(self.child)
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

// Tests removed - used obsolete create_parent_data() API
