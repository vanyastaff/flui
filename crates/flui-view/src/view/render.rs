//! RenderView - Views that create RenderObjects.
//!
//! RenderViews are leaf nodes in the View tree that produce RenderObjects.
//! They bridge the View/Element system with the Render tree for layout and painting.

use super::view::{ElementBase, View};
use crate::element::Lifecycle;
use flui_foundation::ElementId;
use std::any::TypeId;
use std::marker::PhantomData;

/// A View that creates a RenderObject for layout and painting.
///
/// RenderViews are the bridge between the declarative View tree and
/// the imperative RenderObject tree. Each RenderView corresponds to
/// a specific RenderObject type.
///
/// # Type Parameters
///
/// * `R` - The RenderObject type this View creates
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `RenderObjectWidget` and its subclasses:
/// - `LeafRenderObjectWidget` - No children
/// - `SingleChildRenderObjectWidget` - One child
/// - `MultiChildRenderObjectWidget` - Multiple children
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{RenderView, BuildContext};
/// use flui_rendering::RenderBox;
///
/// struct ColoredBox {
///     color: Color,
///     child: Option<Box<dyn View>>,
/// }
///
/// impl RenderView for ColoredBox {
///     type RenderObject = RenderDecoratedBox;
///
///     fn create_render_object(&self, ctx: &dyn BuildContext) -> Self::RenderObject {
///         RenderDecoratedBox::new(self.color)
///     }
///
///     fn update_render_object(&self, ctx: &dyn BuildContext, render: &mut Self::RenderObject) {
///         render.set_color(self.color);
///     }
/// }
/// ```
pub trait RenderView: Send + Sync + 'static + Sized {
    /// The RenderObject type this View creates.
    type RenderObject: Send + Sync + 'static;

    /// Create a new RenderObject.
    ///
    /// Called once when the Element is first mounted.
    fn create_render_object(&self) -> Self::RenderObject;

    /// Update an existing RenderObject with new configuration.
    ///
    /// Called when this View updates an existing Element.
    fn update_render_object(&self, render_object: &mut Self::RenderObject);

    /// Whether this View can have children.
    ///
    /// Override to return true for single/multi child variants.
    fn has_children(&self) -> bool {
        false
    }
}

/// Implement View for a RenderView type.
///
/// This macro creates the View implementation for a RenderView type.
///
/// ```rust,ignore
/// impl RenderView for MyColoredBox {
///     type RenderObject = RenderDecoratedBox;
///     // ...
/// }
/// impl_render_view!(MyColoredBox);
/// ```
#[macro_export]
macro_rules! impl_render_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                Box::new($crate::RenderElement::new(self))
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
    };
}

// ============================================================================
// RenderElement
// ============================================================================

/// Element for RenderViews.
///
/// Manages the lifecycle of a RenderView and its associated RenderObject.
/// This is the glue between the Element tree and the Render tree.
pub struct RenderElement<V: RenderView> {
    /// The current View configuration.
    view: V,
    /// The RenderObject (created lazily on mount).
    render_object: Option<V::RenderObject>,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
    /// Child elements (for single/multi child variants).
    children: Vec<Box<dyn ElementBase>>,
    /// Whether we need to rebuild.
    dirty: bool,
    /// Marker for RenderObject type.
    _marker: PhantomData<V::RenderObject>,
}

impl<V: RenderView> RenderElement<V>
where
    V: Clone,
{
    /// Create a new RenderElement for the given View.
    pub fn new(view: &V) -> Self {
        Self {
            view: view.clone(),
            render_object: None,
            lifecycle: Lifecycle::Initial,
            depth: 0,
            children: Vec::new(),
            dirty: true,
            _marker: PhantomData,
        }
    }

    /// Get a reference to the RenderObject.
    pub fn render_object(&self) -> Option<&V::RenderObject> {
        self.render_object.as_ref()
    }

    /// Get a mutable reference to the RenderObject.
    pub fn render_object_mut(&mut self) -> Option<&mut V::RenderObject> {
        self.render_object.as_mut()
    }
}

