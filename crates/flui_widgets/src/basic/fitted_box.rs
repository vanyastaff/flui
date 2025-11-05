//! FittedBox widget - scales and positions child according to fit mode
//!
//! A widget that scales and positions its child within itself according to fit.
//! Similar to Flutter's FittedBox widget.

use bon::Builder;
use flui_core::{BuildContext, Element, RenderElement};
use flui_core::render::RenderNode;
use flui_core::view::{View, ChangeFlags, AnyView};
use flui_rendering::RenderFittedBox;
use flui_types::Alignment;

// Re-export BoxFit from types for convenience
pub use flui_types::styling::BoxFit;

/// A widget that scales and positions its child within itself according to fit.
///
/// FittedBox scales its child to fit the parent's size using different fit strategies,
/// while maintaining or distorting the aspect ratio as specified.
///
/// ## Layout Behavior
///
/// 1. FittedBox sizes itself to the maximum size allowed by parent constraints
/// 2. Child is laid out with unbounded constraints to get its natural size
/// 3. Child is scaled according to the fit mode
/// 4. Scaled child is positioned using alignment
///
/// ## BoxFit Modes
///
/// - **Fill**: Distorts aspect ratio to fill parent completely
/// - **Contain**: Scales to fit entirely within parent (may leave empty space)
/// - **Cover**: Scales to fill parent completely (may clip child)
/// - **FitWidth**: Fills width, scales height maintaining aspect ratio
/// - **FitHeight**: Fills height, scales width maintaining aspect ratio
/// - **None**: No scaling, uses original child size
/// - **ScaleDown**: Like Contain, but never scales up (only down)
///
/// ## Common Use Cases
///
/// ### Image that fills container
/// ```rust,ignore
/// Container::builder()
///     .width(200.0)
///     .height(100.0)
///     .child(FittedBox::builder()
///         .fit(BoxFit::Cover)
///         .child(Image::new("photo.jpg"))
///         .build())
///     .build()
/// ```
///
/// ### Icon that scales to fit
/// ```rust,ignore
/// SizedBox::builder()
///     .width(48.0)
///     .height(48.0)
///     .child(FittedBox::new(BoxFit::Contain, icon_widget))
///     .build()
/// ```
///
/// ### Text that never overflows
/// ```rust,ignore
/// FittedBox::builder()
///     .fit(BoxFit::ScaleDown)  // Only shrink if needed
///     .alignment(Alignment::CENTER_LEFT)
///     .child(Text::new("Long text that might overflow"))
///     .build()
/// ```
///
/// ## Examples
///
/// ```rust,ignore
/// // Cover the entire box
/// FittedBox::new(BoxFit::Cover, child)
///
/// // Contain within box with custom alignment
/// FittedBox::builder()
///     .fit(BoxFit::Contain)
///     .alignment(Alignment::TOP_LEFT)
///     .child(widget)
///     .build()
///
/// // Fill width
/// FittedBox::new(BoxFit::FitWidth, child)
/// ```
#[derive(Builder)]
#[builder(on(String, into), on(BoxFit, into), on(Alignment, into), finish_fn = build_fitted_box)]
pub struct FittedBox {
    /// Optional key for widget identification
    pub key: Option<String>,

    /// How the child should be inscribed into the available space.
    /// Default: BoxFit::Contain
    #[builder(default = BoxFit::Contain)]
    pub fit: BoxFit,

    /// How to align the child within the fitted box.
    /// Default: Alignment::CENTER
    #[builder(default = Alignment::CENTER)]
    pub alignment: Alignment,

    /// The child widget to scale and position.
    #[builder(setters(vis = "", name = child_internal))]
    pub child: Option<Box<dyn AnyView>>,
}

impl std::fmt::Debug for FittedBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FittedBox")
            .field("key", &self.key)
            .field("fit", &self.fit)
            .field("alignment", &self.alignment)
            .field("child", &if self.child.is_some() { "<AnyView>" } else { "None" })
            .finish()
    }
}

impl Clone for FittedBox {
    fn clone(&self) -> Self {
        Self {
            key: self.key.clone(),
            fit: self.fit,
            alignment: self.alignment,
            child: self.child.clone(),
        }
    }
}

impl FittedBox {
    /// Creates a new FittedBox with the given fit mode.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = FittedBox::new(BoxFit::Cover, child);
    /// ```
    pub fn new(fit: BoxFit, child: impl View + 'static) -> Self {
        Self {
            key: None,
            fit,
            alignment: Alignment::CENTER,
            child: Some(Box::new(child)),
        }
    }

