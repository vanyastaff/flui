//! RenderObjectWidget - widgets that create RenderObjects
//!
//! Based on idea.md Chapter 4.2-4.6

use super::{Widget, DynWidget, sealed};
use crate::render::RenderObject;
use crate::element::RenderObjectElement;

/// RenderObjectWidget - Widget that creates a RenderObject
///
/// This is the lowest level of widget - it directly controls layout and painting
/// by creating a RenderObject.
///
/// # Arity Flow
///
/// The arity flows through the architecture:
/// ```text
/// Widget::Arity → Element::Arity → RenderObject::Arity
/// ```
///
/// The constraint `type Render: RenderObject<Arity = Self::Arity>` ensures
/// that the RenderObject's arity matches the Widget's arity at compile time.
///
/// # Example
///
/// ```rust,ignore
/// use flui_core::{Widget, DynWidget, RenderObjectWidget, RenderObject};
/// use flui_core::{LeafArity, LayoutCx, PaintCx, BoxedLayer};
/// use flui_types::Size;
///
/// #[derive(Clone, Debug)]
/// pub struct ColoredBox {
///     pub color: Color,
///     pub width: f32,
///     pub height: f32,
/// }
///
/// // Manually implement Widget for RenderObjectWidget
/// impl Widget for ColoredBox {
///     fn key(&self) -> Option<&str> { None }
/// }
///
/// // Manually implement DynWidget for RenderObjectWidget
/// impl DynWidget for ColoredBox {
///     fn as_any(&self) -> &dyn std::any::Any { self }
///     fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
/// }
///
/// impl RenderObjectWidget for ColoredBox {
///     type Arity = LeafArity;  // No children
///     type Render = RenderColoredBox;
///
///     fn create_render_object(&self) -> Self::Render {
///         RenderColoredBox {
///             color: self.color,
///             width: self.width,
///             height: self.height,
///         }
///     }
///
///     fn update_render_object(&self, render: &mut Self::Render) {
///         render.color = self.color;
///         render.width = self.width;
///         render.height = self.height;
///     }
/// }
///
/// #[derive(Debug)]
/// struct RenderColoredBox {
///     color: Color,
///     width: f32,
///     height: f32,
/// }
///
/// impl RenderObject for RenderColoredBox {
///     type Arity = LeafArity;  // Must match Widget::Arity!
///
///     fn layout(&mut self, cx: &mut LayoutCx<Self::Arity>) -> Size {
///         Size::new(self.width, self.height)
///     }
///
///     fn paint(&self, cx: &PaintCx<Self::Arity>) -> BoxedLayer {
///         // Paint the colored box...
///         Box::new(PictureLayer::new())
///     }
/// }
/// ```
///
/// # Automatic Widget Implementation
///
/// RenderObjectWidget automatically implements `Widget` and `DynWidget` via blanket impl.
/// No manual implementation needed!
pub trait RenderObjectWidget: std::fmt::Debug + Clone + Send + Sync + 'static {
    /// The arity (child count) of this widget
    ///
    /// This flows through the architecture:
    /// Widget::Arity → Element::Arity → RenderObject::Arity
    ///
    /// # Examples
    ///
    /// - `LeafArity` - No children (e.g., Text, Image, ColoredBox)
    /// - `SingleArity` - Exactly one child (e.g., Opacity, Padding, Transform)
    /// - `MultiArity` - Multiple children (e.g., Flex, Stack, Wrap)
    type Arity: crate::Arity;

    /// The type of RenderObject this widget creates
    ///
    /// This associated type creates a compile-time link between
    /// the Widget and its RenderObject. No downcasts needed!
    ///
    /// **Important**: The RenderObject's Arity must match Widget's Arity.
    /// This is enforced by the constraint `RenderObject<Arity = Self::Arity>`.
    type Render: RenderObject<Arity = Self::Arity>;

    /// Create a new RenderObject instance
    ///
    /// Called when the widget is first mounted to the element tree.
    /// The RenderObject will be owned by a RenderObjectElement.
    fn create_render_object(&self) -> Self::Render;

    /// Update an existing RenderObject
    ///
    /// Called when the widget is updated but the RenderObject can be reused.
    /// This is more efficient than creating a new RenderObject.
    ///
    /// Update only the fields that changed - the RenderObject will be marked
    /// dirty and relayout/repaint will be scheduled automatically.
    fn update_render_object(&self, render: &mut Self::Render);
}

// ========== Automatic Implementations ==========

/// Automatically implement sealed::Sealed for all RenderObjectWidgets
///
/// This makes RenderObjectWidget types eligible for the Widget trait.
/// The ElementType is set to RenderObjectElement<W, W::Arity>.
impl<W> sealed::Sealed for W
where
    W: RenderObjectWidget,
{
    type ElementType = RenderObjectElement<W, W::Arity>;
}

/// Automatically implement Widget for all RenderObjectWidgets
///
/// Thanks to the sealed trait pattern, this blanket impl doesn't conflict
/// with other widget type implementations.
impl<W> Widget for W
where
    W: RenderObjectWidget,
{
    fn key(&self) -> Option<&str> {
        None
    }

    fn into_element(self) -> RenderObjectElement<W, W::Arity> {
        RenderObjectElement::new(self)
    }
}

/// Automatically implement DynWidget for all RenderObjectWidgets
impl<W> DynWidget for W
where
    W: RenderObjectWidget,
{
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
