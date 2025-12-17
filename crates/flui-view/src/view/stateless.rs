//! StatelessView - Views without internal state.
//!
//! StatelessViews are the simplest type of View. They describe UI purely
//! as a function of their configuration (fields) and inherited data.

use super::view::{ElementBase, View};
use crate::context::{BuildContext, ElementBuildContext};
use crate::element::Lifecycle;
use flui_foundation::{ElementId, RenderId};
use flui_rendering::pipeline::PipelineOwner;
use parking_lot::RwLock;
use std::any::{Any, TypeId};
use std::sync::Arc;

/// A View that has no mutable state.
///
/// StatelessViews rebuild their child tree based solely on:
/// - Their own configuration (struct fields)
/// - Data from ancestor InheritedViews
///
/// They are rebuilt when:
/// - Their configuration changes (parent rebuilds with new View)
/// - An InheritedView they depend on changes
///
/// # Flutter Equivalent
///
/// This corresponds to Flutter's `StatelessWidget`.
///
/// # Example
///
/// ```rust,ignore
/// use flui_view::{StatelessView, BuildContext};
///
/// #[derive(Clone)]
/// struct Greeting {
///     name: String,
/// }
///
/// impl StatelessView for Greeting {
///     fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View> {
///         Text::new(format!("Hello, {}!", self.name)).boxed()
///     }
/// }
/// ```
///
/// # Note
///
/// Types implementing `StatelessView` must also implement `Clone`.
/// Use the derive macro: `#[derive(Clone)]`
pub trait StatelessView: Clone + Send + Sync + 'static {
    /// Build the child View tree.
    ///
    /// Called whenever this View needs to be rendered. The returned View
    /// describes what should be displayed.
    fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View>;
}

/// Implement View for all StatelessViews.
///
/// This macro creates the View implementation for a StatelessView type.
/// Use it after implementing StatelessView:
///
/// ```rust,ignore
/// impl StatelessView for MyView {
///     fn build(&self, ctx: &dyn BuildContext) -> Box<dyn View> {
///         // ...
///     }
/// }
/// impl_stateless_view!(MyView);
/// ```
#[macro_export]
macro_rules! impl_stateless_view {
    ($ty:ty) => {
        impl $crate::View for $ty {
            fn create_element(&self) -> Box<dyn $crate::ElementBase> {
                Box::new($crate::StatelessElement::new(self))
            }
        }
    };
}

// ============================================================================
// StatelessElement
// ============================================================================

/// Element for StatelessViews.
///
/// Manages the lifecycle of a StatelessView and its child.
pub struct StatelessElement<V: StatelessView> {
    /// The current View configuration.
    view: V,
    /// Current lifecycle state.
    lifecycle: Lifecycle,
    /// Depth in tree.
    depth: usize,
    /// Child element (built from view.build()).
    child: Option<Box<dyn ElementBase>>,
    /// Whether we need to rebuild.
    dirty: bool,
    /// PipelineOwner for render tree access (propagated to children).
    pipeline_owner: Option<Arc<RwLock<PipelineOwner>>>,
    /// Parent's RenderId for tree structure (propagated to children).
    parent_render_id: Option<RenderId>,
}

impl<V: StatelessView> StatelessElement<V> {
    /// Create a new StatelessElement for the given View.
    pub fn new(view: &V) -> Self {
        Self {
            view: view.clone(),
            lifecycle: Lifecycle::Initial,
            depth: 0,
            child: None,
            dirty: true,
            pipeline_owner: None,
            parent_render_id: None,
        }
    }
}

impl<V: StatelessView> std::fmt::Debug for StatelessElement<V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StatelessElement")
            .field("lifecycle", &self.lifecycle)
            .field("depth", &self.depth)
            .field("dirty", &self.dirty)
            .finish_non_exhaustive()
    }
}

