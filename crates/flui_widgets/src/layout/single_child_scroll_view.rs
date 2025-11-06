//! SingleChildScrollView - A scrollable widget with a single child
//!
//! Based on Flutter's SingleChildScrollView. Scrolls a single child
//! widget that can exceed the viewport size.

use flui_core::view::{AnyView, BuildContext, IntoElement, View};
use flui_types::layout::Axis;

/// A scrollable widget with a single child
///
/// This widget scrolls its child when the child exceeds the viewport size.
/// Similar to Flutter's SingleChildScrollView.
///
/// # Example
///
/// ```rust,ignore
/// use flui_widgets::SingleChildScrollView;
/// use flui_types::layout::Axis;
///
/// SingleChildScrollView::builder()
///     .direction(Axis::Vertical)
///     .child(Column::new()
///         .children(many_items))
///     .build()
/// ```
#[derive(Clone)]
pub struct SingleChildScrollView {
    /// The child widget to scroll
    pub child: Box<dyn AnyView>,

    /// The scroll direction (Vertical or Horizontal)
    pub direction: Axis,

    /// Whether to reverse the scroll direction
    pub reverse: bool,

    /// Padding around the scrollable child
    pub padding: Option<flui_types::EdgeInsets>,
}

impl SingleChildScrollView {
    /// Create a new SingleChildScrollView
    pub fn new(child: impl View + 'static) -> Self {
        Self {
            child: Box::new(child),
            direction: Axis::Vertical,
            reverse: false,
            padding: None,
        }
    }

    /// Create a vertical SingleChildScrollView
    pub fn vertical(child: impl View + 'static) -> Self {
        Self::new(child)
    }

    /// Create a horizontal SingleChildScrollView
    pub fn horizontal(child: impl View + 'static) -> Self {
        Self {
            child: Box::new(child),
            direction: Axis::Horizontal,
            reverse: false,
            padding: None,
        }
    }

    /// Builder for SingleChildScrollView
    pub fn builder() -> SingleChildScrollViewBuilder {
        SingleChildScrollViewBuilder::new()
    }
}


impl std::fmt::Debug for SingleChildScrollView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SingleChildScrollView")
            .field("direction", &self.direction)
            .field("reverse", &self.reverse)
            .field("has_padding", &self.padding.is_some())
            .field("child", &"<Widget>")
            .finish()
    }
}

/// Builder for SingleChildScrollView
pub struct SingleChildScrollViewBuilder {
    child: Option<Box<dyn AnyView>>,
    direction: Axis,
    reverse: bool,
    padding: Option<flui_types::EdgeInsets>,
}

impl SingleChildScrollViewBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            child: None,
            direction: Axis::Vertical,
            reverse: false,
            padding: None,
        }
    }

    /// Set the child widget
    pub fn child(mut self, child: impl View + 'static) -> Self {
        self.child = Some(Box::new(child));
        self
    }

    /// Set the scroll direction
    pub fn direction(mut self, direction: Axis) -> Self {
        self.direction = direction;
        self
    }

    /// Set whether to reverse the scroll direction
    pub fn reverse(mut self, reverse: bool) -> Self {
        self.reverse = reverse;
        self
    }

    /// Set padding around the scrollable child
    pub fn padding(mut self, padding: flui_types::EdgeInsets) -> Self {
        self.padding = Some(padding);
        self
    }

    /// Build the SingleChildScrollView
    pub fn build(self) -> SingleChildScrollView {
        SingleChildScrollView {
            child: self.child.expect("SingleChildScrollView requires a child"),
            direction: self.direction,
            reverse: self.reverse,
            padding: self.padding,
        }
    }
}

impl Default for SingleChildScrollViewBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SizedBox;

    #[test]
    fn test_scroll_view_new() {
        let child = SizedBox::builder().width(100.0).height(1000.0).build();
        let scroll_view = SingleChildScrollView::new(child);

        assert_eq!(scroll_view.direction, Axis::Vertical);
        assert!(!scroll_view.reverse);
        assert!(scroll_view.padding.is_none());
    }

    #[test]
    fn test_scroll_view_vertical() {
        let child = SizedBox::builder().width(100.0).height(1000.0).build();
        let scroll_view = SingleChildScrollView::vertical(child);

        assert_eq!(scroll_view.direction, Axis::Vertical);
    }

    #[test]
    fn test_scroll_view_horizontal() {
        let child = SizedBox::builder().width(1000.0).height(100.0).build();
        let scroll_view = SingleChildScrollView::horizontal(child);

        assert_eq!(scroll_view.direction, Axis::Horizontal);
    }

    #[test]
    fn test_scroll_view_builder() {
        let child = SizedBox::builder().width(100.0).height(1000.0).build();
        let scroll_view = SingleChildScrollView::builder()
            .child(child)
            .direction(Axis::Horizontal)
            .reverse(true)
            .padding(flui_types::EdgeInsets::all(16.0))
            .build();

        assert_eq!(scroll_view.direction, Axis::Horizontal);
        assert!(scroll_view.reverse);
        assert!(scroll_view.padding.is_some());
    }
}
impl View for SingleChildScrollView {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // For now, ignore padding - child is used directly
        // TODO: Add proper padding support when needed
        let child = self.child;

        // Create RenderScrollView
        (
            flui_rendering::objects::RenderScrollView::new(self.direction, self.reverse),
            Some(child),
        )
    }
}
