//! RenderView - Views that create RenderObjects.
//!
//! RenderViews are leaf nodes in the View tree that produce RenderObjects.
//! They bridge the View/Element system with the Render tree for layout and
//! painting.

use crate::view::View;

/// Owner-runtime capabilities available while a [`RenderView`] creates or
/// updates its render object.
///
/// The context is intentionally narrow: it carries only the composition
/// capabilities a render-object widget needs to register owner-local
/// interaction callbacks while keeping the render object itself data-only and
/// `Send + Sync`.
#[derive(Debug, Clone, Copy)]
pub struct RenderObjectContext<'a> {
    interaction_dispatch: Option<&'a flui_interaction::InteractionDispatchHandle>,
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
    /// Construct a context from the active owner interaction handle.
    pub(crate) const fn new(
        interaction_dispatch: Option<&'a flui_interaction::InteractionDispatchHandle>,
    ) -> Self {
        Self {
            interaction_dispatch,
        }
    }

    /// A detached context for tests or hand-built render objects that are not
    /// mounted under a FLUI owner runtime.
    #[must_use]
    pub const fn detached() -> Self {
        Self::new(None)
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
        handler: impl Fn(&flui_interaction::PointerEvent) + 'static,
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
        handler: impl Fn(&flui_interaction::PointerEvent) + 'static,
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
        callbacks: flui_interaction::MouseRegionCallbacks,
    ) -> Result<flui_interaction::MouseRegionTarget, RenderObjectContextError> {
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
        target: flui_interaction::MouseRegionTarget,
        callbacks: flui_interaction::MouseRegionCallbacks,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self
            .dispatch_handle()?
            .replace_mouse_region(target, callbacks)?)
    }

    /// Remove a mouse-region target from future annotation resolution.
    ///
    /// Existing tracker state may still retain a strong owner-local cell long
    /// enough to emit the matching exit callback for a previously active
    /// annotation.
    ///
    /// # Errors
    ///
    /// Returns the lane's typed dispatch error for wrong/detached owner state
    /// or for a target already removed from the active owner lane.
    pub fn unregister_mouse_region(
        &self,
        target: flui_interaction::MouseRegionTarget,
    ) -> Result<(), RenderObjectContextError> {
        Ok(self.dispatch_handle()?.unregister_mouse_region(target)?)
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
        clipper: impl Fn(flui_types::Size) -> flui_types::painting::Path + 'static,
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
        clipper: impl Fn(flui_types::Size) -> flui_types::painting::Path + 'static,
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
        factory: impl Fn(flui_types::Rect<flui_types::Pixels>) -> flui_types::painting::Shader + 'static,
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
        factory: impl Fn(flui_types::Rect<flui_types::Pixels>) -> flui_types::painting::Shader + 'static,
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
/// use flui_view::{RenderObjectContext, RenderView};
/// use flui_rendering::RenderBox;
///
/// struct ColoredBox {
///     color: Color,
///     child: Option<Box<dyn View>>,
/// }
///
/// impl RenderView for ColoredBox {
///     type Protocol = flui_rendering::protocol::BoxProtocol;
///     type RenderObject = RenderDecoratedBox;
///
///     fn create_render_object(&self, _ctx: &RenderObjectContext<'_>) -> Self::RenderObject {
///         RenderDecoratedBox::new(self.color)
///     }
///
///     fn update_render_object(&self, _ctx: &RenderObjectContext<'_>, render: &mut Self::RenderObject) {
///         render.set_color(self.color);
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
    );

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

