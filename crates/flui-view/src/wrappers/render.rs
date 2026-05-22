//! `RenderViewWrapper` - Wrapper that holds a `RenderView`
//!
//! Implements `ViewObject` for `RenderView` types, enabling render views
//! to be used in the element tree alongside component views.

use std::any::Any;

use flui_rendering::{BoxProtocol, Protocol, RenderObject};

use crate::handle::ViewConfig;
use crate::traits::{RenderObjectFor, RenderView};
use crate::{BuildContext, IntoView, IntoViewConfig, ViewMode, ViewObject};

/// Wrapper for `RenderView` that implements `ViewObject`
///
/// This wrapper bridges render views (which create RenderObjects) with
/// the view/element system. It stores the view configuration and the
/// created render object.
///
/// # Type Parameters
///
/// - `V`: The RenderView type
/// - `P`: Protocol (BoxProtocol or SliverProtocol)
pub struct RenderViewWrapper<V, P: Protocol = BoxProtocol>
where
    V: RenderView<P>,
{
    /// The view configuration (consumed on create)
    view: Option<V>,
    /// The created render object
    render_object: Option<Box<dyn RenderObject>>,
    /// Protocol marker
    _protocol: std::marker::PhantomData<P>,
}

impl<V, P> RenderViewWrapper<V, P>
where
    V: RenderView<P>,
    P: Protocol,
{
    /// Create a new wrapper with the view configuration
    pub fn new(view: V) -> Self {
        Self {
            view: Some(view),
            render_object: None,
            _protocol: std::marker::PhantomData,
        }
    }

    /// Get the view configuration (if not yet consumed)
    pub fn view(&self) -> Option<&V> {
        self.view.as_ref()
    }

    /// Get the render object (if created)
    pub fn render_object(&self) -> Option<&dyn RenderObject> {
        self.render_object.as_ref().map(|r| r.as_ref())
    }

    /// Get mutable render object (if created)
    pub fn render_object_mut(&mut self) -> Option<&mut dyn RenderObject> {
        self.render_object.as_mut().map(|r| r.as_mut())
    }

    /// Extract the inner view, consuming the wrapper.
    ///
    /// Returns `None` if the view has already been consumed to create the render object.
    #[inline]
    pub fn into_inner(self) -> Option<V> {
        self.view
    }
}

impl<V, P> std::fmt::Debug for RenderViewWrapper<V, P>
where
    V: RenderView<P>,
    P: Protocol,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderViewWrapper")
            .field("has_view", &self.view.is_some())
            .field("has_render_object", &self.render_object.is_some())
            .finish()
    }
}

impl<V, P> ViewObject for RenderViewWrapper<V, P>
where
    V: RenderView<P>,
    P: Protocol + 'static,
    V::RenderObject: RenderObjectFor<P> + 'static,
{
    #[inline]
    fn mode(&self) -> ViewMode {
        // Determine mode based on protocol
        if std::any::TypeId::of::<P>() == std::any::TypeId::of::<BoxProtocol>() {
            ViewMode::RenderBox
        } else {
            ViewMode::RenderSliver
        }
    }

    fn build(&mut self, _ctx: &dyn BuildContext) -> Option<Box<dyn ViewObject>> {
        // Render views don't build children through ViewObject::build()
        // Instead, they create RenderObjects which are stored separately
        //
        // If we haven't created the render object yet, do it now
        if self.render_object.is_none() {
            if let Some(view) = &self.view {
                let render_obj = view.create();
                self.render_object = Some(Box::new(render_obj));
            }
        }

        // Return None - render views don't have view children
        None
    }

    fn render_state(&self) -> Option<&dyn Any> {
        self.render_object.as_ref().map(|r| r.as_any())
    }

    fn render_state_mut(&mut self) -> Option<&mut dyn Any> {
        self.render_object.as_mut().map(|r| r.as_any_mut())
    }

    #[inline]
    fn debug_name(&self) -> &'static str {
        std::any::type_name::<V>()
    }

    #[inline]
    fn as_any(&self) -> &dyn Any {
        self
    }

    #[inline]
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ============================================================================
// IntoView IMPLEMENTATION
// ============================================================================

/// Helper struct to convert `RenderView` into `ViewObject`
///
/// Use `Render(my_render_view)` to create a view object from a render view.
#[derive(Debug)]
pub struct Render<V, P: Protocol = BoxProtocol>(pub V, std::marker::PhantomData<P>)
where
    V: RenderView<P>;

impl<V, P> Render<V, P>
where
    V: RenderView<P>,
    P: Protocol,
{
    /// Create a new Render wrapper
    pub fn new(view: V) -> Self {
        Self(view, std::marker::PhantomData)
    }
}

impl<V, P> IntoView for Render<V, P>
where
    V: RenderView<P>,
    P: Protocol + 'static,
    V::RenderObject: RenderObjectFor<P> + 'static,
{
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(RenderViewWrapper::new(self.0))
    }
}

/// Convenience: RenderViewWrapper itself implements IntoView
impl<V, P> IntoView for RenderViewWrapper<V, P>
where
    V: RenderView<P>,
    P: Protocol + 'static,
    V::RenderObject: RenderObjectFor<P> + 'static,
{
    fn into_view(self) -> Box<dyn ViewObject> {
        Box::new(self)
    }
}

// ============================================================================
// IntoViewConfig IMPLEMENTATION
// ============================================================================

/// Implementation for `RenderViewWrapper`.
///
/// This allows render views to be converted to `ViewConfig` when wrapped:
///
/// ```rust,ignore
/// use flui_view::{Render, RenderView, IntoViewConfig};
///
/// let config = RenderViewWrapper::new(MyRenderView { ... }).into_view_config();
/// ```
impl<V, P> IntoViewConfig for RenderViewWrapper<V, P>
where
    V: RenderView<P> + Clone + Send + Sync + 'static,
    P: Protocol + 'static,
    V::RenderObject: RenderObjectFor<P> + 'static,
{
    fn into_view_config(self) -> ViewConfig {
        let view = self
            .view
            .expect("View has been consumed to create render object");
        ViewConfig::new_with_factory(view, |v: &V| {
            Box::new(RenderViewWrapper::<V, P>::new(v.clone()))
        })
    }
}

/// Implementation for `Render` helper.
///
/// ```rust,ignore
/// use flui_view::{Render, IntoViewConfig};
///
/// let config = Render::new(MyRenderView { ... }).into_view_config();
/// ```
impl<V, P> IntoViewConfig for Render<V, P>
where
    V: RenderView<P> + Clone + Send + Sync + 'static,
    P: Protocol + 'static,
    V::RenderObject: RenderObjectFor<P> + 'static,
{
    fn into_view_config(self) -> ViewConfig {
        ViewConfig::new_with_factory(self.0, |v: &V| {
            Box::new(RenderViewWrapper::<V, P>::new(v.clone()))
        })
    }
}

// ============================================================================
// TESTS
// ============================================================================

// Tests temporarily disabled - RenderBox API changed significantly.
// TODO: Re-enable once flui_rendering API stabilizes.
// The tests need to be rewritten to use the new RenderBox trait signature
// which no longer takes Arity as a generic parameter.
