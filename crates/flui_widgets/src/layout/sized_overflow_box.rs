//! SizedOverflowBox widget - fixed size with child overflow
//!
//! A widget with a specific size that allows its child to have different constraints,
//! potentially causing the child to overflow the widget's bounds.

use bon::Builder;
use flui_core::view::{AnyView, IntoElement, SingleRenderBuilder, View};

use flui_core::BuildContext;
use flui_rendering::RenderSizedOverflowBox;
use flui_types::Alignment;

/// A widget with a specific size that allows its child to overflow.
///
/// SizedOverflowBox is a combination of SizedBox and OverflowBox:
/// - The widget itself has a specific size (width/height)
/// - The child can have different constraints, allowing it to overflow
/// - The child is aligned within the widget
///
/// ## Key Properties
///
/// - **width**: Explicit width for this widget
/// - **height**: Explicit height for this widget
/// - **child_min_width**: Minimum width constraint for child
/// - **child_max_width**: Maximum width constraint for child
/// - **child_min_height**: Minimum height constraint for child
/// - **child_max_height**: Maximum height constraint for child
/// - **alignment**: How to align the child (default: CENTER)
/// - **child**: The child widget
///
/// ## Common Use Cases
///
/// ### Fixed size with larger child
/// ```rust,ignore
/// SizedOverflowBox::builder()
///     .width(100.0)
///     .height(100.0)
///     .child_max_width(200.0)
///     .child_max_height(200.0)
///     .child(large_image)
///     .build()
/// ```
///
/// ### Clipped preview
/// ```rust,ignore
/// SizedOverflowBox::new(
///     Some(50.0),
///     Some(50.0),
///     content
/// )
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Simple fixed size allowing overflow
/// SizedOverflowBox::new(Some(100.0), Some(100.0), child)
///
/// // With specific child constraints
/// SizedOverflowBox::builder()
///     .width(100.0)
///     .height(100.0)
///     .child_max_width(200.0)
///     .child_max_height(200.0)
///     .alignment(Alignment::TOP_LEFT)
///     .child(widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(
    on(String, into),
    finish_fn(name = build_internal, vis = "")
)]
pub struct SizedOverflowBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Explicit width for this widget
    pub width: Option<f32>,

    /// Explicit height for this widget
    pub height: Option<f32>,

    /// Minimum width constraint for child
    pub child_min_width: Option<f32>,

    /// Maximum width constraint for child
    pub child_max_width: Option<f32>,

    /// Minimum height constraint for child
    pub child_min_height: Option<f32>,

    /// Maximum height constraint for child
    pub child_max_height: Option<f32>,

    /// How to align the child
    /// Default: Alignment::CENTER
    #[builder(default = Alignment::CENTER)]
    pub alignment: Alignment,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for SizedOverflowBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizedOverflowBox")
            .field("key", &self.key)
            .field("width", &self.width)
            .field("height", &self.height)
            .field(
                "child_min_width",
                &if self.child_min_width.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .field(
                "child_max_width",
                &if self.child_max_width.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .field(
                "child_min_height",
                &if self.child_min_height.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .field(
                "child_max_height",
                &if self.child_max_height.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .field("alignment", &self.alignment)
            .field(
                "child",
                &if self.child.is_some() {
                    "<AnyView>"
                } else {
                    "None"
                },
            )
            .finish()
    }
}

impl Clone for SizedOverflowBox {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            width: self.width,
            height: self.height,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: self.alignment,
            child: self.child.clone(),
        }
    }
}

impl SizedOverflowBox {
    /// Creates a new SizedOverflowBox with specific size.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let box = SizedOverflowBox::new(Some(100.0), Some(100.0), child);
    /// ```
    pub fn new(width: Option<f32>, height: Option<f32>, child: impl View + 'static) -> Self {
        Self {
            key: None,
            width,
            height,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: Alignment::CENTER,
            child: Some(Box::new(child)),
        }
    }

    /// Creates a SizedOverflowBox with child constraints.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let box = SizedOverflowBox::with_child_constraints(
    ///     Some(100.0), Some(100.0),
    ///     None, Some(200.0),
    ///     None, Some(200.0),
    ///     child
    /// );
    /// ```
    pub fn with_child_constraints(
        width: Option<f32>,
        height: Option<f32>,
        child_min_width: Option<f32>,
        child_max_width: Option<f32>,
        child_min_height: Option<f32>,
        child_max_height: Option<f32>,
        child: impl View + 'static,
    ) -> Self {
        Self {
            key: None,
            width,
            height,
            child_min_width,
            child_max_width,
            child_min_height,
            child_max_height,
            alignment: Alignment::CENTER,
            child: Some(Box::new(child)),
        }
    }

