//! OverflowBox widget - allows child to overflow parent constraints
//!
//! A widget that imposes different constraints on its child than it gets from
//! its parent, allowing the child to overflow.
//! Similar to Flutter's OverflowBox widget.

use bon::Builder;
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
use flui_core::BuildContext;
use flui_rendering::RenderOverflowBox;
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
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), on(Alignment, into), finish_fn = build_overflow_box)]
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
    pub child: Option<Widget>,
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
        child: Widget,
    ) -> Self {
        Self {
            key: None,
            min_width,
            max_width,
            min_height,
            max_height,
            alignment: Alignment::CENTER,
            child: Some(child),
        }
    }

    /// Creates an OverflowBox with specific alignment.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = OverflowBox::with_alignment(Alignment::TOP_LEFT, child);
    /// ```
    pub fn with_alignment(alignment: Alignment, child: Widget) -> Self {
        Self {
            key: None,
            min_width: None,
            max_width: None,
            min_height: None,
            max_height: None,
            alignment,
            child: Some(child),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
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
    pub fn child(self, child: Widget) -> OverflowBoxBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

impl<S: State> OverflowBoxBuilder<S> {
    /// Builds the OverflowBox widget.
    pub fn build(self) -> Widget {
        Widget::render(self.build_overflow_box())
    }
}

// Implement RenderWidget
impl RenderWidget for OverflowBox {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        let mut render = RenderOverflowBox::new();
        render.set_min_width(self.min_width);
        render.set_max_width(self.max_width);
        render.set_alignment(self.alignment);
        // Note: RenderOverflowBox doesn't have setters for min/max height in the API
        // We'll need to use with_constraints instead
        RenderNode::single(Box::new(RenderOverflowBox::with_constraints(
            self.min_width,
            self.max_width,
            self.min_height,
            self.max_height,
        )))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(overflow_box) = render.downcast_mut::<RenderOverflowBox>() {
                overflow_box.set_min_width(self.min_width);
                overflow_box.set_max_width(self.max_width);
                overflow_box.set_alignment(self.alignment);
                // Note: min_height and max_height don't have setters in RenderOverflowBox
                // This is a limitation of the current API
                overflow_box.min_height = self.min_height;
                overflow_box.max_height = self.max_height;
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(OverflowBox, render);

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
            Widget::from(()),
        );
        assert_eq!(widget.min_width, Some(100.0));
        assert_eq!(widget.max_width, Some(300.0));
        assert_eq!(widget.min_height, Some(50.0));
        assert_eq!(widget.max_height, Some(200.0));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_overflow_box_with_alignment() {
        let widget = OverflowBox::with_alignment(Alignment::TOP_LEFT, Widget::from(()));
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
            Widget::from(()),
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
            Widget::from(()),
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
            Widget::from(()),
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
