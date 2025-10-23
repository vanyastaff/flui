//! Padding widget - adds empty space around a child (New API)
//!
//! Refactored to use the new Widget API with associated types.

use bon::Builder;
use flui_core::{DynRenderObject, DynWidget, RenderObjectWidget, SingleChildRenderObjectWidget, Widget, SingleChildRenderObjectElement};
use flui_rendering::RenderPadding;
use flui_types::EdgeInsets;

/// A widget that insets its child by the given padding.
///
/// ## Layout Behavior
///
/// - The padding is applied inside any decoration constraints
/// - Negative padding is not supported and will be clamped to zero
/// - The child size is reduced by the padding amount
#[derive(Debug, Clone, Builder)]
#[builder(
    on(String, into),
    on(EdgeInsets, into),
    finish_fn = build_padding
)]
pub struct Padding {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// The amount of space by which to inset the child.
    #[builder(default = EdgeInsets::ZERO)]
    pub padding: EdgeInsets,

    /// The child widget to pad.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn DynWidget>>,
}

impl Padding {
    /// Creates a new Padding with zero padding.
    pub fn new() -> Self {
        Self {
            key: None,
            padding: EdgeInsets::ZERO,
            child: None,
        }
    }

    /// Creates a Padding with the given padding and child.
    pub fn with_child(padding: EdgeInsets, child: Box<dyn DynWidget>) -> Self {
        Self {
            key: None,
            padding,
            child: Some(child),
        }
    }
}

impl Default for Padding {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Widget trait with associated type
impl Widget for Padding {
    type Element = SingleChildRenderObjectElement<Self>;

    fn into_element(self) -> Self::Element {
        SingleChildRenderObjectElement::new(self)
    }
}

// Implement RenderObjectWidget
impl RenderObjectWidget for Padding {
    fn create_render_object(&self) -> Box<dyn DynRenderObject> {
        Box::new(RenderPadding::new(self.padding))
    }

    fn update_render_object(&self, render_object: &mut dyn DynRenderObject) {
        if let Some(padding) = render_object.downcast_mut::<RenderPadding>() {
            padding.set_padding(self.padding);
        }
    }
}

// Implement SingleChildRenderObjectWidget
impl SingleChildRenderObjectWidget for Padding {
    fn child(&self) -> &dyn DynWidget {
        self.child
            .as_ref()
            .map(|b| &**b as &dyn DynWidget)
            .unwrap_or_else(|| panic!("Padding requires a child"))
    }
}

// bon Builder Extensions
use padding_builder::{IsUnset, SetChild, State};

// Custom child setter
impl<S: State> PaddingBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child<W: Widget + 'static>(self, child: W) -> PaddingBuilder<SetChild<S>> {
        self.child_internal(Some(Box::new(child) as Box<dyn DynWidget>))
    }
}

// Build wrapper
impl<S: State> PaddingBuilder<S> {
    /// Builds the Padding widget.
    pub fn build(self) -> Padding {
        self.build_padding()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding_new() {
        let padding = Padding::new();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
        assert!(padding.child.is_none());
    }

    #[test]
    fn test_padding_default() {
        let padding = Padding::default();
        assert_eq!(padding.padding, EdgeInsets::ZERO);
    }

    #[test]
    fn test_padding_widget_trait() {
        let padding = Padding::new();
        // Test that it implements Widget and can create an element
        let _element = padding.into_element();
    }
}