impl<V: StatelessView> ElementBase for StatelessElement<V> {
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
            self.dirty = true;
        }
    }

    fn mark_needs_build(&mut self) {
        self.dirty = true;
    }

    fn perform_build(&mut self) {
        if !self.dirty || !self.lifecycle.can_build() {
            tracing::trace!(
                "StatelessElement::perform_build skipped (dirty={}, can_build={})",
                self.dirty,
                self.lifecycle.can_build()
            );
            return;
        }

        tracing::debug!("StatelessElement::perform_build starting rebuild");

        // Create BuildContext for the build call
        let ctx = ElementBuildContext::new_minimal(self.depth);

        // Call view.build() to get the child View
        let child_view = self.view.build(&ctx);

        if self.child.is_none() {
            // First build - create child element
            let mut child_element = child_view.create_element();

            // Propagate PipelineOwner and parent_render_id to child
            if let Some(ref pipeline_owner) = self.pipeline_owner {
                let owner_any: Arc<dyn Any + Send + Sync> =
                    Arc::clone(pipeline_owner) as Arc<dyn Any + Send + Sync>;
                child_element.set_pipeline_owner_any(owner_any);
                child_element.set_parent_render_id(self.parent_render_id);

                tracing::debug!(
                    "StatelessElement::perform_build propagated PipelineOwner and parent_id={:?} to child",
                    self.parent_render_id
                );
            }

            // Mount child
            child_element.mount(None, self.depth + 1);

            // Build child's children
            child_element.perform_build();

            self.child = Some(child_element);
        } else {
            // Rebuild - update existing child
            tracing::debug!("StatelessElement::perform_build updating existing child");
            if let Some(ref mut child) = self.child {
                child.update(child_view.as_ref());
                child.perform_build();
            }
        }

        self.dirty = false;
    }

    fn mount(&mut self, _parent: Option<ElementId>, _slot: usize) {
        self.lifecycle = Lifecycle::Active;
        self.dirty = true;
    }

    fn deactivate(&mut self) {
        self.lifecycle = Lifecycle::Inactive;
        if let Some(child) = &mut self.child {
            child.deactivate();
        }
    }

    fn activate(&mut self) {
        self.lifecycle = Lifecycle::Active;
        if let Some(child) = &mut self.child {
            child.activate();
        }
    }

    fn unmount(&mut self) {
        self.lifecycle = Lifecycle::Defunct;
        if let Some(child) = &mut self.child {
            child.unmount();
        }
        self.child = None;
    }

    fn visit_children(&self, visitor: &mut dyn FnMut(ElementId)) {
        // StatelessElement manages its child internally
        // In a full implementation, we'd track child ElementIds
        let _ = visitor;
    }

    fn depth(&self) -> usize {
        self.depth
    }

    fn set_pipeline_owner_any(&mut self, owner: Arc<dyn Any + Send + Sync>) {
        // Downcast from Arc<dyn Any> to Arc<RwLock<PipelineOwner>>
        if let Ok(pipeline_owner) = owner.downcast::<RwLock<PipelineOwner>>() {
            self.pipeline_owner = Some(pipeline_owner);
            tracing::debug!("StatelessElement::set_pipeline_owner_any received PipelineOwner");
        } else {
            tracing::warn!("StatelessElement::set_pipeline_owner_any received wrong type");
        }
    }

    fn set_parent_render_id(&mut self, parent_id: Option<RenderId>) {
        self.parent_render_id = parent_id;
        tracing::debug!(
            "StatelessElement::set_parent_render_id parent_id={:?}",
            parent_id
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct TestView {
        text: String,
    }

    impl StatelessView for TestView {
        fn build(&self, _ctx: &dyn BuildContext) -> Box<dyn View> {
            // Return self for testing - in real code this would return child views
            Box::new(self.clone())
        }
    }

    // Implement View for TestView using the macro pattern
    impl View for TestView {
        fn create_element(&self) -> Box<dyn ElementBase> {
            Box::new(StatelessElement::new(self))
        }
    }

    #[test]
    fn test_stateless_element_creation() {
        let view = TestView {
            text: "Hello".to_string(),
        };
        let element = StatelessElement::new(&view);
        assert_eq!(element.lifecycle(), Lifecycle::Initial);
        assert!(element.dirty);
    }

    #[test]
    fn test_stateless_element_mount() {
        let view = TestView {
            text: "Hello".to_string(),
        };
        let mut element = StatelessElement::new(&view);
        element.mount(None, 0);
        assert_eq!(element.lifecycle(), Lifecycle::Active);
    }
}