    /// Validates SizedOverflowBox configuration.
    pub fn validate(&self) -> Result<(), String> {
        // Validate width
        if let Some(width) = self.width {
            if width < 0.0 || width.is_nan() {
                return Err(format!(
                    "Invalid width: {}. Width must be non-negative and finite.",
                    width
                ));
            }
        }

        // Validate height
        if let Some(height) = self.height {
            if height < 0.0 || height.is_nan() {
                return Err(format!(
                    "Invalid height: {}. Height must be non-negative and finite.",
                    height
                ));
            }
        }

        // Validate child constraints
        if let Some(child_min_width) = self.child_min_width {
            if child_min_width < 0.0 || child_min_width.is_nan() {
                return Err(format!(
                    "Invalid child_min_width: {}. Must be non-negative and finite.",
                    child_min_width
                ));
            }
        }

        if let Some(child_max_width) = self.child_max_width {
            if child_max_width < 0.0 || child_max_width.is_nan() {
                return Err(format!(
                    "Invalid child_max_width: {}. Must be non-negative and finite.",
                    child_max_width
                ));
            }
        }

        if let Some(child_min_height) = self.child_min_height {
            if child_min_height < 0.0 || child_min_height.is_nan() {
                return Err(format!(
                    "Invalid child_min_height: {}. Must be non-negative and finite.",
                    child_min_height
                ));
            }
        }

        if let Some(child_max_height) = self.child_max_height {
            if child_max_height < 0.0 || child_max_height.is_nan() {
                return Err(format!(
                    "Invalid child_max_height: {}. Must be non-negative and finite.",
                    child_max_height
                ));
            }
        }

        Ok(())
    }
}

impl Default for SizedOverflowBox {
    fn default() -> Self {
        Self {
            key: None,
            width: None,
            height: None,
            child_min_width: None,
            child_max_width: None,
            child_min_height: None,
            child_max_height: None,
            alignment: Alignment::CENTER,
            child: None,
        }
    }
}

// bon Builder Extensions
use sized_overflow_box_builder::{IsUnset, SetChild, State};

// Custom setter for child
impl<S: State> SizedOverflowBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// SizedOverflowBox::builder()
    ///     .width(100.0)
    ///     .height(100.0)
    ///     .child(Container::new())
    ///     .build()
    /// ```
    pub fn child(self, child: impl View + 'static) -> SizedOverflowBoxBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

// Public build() wrapper
impl<S: State> SizedOverflowBoxBuilder<S> {
    /// Builds the SizedOverflowBox with optional validation.
    pub fn build(self) -> SizedOverflowBox {
        let sized_overflow_box = self.build_internal();

        #[cfg(debug_assertions)]
        {
            if let Err(e) = sized_overflow_box.validate() {
                tracing::warn!("SizedOverflowBox validation failed: {}", e);
            }
        }

        sized_overflow_box
    }
}

// Implement View trait - Simplified API
impl View for SizedOverflowBox {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        let mut render = if self.child_min_width.is_some()
            || self.child_max_width.is_some()
            || self.child_min_height.is_some()
            || self.child_max_height.is_some()
        {
            RenderSizedOverflowBox::with_child_constraints(
                self.width,
                self.height,
                self.child_min_width,
                self.child_max_width,
                self.child_min_height,
                self.child_max_height,
            )
        } else {
            RenderSizedOverflowBox::new(self.width, self.height)
        };
        render.alignment = self.alignment;

        SingleRenderBuilder::new(render).with_optional_child(self.child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sized_overflow_box_new() {
        let box_widget = SizedOverflowBox::new(Some(100.0), Some(100.0), crate::SizedBox::new());
        assert_eq!(box_widget.width, Some(100.0));
        assert_eq!(box_widget.height, Some(100.0));
        assert!(box_widget.child.is_some());
    }

    #[test]
    fn test_sized_overflow_box_with_child_constraints() {
        let box_widget = SizedOverflowBox::with_child_constraints(
            Some(100.0),
            Some(100.0),
            None,
            Some(200.0),
            None,
            Some(200.0),
            crate::SizedBox::new(),
        );
        assert_eq!(box_widget.child_max_width, Some(200.0));
        assert_eq!(box_widget.child_max_height, Some(200.0));
    }

    #[test]
    fn test_sized_overflow_box_builder() {
        let box_widget = SizedOverflowBox::builder()
            .width(100.0)
            .height(100.0)
            .child_max_width(200.0)
            .alignment(Alignment::TOP_LEFT)
            .build();
        assert_eq!(box_widget.width, Some(100.0));
        assert_eq!(box_widget.height, Some(100.0));
        assert_eq!(box_widget.child_max_width, Some(200.0));
        assert_eq!(box_widget.alignment, Alignment::TOP_LEFT);
    }

    #[test]
    fn test_sized_overflow_box_validate_ok() {
        let box_widget = SizedOverflowBox::new(Some(100.0), Some(100.0), crate::SizedBox::new());
        assert!(box_widget.validate().is_ok());
    }

    #[test]
    fn test_sized_overflow_box_validate_invalid_width() {
        let mut box_widget =
            SizedOverflowBox::new(Some(100.0), Some(100.0), crate::SizedBox::new());
        box_widget.width = Some(-1.0);
        assert!(box_widget.validate().is_err());
    }

    #[test]
    fn test_sized_overflow_box_validate_invalid_child_constraint() {
        let mut box_widget =
            SizedOverflowBox::new(Some(100.0), Some(100.0), crate::SizedBox::new());
        box_widget.child_max_width = Some(-1.0);
        assert!(box_widget.validate().is_err());
    }

    #[test]
    fn test_sized_overflow_box_default() {
        let box_widget = SizedOverflowBox::default();
        assert_eq!(box_widget.width, None);
        assert_eq!(box_widget.height, None);
        assert!(box_widget.child.is_none());
    }
}
