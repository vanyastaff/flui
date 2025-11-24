//! Column widget - vertical flex layout
//!
//! A widget that displays its children in a vertical array.
//! Similar to Flutter's Column widget.
//!
//! # Usage Patterns
//!
//! ## 1. Struct Literal
//! ```rust,ignore
//! Column {
//!     main_axis_alignment: MainAxisAlignment::Center,
//!     children: vec![child1, child2],
//!     ..Default::default()
//! }
//! ```
//!
//! ## 2. Builder Pattern
//! ```rust,ignore
//! Column::builder()
//!     .main_axis_alignment(MainAxisAlignment::Center)
//!     .cross_axis_alignment(CrossAxisAlignment::Start)
//!     .children(vec![child1, child2])
//!     .build()
//! ```
//!
//! ## 3. Macro
//! ```rust,ignore
//! column! {
//!     main_axis_alignment: MainAxisAlignment::Center,
//!     children: vec![child1, child2],
//! }
//! ```

use bon::Builder;
use flui_core::render::RenderBoxExt;
use flui_core::view::children::Children;
use flui_core::view::{IntoElement, StatelessView};
use flui_core::BuildContext;
use flui_rendering::RenderFlex;
use flui_types::layout::{CrossAxisAlignment, MainAxisAlignment, MainAxisSize};

use crate::SizedBox;

/// A widget that displays its children in a vertical array.
///
/// Column is a flex container that lays out its children vertically (along the y-axis).
/// The children are positioned according to the main axis and cross axis alignment.
///
/// ## Layout Behavior
///
/// - **Main axis** (vertical): Children are laid out top-to-bottom
/// - **Cross axis** (horizontal): Children alignment depends on cross_axis_alignment
/// - **Main axis size**: Can be `Max` (fill available height) or `Min` (shrink to children)
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple column with centered children
/// Column::builder()
///     .main_axis_alignment(MainAxisAlignment::Center)
///     .children(vec![
///         Text::new("Hello").into_element(),
///         Text::new("World").into_element(),
///     ])
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(MainAxisAlignment, into),
    on(CrossAxisAlignment, into),
    on(MainAxisSize, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct Column {
    /// The widgets to display in this column.
    ///
    /// Children are laid out vertically (top-to-bottom) in the order they appear in the vector.
    #[builder(default, setters(vis = "", name = children_internal))]
    pub children: Children,

    /// Optional key for widget identification
    pub key: Option<String>,

    /// How children should be placed along the main axis (vertical).
    ///
    /// Defaults to MainAxisAlignment::Start if not specified.
    #[builder(default = MainAxisAlignment::Start)]
    pub main_axis_alignment: MainAxisAlignment,

    /// How children should be aligned along the cross axis (horizontal).
    ///
    /// Defaults to CrossAxisAlignment::Center if not specified.
    #[builder(default = CrossAxisAlignment::Center)]
    pub cross_axis_alignment: CrossAxisAlignment,

    /// How much space should be occupied in the main axis.
    ///
    /// - `MainAxisSize::Max`: Column takes all available vertical space
    /// - `MainAxisSize::Min`: Column shrinks to fit children
    ///
    /// Defaults to MainAxisSize::Max.
    #[builder(default = MainAxisSize::Max)]
    pub main_axis_size: MainAxisSize,
}

impl std::fmt::Debug for Column {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Column")
            .field("children", &format!("[{} children]", self.children.len()))
            .field("key", &self.key)
            .field("main_axis_alignment", &self.main_axis_alignment)
            .field("cross_axis_alignment", &self.cross_axis_alignment)
            .field("main_axis_size", &self.main_axis_size)
            .finish()
    }
}

// bon Builder Extensions - Custom builder methods for ColumnBuilder
use column_builder::{IsUnset, SetChildren, State};

impl<S: State> ColumnBuilder<S>
where
    S::Children: IsUnset,
{
    /// Sets all children at once.
    pub fn children(self, children: impl Into<Children>) -> ColumnBuilder<SetChildren<S>> {
        self.children_internal(children.into())
    }
}

impl<S: State> ColumnBuilder<S> {
    /// Builds the Column with optional validation.
    pub fn build(self) -> Column {
        let column = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = column.validate() {
                tracing::warn!("Column validation failed: {}", e);
            }
        }

        column
    }
}

impl Column {
    /// Creates a new empty Column with default values.
    pub fn new() -> Self {
        Self {
            children: Children::default(),
            key: None,
            main_axis_alignment: MainAxisAlignment::Start,
            cross_axis_alignment: CrossAxisAlignment::Center,
            main_axis_size: MainAxisSize::Max,
        }
    }

    // ========================================================================
    // Convenience Methods
    // ========================================================================

    /// Creates a Column with centered alignment.
    ///
    /// Both main axis and cross axis are centered.
    pub fn centered(children: impl Into<Children>) -> Self {
        Self::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .cross_axis_alignment(CrossAxisAlignment::Center)
            .children(children)
            .build()
    }

    /// Creates a Column with spacing between children.
    ///
    /// Automatically inserts SizedBox spacers between children.
    pub fn spaced(spacing: f32, children: impl Into<Children>) -> Self {
        let children: Children = children.into();
        if children.is_empty() {
            return Self::builder().children(Children::default()).build();
        }

        let mut spaced_children = Children::default();
        for (i, child) in children.into_inner().into_iter().enumerate() {
            if i > 0 {
                spaced_children.push(SizedBox::v_space(spacing));
            }
            spaced_children.push_element(child);
        }

        Self::builder().children(spaced_children).build()
    }

