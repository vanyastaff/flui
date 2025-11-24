//! View testing utilities
//!
//! Provides utilities for testing individual views in isolation.

use crate::{
    element::ElementId,
    view::{BuildContext, IntoElement, StatelessView},
};

use super::test_harness::TestHarness;

/// Builder for testing views
///
/// Provides a fluent API for setting up and testing views.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::testing::ViewTester;
///
/// #[test]
/// fn test_my_view() {
///     let result = ViewTester::new()
///         .with_view(MyView::new())
///         .build()
///         .expect("View should build");
///
///     assert!(result.is_mounted());
/// }
/// ```
#[derive(Debug)]
pub struct ViewTester {
    harness: TestHarness,
}

impl ViewTester {
    /// Create a new view tester
    pub fn new() -> Self {
        Self {
            harness: TestHarness::new(),
        }
    }

    /// Mount a view for testing
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let tester = ViewTester::new()
    ///     .with_view(MyView { text: "Hello".to_string() });
    /// ```
    pub fn with_view<V: StatelessView>(mut self, view: V) -> ViewTestResult {
        let root_id = self.harness.mount_stateless(view);
        self.harness.pump_build();

        ViewTestResult {
            harness: self.harness,
            root_id,
        }
    }
}

impl Default for ViewTester {
    fn default() -> Self {
        Self::new()
    }
}

/// Result of a view test
///
/// Provides access to the test harness and assertions about the built view.
#[derive(Debug)]
pub struct ViewTestResult {
    harness: TestHarness,
    root_id: ElementId,
}

impl ViewTestResult {
    /// Check if the view is mounted
    pub fn is_mounted(&self) -> bool {
        self.harness.is_mounted()
    }

    /// Get the root element ID
    pub fn root_id(&self) -> ElementId {
        self.root_id
    }

    /// Get the number of elements in the tree
    pub fn element_count(&self) -> usize {
        self.harness.element_count()
    }

    /// Get access to the underlying test harness
    pub fn harness(&self) -> &TestHarness {
        &self.harness
    }

    /// Get mutable access to the underlying test harness
    pub fn harness_mut(&mut self) -> &mut TestHarness {
        &mut self.harness
    }

    /// Pump the pipeline to process updates
    pub fn pump(&mut self) {
        self.harness.pump();
    }

    /// Pump only the build phase
    pub fn pump_build(&mut self) {
        self.harness.pump_build();
    }
}

/// Test utility for creating simple test views with a name
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::testing::TestView;
///
/// let view = TestView::new("test-view");
/// ```
#[derive(Clone, Debug)]
pub struct TestView {
    name: String,
}

/// Simple test widget without any state
///
/// Use this for basic testing where you just need a simple view
/// that terminates the tree.
///
/// # Examples
///
/// ```rust,ignore
/// use flui_core::testing::TestWidget;
///
/// let widget = TestWidget;
/// ```
#[derive(Clone, Debug)]
pub struct TestWidget;

impl TestView {
    /// Create a new test view with a name
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }

    /// Get the name of this test view
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl StatelessView for TestView {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Return None for minimal testing (terminates tree)
        Option::<crate::element::Element>::None
    }
}

impl StatelessView for TestWidget {
    fn build(self, _ctx: &BuildContext) -> impl IntoElement {
        // Return None to terminate tree
        Option::<crate::element::Element>::None
    }
}

// Note: TestWidget can be used with BuildFn via View trait
// because it implements View + Clone

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_view_tester() {
        let result = ViewTester::new().with_view(TestView::new("test"));

        assert!(result.is_mounted());
        assert!(result.element_count() > 0);
    }

    #[test]
    fn test_view_creation() {
        let view = TestView::new("my-test");
        assert_eq!(view.name(), "my-test");
    }
}
