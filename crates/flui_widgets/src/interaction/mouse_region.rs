//! MouseRegion widget - tracks mouse enter/exit/hover events
//!
//! A widget that tracks mouse pointer events for a region.
//! Similar to Flutter's MouseRegion widget.
//!
//! # Usage Patterns
//!
//! ## 1. Builder Pattern
//! ```rust,ignore
//! MouseRegion::builder()
//!     .on_enter(|event| println!("Mouse entered!"))
//!     .on_exit(|event| println!("Mouse left!"))
//!     .on_hover(|event| println!("Mouse moved!"))
//!     .child(some_widget)
//!     .build()
//! ```

use bon::Builder;
use flui_core::widget::{Widget, RenderWidget};
use flui_core::render::RenderNode;
use flui_core::BuildContext;
use flui_rendering::{RenderMouseRegion, MouseCallbacks};
use flui_types::events::{PointerEvent, PointerEventHandler};

/// A widget that tracks mouse pointer events.
///
/// MouseRegion calls callbacks for mouse enter, exit, and hover (move) events.
/// It participates in hit testing even when transparent to track the mouse cursor.
///
/// ## Layout Behavior
///
/// - Simply passes constraints to child and adopts child size
/// - No effect on layout, only affects pointer event tracking
///
/// ## Event Callbacks
///
/// - **on_enter**: Called when mouse cursor enters the region bounds
/// - **on_exit**: Called when mouse cursor exits the region bounds
/// - **on_hover**: Called when mouse cursor moves within the region (hover)
///
/// ## Examples
///
/// ```rust,ignore
/// // Track mouse enter/exit
/// MouseRegion::builder()
///     .on_enter(|e| println!("Welcome!"))
///     .on_exit(|e| println!("Goodbye!"))
///     .child(Container::new())
///     .build()
///
/// // Track hover for tooltips
/// MouseRegion::builder()
///     .on_hover(|e| show_tooltip(e.position()))
///     .child(icon_widget)
///     .build()
/// ```
#[derive(Builder)]
#[builder(on(String, into), finish_fn = build_mouse_region)]
pub struct MouseRegion {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// Called when mouse enters the region
    #[builder(setters(vis = "", name = on_enter_internal))]
    pub on_enter: Option<PointerEventHandler>,

    /// Called when mouse exits the region
    #[builder(setters(vis = "", name = on_exit_internal))]
    pub on_exit: Option<PointerEventHandler>,

    /// Called when mouse moves within the region (hover)
    #[builder(setters(vis = "", name = on_hover_internal))]
    pub on_hover: Option<PointerEventHandler>,

    /// The child widget
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Widget>,
}

impl MouseRegion {
    /// Creates a new MouseRegion widget.
    pub fn new() -> Self {
        Self {
            key: None,
            on_enter: None,
            on_exit: None,
            on_hover: None,
            child: None,
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: Widget) {
        self.child = Some(child);
    }
}

impl Default for MouseRegion {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Widget trait with associated type


impl Clone for MouseRegion {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            on_enter: self.on_enter.clone(),
            on_exit: self.on_exit.clone(),
            on_hover: self.on_hover.clone(),
            child: None, // Widgets aren't cloned deeply
        }
    }
}

impl std::fmt::Debug for MouseRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MouseRegion")
            .field("key", &self.key)
            .field("has_on_enter", &self.on_enter.is_some())
            .field("has_on_exit", &self.on_exit.is_some())
            .field("has_on_hover", &self.on_hover.is_some())
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

// bon Builder Extensions
use mouse_region_builder::{IsUnset, SetChild, SetOnEnter, SetOnExit, SetOnHover, State};

// Custom child setter
impl<S: State> MouseRegionBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: Widget) -> MouseRegionBuilder<SetChild<S>> {
        self.child_internal(child)
    }
}

// Custom on_enter setter
impl<S: State> MouseRegionBuilder<S>
where
    S::OnEnter: IsUnset,
{
    /// Sets the on_enter callback.
    pub fn on_enter(
        self,
        callback: impl Fn(&PointerEvent) + Send + Sync + 'static,
    ) -> MouseRegionBuilder<SetOnEnter<S>> {
        self.on_enter_internal(std::sync::Arc::new(callback))
    }
}

// Custom on_exit setter
impl<S: State> MouseRegionBuilder<S>
where
    S::OnExit: IsUnset,
{
    /// Sets the on_exit callback.
    pub fn on_exit(
        self,
        callback: impl Fn(&PointerEvent) + Send + Sync + 'static,
    ) -> MouseRegionBuilder<SetOnExit<S>> {
        self.on_exit_internal(std::sync::Arc::new(callback))
    }
}