    /// Creates a Column with start alignment.
    pub fn start(children: impl Into<Children>) -> Self {
        Self::builder()
            .main_axis_alignment(MainAxisAlignment::Start)
            .children(children)
            .build()
    }

    /// Creates a Column with end alignment.
    pub fn end(children: impl Into<Children>) -> Self {
        Self::builder()
            .main_axis_alignment(MainAxisAlignment::End)
            .children(children)
            .build()
    }

    /// Creates a Column with space-between alignment.
    pub fn space_between(children: impl Into<Children>) -> Self {
        Self::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .children(children)
            .build()
    }

    /// Creates a Column with space-around alignment.
    pub fn space_around(children: impl Into<Children>) -> Self {
        Self::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceAround)
            .children(children)
            .build()
    }

    /// Creates a Column with space-evenly alignment.
    pub fn space_evenly(children: impl Into<Children>) -> Self {
        Self::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceEvenly)
            .children(children)
            .build()
    }

    /// Validates column configuration.
    pub fn validate(&self) -> Result<(), String> {
        // No specific validation needed for Column
        // All enum values are already valid
        Ok(())
    }
}

impl Default for Column {
    fn default() -> Self {
        Self::new()
    }
}

// Implement View for Column
impl StatelessView for Column {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let render_flex = RenderFlex::column()
            .with_main_axis_alignment(self.main_axis_alignment)
            .with_cross_axis_alignment(self.cross_axis_alignment)
            .with_main_axis_size(self.main_axis_size);

        render_flex.children(self.children.into_inner())
    }
}

/// Macro for creating Column with declarative syntax.
#[macro_export]
macro_rules! column {
    // Empty column
    () => {
        $crate::Column::new()
    };

    // With children only (using bracket syntax like vec!)
    [$($child:expr),* $(,)?] => {
        $crate::Column::builder()
            .children(vec![$($child.into_element()),*])
            .build()
    };

    // With properties only (using brace syntax) - uses builder
    {$($field:ident : $value:expr),+ $(,)?} => {
        $crate::Column::builder()
            $(.$field($value))+
            .build()
    };

    // With properties and children (separated by semicolon)
    {$($field:ident : $value:expr),+ ; [$($child:expr),* $(,)?]} => {
        $crate::Column::builder()
            $(.$field($value))+
            .children(vec![$($child.into_element()),*])
            .build()
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_rendering::RenderEmpty;

    // Mock view for testing
    #[derive(Clone)]
    struct MockView;

    impl StatelessView for MockView {
        fn build(self, _ctx: &BuildContext) -> impl IntoElement {
            RenderEmpty.leaf()
        }
    }

    #[test]
    fn test_column_new() {
        let column = Column::new();
        assert!(column.key.is_none());
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Center);
        assert_eq!(column.main_axis_size, MainAxisSize::Max);
        assert_eq!(column.children.len(), 0);
    }

    #[test]
    fn test_column_default() {
        let column = Column::default();
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Start);
        assert_eq!(column.children.len(), 0);
    }

    #[test]
    fn test_column_builder() {
        let column = Column::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .build();
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::Center);
    }

    #[test]
    fn test_column_builder_chaining() {
        let column = Column::builder()
            .main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .cross_axis_alignment(CrossAxisAlignment::Start)
            .main_axis_size(MainAxisSize::Min)
            .build();

        assert_eq!(column.main_axis_alignment, MainAxisAlignment::SpaceBetween);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Start);
        assert_eq!(column.main_axis_size, MainAxisSize::Min);
    }

    #[test]
    fn test_column_validate_ok() {
        let column = Column::builder()
            .main_axis_alignment(MainAxisAlignment::Center)
            .build();
        assert!(column.validate().is_ok());
    }

    #[test]
    fn test_column_all_main_axis_alignments() {
        for alignment in [
            MainAxisAlignment::Start,
            MainAxisAlignment::End,
            MainAxisAlignment::Center,
            MainAxisAlignment::SpaceBetween,
            MainAxisAlignment::SpaceAround,
            MainAxisAlignment::SpaceEvenly,
        ] {
            let column = Column::builder().main_axis_alignment(alignment).build();
            assert_eq!(column.main_axis_alignment, alignment);
        }
    }

    #[test]
    fn test_column_all_cross_axis_alignments() {
        for alignment in [
            CrossAxisAlignment::Start,
            CrossAxisAlignment::End,
            CrossAxisAlignment::Center,
            CrossAxisAlignment::Stretch,
        ] {
            let column = Column::builder().cross_axis_alignment(alignment).build();
            assert_eq!(column.cross_axis_alignment, alignment);
        }
    }

    #[test]
    fn test_column_macro_empty() {
        let column = column!();
        assert_eq!(column.children.len(), 0);
    }

    #[test]
    fn test_column_macro_with_fields() {
        let column = column! {
            main_axis_alignment: MainAxisAlignment::End,
            cross_axis_alignment: CrossAxisAlignment::Stretch,
        };
        assert_eq!(column.main_axis_alignment, MainAxisAlignment::End);
        assert_eq!(column.cross_axis_alignment, CrossAxisAlignment::Stretch);
    }
}