    /// Creates a FittedBox with custom alignment.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let widget = FittedBox::with_alignment(
    ///     BoxFit::Contain,
    ///     Alignment::TOP_LEFT,
    ///     child
    /// );
    /// ```
    pub fn with_alignment(fit: BoxFit, alignment: Alignment, child: impl View + 'static) -> Self {
        Self {
            key: None,
            fit,
            alignment,
            child: Some(Box::new(child)),
        }
    }

    /// Sets the child widget.
    pub fn set_child(&mut self, child: impl View + 'static) {
        self.child = Some(Box::new(child));
    }
}

impl Default for FittedBox {
    fn default() -> Self {
        Self {
            key: None,
            fit: BoxFit::Contain,
            alignment: Alignment::CENTER,
            child: None,
        }
    }
}

// Implement View for FittedBox - New architecture
impl View for FittedBox {
    type Element = Element;
    type State = Option<Box<dyn std::any::Any>>;

    fn build(self, ctx: &mut BuildContext) -> (Self::Element, Self::State) {
        // Build child if present
        let (child_id, child_state) = if let Some(child) = self.child {
            let (elem, state) = child.build_any(ctx);
            let id = ctx.tree().write().insert(elem.into_element());
            (Some(id), Some(state))
        } else {
            (None, None)
        };

        // Create RenderNode (always Single for SingleRender widgets)
        let render_node = RenderNode::Single {
            render: Box::new(RenderFittedBox::with_alignment(self.fit, self.alignment)),
            child: child_id,
        };

        // Create RenderElement using constructor
        let render_element = RenderElement::new(render_node);

        (Element::Render(render_element), child_state)
    }

    fn rebuild(
        self,
        prev: &Self,
        state: &mut Self::State,
        element: &mut Self::Element,
    ) -> ChangeFlags {
        // TODO: Implement proper rebuild logic if needed
        // For now, return NONE as View architecture handles rebuilding
        ChangeFlags::NONE
    }
}

// bon Builder Extensions
use fitted_box_builder::{IsUnset, SetChild, State};

impl<S: State> FittedBoxBuilder<S>
where
    S::Child: IsUnset,
{
    /// Sets the child widget (works in builder chain).
    pub fn child(self, child: impl View + 'static) -> FittedBoxBuilder<SetChild<S>> {
        self.child_internal(Box::new(child))
    }
}

impl<S: State> FittedBoxBuilder<S> {
    /// Builds the FittedBox widget.
    pub fn build(self) -> FittedBox {
        self.build_fitted_box()
    }
}

// FittedBox now implements View trait directly

#[cfg(test)]
mod tests {
    use super::*;

    // Mock view for testing
    #[derive(Debug, Clone)]
    struct MockView;

    impl View for MockView {
        type Element = Element;
        type State = ();

        fn build(self, _ctx: &mut BuildContext) -> (Self::Element, Self::State) {
            use flui_rendering::RenderColoredBox;
            use flui_types::Color;
            let render_node = RenderNode::Leaf(Box::new(RenderColoredBox::new(Color::BLACK)));
            let render_element = RenderElement::new(render_node);
            (Element::Render(render_element), ())
        }

        fn rebuild(self, _prev: &Self, _state: &mut Self::State, _element: &mut Self::Element) -> ChangeFlags {
            ChangeFlags::NONE
        }
    }

    #[test]
    fn test_fitted_box_new() {
        let widget = FittedBox::new(BoxFit::Cover, MockView);
        assert_eq!(widget.fit, BoxFit::Cover);
        assert_eq!(widget.alignment, Alignment::CENTER);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_fitted_box_with_alignment() {
        let widget = FittedBox::with_alignment(
            BoxFit::Contain,
            Alignment::TOP_LEFT,
            MockView,
        );
        assert_eq!(widget.fit, BoxFit::Contain);
        assert_eq!(widget.alignment, Alignment::TOP_LEFT);
        assert!(widget.child.is_some());
    }

    #[test]
    fn test_fitted_box_builder() {
        let widget = FittedBox::builder()
            .fit(BoxFit::Fill)
            .alignment(Alignment::BOTTOM_RIGHT)
            .build();
        assert_eq!(widget.fit, BoxFit::Fill);
        assert_eq!(widget.alignment, Alignment::BOTTOM_RIGHT);
    }

    #[test]
    fn test_fitted_box_default() {
        let widget = FittedBox::default();
        assert_eq!(widget.fit, BoxFit::Contain);
        assert_eq!(widget.alignment, Alignment::CENTER);
        assert!(widget.child.is_none());
    }

    #[test]
    fn test_fitted_box_set_child() {
        let mut widget = FittedBox::default();
        assert!(widget.child.is_none());

        widget.set_child(MockView);
        assert!(widget.child.is_some());
    }
}
