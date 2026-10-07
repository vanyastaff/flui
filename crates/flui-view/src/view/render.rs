//! RenderView - Views that create RenderObjects.
//!
//! RenderViews are leaf nodes in the View tree that produce RenderObjects.
//! They bridge the View/Element system with the Render tree for layout and
//! painting.

use crate::reactive::{Reactive, WriterSource};
use crate::view::View;

/// Owner-runtime capabilities available while a [`RenderView`] creates or
/// updates its render object.
///
/// The context is intentionally narrow: it carries only the composition
/// capabilities a render-object widget needs to register owner-local
/// interaction callbacks while keeping the render object itself data-only and
/// `Send + Sync`, and the [`WriterSource`] those callbacks open their
/// [`EventCx`](crate::EventCx) from (ADR-0086 §3).
///
/// `build` cannot reach it: the element creates and updates the render object
/// outside the build pass, so a render view has no path to a writer inside
/// its own `build` either.
#[derive(Debug, Clone, Copy)]
pub struct RenderObjectContext<'a> {
    interaction_dispatch: Option<&'a flui_interaction::InteractionDispatchHandle>,
    graph: Option<&'a Reactive>,
}

/// Errors returned by owner-runtime operations exposed through
/// [`RenderObjectContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RenderObjectContextError {
    /// The render object lifecycle call was not attached to an owner runtime
    /// with an interaction lane.
    #[error("render object context has no interaction capability")]
    InteractionUnavailable,
    /// The owner interaction lane rejected the operation.
    #[error(transparent)]
    Interaction(#[from] flui_interaction::InteractionDispatchError),
}

impl<'a> RenderObjectContext<'a> {
    /// Construct a context from the active owner interaction handle and the
    /// owner's reactive graph.
    pub(crate) const fn new(
        interaction_dispatch: Option<&'a flui_interaction::InteractionDispatchHandle>,
        graph: Option<&'a Reactive>,
    ) -> Self {
        Self {
            interaction_dispatch,
            graph,
        }
    }

    /// A detached context for tests or hand-built render objects that are not
    /// mounted under a FLUI owner runtime.
    #[must_use]
    pub const fn detached() -> Self {
        Self::new(None, None)
    }

    /// The writer source of the owner the render object is mounted under, for
    /// the event callbacks a render view registers (a `Listener`'s pointer
    /// handlers, a `MouseRegion`'s enter and exit). `None` in a
    /// [`detached`](Self::detached) context.
    ///
    /// A render view has no `init_state`, so this is its counterpart of
    /// [`LifecycleContext::writer_source`](crate::LifecycleContext::writer_source).
    #[must_use]
    pub fn writer_source(&self) -> Option<WriterSource> {
        self.graph.map(|graph| WriterSource::new(graph.clone()))
    }

    fn dispatch_handle(
        &self,
    ) -> Result<&flui_interaction::InteractionDispatchHandle, RenderObjectContextError> {
        self.interaction_dispatch
            .ok_or(RenderObjectContextError::InteractionUnavailable)
    }

