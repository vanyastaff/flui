//! Offstage widget - hides widget without removing it from tree
//!
//! A widget that lays out its child as if it was in the tree, but without painting it.
//! Similar to Flutter's Offstage widget.
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern
//! ```rust,ignore
//! Offstage::builder()
//!     .offstage(true)
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
use flui_core::BuildContext;
use flui_rendering::RenderOffstage;

/// A widget that lays out its child as if it was in the tree, but without painting or hit testing.
///
/// When `offstage` is true:
/// - The child is NOT painted (invisible)
/// - The child is NOT hit tested (doesn't receive pointer events)
/// - The child IS still laid out (maintains its size and state)
///
/// ## Use Cases
///
/// - **Preserving State**: Keep a widget's state while hiding it
/// - **Animation**: Smoothly animate visibility without rebuilding
/// - **Performance**: Avoid rebuilding expensive widgets when showing/hiding
/// - **Conditional Display**: Toggle visibility without changing the widget tree
///
/// ## Layout Behavior
///
/// - Simply passes constraints to child and adopts child size
/// - Child is always laid out, even when offstage
///
/// ## Difference from Visibility Widget
///
/// - **Offstage**: Child is laid out but not painted (takes up space)
/// - **Visibility (gone)**: Child is not laid out and not painted (no space)
///
/// ## Examples
///
/// ```rust,ignore
/// // Hide a widget while preserving its state
/// Offstage::builder()
///     .offstage(is_hidden)
///     .child(ExpensiveWidget::new())
///     .build()
///
/// // Toggle visibility
/// Offstage::builder()
///     .offstage(!is_visible)
///     .child(content)
///     .build()
/// ```
#[derive(Debug, Clone, Builder)]
#[builder(on(String, into), finish_fn = build_offstage)]
pub struct Offstage {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Whether the child is offstage (hidden).
    ///
    /// When true, child is laid out but not painted or hit tested.
    #[builder(default = true)]
    pub offstage: bool,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl Offstage {
    /// Creates a new Offstage widget.
    ///
    /// # Parameters
    ///
    /// - `offstage`: If true, child is hidden (default: true)
    pub fn new(offstage: bool) -> Self {
        Self {
            key: None,
            offstage,
            child: None,
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
    }
}

impl Default for Offstage {
    fn default() -> Self {
        Self::new(true)
    }
}

// Implement Widget trait with associated type


// bon Builder Extensions
use offstage_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> OffstageBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: Widget) -> OffstageBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

// Build wrapper
impl<S: State> OffstageBuilder<S> {
    /// Builds the Offstage widget.
    pub fn build(self) -> Offstage {
        self.build_offstage()
    }
}

// Implement RenderObjectWidget
impl RenderWidget for Offstage {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        RenderNode::single(Box::new(RenderOffstage::new(self.offstage)))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(obj) = render.downcast_mut::<RenderOffstage>() {
                obj.set_offstage(self.offstage);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

/// Macro for creating Offstage with declarative syntax.
#[macro_export]
macro_rules! offstage {
    () => {
        $crate::Offstage::new(true)
    };
    (offstage: $offstage:expr) => {
        $crate::Offstage::new($offstage)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_core::LeafRenderObjectElement;
    use flui_types::EdgeInsets;
    use flui_rendering::RenderPadding;

    #[derive(Debug, Clone)]
    struct MockWidget;

    

    impl RenderObjectWidget for MockWidget {
        fn create_render_object(&self) -> Box<dyn DynRenderObject> {
            Box::new(RenderPadding::new(EdgeInsets::ZERO))
        }

        fn update_render_object(&self, _render_object: &mut dyn DynRenderObject) {}
    }

    impl flui_core::LeafRenderObjectWidget for MockWidget {}

    #[test]
    fn test_offstage_new() {
        let widget = Offstage::new(true);
        assert!(widget.key.is_none());
        assert!(widget.offstage);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_offstage_new_false() {
        let widget = Offstage::new(false);
        assert!(!widget.offstage);
    }

    #[test]
    fn test_offstage_default() {
        let widget = Offstage::default();
        assert!(widget.offstage);
    }

    #[test]
    fn test_offstage_builder() {
        let widget = Offstage::builder().build();
        assert!(widget.offstage); // Default is true
    }

    #[test]
    fn test_offstage_builder_with_child() {
        let widget = Offstage::builder().child(MockWidget).build();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_offstage_builder_with_offstage_false() {
        let widget = Offstage::builder().offstage(false).build();
        assert!(!widget.offstage);
    }

    #[test]
    fn test_offstage_set_child() {
        let mut widget = Offstage::new(true);
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_offstage_macro_default() {
        let widget = offstage!();
        assert!(widget.offstage);
    }

    #[test]
    fn test_offstage_macro_with_value() {
        let widget = offstage!(offstage: false);
        assert!(!widget.offstage);
    }

    #[test]
    fn test_offstage_widget_trait() {
        let widget = Offstage::builder()
            .offstage(true)
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_offstage_render_object_creation() {
        let widget = Offstage::new(true);
        let render_object = widget.create_render_object();
        assert!(render_object.downcast_ref::<RenderOffstage>().is_some());
    }

    #[test]
    fn test_offstage_render_object_update() {
        let widget1 = Offstage::new(true);
        let mut render_object = widget1.create_render_object();

        let widget2 = Offstage::new(false);
        widget2.update_render_object(&mut *render_object);

        let offstage_render = render_object.downcast_ref::<RenderOffstage>().unwrap();
        assert!(!offstage_render.offstage());
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(Offstage, render);
