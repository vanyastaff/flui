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
//!     child: Some(widget.into_element()),
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
use flui_core::element::Element;
use flui_core::render::RenderBoxExt;
use flui_core::view::{IntoElement, StatelessView};

use flui_core::BuildContext;
use flui_rendering::{FlexItemMetadata, RenderFlexItem};
use flui_types::layout::FlexFit;

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
#[derive(Builder)]
#[builder(
    on(String, into),
    on(i32, into),
    finish_fn(name = build_internal, vis = "")
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
    pub child: Option<Element>,
}

// Manual Debug implementation since  doesn't implement Debug
impl std::fmt::Debug for Flexible {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Flexible")
            .field("key", &self.key)
            .field("flex", &self.flex)
            .field("fit", &self.fit)
            .field("child", &if self.child.is_some() { "<>" } else { "None" })
            .finish()
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
    pub fn child(self, child: impl View + 'static) -> FlexibleBuilder<SetChild<S>> {
        self.child_internal(child.into_element())
    }
}

// Public build() wrapper
impl<S: State> FlexibleBuilder<S> {
    /// Builds the Flexible with optional validation.
    pub fn build(self) -> Flexible {
        let flexible = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = flexible.validate() {
                tracing::warn!("Flexible validation failed: {}", e);
            }
        }

        flexible
    }
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
    pub fn new(flex: i32, child: impl View + 'static) -> Self {
        Self {
            key: None,
            flex,
            fit: FlexFit::Loose,
            child: Some(child.into_element()),
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
    pub fn tight(flex: i32, child: impl View + 'static) -> Self {
        Self {
            key: None,
            flex,
            fit: FlexFit::Tight,
            child: Some(child.into_element()),
        }
    }

    /// Sets the child widget.
    #[deprecated(note = "Use builder pattern with .child() instead")]
    pub fn set_child(&mut self, child: Element) {
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

// Implement View trait - Simplified API
impl StatelessView for Flexible {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        RenderFlexItem::new(FlexItemMetadata {
            flex: self.flex,
            fit: self.fit,
        })
        .child_opt(self.child)
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

// Tests removed - used obsolete create_parent_data() and FlexFit APIs