impl<V: RenderView + Clone> std::fmt::Debug for RenderElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("dirty", &self.dirty)
            .field("has_render_object", &self.render_object.is_some())
            .field("children_count", &self.children.len())
            .finish_non_exhaustive()
    }
}

impl<V: RenderView + Clone> ElementBase for RenderElement<V> {
    fn view_type_id(&self) -> TypeId {
        TypeId::of::<V>()
    }

    fn lifecycle(&self) -> Lifecycle {
        self.lifecycle
    }

    fn update(&mut self, new_view: &dyn View) {
        // Use View::as_any() for safe downcasting
        if let Some(v) = new_view.as_any().downcast_ref::<V>() {
            self.view = v.clone();

            // Update the RenderObject if it exists
            if let Some(ref mut render_object) = self.render_object {
                self.view.update_render_object(render_object);
            }

            self.dirty = true;
        }
    }

    fn mark_needs_build(&mut self) {
        self.dirty = true;
    }

    fn perform_build(&mut self) {
        if !self.dirty || !self.lifecycle.can_build() {
            return;
        }

        // RenderElements typically don't rebuild in the same way as
        // ComponentElements. Their "build" is creating/updating the RenderObject.
        self.dirty = false;
    }

    fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {
        self.lifecycle = Lifecycle::Active;

        // Create the RenderObject on mount
        self.render_object = Some(self.view.create_render_object());

        self.dirty = true;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        for child in &mut self.children {
            child.deactivate();
        }
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        for child in &mut self.children {
            child.activate();
        }
    }

    fn unmount(&mut self) {
        self.lifecycle = Lifecycle::Defunct;

        // Drop the RenderObject
        self.render_object = None;

        for child in &mut self.children {
            child.unmount();
        }
        self.children.clear();
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        // In a full implementation, we'd track child ElementIds
        let _ = visitor;
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A simple test RenderObject
    #[derive(Debug, Clone, Default)]
    struct TestRenderBox {
        color: u32,
        size: (f32, f32),
    }

    impl TestRenderBox {
        fn new(color: u32) -> Self {
            Self {
                color,
                size: (0.0, 0.0),
            }
        }

        fn set_color(&mut self, color: u32) {
            self.color = color;
        }
    }

    /// A simple test RenderView
    #[derive(Clone)]
    struct ColoredBox {
        color: u32,
    }

    impl RenderView for ColoredBox {
        type RenderObject = TestRenderBox;

        fn create_render_object(&self) -> Self::RenderObject {
            TestRenderBox::new(self.color)
        }

        fn update_render_object(&self, render_object: &mut Self::RenderObject) {
            render_object.set_color(self.color);
        }
    }

    impl View for ColoredBox {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(RenderElement::new(self))
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[test]
    fn test_render_element_creation() {
        let view = ColoredBox { color: 0xFF0000 };
        let element = RenderElement::new(&view);

        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        assert!(element.render_object().is_none()); // Not created until mount
    }

    #[test]
    fn test_render_element_mount() {
        let view = ColoredBox { color: 0xFF0000 };
        let mut element = RenderElement::new(&view);

        element.mount(None, 0);

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        assert!(element.render_object().is_some());
        assert_eq!(element.render_object().unwrap().color, 0xFF0000);
    }

    #[test]
    fn test_render_element_update() {
        let view = ColoredBox { color: 0xFF0000 };
        let mut element = RenderElement::new(&view);
        element.mount(None, 0);

        // Update with new view
        let new_view = ColoredBox { color: 0x00FF00 };
        element.update(&new_view);

        assert_eq!(element.render_object().unwrap().color, 0x00FF00);
    }

    #[test]
    fn test_render_element_unmount() {
        let view = ColoredBox { color: 0xFF0000 };
        let mut element = RenderElement::new(&view);
        element.mount(None, 0);

        assert!(element.render_object().is_some());

        element.unmount();

        assert_eq!(element.lifecycle(), Lifecycle::Defunct);
        assert!(element.render_object().is_none());
    }
}
