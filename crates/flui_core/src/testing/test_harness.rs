//! Test harness for element tree testing
//!
//! Provides utilities for testing FLUI views and elements in isolation.

use crate::{
    element::{ElementId, ElementTree},
    foundation::Key,
    pipeline::{PipelineBuilder, PipelineOwner},
    view::{BuildContext, IntoElement, StatelessView},
};
use flui_types::{BoxConstraints, Size};
use parking_lot::RwLock;
use std::sync::Arc;

/// Test harness for testing FLUI views
///
/// Provides a controlled environment for building and testing view trees.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::testing::TestHarness;
///
/// #[test]
/// fn test_simple_view() {
///     let mut harness = TestHarness::new();
///
///     // Mount a view
///     harness.mount(MyView::new());
///
///     // Pump the pipeline to build
///     harness.pump();
///
///     // Assert state
///     assert!(harness.is_mounted());
/// }
/// ```
#[derive(Debug)]
pub struct TestHarness {
    pipeline: PipelineOwner,
    tree: Arc<RwLock<ElementTree>>,
    root_id: Option<ElementId>,
}

impl TestHarness {
    /// Create a new test harness
    pub fn new() -> Self {
        let tree = Arc::new(RwLock::new(ElementTree::new()));
        let pipeline = PipelineBuilder::new().build();

        Self {
            pipeline,
            tree,
            root_id: None,
        }
    }

    /// Mount a stateless view as the root of the test tree
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// harness.mount_stateless(MyView::new());
    /// ```
    pub fn mount_stateless<V: StatelessView>(&mut self, view: V) -> ElementId {
        let tree = self.tree.clone();
        let element_id = {
            let mut tree_guard = tree.write();

            // Create build context (minimal for testing)
            let ctx = BuildContext::new(
                tree.clone(),
                ElementId::new(1), // Placeholder element ID
            );

            // Build the view into an element
            let element = view.build(&ctx).into_element();

            // Insert the element into the tree
            tree_guard.insert(element)
        };

        self.root_id = Some(element_id);
        element_id
    }

    /// Pump the pipeline to process pending builds, layouts, and paints
    ///
    /// This triggers:
    /// 1. Build phase - rebuilds dirty components
    /// 2. Layout phase - computes sizes and positions
    /// 3. Paint phase - generates paint layers
    pub fn pump(&mut self) {
        self.pump_build();
        self.pump_layout(BoxConstraints::tight(Size::new(800.0, 600.0)));
        self.pump_paint();
    }

    /// Pump only the build phase
    pub fn pump_build(&mut self) {
        self.pipeline.flush_build();
    }

    /// Pump only the layout phase with given constraints
    pub fn pump_layout(&mut self, constraints: BoxConstraints) {
        let _ = self.pipeline.flush_layout(constraints);
    }

    /// Pump only the paint phase
    pub fn pump_paint(&mut self) {
        let _ = self.pipeline.flush_paint();
    }

    /// Check if a root element is mounted
    pub fn is_mounted(&self) -> bool {
        self.root_id.is_some()
    }

    /// Get the root element ID
    pub fn root_id(&self) -> Option<ElementId> {
        self.root_id
    }

    /// Get access to the element tree
    pub fn tree(&self) -> &Arc<RwLock<ElementTree>> {
        &self.tree
    }

    /// Get access to the pipeline
    pub fn pipeline(&self) -> &PipelineOwner {
        &self.pipeline
    }

    /// Find an element by key
    ///
    /// # Note
    ///
    /// Currently not implemented - key system is pending implementation.
    /// Returns None for all keys.
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let button_id = harness.find_by_key(Key::from_str("submit-button"));
    /// ```
    pub fn find_by_key(&self, _key: Key) -> Option<ElementId> {
        // TODO: Implement when key system is added to Element
        None
    }

    /// Get the number of elements in the tree
    pub fn element_count(&self) -> usize {
        let tree = self.tree.read();
        tree.len()
    }

    /// Unmount the root element
    ///
    /// Note: This currently just clears the root_id reference.
    /// Full unmounting would require implementing element cleanup in ElementTree.
    pub fn unmount(&mut self) {
        self.root_id = None;
    }
}

impl Default for TestHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestHarness {
    fn drop(&mut self) {
        self.unmount();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug)]
    struct TestView;

    impl View for TestView {
        fn build(self, _ctx: &BuildContext) -> impl crate::IntoElement {
            // Return None for minimal testing (terminates tree)
            Option::<TestView>::None
        }
    }

    #[test]
    fn test_harness_creation() {
        let harness = TestHarness::new();
        assert!(!harness.is_mounted());
        assert_eq!(harness.element_count(), 0);
    }

    #[test]
    fn test_mount_unmount() {
        let mut harness = TestHarness::new();

        let _root_id = harness.mount(TestView);
        assert!(harness.is_mounted());

        harness.unmount();
        assert!(!harness.is_mounted());
    }
}