    /// Register an ordinary pointer handler in the active owner lane.
    ///
    /// The returned target is data-only and may be stored in a render object;
    /// the executable handler remains in the owner-local interaction lane.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error when no owner lane is active,
    /// the element was mounted detached, or the owner is gone.
    pub fn register_pointer(
        &self,
        handler: impl Fn(flui_interaction::PointerDispatch<'_>) + 'static,
    ) -> Result<flui_interaction::PointerTarget, RenderObjectContextError> {
        Ok(self.dispatch_handle()?.register_pointer(handler)?)
    }

    /// Replace an existing pointer target's handler without changing its
    /// data-plane identity.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target that no longer belongs to the active owner lane.
    pub fn replace_pointer(
        &self,
        target: flui_interaction::PointerTarget,
        handler: impl Fn(flui_interaction::PointerDispatch<'_>) + 'static,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.replace_pointer(target, handler)?)
    }

    /// Remove a pointer target from future route resolution.
    ///
    /// Existing cached routes retain their strong owner-local cells until they
    /// are released by the dispatch owner.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_pointer(
        &self,
        target: flui_interaction::PointerTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_pointer(target)?)
    }

    /// Register an arbitrated scroll-signal handler in the active owner lane.
    ///
    /// Unlike an ordinary pointer handler (which only observes), a scroll
    /// handler *competes* for the tick: leaf-first dispatch stops at the first
    /// handler returning `EventPropagation::Stop`. The returned target is data-only
    /// and may be stored in a render object.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error when no owner lane is active,
    /// the element was mounted detached, or the owner is gone.
    pub fn register_scroll(
        &self,
        handler: impl Fn(
            &flui_platform_api::pointer::ScrollEvent,
        ) -> flui_interaction::routing::EventPropagation
        + 'static,
    ) -> Result<flui_interaction::routing::ScrollTarget, RenderObjectContextError> {
        Ok(self.dispatch_handle()?.register_scroll(handler)?)
    }

    /// Replace an existing scroll target's handler without changing its
    /// data-plane identity.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target that no longer belongs to the active owner lane.
    pub fn replace_scroll(
        &self,
        target: flui_interaction::routing::ScrollTarget,
        handler: impl Fn(
            &flui_platform_api::pointer::ScrollEvent,
        ) -> flui_interaction::routing::EventPropagation
        + 'static,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.replace_scroll(target, handler)?)
    }

    /// Remove a scroll target from future arbitration.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_scroll(
        &self,
        target: flui_interaction::routing::ScrollTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_scroll(target)?)
    }

    /// Register an arbitrated trackpad pan-zoom handler in the active owner
    /// lane.
    ///
    /// The pan-zoom counterpart of [`register_scroll`](Self::register_scroll):
    /// the handler *competes* for the tick, leaf-first dispatch stopping at
    /// the first one returning `EventPropagation::Stop`, so nested pinch
    /// consumers do not all act on the same gesture. The returned target is
    /// data-only and may be stored in a render object.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error when no owner lane is active,
    /// the element was mounted detached, or the owner is gone.
    pub fn register_pan_zoom(
        &self,
        handler: impl Fn(
            &flui_interaction::PointerPanZoomEvent,
        ) -> flui_interaction::routing::EventPropagation
        + 'static,
    ) -> Result<flui_interaction::routing::PanZoomTarget, RenderObjectContextError> {
        Ok(self.dispatch_handle()?.register_pan_zoom(handler)?)
    }

    /// Replace an existing pan-zoom target's handler without changing its
    /// data-plane identity.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target that no longer belongs to the active owner lane.
    pub fn replace_pan_zoom(
        &self,
        target: flui_interaction::routing::PanZoomTarget,
        handler: impl Fn(
            &flui_interaction::PointerPanZoomEvent,
        ) -> flui_interaction::routing::EventPropagation
        + 'static,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.replace_pan_zoom(target, handler)?)
    }

    /// Remove a pan-zoom target from future arbitration.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_pan_zoom(
        &self,
        target: flui_interaction::routing::PanZoomTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_pan_zoom(target)?)
    }

    /// Register mouse-region callbacks in the active owner lane.
    ///
    /// The returned target is data-only and may be stored in a render object;
    /// enter/exit/hover callbacks remain in the owner-local interaction lane.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error when no owner lane is active,
    /// the element was mounted detached, or the owner is gone.
    pub fn register_mouse_region(
        &self,
        callbacks: flui_interaction::routing::MouseRegionCallbacks,
    ) -> Result<flui_interaction::routing::MouseRegionTarget, RenderObjectContextError> {
        Ok(self.dispatch_handle()?.register_mouse_region(callbacks)?)
    }

    /// Replace an existing mouse-region target's callbacks without changing
    /// its data-plane identity.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target that no longer belongs to the active owner lane.
    pub fn replace_mouse_region(
        &self,
        target: flui_interaction::routing::MouseRegionTarget,
        callbacks: flui_interaction::routing::MouseRegionCallbacks,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self
            .dispatch_handle()?
            .replace_mouse_region(target, callbacks)?)
    }

    /// Remove a mouse-region target from future annotation resolution,
    /// without invalidating its shared cell's current contents.
    ///
    /// A still-mounted region rebuilt with an empty callback set does NOT
    /// call this — `MouseRegion`'s widget-level sync
    /// (`crates/flui-widgets/src/interaction/mouse_region.rs`) keeps
    /// the target registered and calls
    /// [`replace_mouse_region`](Self::replace_mouse_region) with the empty
    /// set instead, so the region stays a valid annotation with empty
    /// callbacks until it is detached. This method is the lower-level lane
    /// primitive underneath it: existing tracker state may still retain a
    /// strong owner-local cell clone long enough to emit a matching exit
    /// callback for an annotation that was already resolved before this
    /// call. Use [`detach_mouse_region`](Self::detach_mouse_region) instead
    /// when the region's render object is being permanently removed from
    /// the tree (unmounted) — there, the softer contract this method keeps
    /// would let a stationary device's postframe recheck fire a spurious
    /// exit for a region that no longer exists.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_mouse_region(
        &self,
        target: flui_interaction::routing::MouseRegionTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_mouse_region(target)?)
    }

    /// Remove a mouse-region target AND immediately invalidate its
    /// callbacks, for a region whose render object is being permanently
    /// detached (unmounted). Call this from
    /// [`RenderView::did_unmount_render_object`]
    /// rather than [`unregister_mouse_region`](Self::unregister_mouse_region)
    /// — that method's softer contract (a pending exit may still fire
    /// against a just-unregistered target) is for a lower-level lane caller,
    /// not for this widget-lifecycle case, and would let a stationary
    /// device's postframe recheck fire a spurious exit for a region that no
    /// longer exists.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn detach_mouse_region(
        &self,
        target: flui_interaction::routing::MouseRegionTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.detach_mouse_region(target)?)
    }

    /// Register a path clipper in the active owner lane.
    ///
    /// The returned target is data-only and may be stored in a render object;
    /// the executable `Fn(Size) -> Path` remains owner-local.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error when no owner lane is active,
    /// the element was mounted detached, or the owner is gone.
    pub fn register_path_clipper(
        &self,
        clipper: impl Fn(flui_foundation::geometry::Size) -> flui_painting::paint::Path + 'static,
    ) -> Result<flui_interaction::PathClipTarget, RenderObjectContextError> {
        Ok(self.dispatch_handle()?.register_path_clipper(clipper)?)
    }

    /// Replace an existing path clipper without changing its data-plane
    /// identity.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target that no longer belongs to the active owner lane.
    pub fn replace_path_clipper(
        &self,
        target: flui_interaction::PathClipTarget,
        clipper: impl Fn(flui_foundation::geometry::Size) -> flui_painting::paint::Path + 'static,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self
            .dispatch_handle()?
            .replace_path_clipper(target, clipper)?)
    }

    /// Remove a path clipper from future owner-lane resolution.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_path_clipper(
        &self,
        target: flui_interaction::PathClipTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_path_clipper(target)?)
    }

    /// Register a shader-mask factory in the active owner lane.
    ///
    /// The returned target is data-only and may be stored in a render object;
    /// the executable `Fn(Rect) -> Shader` remains owner-local.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error when no owner lane is active,
    /// the element was mounted detached, or the owner is gone.
    pub fn register_shader_mask(
        &self,
        factory: impl Fn(flui_foundation::geometry::Rect<f64>) -> flui_painting::paint::Shader + 'static,
    ) -> Result<flui_interaction::ShaderMaskTarget, RenderObjectContextError> {
        Ok(self.dispatch_handle()?.register_shader_mask(factory)?)
    }

    /// Replace an existing shader-mask factory without changing its data-plane
    /// identity.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target that no longer belongs to the active owner lane.
    pub fn replace_shader_mask(
        &self,
        target: flui_interaction::ShaderMaskTarget,
        factory: impl Fn(flui_foundation::geometry::Rect<f64>) -> flui_painting::paint::Shader + 'static,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self
            .dispatch_handle()?
            .replace_shader_mask(target, factory)?)
    }

    /// Remove a shader-mask factory from future owner-lane resolution.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_shader_mask(
        &self,
        target: flui_interaction::ShaderMaskTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_shader_mask(target)?)
    }

    /// Register an owner-local payload in the active owner lane.
    ///
    /// For a render view whose executable state does not fit one callback
    /// shape — a drag target's slot, a semantics node's action table. The
    /// returned target is data-only and may be stored in a render object or
    /// published as hit-test metadata; its dispatcher resolves it back with
    /// `flui_interaction::resolve_local_payload` on the owner thread.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error when no owner lane is active,
    /// the element was mounted detached, or the owner is gone.
    pub fn register_local_payload(
        &self,
        payload: std::rc::Rc<dyn std::any::Any>,
    ) -> Result<flui_interaction::LocalPayloadTarget, RenderObjectContextError> {
        Ok(self.dispatch_handle()?.register_local_payload(payload)?)
    }

    /// Replace an existing payload without changing its data-plane identity.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target that no longer belongs to the active owner lane.
    pub fn replace_local_payload(
        &self,
        target: flui_interaction::LocalPayloadTarget,
        payload: std::rc::Rc<dyn std::any::Any>,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self
            .dispatch_handle()?
            .replace_local_payload(target, payload)?)
    }

    /// Remove a payload from future owner-lane resolution.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_local_payload(
        &self,
        target: flui_interaction::LocalPayloadTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_local_payload(target)?)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

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
/// # Example
///
/// ```rust
/// use flui_objects::RenderColoredBox;
/// use flui_rendering::RenderUpdateImpact;
/// use flui_rendering::protocol::BoxProtocol;
/// use flui_foundation::geometry::Size;
/// use flui_view::{RenderObjectContext, RenderView};
///
/// #[derive(Clone)]
/// struct ColoredBox {
///     color: [f32; 4],
/// }
///
/// impl RenderView for ColoredBox {
///     type Protocol = BoxProtocol;
///     type RenderObject = RenderColoredBox;
///
///     fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
///         RenderColoredBox::new(self.color, Size::new(40.0, 24.0))
///     }
///
///     fn update_render_object(
///         &self,
///         _ctx: &RenderObjectContext<'_>,
///         render: &mut Self::RenderObject,
///     ) -> RenderUpdateImpact {
///         render.set_color(self.color)
///     }
/// }
/// ```
pub trait RenderView: Clone + 'static + Sized {
    /// The layout protocol this View uses (BoxProtocol or SliverProtocol).
    type Protocol: flui_rendering::protocol::Protocol;

    /// The RenderObject type this View creates.
    /// Must implement RenderObject<Self::Protocol> for RenderTree storage.
    type RenderObject: flui_rendering::traits::RenderObject<Self::Protocol> + Send + Sync + 'static;

    /// Create a new RenderObject.
    ///
    /// Called once when the Element is first mounted.
    fn create_render_object(&self, ctx: &RenderObjectContext<'_>) -> Self::RenderObject;

    /// Update an existing RenderObject with new configuration.
    ///
    /// Called when this View updates an existing Element.
    fn update_render_object(
        &self,
        ctx: &RenderObjectContext<'_>,
        render_object: &mut Self::RenderObject,
    ) -> flui_rendering::RenderUpdateImpact;

    /// Release owner-runtime resources associated with a render object before it
    /// is removed from the render tree.
    ///
    /// Default implementation is a no-op. Interactive render-object widgets use
    /// this hook to unregister owner-local targets while the same owner context
    /// that created/updated them is active.
    fn did_unmount_render_object(
        &self,
        _ctx: &RenderObjectContext<'_>,
        _render_object: &mut Self::RenderObject,
    ) {
    }

    /// Whether this View can have children.
    ///
    /// Override to return true for single/multi child variants.
    fn has_children(&self) -> bool {
        false
    }

    /// Visit child views for mounting.
    ///
    /// Override for single/multi child variants to provide access to children.
    /// The visitor is called once for each child View.
    ///
    /// Default implementation does nothing (leaf widgets have no children).
    fn visit_child_views(&self, _visitor: &mut dyn FnMut(&dyn View)) {
        // Default: no children
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
            fn create_element(&self) -> $crate::element::ElementKind {
                $crate::element::ElementKind::render_variable(self)
            }
        }
    };
}

/// Implements [`RenderView::has_children`] + [`RenderView::visit_child_views`]
/// for the standard single-child widget shape (`child: Option<…>`), invoked
/// from *inside* the `impl RenderView for …` block:
///
/// ```rust,ignore
/// impl RenderView for MyProxyBox {
///     // ...
///     flui_view::single_child_view_children!();
/// }
/// ```
///
/// Pass a field name when the child is stored under something other than
/// `child`.
#[macro_export]
macro_rules! single_child_view_children {
    () => {
        $crate::single_child_view_children!(child);
    };
    ($field:ident) => {
        fn has_children(&self) -> bool {
            self.$field.is_some()
        }

        fn visit_child_views(&self, visitor: &mut dyn FnMut(&dyn $crate::View)) {
            if let Some(child) = self.$field.as_ref() {
                visitor(child);
            }
        }
    };
}

// NOTE: RenderElement implementation has been moved to unified Element
// architecture. See crates/flui-view/src/element/unified.rs and
// element/behavior.rs The type alias is exported from element/mod.rs:
//   pub type RenderElement<V> = Element<V, Variable, RenderBehavior<V>>;
