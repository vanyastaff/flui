//! OverflowBox widget - allows child to overflow parent constraints
//!
//! A widget that imposes different constraints on its child than it gets from
//! its parent, allowing the child to overflow.
//! Similar to Flutter's OverflowBox widget.

use bon::Builder;
use flui_core::element::Element;
use flui_core::view::{IntoElement, StatelessView};

use flui_core::BuildContext;
use flui_objects::RenderOverflowBox;
use flui_types::Alignment;

/// A widget that imposes different constraints on its child than it gets from its parent.
///
/// OverflowBox allows a child to size itself differently than what its parent
/// would normally allow, potentially overflowing the parent's bounds. This is useful
/// for creating effects where content intentionally exceeds its container.
///
/// ## Layout Behavior
///
/// 1. OverflowBox passes its own constraints to the child, overriding parent constraints
/// 2. The OverflowBox itself sizes according to parent constraints
/// 3. The child is positioned using the alignment property
///
/// If constraints are None, the parent's constraints are used for that dimension.
///
/// ## Common Use Cases
///
/// ### Badge that overflows button
/// ```rust,ignore
/// Stack::new()
///     .children(vec![
///         Button::new("Click"),
///         Positioned::builder()
///             .top(0.0)
///             .right(0.0)
///             .child(OverflowBox::builder()
///                 .max_width(30.0)
///                 .max_height(30.0)
///                 .child(Badge::new("5"))
///                 .build())
///             .build()
///     ])
/// ```
///
/// ### Allow child to expand beyond parent
/// ```rust,ignore
/// Container::builder()
///     .width(100.0)
///     .height(100.0)
///     .child(OverflowBox::builder()
///         .max_width(200.0)  // Child can be twice as wide
///         .alignment(Alignment::TOP_LEFT)
///         .child(large_widget)
///         .build())
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Allow child to be larger than parent
/// OverflowBox::builder()
///     .min_width(200.0)
///     .max_width(400.0)
///     .alignment(Alignment::CENTER)
///     .child(oversized_content)
///     .build()
///
/// // Let child overflow vertically
/// OverflowBox::builder()
///     .max_height(500.0)
///     .child(tall_widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    on(Alignment, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct OverflowBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Minimum width constraint for child (overrides parent).
    /// If None, uses parent's min width.
    pub min_width: Option<f32>,

    /// Maximum width constraint for child (overrides parent).
    /// If None, uses parent's max width.
    pub max_width: Option<f32>,

    /// Minimum height constraint for child (overrides parent).
    /// If None, uses parent's min height.
    pub min_height: Option<f32>,

    /// Maximum height constraint for child (overrides parent).
    /// If None, uses parent's max height.
    pub max_height: Option<f32>,

    /// How to align the child within the overflow box.
    /// Default: Alignment::CENTER
    #[builder(default = Alignment::CENTER)]
    pub alignment: Alignment,

    /// The child widget that may overflow.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Element>,
}

impl std::fmt::Debug for OverflowBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OverflowBox")
            .field("key", &self.key)
            .field("min_width", &self.min_width)
            .field("max_width", &self.max_width)
            .field("min_height", &self.min_height)
            .field("max_height", &self.max_height)
            .field("alignment", &self.alignment)
            .field("child", &if self.child.is_some() { "<>" } else { "None" })
            .finish()
    }
}

impl OverflowBox {
    /// Creates a new OverflowBox.
    pub fn new() -> Self {
        Self {
            key: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment: Alignment::CENTER,
            child: None,
        }
    }

    /// Creates an OverflowBox with specific constraints.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = OverflowBox::with_constraints(
    ///     Some(100.0), Some(300.0),  // width: 100-300
    ///     Some(50.0), Some(200.0),   // height: 50-200
    ///     child
    /// );
    /// ```
    pub fn with_constraints(
        min_width: Option<f32>,
        max_width: Option<f32>,
        min_height: Option<f32>,
        max_height: Option<f32>,
        child: impl IntoElement,
    ) -> Self {
        Self {
            key: None,
            min_width,
            max_width,
            min_height,
            max_height,
            alignment: Alignment::CENTER,
            child: Some(child.into_element()),
        }
    }

