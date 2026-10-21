//! [`AnnotatedRegion`] — attaches a value to its child's box in the
//! composited scene.

use std::{fmt, sync::Arc};

use flui_objects::RenderAnnotatedRegion;
use flui_rendering::protocol::BoxProtocol;
use flui_view::{Child, IntoView, RenderView};

/// Attaches `value` to the box its child occupies, for readers of the
/// composited scene.
///
/// The presentation reads a
/// [`SystemUiOverlayStyle`](flui_platform::SystemUiOverlayStyle) region at
/// the top edge of the window after every frame and hands the topmost one to
/// the window's system chrome. Any other `T` is found with
/// [`LayerTree::find_annotations`](flui_rendering::layer::LayerTree::find_annotations).
/// Layout and hit testing are the child's; an equal value on rebuild does not
/// repaint.
pub struct AnnotatedRegion<T> {
    value: Arc<T>,
    child: Child,
}

impl<T> AnnotatedRegion<T> {
    /// Annotates `child`'s box with `value`.
    pub fn new(value: T, child: impl IntoView) -> Self {
        Self {
            value: Arc::new(value),
            child: Child::some(child.into_view()),
        }
    }
}

impl<T> Clone for AnnotatedRegion<T> {
    fn clone(&self) -> Self {
        Self {
            value: Arc::clone(&self.value),
            child: self.child.clone(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for AnnotatedRegion<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AnnotatedRegion")
            .field("value", &self.value)
            .finish_non_exhaustive()
    }
}

impl<T> RenderView for AnnotatedRegion<T>
where
    T: PartialEq + fmt::Debug + Send + Sync + 'static,
{
    type Protocol = BoxProtocol;
    type RenderObject = RenderAnnotatedRegion;

    fn create_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
    ) -> Self::RenderObject {
        RenderAnnotatedRegion::new(Arc::clone(&self.value))
    }

    fn update_render_object(
        &self,
        _ctx: &flui_view::RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact {
        if render_object.value().downcast_ref::<T>() == Some(&*self.value) {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        render_object.set_value(Arc::clone(&self.value))
    }

    flui_view::single_child_view_children!();
}

impl<T> flui_view::View for AnnotatedRegion<T>
where
    T: PartialEq + fmt::Debug + Send + Sync + 'static,
{
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::render_variable(self)
    }
}