// NOTE: RenderElement implementation has been moved to unified Element
// architecture. See crates/flui-view/src/element/unified.rs and
// element/behavior.rs The type alias is exported from element/mod.rs:
//   pub type RenderElement<V> = Element<V, Variable, RenderBehavior<V>>;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use flui_foundation::RenderId;
    use flui_objects::RenderSizedBox;
    use flui_rendering::pipeline::PipelineOwner;
    use flui_types::geometry::px;
    use parking_lot::RwLock;

    use super::*;
    use crate::{
        RenderElement,
        element::{Lifecycle, RenderBehavior},
        view::{ElementBase, View},
    };

    /// A simple test RenderView using RenderSizedBox
    #[derive(Clone)]
    struct SizedBoxView {
        width: f32,
        height: f32,
    }

    impl RenderView for SizedBoxView {
        type Protocol = flui_rendering::protocol::BoxProtocol;
        type RenderObject = RenderSizedBox;

        fn create_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
        ) -> Self::RenderObject {
            RenderSizedBox::new(Some(px(self.width)), Some(px(self.height)))
        }

        fn update_render_object(
            &self,
            _ctx: &crate::RenderObjectContext<'_>,
            _render_object: &mut Self::RenderObject,
        ) {
            // RenderSizedBox doesn't have setters for width/height after
            // creation In a real implementation, we'd update the
            // constraints
        }
    }

    impl View for SizedBoxView {
        fn create_element(&self) -> crate::element::ElementKind {
            crate::element::ElementKind::render_variable(self)
        }
    }

    #[test]
    fn test_render_element_creation() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let element = RenderElement::new(&view, RenderBehavior::new());

        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        assert!(element.render_id().is_none()); // Not created until mount
    }

    #[test]
    fn test_render_element_mount_without_pipeline_owner() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view, RenderBehavior::new());

        // Mount without PipelineOwner - should still set lifecycle but no render_id
        let mut build_owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut build_owner.element_owner_mut());

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        assert!(element.render_id().is_none()); // No PipelineOwner, so no render_id
    }

    #[test]
    fn test_render_element_mount_with_pipeline_owner() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view, RenderBehavior::new());

        // Set up PipelineOwner
        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        element.set_pipeline_owner(Arc::clone(&pipeline_owner));

        let mut build_owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut build_owner.element_owner_mut());

        assert_eq!(element.lifecycle(), Lifecycle::Active);
        assert!(element.render_id().is_some());

        // Verify RenderObject was inserted into RenderTree
        let owner = pipeline_owner.read();
        let render_id = element.render_id().unwrap();
        assert!(owner.render_tree().contains(render_id));
    }

    #[test]
    fn test_render_element_unmount() {
        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view, RenderBehavior::new());

        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        element.set_pipeline_owner(Arc::clone(&pipeline_owner));
        let mut build_owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut build_owner.element_owner_mut());

        let render_id = element.render_id().unwrap();
        assert!(pipeline_owner.read().render_tree().contains(render_id));

        element.unmount(&mut build_owner.element_owner_mut());

        assert_eq!(element.lifecycle(), Lifecycle::Defunct);
        assert!(element.render_id().is_none());
        // RenderObject should be removed from tree
        assert!(!pipeline_owner.read().render_tree().contains(render_id));
    }

    #[test]
    fn test_render_object_element_trait() {
        use crate::element::RenderObjectElement;

        let view = SizedBoxView {
            width: 100.0,
            height: 100.0,
        };
        let mut element = RenderElement::new(&view, RenderBehavior::new());

        let pipeline_owner = Arc::new(RwLock::new(PipelineOwner::new()));
        element.set_pipeline_owner(Arc::clone(&pipeline_owner));
        let mut build_owner = crate::BuildOwner::new();
        element.mount(None, 0, &mut build_owner.element_owner_mut());

        // Test RenderObjectElement methods - returns RenderId
        assert!(RenderObjectElement::render_object_any(&element).is_some());

        // Downcast to RenderId
        let render_any = RenderObjectElement::render_object_any(&element).unwrap();
        let render_id = render_any.downcast_ref::<RenderId>().unwrap();

        // Verify we can access the RenderObject through RenderTree
        let owner = pipeline_owner.read();
        let node = owner.render_tree().get(*render_id).unwrap();
        let render_obj = node.box_render_object();
        let sized_box = render_obj.as_any().downcast_ref::<RenderSizedBox>();
        // RenderSizedBox exists - that's enough to verify
        assert!(sized_box.is_some());
    }
}
