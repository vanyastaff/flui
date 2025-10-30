//! FractionallySizedBox widget - sizes child as fraction of available space
//!
//! A widget that sizes its child to a fraction of available space.
//! Similar to Flutter's FractionallySizedBox widget.

use bon::Builder;
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
use flui_core::BuildContext;
use flui_rendering::RenderFractionallySizedBox;

/// A widget that sizes its child to a fraction of the total available space.
///
/// FractionallySizedBox is useful when you want a child to take up a specific
/// percentage of its parent's size. For example, `width_factor: 0.5` makes
/// the child half the parent's width.
///
/// ## Layout Behavior
///
/// The factors are multiplied by the incoming constraints to determine child size:
/// - `width_factor: Some(0.5)` → child width = parent_max_width * 0.5
/// - `height_factor: Some(0.75)` → child height = parent_max_height * 0.75
/// - `None` → child uses incoming constraints (unconstrained in that axis)
///
/// ## Common Use Cases
///
/// ### Half-width button
/// ```rust,ignore
/// FractionallySizedBox::builder()
///     .width_factor(0.5)
///     .child(Button::new("Click me"))
///     .build()
/// ```
///
/// ### 75% height container
/// ```rust,ignore
/// FractionallySizedBox::builder()
///     .height_factor(0.75)
///     .child(Container::new())
///     .build()
/// ```
///
/// ### Responsive grid cells
/// ```rust,ignore
/// Row::new()
///     .children(vec![
///         FractionallySizedBox::new(Some(0.33), None, widget1), // 33% width
///         FractionallySizedBox::new(Some(0.33), None, widget2), // 33% width
///         FractionallySizedBox::new(Some(0.34), None, widget3), // 34% width
///     ])
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // 50% width and height
/// FractionallySizedBox::builder()
///     .width_factor(0.5)
///     .height_factor(0.5)
///     .child(Container::new())
///     .build()
///
/// // Only constrain width
/// FractionallySizedBox::builder()
///     .width_factor(0.8)
///     .child(some_widget)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_fractionally_sized_box)]
pub struct FractionallySizedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Width factor (0.0 - 1.0), where 0.5 means 50% of parent width.
    /// If None, width is unconstrained.
    pub width_factor: Option<f32>,

    /// Height factor (0.0 - 1.0), where 0.75 means 75% of parent height.
    /// If None, height is unconstrained.
    pub height_factor: Option<f32>,

    /// The child widget to size.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl FractionallySizedBox {
    /// Creates a new FractionallySizedBox with the given factors.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // 50% width, 75% height
    /// let widget = FractionallySizedBox::new(Some(0.5), Some(0.75), child);
    ///
    /// // Only 60% width, height unconstrained
    /// let widget = FractionallySizedBox::new(Some(0.6), None, child);
    /// ```
    pub fn new(width_factor: Option<f32>, height_factor: Option<f32>, child: Widget) -> Self {
        Self {
            key: None,
            width_factor,
            height_factor,
            child: Some(child),
        }
    }

    /// Creates a FractionallySizedBox with the same factor for both width and height.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // 50% of both width and height
    /// let widget = FractionallySizedBox::both(0.5, child);
    /// ```
    pub fn both(factor: f32, child: Widget) -> Self {
        Self::new(Some(factor), Some(factor), child)
    }

    /// Creates a FractionallySizedBox with only width factor.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // 80% width, height unconstrained
    /// let widget = FractionallySizedBox::width(0.8, child);
    /// ```
    pub fn width(factor: f32, child: Widget) -> Self {
        Self::new(Some(factor), None, child)
    }

    /// Creates a FractionallySizedBox with only height factor.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// // 60% height, width unconstrained
    /// let widget = FractionallySizedBox::height(0.6, child);
    /// ```
    pub fn height(factor: f32, child: Widget) -> Self {
        Self::new(None, Some(factor), child)
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
    }

    /// Validates FractionallySizedBox configuration.
    ///
    /// Returns an error if:
    /// - width_factor is not in range 0.0..=1.0
    /// - height_factor is not in range 0.0..=1.0
    pub fn validate(&self) -> Result<(), String> {
        if let Some(w) = self.width_factor {
            if !(0.0..=1.0).contains(&w) || w.is_nan() {
                return Err(format!(
                    "Invalid width_factor: {}. Must be between 0.0 and 1.0",
                    w
                ));
            }
        }
        if let Some(h) = self.height_factor {
            if !(0.0..=1.0).contains(&h) || h.is_nan() {
                return Err(format!(
                    "Invalid height_factor: {}. Must be between 0.0 and 1.0",
                    h
                ));
            }
        }
        Ok(())
    }
}

// bon Builder Extensions
use fractionally_sized_box_builder::{IsUnset, SetChild, State};

impl<S: State> FractionallySizedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: Widget) -> FractionallySizedBoxBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

impl<S: State> FractionallySizedBoxBuilder<S> {
    /// Builds the FractionallySizedBox widget.
    pub fn build(self) -> Widget {
        Widget::render_object(self.build_fractionally_sized_box())
    }
}

// Implement RenderWidget
impl RenderWidget for FractionallySizedBox {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::single(Box::new(RenderFractionallySizedBox::new(
            self.width_factor,
            self.height_factor,
        )))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(fractional) = render.downcast_mut::<RenderFractionallySizedBox>() {
                fractional.set_width_factor(self.width_factor);
                fractional.set_height_factor(self.height_factor);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(FractionallySizedBox, render);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fractionally_sized_box_new() {
        let widget = FractionallySizedBox::new(Some(0.5), Some(0.75), Widget::from(()));
        assert_eq!(widget.width_factor, Some(0.5));
        assert_eq!(widget.height_factor, Some(0.75));
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_fractionally_sized_box_both() {
        let widget = FractionallySizedBox::both(0.5, Widget::from(()));
        assert_eq!(widget.width_factor, Some(0.5));
        assert_eq!(widget.height_factor, Some(0.5));
    }

    #[test]
    fn test_fractionally_sized_box_width() {
        let widget = FractionallySizedBox::width(0.6, Widget::from(()));
        assert_eq!(widget.width_factor, Some(0.6));
        assert_eq!(widget.height_factor, None);
    }

    #[test]
    fn test_fractionally_sized_box_height() {
        let widget = FractionallySizedBox::height(0.8, Widget::from(()));
        assert_eq!(widget.width_factor, None);
        assert_eq!(widget.height_factor, Some(0.8));
    }

    #[test]
    fn test_fractionally_sized_box_builder() {
        let widget = FractionallySizedBox::builder()
            .width_factor(0.7)
            .height_factor(0.9)
            .build();
        assert_eq!(widget.width_factor, Some(0.7));
        assert_eq!(widget.height_factor, Some(0.9));
    }

    #[test]
    fn test_fractionally_sized_box_validate() {
        let widget = FractionallySizedBox::new(Some(0.5), Some(0.75), Widget::from(()));
        assert!(widget.validate().is_ok());
    }

    #[test]
    fn test_fractionally_sized_box_validate_invalid_width() {
        let widget = FractionallySizedBox::new(Some(1.5), Some(0.5), Widget::from(()));
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_fractionally_sized_box_validate_invalid_height() {
        let widget = FractionallySizedBox::new(Some(0.5), Some(-0.1), Widget::from(()));
        assert!(widget.validate().is_err());
    }

    #[test]
    fn test_fractionally_sized_box_validate_none_factors() {
        let widget = FractionallySizedBox::new(None, None, Widget::from(()));
        assert!(widget.validate().is_ok());
    }
}