// Custom on_hover setter
impl<S: State> MouseRegionBuilder<S>
where
    S::OnHover: IsUnset,
{
    /// Sets the on_hover callback.
    pub fn on_hover(
        self,
        callback: impl Fn(&PointerEvent) + Send + Sync + 'static,
    ) -> MouseRegionBuilder<SetOnHover<S>> {
        self.on_hover_internal(std::sync::Arc::new(callback))
    }
}

// Build wrapper
impl<S: State> MouseRegionBuilder<S> {
    /// Builds the MouseRegion widget.
    pub fn build(self) -> MouseRegion {
        self.build_mouse_region()
    }
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
    fn test_mouse_region_new() {
        let widget = MouseRegion::new();
        assert!(widget.key.is_none());
        assert!(widget.on_enter.is_none());
        assert!(widget.on_exit.is_none());
        assert!(widget.on_hover.is_none());
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_mouse_region_default() {
        let widget = MouseRegion::default();
        assert!(widget.on_enter.is_none());
    }

    #[test]
    fn test_mouse_region_builder() {
        let widget = MouseRegion::builder().build();
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_mouse_region_builder_with_child() {
        let widget = MouseRegion::builder().child(MockWidget).build();
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_mouse_region_builder_with_on_enter() {
        let widget = MouseRegion::builder()
            .on_enter(|_| {})
            .build();
        assert!(widget.on_enter.is_some());
    }

    #[test]
    fn test_mouse_region_builder_with_on_exit() {
        let widget = MouseRegion::builder()
            .on_exit(|_| {})
            .build();
        assert!(widget.on_exit.is_some());
    }

    #[test]
    fn test_mouse_region_builder_with_on_hover() {
        let widget = MouseRegion::builder()
            .on_hover(|_| {})
            .build();
        assert!(widget.on_hover.is_some());
    }

    #[test]
    fn test_mouse_region_builder_with_all_callbacks() {
        let widget = MouseRegion::builder()
            .on_enter(|_| {})
            .on_exit(|_| {})
            .on_hover(|_| {})
            .child(MockWidget)
            .build();

        assert!(widget.on_enter.is_some());
        assert!(widget.on_exit.is_some());
        assert!(widget.on_hover.is_some());
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_mouse_region_set_child() {
        let mut widget = MouseRegion::new();
        widget.set_child(MockWidget);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_mouse_region_clone() {
        let widget1 = MouseRegion::builder()
            .on_enter(|_| {})
            .build();

        let widget2 = widget1.clone();
        assert!(widget2.on_enter.is_some());
    }

    #[test]
    fn test_mouse_region_debug() {
        let widget = MouseRegion::builder()
            .on_enter(|_| {})
            .on_exit(|_| {})
            .build();

        let debug_str = format!("{:?}", widget);
        assert!(debug_str.contains("MouseRegion"));
        assert!(debug_str.contains("has_on_enter"));
    }

    #[test]
    fn test_mouse_region_widget_trait() {
        let widget = MouseRegion::builder()
            .on_hover(|_| {})
            .child(MockWidget)
            .build();

        // Test that it implements Widget and can create an element
        let _element = widget.into_element();
    }

    #[test]
    fn test_single_child_render_object_widget_trait() {
        let widget = MouseRegion::builder()
            .on_enter(|_| {})
            .child(MockWidget)
            .build();

        // Test child() method
        assert!(widget.child().is_some());
    }
}

// Implement RenderWidget
impl RenderWidget for MouseRegion {
    fn create_render_object(&self, _context: &BuildContext) -> RenderNode {
        // TODO: RenderMouseRegion currently uses fn() callbacks as placeholders
        // The widget's Arc<dyn Fn> callbacks will be properly supported when
        // event handling infrastructure is implemented
        let callbacks = MouseCallbacks {
            on_enter: None,  // Placeholder - widget callbacks not yet supported
            on_exit: None,
            on_hover: None,
        };
        RenderNode::single(Box::new(RenderMouseRegion::new(callbacks)))
    }

    fn update_render_object(&self, _context: &BuildContext, render_object: &mut RenderNode) {
        if let RenderNode::Single { render, .. } = render_object {
            if let Some(mouse_region) = render.downcast_mut::<RenderMouseRegion>() {
                // TODO: Update callbacks when event handling is implemented
                let callbacks = MouseCallbacks {
                    on_enter: None,
                    on_exit: None,
                    on_hover: None,
                };
                mouse_region.set_callbacks(callbacks);
            }
        }
    }

    fn child(&self) -> Option<&Widget> {
        self.child.as_ref()
    }
}

// Implement IntoWidget for ergonomic API
flui_core::impl_into_widget!(MouseRegion, render);