    /// Creates an OverflowBox with specific alignment.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = OverflowBox::with_alignment(Alignment::TOP_LEFT, child);
    /// ```
    pub fn with_alignment(alignment: Alignment, child: impl IntoElement) -> Self {
        Self {
            key: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment,
            child: Some(child.into_element()),
        }
    }

    /// Validates OverflowBox configuration.
    pub fn validate(&self) -> Result<(), String> {
        if let (Some(min), Some(max)) = (self.min_width, self.max_width) {
            if min > max {
                return Err("min_width cannot be greater than max_width".to_string());
            }
        }
        if let (Some(min), Some(max)) = (self.min_height, self.max_height) {
            if min > max {
                return Err("min_height cannot be greater than max_height".to_string());
            }
        }
        Ok(())
    }
}

impl Default for OverflowBox {
    fn default() -> Self {
        Self::new()
    }
}

// bon Builder Extensions
use overflow_box_builder::{IsUnset, SetChild, State};

impl<S: State> OverflowBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// OverflowBox::builder()
    ///     .max_width(200.0)
    ///     .alignment(Alignment::CENTER)
    ///     .child(Container::new())
    ///     .build()
    /// ```
    pub fn child(self, child: impl IntoElement) -> OverflowBoxBuilder<SetChild<S>> {
        self.child_internal(child.into_element())
    }
}

impl<S: State> OverflowBoxBuilder<S> {
    /// Builds the OverflowBox with optional validation.
    pub fn build(self) -> OverflowBox {
        let overflow_box = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = overflow_box.validate() {
                tracing::warn!("OverflowBox validation failed: {}", e);
            }
        }

        overflow_box
    }
}

// Implement View trait - Simplified API
impl IntoElement for OverflowBox {
    fn into_element(self) -> Element {
        let render = RenderOverflowBox::with_constraints(
            self.min_width,
            self.max_width,
            self.min_height,
            self.max_height,
        );

        render.child_opt(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overflow_box_new() {
        let widget = OverflowBox::new();
        assert_eq!(widget.min_width, None);
        assert_eq!(widget.max_width, None);
        assert_eq!(widget.alignment, Alignment::CENTER);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_overflow_box_with_constraints() {
        let widget = OverflowBox::with_constraints(
            Some(100.0),
            Some(300.0),
            Some(50.0),
            Some(200.0),
            crate::SizedBox::new(),
        );
        assert_eq!(widget.min_width, Some(100.0));
        assert_eq!(widget.max_width, Some(300.0));
        assert_eq!(widget.min_height, Some(50.0));
        assert_eq!(widget.max_height, Some(200.0));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_overflow_box_with_alignment() {
        let widget = OverflowBox::with_alignment(Alignment::TOP_LEFT, crate::SizedBox::new());
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_overflow_box_builder() {
        let widget = OverflowBox::builder()
            .min_width(50.0)
            .max_width(250.0)
            .alignment(Alignment::BOTTOM_RIGHT)
            .build();
        assert_eq!(widget.min_width, Some(50.0));
        assert_eq!(widget.max_width, Some(250.0));
        assert_eq!(widget.alignment, Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn test_overflow_box_validate() {
        let widget = OverflowBox::with_constraints(
            Some(100.0),
            Some(200.0),
            Some(50.0),
            Some(150.0),
            crate::SizedBox::new(),
        );
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_overflow_box_validate_invalid_width() {
        let widget = OverflowBox::with_constraints(
            Some(300.0),
            Some(200.0),
            Some(50.0),
            Some(150.0),
            crate::SizedBox::new(),
        );
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_overflow_box_validate_invalid_height() {
        let widget = OverflowBox::with_constraints(
            Some(100.0),
            Some(200.0),
            Some(200.0),
            Some(100.0),
            crate::SizedBox::new(),
        );
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_overflow_box_default() {
        let widget = OverflowBox::default();
        assert_eq!(widget.alignment, Alignment::CENTER);
        assert!(widget.child.is_none());
    }
}
